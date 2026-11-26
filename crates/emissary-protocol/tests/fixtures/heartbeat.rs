//! [INPUT]: 依赖节点元数据契约、UUID 和 JSON 编码。
//! [OUTPUT]: 提供满额启动、网络、清单交付和OTA观察夹具。
//! [POS]: 发送端状态与接收端校验共用的合法边界输入。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::nodes::{MAX_METADATA_BYTES, MetadataSnapshot};
use artifactory_emissary_protocol::ota::{Drain, Observability, Progress};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn maximum_startup() -> MetadataSnapshot {
    let mut startup = MetadataSnapshot::new(
        Uuid::new_v4(),
        [("custom.payload".into(), json!(""))].into(),
        vec![
            artifactory_emissary_protocol::commands::CAPABILITY.into(),
            artifactory_emissary_protocol::commands::NOTIFICATION_CAPABILITY.into(),
            "emissary-ota-v1".into(),
            artifactory_emissary_protocol::ota::CAPABILITY.into(),
        ],
    )
    .unwrap();
    let padding = MAX_METADATA_BYTES - serde_json::to_vec(&startup).unwrap().len();
    startup
        .metadata
        .insert("custom.payload".into(), json!("x".repeat(padding)));
    startup.validate().unwrap();
    assert_eq!(serde_json::to_vec(&startup).unwrap().len(), 65536);
    startup
}

pub fn maximum_network() -> Value {
    let mut network = json!({
        "sampled_at":"2026-11-20T01:02:03.000Z","truncated":false,
        "interfaces":(0..16).map(|index|json!({
            "name":format!("{index:02}{}","x".repeat(126)),
            "addresses":(0..8).map(|last|format!("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ff{last:02x}")).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    });
    // 将 ASCII 字符替换为需要转义的引号，每次增加一个 JSON 字节。
    let extra = 8192 - serde_json::to_vec(&network).unwrap().len();
    for index in 0..extra {
        let entry = &mut network["interfaces"][index / 126]["name"];
        let mut name = entry.as_str().unwrap().to_owned();
        name.replace_range(2 + index % 126..3 + index % 126, "\"");
        *entry = json!(name);
    }
    assert_eq!(serde_json::to_vec(&network).unwrap().len(), 8192);
    network
}

pub fn metrics() -> Value {
    json!({"logical_cpus":64,"emissary_rss_bytes":u64::MAX,
        "managed_free_bytes":u64::MAX,"telemetry_dropped_batches":u64::MAX,
        "telemetry":maximum_delivery_diagnostics()})
}

pub fn maximum_delivery_diagnostics() -> Value {
    use artifactory_emissary_protocol::nodes::MAX_DELIVERY_DIAGNOSTICS_BYTES;
    let mut value = json!({});
    for key in [
        "spool_bytes",
        "spool_records",
        "oldest_age_seconds",
        "inventory_bytes",
        "inventory_records",
        "inventory_superseded",
        "inventory_oldest_age_seconds",
        "fresh_records",
        "fresh_oldest_age_seconds",
        "quota_eviction_batches",
        "expired_batches",
        "malformed_batches",
        "disk_errors",
        "schema_wait_records",
        "inventory_schema_wait_records",
        "dropped_batches",
        "fresh_bytes",
        "coalesced_batches",
        "corrupt_records",
        "upload_latency_ms",
        "backoff_seconds",
        "collector_duration_ms",
        "bounds_exceeded_batches",
        "id_content_conflict_batches",
        "identity_retired_batches",
        "fresh_upload_latency_ms",
        "fresh_backoff_seconds",
        "history_upload_latency_ms",
        "history_backoff_seconds",
        "inventory_upload_latency_ms",
        "inventory_backoff_seconds",
        "disk_file_bytes",
        "disk_record_bytes",
        "disk_allocated_bytes",
        "disk_block_overhead_bytes",
        "disk_temporary_bytes",
        "disk_index_bytes",
        "disk_corrupt_bytes",
        "disk_unknown_bytes",
        "disk_files",
        "omitted_host_devices",
        "truncated_host_fields",
    ] {
        value[key] = json!(u64::MAX);
    }
    for key in [
        "last_successful_ack_at",
        "last_inventory_ack_at",
        "last_rejection_at",
        "fresh_last_ack_at",
        "fresh_last_progress_at",
        "history_last_ack_at",
        "history_last_progress_at",
        "inventory_last_ack_at",
        "inventory_last_progress_at",
        "disk_observed_at",
    ] {
        value[key] = json!("+262142-12-31T23:59:59.999999999Z");
    }
    let strings = [
        "state",
        "inventory_state",
        "collector_error",
        "negotiation_state",
        "last_rejection_reason",
        "fresh_state",
        "history_state",
        "disk_state",
    ];
    for key in strings {
        value[key] = json!("x".repeat(128));
    }
    let extra = MAX_DELIVERY_DIAGNOSTICS_BYTES - serde_json::to_vec(&value).unwrap().len();
    assert!(extra <= strings.len() * 128);
    // 合法字符串中的引号增加编码开销，触及完整4KiB投影边界。
    for index in 0..extra {
        let key = strings[index / 128];
        let mut text = value[key].as_str().unwrap().to_owned();
        text.replace_range(index % 128..index % 128 + 1, "\"");
        value[key] = json!(text);
    }
    assert_eq!(
        serde_json::to_vec(&value).unwrap().len(),
        MAX_DELIVERY_DIAGNOSTICS_BYTES
    );
    value
}

pub fn ota_status() -> Value {
    json!({"activation_pending":false,"generation":i64::MAX,
        "update_id":Uuid::new_v4(),"target":"0.2.0","phase":"failed",
        "error":"launcher interrupted during activation; restored previous runtime",
        "current":{"version":"0.2.0","path":"a".repeat(1024)},
        "previous":{"version":"0.1.0","path":"b".repeat(1024)},
        "candidate":{"version":"0.3.0","path":"c".repeat(1024)},
        "launcher_version":"0.2.0","started_at":"2026-11-20T01:02:03Z"})
}

#[allow(dead_code, reason = "共享夹具的接收端用例仅调用既有OTA状态")]
pub fn ota_observability() -> Observability {
    let at = "2026-11-26T01:02:03.123456789Z".parse().unwrap();
    let observation = Observability {
        schema_version: 1,
        sequence: u64::MAX,
        launcher_version: "x".repeat(100),
        launcher_capabilities: [
            "ota-progress-v1",
            "ota-space-v1",
            "ota-storage-gc-v1",
            "ota-range-v1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        progress: Some(Progress {
            update_id: Uuid::new_v4(),
            generation: i64::MAX,
            asset_sha256: "a".repeat(64),
            sequence: u64::MAX,
            subphase: "space_check".into(),
            bytes_done: 128 << 20,
            bytes_total: 128 << 20,
            download_attempt: 2,
            last_progress_at: at,
            updated_at: at,
            error_code: Some("space_check_unavailable".into()),
        }),
        drain: Some(Drain {
            active_workers: u32::MAX,
            pending_log_records: u64::MAX,
            pending_log_bytes: u64::MAX,
            pending_terminal_records: u32::MAX,
            terminals: u32::MAX,
            operations: u32::MAX,
            waiting_ids: (0..8).map(|_| Uuid::new_v4()).collect(),
            remaining_objects: u32::MAX,
        }),
        last_progress_at: at,
        stalled: true,
    };
    observation.validate().unwrap();
    observation
}
