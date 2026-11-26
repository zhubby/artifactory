//! [INPUT]: 依赖真实操作日志、连接消息与冻结操作信封；Shell围栏仅用于Unix。
//! [OUTPUT]: 验证重连隔离结果重放和确认，精确再授权不重跑探测。
//! [POS]: 操作旁路的认证、维护围栏和关闭边界回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::json;
use std::sync::{Arc, atomic::AtomicBool};

fn envelope() -> OperationEnvelope {
    let input = validate_input(
        OperationKind::CollectDiagnostics,
        1,
        &json!({"probes":["git"]}),
    )
    .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    OperationEnvelope {
        operation_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        node_id: Uuid::new_v4(),
        startup_id: Uuid::new_v4(),
        control_session: Uuid::new_v4(),
        actor_id: Uuid::new_v4(),
        kind: input.kind(),
        schema: 1,
        input: input.value(),
        input_digest: input.digest(),
        issued_at_ms: now,
        expires_at_ms: now + 30_000,
    }
}

async fn connect(input: &Handle, session: Uuid) -> mpsc::Receiver<pb::EmissaryEvent> {
    let (sender, output) = mpsc::channel(32);
    let now = Instant::now();
    input
        .send(Input::Connected {
            sender,
            session,
            server_time_ms: chrono::Utc::now().timestamp_millis(),
            started: now,
            received: now,
        })
        .await
        .unwrap();
    output
}

async fn command(input: &Handle, kind: &str, payload: &impl serde::Serialize) {
    input
        .send(Input::Command(Box::new(pb::Command {
            kind: kind.into(),
            payload_json: serde_json::to_string(payload).unwrap(),
            ..Default::default()
        })))
        .await
        .unwrap();
}

async fn update(output: &mut mpsc::Receiver<pb::EmissaryEvent>) -> OperationUpdate {
    let event = tokio::time::timeout(Duration::from_secs(2), output.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.kind, "node_operation_update");
    serde_json::from_str(&event.payload_json).unwrap()
}

async fn quiet(output: &mut mpsc::Receiver<pb::EmissaryEvent>) {
    assert!(
        tokio::time::timeout(Duration::from_millis(100), output.recv())
            .await
            .is_err(),
        "unauthorized connection must not receive an existing result"
    );
}

#[cfg(unix)]
async fn journal_state(root: &std::path::Path, id: Uuid, expected: OperationState) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let state = std::fs::read(root.join("operations/journal.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            if state.as_ref().is_some_and(|state| {
                state["entries"].as_array().is_some_and(|entries| {
                    entries.iter().any(|entry| {
                        entry["update"]["operation_id"] == id.to_string()
                            && entry["update"]["state"] == serde_json::to_value(expected).unwrap()
                    })
                })
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn full_accepted_outbound_then_resume_never_starts_the_probe() {
    let root = tempfile::tempdir().unwrap();
    let mut original = envelope();
    let tool = root.path().join("git");
    crate::config::private_write(
        &tool,
        b"#!/bin/sh\necho called >> \"$0.called\"\necho version 2.40.1\n",
        true,
        true,
    )
    .unwrap();
    let mut cfg = Config {
        root: root.path().into(),
        emissary_id: original.node_id.to_string(),
        ..Default::default()
    };
    cfg.tools
        .insert("git".into(), tool.to_string_lossy().into_owned());
    let input = start(
        cfg,
        original.startup_id,
        None,
        Default::default(),
        Default::default(),
    );
    let (sender, _blocked) = mpsc::channel(1);
    sender.send(pb::EmissaryEvent::default()).await.unwrap();
    let now = Instant::now();
    input
        .send(Input::Connected {
            sender,
            session: original.control_session,
            server_time_ms: original.issued_at_ms,
            started: now,
            received: now,
        })
        .await
        .unwrap();
    command(&input, "node_operation", &original).await;
    journal_state(root.path(), original.operation_id, OperationState::Accepted).await;
    assert!(!tool.with_extension("called").exists());
    input.send(Input::Disconnected).await.unwrap();
    original.control_session = Uuid::new_v4();
    let mut output = connect(&input, original.control_session).await;
    let resume = OperationResume {
        operation_id: original.operation_id,
        tenant_id: original.tenant_id,
        node_id: original.node_id,
        original_startup_id: original.startup_id,
        current_startup_id: original.startup_id,
        control_session: original.control_session,
        input_digest: original.input_digest.clone(),
    };
    command(&input, "node_operation_resume", &resume).await;
    assert_eq!(update(&mut output).await.state, OperationState::Accepted);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(
        !tool.with_extension("called").exists(),
        "status reauthorization must not execute Accepted"
    );
    journal_state(root.path(), original.operation_id, OperationState::Accepted).await;
    command(&input, "node_operation", &original).await;
    loop {
        let update = update(&mut output).await;
        if update.state.terminal() {
            assert_eq!(update.state, OperationState::Succeeded);
            break;
        }
    }
    assert_eq!(
        std::fs::read_to_string(tool.with_extension("called"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    input.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_before_envelope_persists_terminal_without_probe_execution() {
    let root = tempfile::tempdir().unwrap();
    let original = envelope();
    let tool = root.path().join("git");
    crate::config::private_write(
        &tool,
        b"#!/bin/sh\ntouch \"$0.called\"\necho version 2.40.1\n",
        true,
        true,
    )
    .unwrap();
    let mut cfg = Config {
        root: root.path().into(),
        emissary_id: original.node_id.to_string(),
        ..Default::default()
    };
    cfg.tools
        .insert("git".into(), tool.to_string_lossy().into_owned());
    let input = start(
        cfg,
        original.startup_id,
        None,
        Default::default(),
        Default::default(),
    );
    let mut output = connect(&input, original.control_session).await;
    command(
        &input,
        "node_operation_cancel",
        &OperationCancel {
            operation_id: original.operation_id,
            startup_id: original.startup_id,
            control_session: original.control_session,
        },
    )
    .await;
    command(&input, "node_operation", &original).await;
    loop {
        let update = update(&mut output).await;
        if update.state.terminal() {
            assert_eq!(update.state, OperationState::Canceled);
            break;
        }
    }
    assert!(!tool.with_extension("called").exists());
    journal_state(root.path(), original.operation_id, OperationState::Canceled).await;
    input.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_joins_child_cleanup_and_durable_terminal() {
    let root = tempfile::tempdir().unwrap();
    let original = envelope();
    let tool = root.path().join("git");
    crate::config::private_write(
        &tool,
        b"#!/bin/sh\nsleep 30 &\necho $! > \"$0.child\"\nwait\n",
        true,
        true,
    )
    .unwrap();
    let mut cfg = Config {
        root: root.path().into(),
        emissary_id: original.node_id.to_string(),
        ..Default::default()
    };
    cfg.tools
        .insert("git".into(), tool.to_string_lossy().into_owned());
    let status = Arc::new(AtomicBool::new(false));
    let input = start(
        cfg,
        original.startup_id,
        None,
        status.clone(),
        Default::default(),
    );
    let mut output = connect(&input, original.control_session).await;
    command(&input, "node_operation", &original).await;
    assert_eq!(update(&mut output).await.state, OperationState::Accepted);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !tool.with_extension("child").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let child = std::fs::read_to_string(tool.with_extension("child")).unwrap();
    input.shutdown().await.unwrap();
    assert!(!status.load(std::sync::atomic::Ordering::Relaxed));
    let alive = std::process::Command::new("kill")
        .args(["-0", child.trim()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(
        !alive.success(),
        "shutdown success must follow process tree cleanup"
    );
    journal_state(root.path(), original.operation_id, OperationState::Canceled).await;
    let reopened = Runtime::open(root.path().join("operations"), Uuid::new_v4()).unwrap();
    assert_eq!(
        reopened.status(original.operation_id).await.unwrap().state,
        OperationState::Canceled
    );
}

#[cfg(unix)]
#[tokio::test]
async fn maintenance_fence_samples_active_work_without_canceling_it() {
    let root = tempfile::tempdir().unwrap();
    let original = envelope();
    let tool = root.path().join("git");
    crate::config::private_write(
        &tool,
        b"#!/bin/sh\ntouch \"$0.running\"\nwhile [ ! -f \"$0.release\" ]; do sleep 0.02; done\necho version 2.40.1\n",
        true,
        true,
    ).unwrap();
    let mut cfg = Config {
        root: root.path().into(),
        emissary_id: original.node_id.to_string(),
        ..Default::default()
    };
    cfg.tools
        .insert("git".into(), tool.to_string_lossy().into_owned());
    let status = Arc::new(AtomicBool::new(false));
    let input = start(
        cfg,
        original.startup_id,
        None,
        status.clone(),
        Default::default(),
    );
    let mut output = connect(&input, original.control_session).await;
    command(&input, "node_operation", &original).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while !tool.with_extension("running").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // 模拟缓存仍是空闲；确认必须先覆盖缓存，再唤醒维护请求。
    status.store(false, std::sync::atomic::Ordering::SeqCst);
    input.maintenance(true);
    let (reply, ready) = tokio::sync::oneshot::channel();
    input.send(Input::Fence(reply)).await.unwrap();
    assert!(ready.await.unwrap());
    assert!(status.load(std::sync::atomic::Ordering::SeqCst));
    let mut rejected = original.clone();
    rejected.operation_id = Uuid::new_v4();
    command(&input, "node_operation", &rejected).await;
    loop {
        let observed = update(&mut output).await;
        if observed.operation_id == rejected.operation_id {
            assert_eq!(observed.state, OperationState::Rejected);
            assert_eq!(observed.reason, Some(OperationReason::Busy));
            break;
        }
    }
    std::fs::write(tool.with_extension("release"), []).unwrap();
    loop {
        let observed = update(&mut output).await;
        if observed.operation_id == original.operation_id && observed.state.terminal() {
            assert_eq!(
                observed.state,
                OperationState::Succeeded,
                "maintenance must not cancel existing work"
            );
            break;
        }
    }
    input.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconnect_requires_exact_authorization_before_result_replay_or_ack() {
    let root = tempfile::tempdir().unwrap();
    let original = envelope();
    let identity = Identity {
        tenant_id: original.tenant_id,
        node_id: original.node_id,
        startup_id: original.startup_id,
        control_session: original.control_session,
    };
    let runtime = Runtime::open(root.path().join("operations"), original.startup_id).unwrap();
    let now = Instant::now();
    let clock = ServerClock::new(original.issued_at_ms, now, now).unwrap();
    runtime
        .accept(original.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    let terminal = runtime
        .cancel(
            &OperationCancel {
                operation_id: original.operation_id,
                startup_id: original.startup_id,
                control_session: original.control_session,
            },
            &identity,
        )
        .await
        .unwrap();
    drop(runtime);

    let cfg = Config {
        root: root.path().into(),
        emissary_id: original.node_id.to_string(),
        ..Default::default()
    };
    let input = start(
        cfg,
        original.startup_id,
        None,
        Arc::new(AtomicBool::new(false)),
        Default::default(),
    );
    let mut old = connect(&input, original.control_session).await;
    command(&input, "node_operation", &original).await;
    assert_eq!(update(&mut old).await.state, OperationState::Canceled);
    input.send(Input::Disconnected).await.unwrap();
    let current = Uuid::new_v4();
    let mut output = connect(&input, current).await;

    let mut invalid = original.clone();
    command(&input, "node_operation", &invalid).await;
    quiet(&mut output).await;
    invalid.control_session = current;
    invalid.startup_id = Uuid::new_v4();
    command(&input, "node_operation", &invalid).await;
    quiet(&mut output).await;
    invalid.startup_id = original.startup_id;
    invalid.input_digest = "0".repeat(64);
    command(&input, "node_operation", &invalid).await;
    quiet(&mut output).await;
    let conflict = validate_input(
        OperationKind::CollectDiagnostics,
        1,
        &json!({"probes":["cargo"]}),
    )
    .unwrap();
    invalid.input = conflict.value();
    invalid.input_digest = conflict.digest();
    command(&input, "node_operation", &invalid).await;
    quiet(&mut output).await;

    let ack = OperationAck {
        operation_id: original.operation_id,
        result_digest: terminal.result_digest.clone().unwrap(),
    };
    command(&input, "node_operation_result_ack", &ack).await;
    let mut resume = OperationResume {
        operation_id: original.operation_id,
        tenant_id: original.tenant_id,
        node_id: original.node_id,
        original_startup_id: original.startup_id,
        current_startup_id: original.startup_id,
        control_session: current,
        input_digest: "0".repeat(64),
    };
    command(&input, "node_operation_resume", &resume).await;
    quiet(&mut output).await;
    resume.input_digest = original.input_digest.clone();
    command(&input, "node_operation_resume", &resume).await;
    let replay = update(&mut output).await;
    assert_eq!(replay.state, terminal.state);
    assert_eq!(
        replay.sequence, terminal.sequence,
        "resume cannot restart a terminal probe"
    );
    assert_eq!(replay.result_digest, terminal.result_digest);
    assert!(
        replay.result.is_some(),
        "unauthorized ACK must retain the result body"
    );

    // 当前连接授权后，摘要确认才可释放正文。
    command(&input, "node_operation_result_ack", &ack).await;
    command(&input, "node_operation_resume", &resume).await;
    loop {
        let replay = update(&mut output).await;
        assert_eq!(replay.state, terminal.state);
        assert_eq!(replay.sequence, terminal.sequence);
        if replay.result.is_none() {
            break;
        }
    }
    input.shutdown().await.unwrap();
}
