//! [INPUT]: 依赖真实Runner、mTLS命令流和隔离工具探针。
//! [OUTPUT]: 验证终端结构拒收、重复操作不重跑、结果摘要和停止。
//! [POS]: 有限操作从协议入口到本地日志的集成回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod support;
#[cfg(unix)]
#[tokio::test]
async fn real_runner_only_executes_once_and_replays_terminal_after_duplicate_command() {
    use artifactory_emissary::{config, protocol as pb, runner};
    use artifactory_emissary_protocol::operations::*;
    use serde_json::json;
    use std::{fs, time::Duration};
    use uuid::Uuid;
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
    let tool = directory.path().join("probe-git");
    config::private_write(
        &tool,
        b"#!/bin/sh\nprintf x >> \"$0.count\"\nprintf 'git version 2.40.1\\n'\n",
        true,
        true,
    )
    .unwrap();
    fixture
        .cfg
        .tools
        .insert("git".into(), tool.to_string_lossy().into_owned());
    let cfg = fixture.cfg.clone();
    let cancel = tokio_util::sync::CancellationToken::new();
    let token = cancel.clone();
    let task = tokio::spawn(async move { runner::run(cfg, token).await });
    let mut connection = fixture.connection().await;
    let hello: serde_json::Value = serde_json::from_str(&connection.hello.payload_json).unwrap();
    let startup: Uuid = hello["startup"]["startup_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let session = Uuid::new_v4();
    connection.commands.send(pb::Command{kind:"capabilities_ack".into(),signing_session_id:session.to_string(),payload_json:json!({"schema_version":1,"server_time_ms":chrono::Utc::now().timestamp_millis(),"responses":["command_rejected","node_operation_update"]}).to_string(),..Default::default()}).await.unwrap();
    connection
        .commands
        .send(pb::Command {
            kind: "future_action".into(),
            command_id: "unknown-id".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        connection.event("command_rejected").await.command_id,
        "unknown-id"
    );
    let invalid_terminal = artifactory_emissary_protocol::terminal::OpenTerminal {
        terminal_id: Uuid::new_v4(),
        startup_id: startup,
        node_session: session,
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 5_000,
        columns: 0,
        rows: 24,
        profile: "default".into(),
        term: "xterm-256color".into(),
    };
    connection
        .commands
        .send(pb::Command {
            kind: "terminal_open".into(),
            command_id: "invalid-terminal".into(),
            payload_json: serde_json::to_string(&invalid_terminal).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    let rejected = connection.event("command_rejected").await;
    assert_eq!(rejected.command_id, "invalid-terminal");
    let reason: serde_json::Value = serde_json::from_str(&rejected.payload_json).unwrap();
    assert_eq!(reason["code"], "invalid_payload");
    let baseline = fs::read(tool.with_extension("count")).unwrap().len();
    let input = validate_input(
        OperationKind::CollectDiagnostics,
        1,
        &json!({"probes":["git"]}),
    )
    .unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let envelope = OperationEnvelope {
        operation_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        node_id: fixture.cfg.emissary_id.parse().unwrap(),
        startup_id: startup,
        control_session: session,
        actor_id: Uuid::new_v4(),
        kind: input.kind(),
        schema: 1,
        input: input.value(),
        input_digest: input.digest(),
        issued_at_ms: now,
        expires_at_ms: now + 30_000,
    };
    let command = pb::Command {
        kind: "node_operation".into(),
        payload_json: serde_json::to_string(&envelope).unwrap(),
        ..Default::default()
    };
    for _ in 0..2 {
        connection.commands.send(command.clone()).await.unwrap();
    }
    let terminal = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = connection.event("node_operation_update").await;
            let update: OperationUpdate = serde_json::from_str(&event.payload_json).unwrap();
            if update.state.terminal() {
                break update;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(terminal.state, OperationState::Succeeded);
    assert_eq!(
        terminal.result.as_ref().unwrap().probes["git"]
            .version
            .as_deref(),
        Some("2.40.1")
    );
    connection.commands.send(command).await.unwrap();
    loop {
        let event = connection.event("node_operation_update").await;
        let update: OperationUpdate = serde_json::from_str(&event.payload_json).unwrap();
        if update.state.terminal() {
            assert_eq!(update.result_digest, terminal.result_digest);
            break;
        }
    }
    assert_eq!(
        fs::read(tool.with_extension("count")).unwrap().len(),
        baseline + 1
    );
    let ack = OperationAck {
        operation_id: envelope.operation_id,
        result_digest: terminal.result_digest.unwrap(),
    };
    connection
        .commands
        .send(pb::Command {
            kind: "node_operation_result_ack".into(),
            payload_json: serde_json::to_string(&ack).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
}
