//! Offline history backfill from the trusted control-plane project map.
use anyhow::{Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql};
use std::path::Path;
use uuid::Uuid;

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
            "SELECT to_regclass('runtime_runs') IS NOT NULL",
            params![],
        )
        .await?;
        if !initialized {
            return Ok(());
        }
        let active: i64 = sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM runtime_runs WHERE status IN ('queued','provisioning','running')",
            params![],
        )
        .await?;
        ensure!(active == 0, "drain Runtime work before migration");
        Ok(())
    }
    pub async fn backfill(&self, mapping: &[(Uuid, Uuid)]) -> Result<()> {
        self.verify_drained().await?;
        artifactory_database::migrate(&self.pool, &super::MIGRATIONS).await?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        for (project, tenant) in mapping {
            sql::execute(&mut tx,"INSERT INTO runtime_project_tenants(project_id,tenant_id) VALUES($1,$2) ON CONFLICT(project_id) DO NOTHING",params![project,tenant]).await?;
            let valid: bool = sql::scalar(
                &mut tx,
                "SELECT tenant_id=$2 FROM runtime_project_tenants WHERE project_id=$1",
                params![project, tenant],
            )
            .await?;
            ensure!(
                valid,
                "Runtime project ownership conflicts with control plane"
            );
        }
        sql::execute(&mut tx,"UPDATE runtime_runs r SET tenant_id=m.tenant_id FROM runtime_project_tenants m WHERE m.project_id=r.project_id AND r.tenant_id IS NULL",params![]).await?;
        let missing: i64 = sql::scalar(
            &mut tx,
            "SELECT count(*) FROM runtime_runs WHERE project_id IS NOT NULL AND tenant_id IS NULL",
            params![],
        )
        .await?;
        ensure!(
            missing == 0,
            "Runtime contains projects absent from control plane; restore or correct the historical project map"
        );
        tx.commit().await?;
        Ok(())
    }
    pub fn write_snapshot(&self, path: &Path) -> Result<()> {
        toasty::migration::Snapshot::new(self.pool.schema().db.clone()).save(path)?;
        Ok(())
    }
}
