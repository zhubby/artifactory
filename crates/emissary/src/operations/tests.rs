//! [INPUT]: 依赖真实私有目录、身份信封和 Runtime 日志。
//! [OUTPUT]: 验证接受落盘、重启不重跑与摘要回执。
//! [POS]: 有限操作设备边界回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::json;

fn fixture() -> (Identity, OperationEnvelope, ServerClock) {
    let identity = Identity {
        tenant_id: Uuid::new_v4(),
        node_id: Uuid::new_v4(),
        startup_id: Uuid::new_v4(),
        control_session: Uuid::new_v4(),
    };
    let input = artifactory_emissary_protocol::operations::validate_input(
        OperationKind::CollectDiagnostics,
        1,
        &json!({"probes":["git"]}),
    )
    .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let envelope = OperationEnvelope {
        operation_id: Uuid::new_v4(),
        tenant_id: identity.tenant_id,
        node_id: identity.node_id,
        startup_id: identity.startup_id,
        control_session: identity.control_session,
        actor_id: Uuid::new_v4(),
        kind: input.kind(),
        schema: 1,
        input: input.value(),
        input_digest: input.digest(),
        issued_at_ms: now,
        expires_at_ms: now + 30_000,
    };
    let instant = Instant::now();
    (
        identity,
        envelope,
        ServerClock::new(now, instant, instant).unwrap(),
    )
}
#[tokio::test]
async fn acceptance_is_durable_and_restart_never_reexecutes() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    assert_eq!(
        runtime
            .accept(envelope.clone(), &identity, Some(&clock))
            .await
            .unwrap()
            .state,
        OperationState::Accepted
    );
    assert_eq!(
        runtime
            .accept(envelope.clone(), &identity, Some(&clock))
            .await
            .unwrap()
            .state,
        OperationState::Accepted
    );
    drop(runtime);
    let reopened = Runtime::open(directory.path().to_owned(), Uuid::new_v4()).unwrap();
    let update = reopened.updates().await.pop().unwrap();
    assert_eq!(update.state, OperationState::Interrupted);
    let bad = OperationAck {
        operation_id: envelope.operation_id,
        result_digest: "0".repeat(64),
    };
    assert!(!reopened.ack(&bad).await.unwrap());
    let good = OperationAck {
        operation_id: envelope.operation_id,
        result_digest: update.result_digest.unwrap(),
    };
    assert!(reopened.ack(&good).await.unwrap());
    assert!(reopened.updates().await.is_empty());
}
#[tokio::test]
async fn unsynchronized_or_expired_operation_is_never_accepted() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, mut envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    assert_eq!(
        runtime
            .accept(envelope.clone(), &identity, None)
            .await
            .unwrap_err(),
        OperationReason::ClockUnsynchronized
    );
    envelope.issued_at_ms -= 60_000;
    envelope.expires_at_ms -= 60_000;
    assert_eq!(
        runtime
            .accept(envelope, &identity, Some(&clock))
            .await
            .unwrap_err(),
        OperationReason::ReceiveExpired
    );
    assert!(runtime.updates().await.is_empty());
}
#[tokio::test]
async fn cancellation_of_accepted_operation_is_terminal_without_starting_probes() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    runtime
        .accept(envelope.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    let cancel = OperationCancel {
        operation_id: envelope.operation_id,
        startup_id: identity.startup_id,
        control_session: identity.control_session,
    };
    let update = runtime.cancel(&cancel, &identity).await.unwrap();
    assert_eq!(update.state, OperationState::Canceled);
    assert!(!runtime.active().await);
    assert!(
        runtime
            .start(
                envelope.operation_id,
                crate::config::Config::default(),
                None
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn shutdown_reports_terminal_storage_failure_and_retains_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().into(), identity.startup_id).unwrap();
    runtime
        .accept(envelope.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    let journal = directory.path().join("journal.json");
    let saved = directory.path().join("journal.saved");
    std::fs::rename(&journal, &saved).unwrap();
    std::fs::create_dir(&journal).unwrap();
    assert!(runtime.shutdown(Duration::from_millis(100)).await.is_err());
    assert!(
        runtime.active().await,
        "failed terminal write cannot claim drained"
    );
    runtime.resume().await;
    assert!(
        runtime
            .start(
                envelope.operation_id,
                crate::config::Config::default(),
                None
            )
            .await
            .is_err()
    );
    std::fs::remove_dir(&journal).unwrap();
    std::fs::rename(&saved, &journal).unwrap();
    runtime.shutdown(Duration::from_secs(1)).await.unwrap();
    assert_eq!(
        runtime.status(envelope.operation_id).await.unwrap().state,
        OperationState::Canceled
    );
}

#[cfg(unix)]
#[tokio::test]
async fn running_cancel_waits_for_child_cleanup_and_redacts_tool_output() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, envelope, clock) = fixture();
    let tool = directory.path().join("git");
    crate::config::private_write(&tool,b"#!/bin/sh\necho secret-token-123 version 2.40.1\nsleep 30 &\necho $! > \"$0.child\"\nwait\n",true,true).unwrap();
    let mut cfg = crate::config::Config::default();
    cfg.tools
        .insert("git".into(), tool.to_string_lossy().into_owned());
    let runtime = Runtime::open(directory.path().join("journal"), identity.startup_id).unwrap();
    runtime
        .accept(envelope.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    runtime
        .start(envelope.operation_id, cfg, None)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !tool.with_extension("child").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let cancel = OperationCancel {
        operation_id: envelope.operation_id,
        startup_id: identity.startup_id,
        control_session: identity.control_session,
    };
    let cancel_update = runtime.cancel(&cancel, &identity).await.unwrap();
    assert_eq!(cancel_update.state, OperationState::Running);
    let terminal = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let update = runtime.updates().await.pop().unwrap();
            if update.state.terminal() {
                break update;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(terminal.state, OperationState::Canceled);
    assert!(
        !serde_json::to_string(&terminal)
            .unwrap()
            .contains("secret-token")
    );
    let pid = std::fs::read_to_string(tool.with_extension("child")).unwrap();
    let status = std::process::Command::new("kill")
        .args(["-0", pid.trim()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "canceled terminal must follow child cleanup"
    );
}

#[tokio::test]
async fn same_startup_reauthorization_does_not_reset_accepted_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, mut envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    runtime
        .accept(envelope.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    let mut new_identity = identity.clone();
    new_identity.control_session = Uuid::new_v4();
    envelope.control_session = new_identity.control_session;
    let replay = runtime
        .accept(envelope.clone(), &new_identity, None)
        .await
        .unwrap();
    assert_eq!(replay.sequence, 1);
    envelope.expires_at_ms += 30_000;
    envelope.issued_at_ms += 30_000;
    assert_eq!(
        runtime
            .accept(envelope, &new_identity, None)
            .await
            .unwrap_err(),
        OperationReason::InputConflict
    );
}
#[tokio::test]
async fn cannot_evict_unacknowledged_results_to_admit_more_operations() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, mut envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    for _ in 0..64 {
        envelope.operation_id = Uuid::new_v4();
        runtime
            .accept(envelope.clone(), &identity, Some(&clock))
            .await
            .unwrap();
        runtime
            .cancel(
                &OperationCancel {
                    operation_id: envelope.operation_id,
                    startup_id: identity.startup_id,
                    control_session: identity.control_session,
                },
                &identity,
            )
            .await
            .unwrap();
    }
    envelope.operation_id = Uuid::new_v4();
    assert_eq!(
        runtime
            .accept(envelope, &identity, Some(&clock))
            .await
            .unwrap_err(),
        OperationReason::JournalFull
    );
    assert_eq!(runtime.updates().await.len(), 64);
}

#[tokio::test]
async fn restart_result_requires_resume_bound_to_current_startup_and_digest() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    runtime
        .accept(envelope.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    drop(runtime);
    let mut current = identity.clone();
    current.startup_id = Uuid::new_v4();
    current.control_session = Uuid::new_v4();
    let runtime = Runtime::open(directory.path().to_owned(), current.startup_id).unwrap();
    let mut resume = OperationResume {
        operation_id: envelope.operation_id,
        tenant_id: identity.tenant_id,
        node_id: identity.node_id,
        original_startup_id: identity.startup_id,
        current_startup_id: current.startup_id,
        control_session: current.control_session,
        input_digest: "0".repeat(64),
    };
    assert!(runtime.reauthorize(&resume, &current).await.is_err());
    resume.input_digest = envelope.input_digest;
    let update = runtime.reauthorize(&resume, &current).await.unwrap();
    assert_eq!(update.state, OperationState::Interrupted);
    assert_eq!(update.startup_id, identity.startup_id);
    assert!(
        runtime
            .start(
                envelope.operation_id,
                crate::config::Config::default(),
                None
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn persistence_failure_rejects_without_an_in_memory_acceptance() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, envelope, clock) = fixture();
    let runtime = Runtime::open(directory.path().to_owned(), identity.startup_id).unwrap();
    let journal = directory.path().join("journal.json");
    std::fs::remove_file(&journal).unwrap();
    std::fs::create_dir(&journal).unwrap();
    assert_eq!(
        runtime
            .accept(envelope, &identity, Some(&clock))
            .await
            .unwrap_err(),
        OperationReason::StorageUnavailable
    );
    assert!(runtime.updates().await.is_empty());
    assert!(!runtime.active().await);
}
#[cfg(unix)]
#[tokio::test]
async fn full_refresh_succeeds_only_after_durable_sink_returns_observation() {
    let directory = tempfile::tempdir().unwrap();
    let (identity, mut envelope, clock) = fixture();
    envelope.kind = OperationKind::RefreshInventory;
    envelope.input = json!({"mode":"full"});
    envelope.input_digest = validate_input(envelope.kind, 1, &envelope.input)
        .unwrap()
        .digest();
    let mut cfg = crate::config::Config::default();
    for id in PROBE_IDS {
        cfg.tools.insert(
            (*id).into(),
            directory
                .path()
                .join("missing")
                .to_string_lossy()
                .into_owned(),
        );
    }
    cfg.tools.insert(
        "nvidia-smi".into(),
        directory
            .path()
            .join("missing")
            .to_string_lossy()
            .into_owned(),
    );
    let observation = Uuid::new_v4();
    let sink: InventorySink = Arc::new(move |inventory| {
        Box::pin(async move {
            assert_eq!(inventory.tools.len(), PROBE_IDS.len());
            Ok(InventoryRefresh {
                observation_id: observation,
                revision: 1,
                delivery: InventoryDelivery::Pending,
            })
        })
    });
    let runtime = Runtime::open(directory.path().join("journal"), identity.startup_id).unwrap();
    runtime
        .accept(envelope.clone(), &identity, Some(&clock))
        .await
        .unwrap();
    runtime
        .start(envelope.operation_id, cfg, Some(sink))
        .await
        .unwrap();
    let update = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let update = runtime.updates().await.pop().unwrap();
            if update.state.terminal() {
                break update;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(update.state, OperationState::Succeeded);
    let refresh = update.result.unwrap().inventory.unwrap();
    assert_eq!(refresh.observation_id, observation);
    assert_eq!(refresh.delivery, InventoryDelivery::Pending);
}
