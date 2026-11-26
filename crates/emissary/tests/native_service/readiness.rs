//! [INPUT]: 真实mTLS连接、当前startup和有限操作接收期限。
//! [OUTPUT]: 等待已过期操作的明确拒收，确认操作日志已重开。
//! [POS]: 原生权限基线之前的就绪屏障，不启动探测或新增记录。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::support;
use artifactory_emissary::protocol as pb;
use artifactory_emissary_protocol::operations::*;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) async fn wait(connection: &mut support::Connection) {
    let hello: Value = serde_json::from_str(&connection.hello.payload_json).unwrap();
    let startup_id = hello["startup"]["startup_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let control_session = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    connection
        .commands
        .send(pb::Command {
            kind: "capabilities_ack".into(),
            signing_session_id: control_session.to_string(),
            payload_json: json!({"schema_version":1,"server_time_ms":now,
            "responses":["command_rejected","node_operation_update"]})
            .to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let input = validate_input(
        OperationKind::CollectDiagnostics,
        1,
        &json!({"probes":["git"]}),
    )
    .unwrap();
    let envelope = OperationEnvelope {
        operation_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        node_id: connection.hello.emissary_id.parse().unwrap(),
        startup_id,
        control_session,
        actor_id: Uuid::new_v4(),
        kind: input.kind(),
        schema: 1,
        input: input.value(),
        input_digest: input.digest(),
        issued_at_ms: now - 60_000,
        expires_at_ms: now - 30_000,
    };
    envelope.validate().unwrap();
    connection
        .commands
        .send(pb::Command {
            kind: "node_operation".into(),
            payload_json: serde_json::to_string(&envelope).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let event = connection.event("node_operation_update").await;
            let update: OperationUpdate = serde_json::from_str(&event.payload_json).unwrap();
            if update.operation_id == envelope.operation_id {
                update.validate().unwrap();
                assert_eq!(update.state, OperationState::Rejected);
                assert_eq!(update.reason, Some(OperationReason::ReceiveExpired));
                break;
            }
        }
    })
    .await
    .expect("operation journal readiness deadline exceeded");
}

#[tokio::test]
async fn expired_readiness_request_never_creates_a_journal_entry() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = support::Fixture::start(root.path(), &[&rustls::version::TLS13]).await;
    let cfg = fixture.cfg.clone();
    let journal = cfg.root.join("operations/journal.json");
    let stop = tokio_util::sync::CancellationToken::new();
    let token = stop.clone();
    let task = tokio::spawn(async move { artifactory_emissary::runner::run(cfg, token).await });
    let mut connection = fixture.connection().await;
    wait(&mut connection).await;
    let state: Value = serde_json::from_slice(&std::fs::read(journal).unwrap()).unwrap();
    assert!(state["entries"].as_array().unwrap().is_empty());
    stop.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(15), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
