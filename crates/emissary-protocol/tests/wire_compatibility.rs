//! [INPUT]: 依赖固定 JSON 样本、公开节点契约和 Prost 编码。
//! [OUTPUT]: 验证遥测重放字节、元数据摘要和 RPC 字段编码。
//! [POS]: 节点协议合并的兼容回归；固定摘要使用的原始字节。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary_protocol::{
    Command, TelemetryAck, TelemetryBatch, nodes::MetadataSnapshot, telemetry::Batch,
};
use prost::Message;
use sha2::{Digest, Sha256};

const BATCH: &[u8] = include_bytes!("fixtures/telemetry-batch.json");
const METADATA: &[u8] = include_bytes!("fixtures/metadata-snapshot.json");

#[test]
fn command_notification_marker_is_additive_and_defaults_to_execution_rejection() {
    let old = [
        0x0a, 0x02, b'i', b'd', 0x12, 0x06, b'n', b'o', b't', b'i', b'c', b'e',
    ];
    let mut command = Command::decode(old.as_slice()).unwrap();
    assert!(!command.ignorable_notification);
    assert_eq!(command.encode_to_vec(), old);
    command.ignorable_notification = true;
    let mut appended = old.to_vec();
    appended.extend([0x60, 0x01]);
    assert_eq!(
        command.encode_to_vec(),
        appended,
        "新增布尔值只能使用字段12"
    );
    assert!(
        Command::decode(appended.as_slice())
            .unwrap()
            .ignorable_notification
    );
}

#[test]
fn telemetry_batch_preserves_replay_bytes() {
    let batch: Batch = serde_json::from_slice(BATCH).expect("固定批次符合原遥测契约");
    batch.validate().expect("固定批次在接收边界内");

    let encoded = serde_json::to_vec(&batch).expect("固定批次能序列化");
    assert_eq!(encoded, BATCH, "字段顺序和空值必须保留重放摘要");
    assert_eq!(
        format!("{:x}", Sha256::digest(&encoded)),
        "132f630ebbcaa6195a712c3e17cd9b178ef8f5ee3ad96a4b9bb518bd30f1d6f0",
        "相同批次 ID 必须继续对应相同内容摘要"
    );
}

#[test]
fn metadata_snapshot_preserves_bytes_and_placement_digest() {
    let snapshot: MetadataSnapshot =
        serde_json::from_slice(METADATA).expect("固定启动快照符合原节点契约");
    snapshot.validate().expect("固定启动快照在接收边界内");

    assert_eq!(
        serde_json::to_vec(&snapshot).expect("固定启动快照能序列化"),
        METADATA,
        "元数据映射排序和启动快照字段顺序必须稳定"
    );
    assert_eq!(
        snapshot.digest().expect("固定元数据能生成摘要"),
        "41b1c57e7b77dab144a925df52062862aceb93712fe6cab8c05a590c1aae93b2",
        "选机围栏必须继续识别原元数据摘要"
    );
}

#[test]
fn telemetry_rpc_preserves_protobuf_field_numbers() {
    let batch_bytes = [0x0a, 0x02, b'{', b'}'];
    let batch = TelemetryBatch::decode(batch_bytes.as_slice()).expect("原批次信封能解码");
    assert_eq!(batch.json, b"{}");
    assert_eq!(batch.encode_to_vec(), batch_bytes);

    let ack_bytes = [
        0x0a, 0x05, b'b', b'a', b't', b'c', b'h', 0x10, 0x01, 0x18, 0x2a,
    ];
    let ack = TelemetryAck::decode(ack_bytes.as_slice()).expect("原批次回执能解码");
    assert_eq!(ack.batch_id, "batch");
    assert!(ack.duplicate);
    assert_eq!(ack.server_time_ms, 42);
    assert_eq!(ack.encode_to_vec(), ack_bytes);
}

#[test]
fn telemetry_schema_and_digest_ack_survive_relay_encoding() {
    let batch_bytes = [0x0a, 0x02, b'{', b'}', 0x10, 0x01];
    let batch = TelemetryBatch::decode(batch_bytes.as_slice()).unwrap();
    assert_eq!(batch.encode_to_vec(), batch_bytes);

    let ack_bytes = [0x0a, 0x01, b'x', 0x22, 0x03, b'a', b'b', b'c', 0x28, 0x01];
    let ack = TelemetryAck::decode(ack_bytes.as_slice()).unwrap();
    assert_eq!(ack.encode_to_vec(), ack_bytes);
}
