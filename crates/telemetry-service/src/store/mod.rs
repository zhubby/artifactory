//! Telemetry persistence and read-only business authorization.
//! [INPUT]: 共享数据库连接与遥测模型。
//! [OUTPUT]: 遥测读写存储、业务授权查询和生命周期清扫。
//! [POS]: 隔离遥测持久化与业务只读访问的存储边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_database::{params, sql};
mod history;
mod ingest;
mod inventory;
mod models;
mod purge;
use crate::internal;
pub use history::{HistoryPage, LatestTelemetry};
pub use ingest::ingest;
pub use inventory::ingest_inventory;
use tonic::Status;
use uuid::Uuid;
#[derive(Clone)]
pub struct TelemetryStore {
    write: toasty::Db,
    read: toasty::Db,
}
#[derive(Clone)]
pub struct BusinessAuthorizationRepository {
    pool: toasty::Db,
}
impl TelemetryStore {
    pub fn new(write: toasty::Db, read: toasty::Db) -> Self {
        Self { write, read }
    }
    pub async fn ingest(
        &self,
        node: Uuid,
        batch: &artifactory_emissary_protocol::telemetry::Batch,
        bytes: &[u8],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, Status> {
        ingest(&self.write, node, batch, bytes, now).await
    }
    pub async fn ingest_inventory(
        &self,
        node: Uuid,
        observation: &artifactory_emissary_protocol::InventoryObservation,
        received: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, Status> {
        ingest_inventory(&self.write, node, observation, received).await
    }
    /// 业务库分批提供当前startup，长期离线设备的当前摘要不受历史清扫影响。
    pub async fn retain_startups(&self, nodes: &[(Uuid, Option<Uuid>)]) -> anyhow::Result<()> {
        anyhow::ensure!(nodes.len() <= 512, "at most 512 startup identities");
        let identities = serde_json::Value::Array(
            nodes
                .iter()
                .map(|(node, startup)| serde_json::json!({"node_id":node,"startup_id":startup}))
                .collect(),
        );
        let mut db = self.write.clone();
        let mut transaction = db.transaction().await?;
        for table in ["startup_inventories", "startup_hosts", "startup_resources"] {
            sql::execute(&mut transaction,&format!("DELETE FROM {table} h USING jsonb_to_recordset($1) AS n(node_id uuid,startup_id uuid) WHERE h.node_id=n.node_id AND h.startup_id IS DISTINCT FROM n.startup_id AND h.received_at<now()-interval '180 days'"),params![identities.clone()]).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
    pub async fn retain(&self) -> anyhow::Result<()> {
        let mut db = self.write.clone();
        let mut transaction = db.transaction().await?;
        let acquired: bool = sql::scalar(&mut transaction, "SELECT pg_try_advisory_xact_lock(hashtextextended('artifactory:telemetry-retention',12))", params![]).await?;
        if !acquired {
            return Ok(());
        }
        // 与接收事务互斥，避免清扫内容版本和新观察提交之间出现孤立摘要。
        sql::execute(
            &mut transaction,
            "SELECT pg_advisory_xact_lock(438284731022932)",
            params![],
        )
        .await?;
        for statement in [
            "DELETE FROM telemetry_batches WHERE received_at<now()-interval '3 days'",
            "DELETE FROM inventory_observations WHERE sampled_at<now()-interval '180 days'",
            "DELETE FROM inventory_deliveries d WHERE received_at<now()-interval '180 days' AND NOT EXISTS(SELECT 1 FROM startup_inventories s WHERE s.node_id=d.node_id AND s.observation_id=d.observation_id)",
            "DELETE FROM inventory_ingress WHERE updated_at<now()-interval '1 day'",
            "DELETE FROM inventory_versions v WHERE NOT EXISTS(SELECT 1 FROM inventory_observations o WHERE o.node_id=v.node_id AND o.digest=v.digest) AND NOT EXISTS(SELECT 1 FROM latest_inventories l WHERE l.node_id=v.node_id AND l.digest=v.digest) AND NOT EXISTS(SELECT 1 FROM inventory_deliveries d WHERE d.node_id=v.node_id AND d.inventory_digest=v.digest) AND NOT EXISTS(SELECT 1 FROM startup_inventories s WHERE s.node_id=v.node_id AND s.inventory_digest=v.digest)",
        ] {
            sql::execute(&mut transaction, statement, params![]).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}
impl BusinessAuthorizationRepository {
    pub fn new(pool: toasty::Db) -> Self {
        Self { pool }
    }
    /// 不把全部节点装入内存，且不以遥测记录推断当前业务身份。
    pub async fn startup_page(
        &self,
        after: Option<Uuid>,
    ) -> anyhow::Result<Vec<(Uuid, Option<Uuid>)>> {
        Ok(sql::tuples(
            &mut self.pool.clone(),
            "SELECT id,startup_id FROM nodes WHERE $1::uuid IS NULL OR id>$1 ORDER BY id LIMIT 512",
            params![after],
        )
        .await?)
    }
    pub async fn identify(&self, certificate_hash: String) -> Result<Uuid, Status> {
        sql::optional_scalar(&mut self.pool.clone(), "SELECT id FROM nodes WHERE certificate_hash=$1 AND NOT revoked AND certificate_expires_at>now()", params![certificate_hash]).await.map_err(internal)?.ok_or_else(|| artifactory_emissary_protocol::telemetry_negotiation::RejectionReason::AuthenticationFailed.status())
    }
    pub async fn authorize_startup(&self, node: Uuid, startup: Uuid) -> Result<(), Status> {
        let owned: bool = sql::scalar(
            &mut self.pool.clone(),
            "SELECT EXISTS(SELECT 1 FROM node_startups WHERE node_id=$1 AND startup_id=$2)",
            params![node, startup],
        )
        .await
        .map_err(internal)?;
        if !owned {
            return Err(artifactory_emissary_protocol::telemetry_negotiation::RejectionReason::StartupUnregistered.status());
        }
        Ok(())
    }
    pub async fn authorize_attempts(&self, node: Uuid, attempts: &[Uuid]) -> Result<(), Status> {
        if attempts.is_empty() {
            return Ok(());
        }
        let owned: i64 = sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM attempts WHERE node_id=$1 AND id=ANY($2)",
            params![node, attempts],
        )
        .await
        .map_err(internal)?;
        if owned as usize != attempts.len() {
            return Err(artifactory_emissary_protocol::telemetry_negotiation::RejectionReason::AuthorizationDenied.status());
        }
        Ok(())
    }
}

mod connect;
pub use connect::connect;

pub static MIGRATIONS: toasty::migration::MigrationSet = toasty::embed_migrations!("migrations");

pub fn models() -> toasty::schema::app::ModelSet {
    toasty::models!(
        models::TelemetryBatch,
        models::InventoryVersion,
        models::InventoryObservation
    )
}
