//! Offline tenant migration operations. Call while all business instances are stopped.
use anyhow::{Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql};
use std::path::Path;
use uuid::Uuid;

/// A connection that does not migrate automatically on open.
pub struct TenantMigration {
    pool: toasty::Db,
}
impl TenantMigration {
    pub async fn connect(url: &str) -> Result<Self> {
        Ok(Self {
            pool: connect(url, super::models(), PoolSettings::new(4)).await?,
        })
    }
    pub async fn verify_drained(&self) -> Result<()> {
        let initialized: bool = sql::scalar(
            &mut self.pool.clone(),
            "SELECT to_regclass('builds') IS NOT NULL",
            params![],
        )
        .await?;
        if !initialized {
            return Ok(());
        }
        let active: i64 = sql::scalar(
            &mut self.pool.clone(),
            "SELECT (SELECT count(*) FROM builds WHERE status NOT IN ('succeeded','failed','lost','canceled') OR publish_status='pending') + (SELECT count(*) FROM planning_runs WHERE state IN ('queued','leased')) + (SELECT count(*) FROM attempts WHERE status IN ('leased','running') AND lease_expires_at>now()) + (SELECT count(*) FROM emissary_updates WHERE status NOT IN ('succeeded','failed','rolled_back','canceled'))",
            params![],
        ).await?;
        ensure!(
            active == 0,
            "finish or cancel all in-flight builds, publishing, planning and OTA before migration"
        );
        Ok(())
    }
    pub async fn apply(&self) -> Result<()> {
        self.verify_drained().await?;
        artifactory_database::migrate(&self.pool, &super::MIGRATIONS).await?;
        Ok(())
    }
    pub async fn project_tenants(&self) -> Result<Vec<(Uuid, Uuid)>> {
        Ok(sql::tuples(
            &mut self.pool.clone(),
            "SELECT id,tenant_id FROM projects",
            params![],
        )
        .await?)
    }
    pub fn write_snapshot(&self, path: &Path) -> Result<()> {
        toasty::migration::Snapshot::new(self.pool.schema().db.clone()).save(path)?;
        Ok(())
    }
}
