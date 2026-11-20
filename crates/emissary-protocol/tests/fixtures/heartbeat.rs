//! [INPUT]: 依赖节点元数据契约、UUID 和 JSON 编码。
//! [OUTPUT]: 提供满额启动、网络与带 OTA 的运行诊断夹具。
//! [POS]: 发送端状态与接收端校验共用的合法边界输入。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::nodes::{MAX_METADATA_BYTES, MetadataSnapshot};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn maximum_startup() -> MetadataSnapshot {
    let mut startup = MetadataSnapshot::new(
        Uuid::new_v4(),
        [("custom.payload".into(), json!(""))].into(),
        vec![],
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
        "managed_free_bytes":u64::MAX,"telemetry_dropped_batches":u64::MAX})
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
