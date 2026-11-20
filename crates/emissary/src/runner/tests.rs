//! [INPUT]: 依赖 Runner 状态夹具与节点协议元数据。
//! [OUTPUT]: 验证主机识别、完整 hello 预算、后台接口缓存与租约边界。
//! [POS]: Runner 状态机的本地回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use crate::config::read_json;
use std::path::Path;

#[path = "../../../emissary-protocol/tests/fixtures/heartbeat.rs"]
mod heartbeat_fixture;

#[test]
fn maximum_hello_and_same_startup_reconnect_pass_the_receiver_decoder() {
    use artifactory_emissary_protocol::nodes::{MAX_HEARTBEAT_BYTES, decode_heartbeat};
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.startup = Some(heartbeat_fixture::maximum_startup());
    state.diagnostics = json!({"hostname":"build-host.local","emissary_version":"0.2.0"});
    state.spool.bytes = u64::MAX;
    let network = heartbeat_fixture::maximum_network();
    state.telemetry = Some(crate::telemetry::cached_test_handle(
        crate::telemetry::Snapshot {
            metrics: heartbeat_fixture::metrics(),
            network_interfaces: Some(network.clone()),
        },
    ));
    state.ota_directory = Some(root.path().into());
    atomic_json(
        &root.path().join("state.json"),
        &heartbeat_fixture::ota_status(),
    )
    .unwrap();
    for _ in 0..32 {
        state.tasks.insert(
            uuid::Uuid::new_v4().to_string(),
            Task::new(
                "a".repeat(64),
                pb::Assignment::default(),
                90,
                Instant::now(),
            ),
        );
    }
    let startup = serde_json::to_value(state.startup.as_ref().unwrap()).unwrap();
    for kind in ["hello", "heartbeat", "hello"] {
        let event = state.heartbeat(kind);
        if kind == "hello" {
            assert!(
                event.payload_json.len() > 72 * 1024,
                "夹具必须触及旧上限缺陷"
            );
        }
        assert!(event.payload_json.len() <= MAX_HEARTBEAT_BYTES);
        let received =
            decode_heartbeat(&event.payload_json).expect("真实发送结果必须通过接收解码器");
        assert_eq!(received["active_attempts"].as_array().unwrap().len(), 32);
        assert_eq!(received["diagnostics"]["network_interfaces"], network);
        if kind == "hello" {
            assert_eq!(received["startup"], startup);
        } else {
            assert!(received["startup"].is_null());
        }
    }
}

#[tokio::test]
async fn doctor_hostname_reaches_every_heartbeat_and_startup_metadata() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.diagnostics = json!({"hostname":"build-host.local","emissary_version":"0.2.0"});
    state.spool.bytes = 17;
    let expected_model = sysinfo::Product::name().filter(|value| !value.trim().is_empty());
    let expected_vendor = sysinfo::Product::vendor_name().filter(|value| !value.trim().is_empty());
    let (snapshot, inventory) =
        crate::telemetry::startup(&state.cfg, uuid::Uuid::new_v4(), &state.diagnostics, vec![])
            .await
            .unwrap();
    for (key, expected) in [
        ("hardware.device_model", expected_model.map(Value::String)),
        ("hardware.device_vendor", expected_vendor.map(Value::String)),
        ("hardware.cpu_model", inventory.cpu_model.map(Value::String)),
        (
            "hardware.physical_memory_bytes",
            inventory.physical_memory_bytes.map(|value| json!(value)),
        ),
    ] {
        assert_eq!(snapshot.metadata.get(key), expected.as_ref(), "{key}");
    }
    state.startup = Some(snapshot);
    for kind in ["hello", "heartbeat", "hello", "heartbeat"] {
        let payload: Value = serde_json::from_str(&state.heartbeat(kind).payload_json).unwrap();
        assert_eq!(payload["diagnostics"]["hostname"], "build-host.local");
        assert_eq!(payload["diagnostics"]["emissary_version"], "0.2.0");
        assert_eq!(payload["diagnostics"]["metrics"], json!({}));
        assert_eq!(payload["diagnostics"]["buffered_log_bytes"], 17);
        if kind == "hello" {
            assert_eq!(
                payload["startup"]["metadata"]["system.hostname"],
                "build-host.local"
            );
        }
    }
}

#[tokio::test]
async fn background_network_snapshot_reaches_hello_and_every_heartbeat() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.telemetry = Some(crate::telemetry::Handle::start(
        state.cfg.clone(),
        uuid::Uuid::new_v4(),
        Default::default(),
    ));
    let cached = tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            if let Some(snapshot) = state
                .telemetry
                .as_ref()
                .and_then(|handle| handle.snapshot())
                && let Some(network) = snapshot.network_interfaces
            {
                break network;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("后台 Collector 必须发布接口快照");
    assert!(
        cached["sampled_at"]
            .as_str()
            .unwrap()
            .parse::<chrono::DateTime<chrono::Utc>>()
            .is_ok()
    );
    assert!(cached["interfaces"].is_array());
    for kind in ["hello", "heartbeat", "hello", "heartbeat"] {
        let payload: Value = serde_json::from_str(&state.heartbeat(kind).payload_json).unwrap();
        assert_eq!(payload["diagnostics"]["network_interfaces"], cached);
        assert!(payload["diagnostics"]["metrics"]["logical_cpus"].is_number());
    }
}

#[test]
fn reconnect_reuses_startup_metadata_and_ordinary_heartbeats_exclude_static_data() {
    use artifactory_emissary_protocol::nodes::MetadataSnapshot;
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    let startup = uuid::Uuid::new_v4();
    state.startup = Some(
        MetadataSnapshot::new(
            startup,
            [
                (
                    "tools.cargo".into(),
                    json!({"available":true,"version":"1.91"}),
                ),
                ("system.hostname".into(), json!("build-host.local")),
                ("hardware.device_model".into(), json!("Mac16,7")),
                ("hardware.device_vendor".into(), json!("Apple Inc.")),
            ]
            .into(),
            vec!["cargo".into()],
        )
        .unwrap(),
    );
    let first: Value = serde_json::from_str(&state.heartbeat("hello").payload_json).unwrap();
    state.diagnostics["tools"] = json!({"cargo":{"version":"changed after startup"}});
    state.diagnostics["hostname"] = json!("changed-host");
    let reconnect: Value = serde_json::from_str(&state.heartbeat("hello").payload_json).unwrap();
    assert_eq!(first["startup"], reconnect["startup"]);
    let heartbeat: Value =
        serde_json::from_str(&state.heartbeat("heartbeat").payload_json).unwrap();
    assert!(heartbeat["startup"].is_null());
    assert!(heartbeat.get("capabilities").is_none());
    assert!(heartbeat["diagnostics"].get("tools").is_none());
    state
        .command(
            pb::Command {
                kind: "metadata_ack".into(),
                request_id: uuid::Uuid::new_v4().to_string(),
                ..Default::default()
            },
            Instant::now(),
        )
        .unwrap();
    assert!(!state.metadata_acked);
    state
        .command(
            pb::Command {
                kind: "metadata_ack".into(),
                request_id: startup.to_string(),
                ..Default::default()
            },
            Instant::now(),
        )
        .unwrap();
    assert!(state.metadata_acked);
}

#[test]
fn bounded_spool_does_not_skip_sequence_on_overflow() {
    let root = tempfile::tempdir().unwrap();
    let mut spool = Spool::new(root.path().into(), 1).unwrap();
    let mut sequence = 0;
    assert!(
        spool
            .append("attempt", &mut sequence, "test", b"data")
            .is_err()
    );
    assert_eq!((sequence, spool.bytes), (0, 0));
    spool.limit = 4096;
    spool
        .append("attempt", &mut sequence, "test", b"data")
        .unwrap();
    assert!(
        root.path()
            .join("spool/attempt/00000000000000000001.json")
            .exists()
    );
}
#[test]
fn duplicate_assignment_and_delayed_renewal_do_not_extend_lease() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    let now = Instant::now();
    let id = uuid::Uuid::new_v4().to_string();
    let assign = pb::Command {
        kind: "assign".into(),
        attempt_id: id.clone(),
        lease_token: "lease".into(),
        lease_seconds: 90,
        assignment: Some(pb::Assignment::default()),
        ..Default::default()
    };
    state.command(assign.clone(), now).unwrap();
    let deadline = state.tasks[&id].lease.deadline();
    state.command(assign, now + Duration::from_secs(5)).unwrap();
    assert_eq!(state.tasks[&id].lease.deadline(), deadline);
    state.tasks.get_mut(&id).unwrap().started = true;
    state.anchors.insert("request".into(), now);
    state
        .command(
            pb::Command {
                kind: "renew".into(),
                attempt_id: id.clone(),
                lease_token: "lease".into(),
                request_id: "request".into(),
                lease_seconds: 30,
                ..Default::default()
            },
            now + Duration::from_secs(15),
        )
        .unwrap();
    assert_eq!(
        state.tasks[&id].lease.deadline(),
        now + Duration::from_secs(30)
    );
    state
        .command(
            pb::Command {
                kind: "renew".into(),
                attempt_id: id.clone(),
                lease_token: "lease".into(),
                request_id: "request".into(),
                lease_seconds: 90,
                ..Default::default()
            },
            now + Duration::from_secs(31),
        )
        .unwrap();
    assert!(!state.tasks.contains_key(&id));
}
#[test]
fn ota_drain_waits_for_terminal_ack_workers_and_logs() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    atomic_json(
        &root.path().join("state.json"),
        &ota::State {
            update_id: "u".into(),
            phase: "draining".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    state.tasks.insert(
        id.clone(),
        Task::new(
            "lease".into(),
            pb::Assignment::default(),
            90,
            Instant::now(),
        ),
    );
    state.workers = 1;
    state.spool.bytes = 5;
    assert!(!state.ota_snapshot("ready").unwrap().activation_pending);
    state.tasks.remove(&id);
    assert!(!state.ota_snapshot("ready").unwrap().activation_pending);
    state.workers = 0;
    assert!(!state.ota_snapshot("ready").unwrap().activation_pending);
    state.spool.bytes = 0;
    assert!(state.ota_snapshot("ready").unwrap().activation_pending);
    assert!(!root.path().join("drained.json").exists());
    state.anchors.insert("ready".into(), Instant::now());
    state
        .command(
            pb::Command {
                kind: "emissary_health".into(),
                request_id: "ready".into(),
                ..Default::default()
            },
            Instant::now(),
        )
        .unwrap();
    assert!(root.path().join("drained.json").exists());
}
#[test]
fn ota_health_requires_current_outstanding_heartbeat() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    let now = Instant::now();
    state.anchors.insert("fresh".into(), now);
    state
        .anchors
        .insert("expired".into(), now - Duration::from_secs(60));
    for id in ["forged", "expired"] {
        state
            .command(
                pb::Command {
                    kind: "emissary_health".into(),
                    request_id: id.into(),
                    ..Default::default()
                },
                now,
            )
            .unwrap();
    }
    assert!(!root.path().join("health.json").exists());
    state
        .command(
            pb::Command {
                kind: "emissary_health".into(),
                request_id: "fresh".into(),
                ..Default::default()
            },
            now,
        )
        .unwrap();
    assert_eq!(
        read_json::<ota::Health>(&root.path().join("health.json"))
            .unwrap()
            .id,
        "fresh"
    );
    assert_eq!(state.health_at, Some(now));
}
fn test_state(root: &Path) -> State {
    State::new(
        Config {
            root: root.into(),
            log_buffer_bytes: 4096,
            ..Default::default()
        },
        json!({}),
        vec![],
    )
    .unwrap()
}
