//! [INPUT]: Toasty模型映射与遥测迁移投影。
//! [OUTPUT]: 导出采样幂等键、内容版本和旧观察模型。
//! [POS]: typed CRUD边界；独立清单表使用有界事务SQL。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "telemetry_batches"]
#[key(node_id, batch_id)]
pub(super) struct TelemetryBatch {
    pub node_id: Uuid,
    pub batch_id: Uuid,
    pub digest: String,
    pub received_at: jiff::Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "inventory_versions"]
#[key(node_id, digest)]
pub(super) struct InventoryVersion {
    pub node_id: Uuid,
    pub digest: String,
    pub first_seen: jiff::Timestamp,
    #[column(type = jsonb)]
    pub inventory: toasty::Json<serde_json::Value>,
}

#[derive(Debug, toasty::Model)]
#[table = "inventory_observations"]
#[key(node_id, sampled_at)]
pub(super) struct InventoryObservation {
    pub node_id: Uuid,
    pub sampled_at: jiff::Timestamp,
    pub digest: String,
}
