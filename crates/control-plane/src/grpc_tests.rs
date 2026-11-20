//! [INPUT]: 依赖真实 gRPC 心跳接收校验与节点启动契约。
//! [OUTPUT]: 验证合法组合 hello、重连与完整事件的字节边界。
//! [POS]: grpc.rs 的本地接收回归，不连接数据库。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_emissary_protocol::nodes::MAX_HEARTBEAT_BYTES;
#[path = "../../emissary-protocol/tests/fixtures/heartbeat.rs"]
mod fixture;

#[test]
fn maximum_startup_network_and_active_attempts_are_accepted_on_reconnect() {
    let startup = fixture::maximum_startup();
    let network = fixture::maximum_network();
    let payload = json!({
        "startup": startup,
        "active_attempts": (0..32).map(|_| json!({
            "attempt_id":Uuid::new_v4().to_string(),
            "lease_token":crate::crypto::token()
        })).collect::<Vec<_>>(),
        "diagnostics": {
            "hostname":"build-host.local", "emissary_version":"0.2.0",
            "network_interfaces":network,
            "metrics":fixture::metrics(),
            "buffered_log_bytes":u64::MAX
        },
        "ota":fixture::ota_status()
    });
    let event = EmissaryEvent {
        kind: "hello".into(),
        payload_json: payload.to_string(),
        ..Default::default()
    };
    assert!(event.payload_json.len() > 72 * 1024);
    let first = heartbeat_payload(&event).expect("满额合法组合必须通过真实接收校验");
    let reconnect = heartbeat_payload(&event).expect("同一启动的重连不能永久拒绝");
    assert_eq!(first["startup"], reconnect["startup"]);
    assert_eq!(first["active_attempts"].as_array().unwrap().len(), 32);
    assert_eq!(first["diagnostics"]["network_interfaces"], network);
}

#[test]
fn full_event_budget_accepts_exact_boundary_and_rejects_one_extra_byte() {
    for kind in ["hello", "heartbeat"] {
        let mut event = EmissaryEvent {
            kind: kind.into(),
            payload_json: json!({"padding":""}).to_string(),
            ..Default::default()
        };
        event.payload_json =
            json!({"padding":"x".repeat(MAX_HEARTBEAT_BYTES - event.payload_json.len())})
                .to_string();
        assert_eq!(event.payload_json.len(), MAX_HEARTBEAT_BYTES);
        heartbeat_payload(&event).expect("完整信封预算边界必须可解码");
        event.payload_json.push(' ');
        let error = heartbeat_payload(&event).unwrap_err();
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert_eq!(error.message(), "oversized heartbeat");
    }
}

#[test]
fn heartbeat_retains_invalid_json_and_legacy_capabilities_rejection() {
    for payload_json in [
        "[".to_owned(),
        json!({"capabilities":{}}).to_string(),
        json!({"capabilities":vec!["git";257]}).to_string(),
    ] {
        let event = EmissaryEvent {
            payload_json,
            ..Default::default()
        };
        assert_eq!(
            heartbeat_payload(&event).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }
}
