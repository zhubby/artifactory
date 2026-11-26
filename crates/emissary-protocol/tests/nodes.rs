//! [INPUT]: 依赖公开节点元数据、资源和选机契约。
//! [OUTPUT]: 验证名称字节边界、快照和选机输入。
//! [POS]: 节点协议的行为回归；阻止过期资源进入分配。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::nodes::{
    MetadataSnapshot, Placement, ResourceRequirements, Resources, normalize_node_name,
};
use chrono::{Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn node_names_trim_and_enforce_utf8_bytes_and_visible_characters() {
    assert_eq!(normalize_node_name("  实验室节点  ").unwrap(), "实验室节点");
    assert_eq!(normalize_node_name(&"x".repeat(100)).unwrap().len(), 100);
    assert_eq!(
        normalize_node_name(&format!("{}a", "中".repeat(33)))
            .unwrap()
            .len(),
        100
    );
    for invalid in [
        "".into(),
        "  ".into(),
        "x".repeat(101),
        "中".repeat(34),
        "node\nname".into(),
        "node\tname".into(),
        "node\0name".into(),
        "node\u{7f}name".into(),
    ] {
        assert!(normalize_node_name(&invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn metadata_is_canonical_bounded_and_namespaced() {
    let id = Uuid::new_v4();
    let snapshot = MetadataSnapshot::new(
        id,
        BTreeMap::from([("system.os".into(), json!("linux"))]),
        vec!["git".into()],
    )
    .unwrap();
    assert_eq!(
        snapshot.digest().unwrap(),
        snapshot.clone().digest().unwrap()
    );
    assert!(
        MetadataSnapshot::new(
            id,
            BTreeMap::from([("labels.admin".into(), json!(true))]),
            vec![]
        )
        .is_err()
    );
    assert!(
        MetadataSnapshot::new(
            id,
            BTreeMap::from([("custom.large".into(), json!("x".repeat(65536)))]),
            vec![]
        )
        .is_err()
    );
    assert!(
        MetadataSnapshot::new(
            id,
            BTreeMap::from([("custom.site".into(), json!("lab"))]),
            vec![]
        )
        .is_ok()
    );
}

#[test]
fn resource_admission_requires_current_startup_and_both_clocks() {
    let now = Utc::now();
    let startup = Uuid::new_v4();
    let mut resources = Resources {
        startup_id: startup,
        sampled_at: now,
        received_at: now,
        available_memory_bytes: Some(4096),
        build_disk_available_bytes: Some(8192),
        cpu_percent: Some(30.0),
    };
    let required = ResourceRequirements {
        min_available_memory_bytes: Some(1024),
        min_build_disk_available_bytes: Some(2048),
        max_cpu_percent: Some(50.0),
    };
    assert!(resources.satisfies(startup, &required, now));
    assert!(!resources.satisfies(Uuid::new_v4(), &required, now));
    resources.sampled_at = now - Duration::seconds(91);
    assert!(!resources.satisfies(startup, &required, now));
    resources.sampled_at = now;
    resources.received_at = now - Duration::seconds(91);
    assert!(!resources.satisfies(startup, &required, now));
    resources.received_at = now;
    resources.available_memory_bytes = None;
    assert!(!resources.satisfies(startup, &required, now));
}

#[test]
fn placement_rejects_duplicates_and_unbounded_candidates() {
    let candidate = json!({"node_id": Uuid::new_v4(), "metadata_digest":"a".repeat(64),"reason":"native toolchain"});
    let invalid: Placement =
        serde_json::from_value(json!({"candidates":[candidate.clone(),candidate],"resources":{}}))
            .unwrap();
    assert!(invalid.validate().is_err());
    let bad: ResourceRequirements = serde_json::from_value(json!({"max_cpu_percent":101})).unwrap();
    assert!(bad.validate().is_err());
}

#[test]
fn delivery_diagnostics_remain_bounded_and_legacy_absence_stays_unknown() {
    use artifactory_emissary_protocol::nodes::{
        MAX_DELIVERY_DIAGNOSTICS_BYTES, decode_heartbeat, delivery_diagnostics,
    };
    assert!(decode_heartbeat(r#"{"diagnostics":{}}"#).is_ok());
    let value = delivery_diagnostics(
        &json!({"state":"schema_unsupported","spool_bytes":u64::MAX,"spool_records":8192,"backoff_seconds":30,"last_successful_ack_at":null,"private_path":"secret","collector_errors":"x".repeat(10000)}),
    );
    assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_DELIVERY_DIAGNOSTICS_BYTES);
    assert_eq!(value["state"], "schema_unsupported");
    assert!(value.get("private_path").is_none());
    assert!(value.get("collector_errors").is_none());
    let oversized = json!({"diagnostics":{"metrics":{"telemetry":{"state":"x".repeat(MAX_DELIVERY_DIAGNOSTICS_BYTES)}}}});
    assert!(decode_heartbeat(&oversized.to_string()).is_err());
}

#[test]
fn delivery_projects_lane_progress_and_durable_loss_without_private_values() {
    use artifactory_emissary_protocol::nodes::delivery_diagnostics;
    let value = json!({"fresh_last_ack_at":"2026-11-26T00:00:00Z","history_last_progress_at":"2026-11-26T00:00:00Z","inventory_backoff_seconds":30,"inventory_conflict_records":0,"inventory_conflicts_total":1,"inventory_last_conflict_at":"2026-11-26T00:00:00Z","last_rejection_reason":"bounds_exceeded","bounds_exceeded_batches":1,"disk_allocated_bytes":null,"disk_temporary_bytes":33,"private_path":"secret"});
    let projected = delivery_diagnostics(&value);
    for key in [
        "fresh_last_ack_at",
        "history_last_progress_at",
        "inventory_backoff_seconds",
        "inventory_conflict_records",
        "inventory_conflicts_total",
        "inventory_last_conflict_at",
        "last_rejection_reason",
        "bounds_exceeded_batches",
        "disk_allocated_bytes",
        "disk_temporary_bytes",
    ] {
        assert_eq!(projected.get(key), value.get(key), "{key}");
    }
    assert!(projected.get("private_path").is_none());
}
