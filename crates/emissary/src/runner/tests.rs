//! [INPUT]: 依赖 Runner 状态夹具与节点协议元数据。
//! [OUTPUT]: 验证主机识别、组合诊断、OTA邮箱与健康保存故障及租约边界。
//! [POS]: Runner 状态机的本地回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use crate::config::read_json;
use std::path::Path;

#[path = "../../../emissary-protocol/tests/fixtures/heartbeat.rs"]
mod heartbeat_fixture;

#[test]
fn ignored_notification_count_reaches_heartbeat_from_memory_and_survives_reconnect() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    let notification = pb::Command {
        kind: "future_notice".into(),
        ignorable_notification: true,
        ..Default::default()
    };
    assert!(matches!(
        state.command_gate.admit(&notification, "node"),
        command_gate::Admission::Ignored
    ));
    state.command_gate.reset(Instant::now());
    let heartbeat: Value =
        serde_json::from_str(&state.heartbeat("heartbeat").payload_json).unwrap();
    assert_eq!(heartbeat["diagnostics"]["command_notifications_ignored"], 1);
    assert!(heartbeat["active_attempts"].as_array().unwrap().is_empty());
    assert!(state.telemetry.is_none(), "计数不依赖磁盘或遥测采样");
}

#[test]
fn confirmed_connection_emits_invalid_terminal_payload_rejection_before_execution() {
    use artifactory_emissary_protocol::commands::{CommandRejected, RejectionCode};
    let mut gate = command_gate::Gate::default();
    let ack = pb::Command {
        kind: "capabilities_ack".into(),
        payload_json:
            json!({"schema_version":1,"server_time_ms":1,"responses":["command_rejected"]})
                .to_string(),
        ..Default::default()
    };
    assert!(matches!(
        gate.admit(&ack, "node"),
        command_gate::Admission::Consumed
    ));
    let invalid = pb::Command {
        kind:"terminal_open".into(),command_id:"invalid-window".into(),
        payload_json:json!({
            "terminal_id":uuid::Uuid::new_v4(),"node_session":uuid::Uuid::new_v4(),
            "startup_id":uuid::Uuid::new_v4(),"expires_at_ms":chrono::Utc::now().timestamp_millis()+5_000,
            "columns":0,"rows":24,"term":"xterm-256color","profile":"default"
        }).to_string(),
        ..Default::default()
    };
    let command_gate::Admission::Rejected(Some(event)) = gate.admit(&invalid, "node") else {
        panic!("invalid dimensions must not proceed to the terminal executor");
    };
    assert_eq!(event.kind, "command_rejected");
    assert_eq!(event.command_id, "invalid-window");
    let rejected: CommandRejected = serde_json::from_str(&event.payload_json).unwrap();
    assert_eq!(rejected.code, RejectionCode::InvalidPayload);
    assert_eq!(rejected.kind, "terminal_open");
    assert!(event.attempt_id.is_empty());
}

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
    state.ota_cached = ota_cache::read_fixture(root.path());
    state.ota_cached.observation = Some(heartbeat_fixture::ota_observability());
    let ack = pb::Command {
        kind: "capabilities_ack".into(),
        payload_json: json!({"schema_version":1,"server_time_ms":1,
            "responses":["command_rejected",artifactory_emissary_protocol::ota::CAPABILITY]})
        .to_string(),
        ..Default::default()
    };
    assert!(matches!(
        state.command_gate.admit(&ack, "node"),
        command_gate::Admission::Consumed
    ));
    state.command_gate.ignored_notifications = u64::MAX;
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
        assert_eq!(
            received["diagnostics"]["command_notifications_ignored"],
            u64::MAX
        );
        let delivery = &received["diagnostics"]["metrics"]["telemetry"];
        assert_eq!(serde_json::to_vec(delivery).unwrap().len(), 4096);
        assert_eq!(delivery, &heartbeat_fixture::maximum_delivery_diagnostics());
        assert_eq!(
            received["ota"]["observability"]["drain"]["waiting_ids"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        assert_eq!(
            received["ota"]["observability"]["progress"]["bytes_total"],
            128 << 20
        );
        if kind == "hello" {
            assert_eq!(received["startup"], startup);
        } else {
            assert!(received["startup"].is_null());
        }
        let mut oversized = event.payload_json;
        oversized.push_str(&" ".repeat(MAX_HEARTBEAT_BYTES - oversized.len() + 1));
        assert!(
            decode_heartbeat(&oversized).is_err(),
            "完整信封越界先于JSON解析拒绝"
        );
    }
}

#[test]
fn excessive_delivery_projection_falls_back_without_losing_other_heartbeat_data() {
    use artifactory_emissary_protocol::nodes::decode_heartbeat;
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    let mut metrics = heartbeat_fixture::metrics();
    metrics["telemetry"]["spool_bytes"] = json!("x".repeat(128));
    state.telemetry = Some(crate::telemetry::cached_test_handle(
        crate::telemetry::Snapshot {
            metrics,
            network_interfaces: Some(heartbeat_fixture::maximum_network()),
        },
    ));
    let event = state.heartbeat("hello");
    let received = decode_heartbeat(&event.payload_json).unwrap();
    assert_eq!(
        received["diagnostics"]["metrics"]["telemetry"],
        json!({"state":"diagnostics_bounds_exceeded"})
    );
    assert_eq!(received["diagnostics"]["metrics"]["logical_cpus"], 64);
    assert_eq!(
        received["diagnostics"]["network_interfaces"]["interfaces"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
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
    state.ota_cached = ota_cache::read_fixture(root.path());
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

#[test]
fn ota_health_write_failure_keeps_runtime_and_allows_same_ack_after_recovery() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    let now = Instant::now();
    state.anchors.insert("fresh".into(), now);
    fs::create_dir(root.path().join("health.json")).unwrap();
    let command = pb::Command {
        kind: "emissary_health".into(),
        request_id: "fresh".into(),
        ..Default::default()
    };
    assert!(
        state.command(command.clone(), now).is_ok(),
        "health persistence failure must not terminate runtime"
    );
    assert!(state.health_at.is_none(), "failed write consumed the ACK");
    assert!(!state.stopping);
    fs::remove_dir(root.path().join("health.json")).unwrap();
    state.command(command, now).unwrap();
    let health: ota::Health = read_json(&root.path().join("health.json")).unwrap();
    assert_eq!(health.id, "fresh");
    assert_eq!(state.health_at, Some(now));
}

#[test]
fn ota_drain_write_failure_keeps_runtime_and_does_not_consume_health_ack() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    atomic_json(
        &root.path().join("state.json"),
        &ota::State {
            update_id: "update".into(),
            phase: "draining".into(),
            ..Default::default()
        },
    )
    .unwrap();
    state.ota_cached = ota_cache::read_fixture(root.path());
    assert!(state.ota_snapshot("ready").unwrap().activation_pending);
    let now = Instant::now();
    state.anchors.insert("ready".into(), now);
    fs::create_dir(root.path().join("drained.json")).unwrap();
    let command = pb::Command {
        kind: "emissary_health".into(),
        request_id: "ready".into(),
        ..Default::default()
    };
    assert!(
        state.command(command.clone(), now).is_ok(),
        "drain persistence failure must not terminate runtime"
    );
    assert!(state.health_at.is_none());
    assert!(!root.path().join("health.json").exists());
    assert!(!state.stopping);
    fs::remove_dir(root.path().join("drained.json")).unwrap();
    let original = fs::read(root.path().join("state.json")).unwrap();
    fs::remove_file(root.path().join("state.json")).unwrap();
    fs::create_dir(root.path().join("state.json")).unwrap();
    state.command(command.clone(), now).unwrap();
    assert!(state.health_at.is_none());
    assert!(!root.path().join("drained.json").exists());
    fs::remove_dir(root.path().join("state.json")).unwrap();
    fs::write(root.path().join("state.json"), original).unwrap();
    state.command(command, now).unwrap();
    assert_eq!(
        read_json::<Value>(&root.path().join("drained.json")).unwrap()["id"],
        "update"
    );
    assert!(root.path().join("health.json").is_file());
    assert_eq!(state.health_at, Some(now));
}

#[test]
fn ota_reconnect_waits_for_durable_health_reset_before_changing_session() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    let previous = state.ota_session.clone();
    let now = Instant::now();
    state.health_at = Some(now);
    fs::create_dir(root.path().join("health.json")).unwrap();
    assert!(!state.reset_ota_session("next"));
    assert_eq!(state.ota_session, previous);
    assert_eq!(state.health_at, Some(now));
    fs::remove_dir(root.path().join("health.json")).unwrap();
    assert!(state.reset_ota_session("next"));
    let health: ota::Health = read_json(&root.path().join("health.json")).unwrap();
    assert_eq!(health.session, "next");
    assert!(health.id.is_empty());
    assert_eq!(state.ota_session, "next");
    assert!(state.health_at.is_none());
}
#[test]
fn terminal_admission_resumes_after_ota_cancel_or_failure() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    for (phase, expected) in [
        ("downloading", true),
        ("draining", true),
        ("activating", true),
        ("canceled", false),
        ("failed", false),
        ("rolled_back", false),
    ] {
        atomic_json(
            &root.path().join("state.json"),
            &ota::State {
                phase: phase.into(),
                ..Default::default()
            },
        )
        .unwrap();
        state.ota_cached = ota_cache::read_fixture(root.path());
        assert_eq!(state.terminal_draining(), expected, "{phase}");
    }
    state.stopping = true;
    assert!(state.terminal_draining());
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

#[test]
fn ordinary_ota_heartbeat_reads_cache_and_stale_cache_closes_admission() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    atomic_json(
        &root.path().join("state.json"),
        &ota::State {
            phase: "failed".into(),
            ..Default::default()
        },
    )
    .unwrap();
    state.ota_cached = ota_cache::read_fixture(root.path());
    fs::remove_file(root.path().join("state.json")).unwrap();
    let event = state.heartbeat("heartbeat");
    let payload: Value = serde_json::from_str(&event.payload_json).unwrap();
    assert_eq!(payload["ota"]["phase"], "failed");
    assert!(!state.terminal_draining());
    state.ota_cached.sampled = Some(Instant::now() - Duration::from_secs(3));
    assert!(state.terminal_draining());
    assert!(state.ota_snapshot("stale").is_none());
}

#[test]
fn ota_mailbox_lock_contention_does_not_fail_the_runtime_command() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    state.ota_cached = ota_cache::read_fixture(root.path());
    let original = ota::Request {
        id: "original".into(),
        generation: 1,
        action: "apply".into(),
        ..Default::default()
    };
    ota::submit(root.path(), &original).unwrap();
    let bytes = fs::read(root.path().join("request.json")).unwrap();
    let lock = fs::File::create(root.path().join("request.lock")).unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    let request = ota::Request {
        id: "next".into(),
        generation: 2,
        action: "apply".into(),
        ..Default::default()
    };
    let result = state.command(
        pb::Command {
            kind: "emissary_update".into(),
            payload_json: serde_json::to_string(&request).unwrap(),
            ..Default::default()
        },
        Instant::now(),
    );
    assert!(
        result.is_ok(),
        "mailbox contention terminated runtime: {:?}",
        result.err()
    );
    assert_eq!(fs::read(root.path().join("request.json")).unwrap(), bytes);
    fs2::FileExt::unlock(&lock).unwrap();
    state.retry_pending_ota(Instant::now() + Duration::from_secs(2));
    let stored: ota::Request = read_json(&root.path().join("request.json")).unwrap();
    assert!(stored == request);
    assert!(state.pending_ota.is_none());
}

#[test]
fn ota_mailbox_retry_keeps_newer_generation_and_same_generation_cancel() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    state.ota_cached = ota_cache::read_fixture(root.path());
    let lock = fs::File::create(root.path().join("request.lock")).unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    for (id, generation, action) in [
        ("old", 1, "apply"),
        ("latest", 2, "apply"),
        ("old", 1, "cancel"),
        ("latest", 2, "cancel"),
        ("latest", 2, "apply"),
    ] {
        let request = ota::Request {
            id: id.into(),
            generation,
            action: action.into(),
            ..Default::default()
        };
        state
            .command(
                pb::Command {
                    kind: "emissary_update".into(),
                    payload_json: serde_json::to_string(&request).unwrap(),
                    ..Default::default()
                },
                Instant::now(),
            )
            .unwrap();
    }
    assert!(!root.path().join("request.json").exists());
    fs2::FileExt::unlock(&lock).unwrap();
    state.retry_pending_ota(Instant::now() + Duration::from_secs(2));
    let stored: ota::Request = read_json(&root.path().join("request.json")).unwrap();
    assert_eq!(stored.id, "latest");
    assert_eq!(stored.generation, 2);
    assert_eq!(stored.action, "cancel");
}

#[test]
fn ota_request_waits_for_fresh_cache_without_stopping_runtime() {
    let root = tempfile::tempdir().unwrap();
    let mut state = test_state(root.path());
    state.ota_directory = Some(root.path().into());
    let request = ota::Request {
        id: "deferred".into(),
        generation: 1,
        action: "cancel".into(),
        ..Default::default()
    };
    assert!(
        state
            .command(
                pb::Command {
                    kind: "emissary_update".into(),
                    payload_json: serde_json::to_string(&request).unwrap(),
                    ..Default::default()
                },
                Instant::now()
            )
            .is_ok()
    );
    assert!(!root.path().join("request.json").exists());
    atomic_json(&root.path().join("state.json"), &ota::State::default()).unwrap();
    state.ota_cached = ota_cache::read_fixture(root.path());
    state.retry_pending_ota(Instant::now());
    let stored: ota::Request = read_json(&root.path().join("request.json")).unwrap();
    assert!(stored == request);
}
