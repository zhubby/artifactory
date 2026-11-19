//! [INPUT]: 依赖租户、项目和业务状态。
//! [OUTPUT]: 认领任务、汇总构建并在草稿锁内发起重排。
//! [POS]: 控制平面的业务边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! Scheduler persistence. Claim, publish and resolver lock lifetimes are owned here.
use super::ControlStore;

use anyhow::Result;
use artifactory_database::{params, sql};
use serde_json::Value;
use toasty::stmt::Type;

use uuid::Uuid;
pub(crate) struct AssignmentContext {
    pub release_id: Uuid,
    pub release_version: String,
    pub target: String,
    pub target_config: Value,
    pub plan_revision_id: Option<Uuid>,
    pub source_manifest_id: Option<Uuid>,
    pub id: Uuid,
    pub commit_sha: String,
    pub config_snapshot: Value,
    pub parameters: Value,
    pub plan: Option<Value>,
}
impl ControlStore {
    pub async fn expire(&self) -> Result<()> {
        let mut transaction_db = self.pool.clone();
        let mut transaction = transaction_db.transaction().await?;
        let rows=sql::records(&mut transaction, "SELECT a.id,a.job_id,a.status,b.cancel_requested FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE a.status IN ('leased','running') AND a.lease_expires_at<=now() FOR UPDATE OF a,j SKIP LOCKED", params![], &[("id", Type::Uuid),("job_id", Type::Uuid),("status", Type::String),("cancel_requested", Type::Bool)]).await?;
        for row in rows {
            let started = row.decode::<String>("status")? == "running";
            let canceled: bool = row.decode("cancel_requested")?;
            sql::execute(&mut transaction, "UPDATE attempts SET status=$2,error='execution lease expired',finished_at=now() WHERE id=$1", params![row.decode::<Uuid>("id")?, if canceled {"canceled"} else {"lost"}]).await?;
            sql::execute(
                &mut transaction,
                "UPDATE jobs SET status=$2 WHERE id=$1",
                params![
                    row.decode::<Uuid>("job_id")?,
                    if canceled {
                        "canceled"
                    } else if started {
                        "lost"
                    } else {
                        "queued"
                    }
                ],
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn refresh_builds(&self) -> Result<()> {
        // Aggregation precedes the UPDATE's row lock. Fence its snapshot by the
        // build's row version so waiting behind a retry or planning transition
        // cannot restore an obsolete terminal state after that action commits.
        sql::execute(
            &mut self.pool.clone(),
            "WITH summaries AS (
                SELECT b.id,b.xmin AS version,
                    CASE WHEN b.cancel_requested THEN
                        CASE WHEN bool_or(j.status IN ('leased','running')) THEN 'canceling' ELSE 'canceled' END
                    WHEN bool_or(j.status='running') THEN 'running'
                    WHEN bool_or(j.status IN ('queued','leased')) THEN 'queued'
                    WHEN bool_or(j.status='failed') THEN 'failed'
                    WHEN bool_or(j.status='lost') THEN 'lost'
                    WHEN bool_or(j.status='canceled') THEN 'canceled'
                    ELSE 'succeeded' END AS status
                FROM builds b JOIN jobs j ON j.build_id=b.id
                WHERE b.status IN ('queued','running','canceling')
                GROUP BY b.id,b.xmin
            )
            UPDATE builds b SET status=s.status,
                finished_at=CASE WHEN s.status IN ('succeeded','failed','lost','canceled') THEN coalesce(b.finished_at,now()) ELSE NULL END
            FROM summaries s WHERE b.id=s.id AND b.xmin=s.version AND (b.status IS DISTINCT FROM s.status OR (s.status IN ('succeeded','failed','lost','canceled') AND b.finished_at IS NULL))",
            params![],
        )
        .await?;
        Ok(())
    }

    pub async fn queue_revisions(&self) -> Result<()> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let draft_releases:Vec<Uuid>=sql::scalars(&mut tx,"SELECT id FROM releases WHERE status='draft' AND id IN (SELECT release_id FROM builds WHERE status='failed' AND revision_count<2) ORDER BY id FOR UPDATE SKIP LOCKED",params![]).await?;
        let rows=sql::records(&mut tx, "SELECT id,model_config_snapshot FROM builds b WHERE b.release_id=ANY($1) AND EXISTS(SELECT 1 FROM releases r WHERE r.id=b.release_id AND r.status='draft') AND goal IS NOT NULL AND current_plan_id IS NOT NULL AND status='failed' AND NOT cancel_requested AND revision_count<2 AND (SELECT state FROM planning_runs WHERE build_id=b.id ORDER BY created_at DESC LIMIT 1)='succeeded' AND EXISTS(SELECT 1 FROM jobs j JOIN LATERAL (SELECT failure_kind FROM attempts WHERE job_id=j.id ORDER BY created_at DESC LIMIT 1) a ON true WHERE j.build_id=b.id AND j.status='failed' AND a.failure_kind='execution') ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 10", params![&draft_releases], &[("id", Type::Uuid),("model_config_snapshot", Type::String)]).await?;
        for row in rows {
            let id: Uuid = row.decode("id")?;
            let failed:Vec<String>=sql::scalars(&mut tx, "SELECT j.target FROM jobs j JOIN LATERAL (SELECT failure_kind FROM attempts WHERE job_id=j.id ORDER BY created_at DESC LIMIT 1) a ON true WHERE j.build_id=$1 AND j.status='failed' AND a.failure_kind='execution'", params![id]).await?;
            sql::execute(&mut tx, "UPDATE builds SET revision_count=revision_count+1,status='replanning',finished_at=NULL WHERE id=$1", params![id]).await?;
            super::models::PlanningRun::create()
                .id(Uuid::new_v4())
                .build_id(id)
                .purpose("revision")
                .model_config(toasty::Json(
                    (row.decode::<Value>("model_config_snapshot")?).clone(),
                ))
                .failed_targets(failed)
                .exec(&mut tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn assignment_context(&self, job: Uuid) -> Result<AssignmentContext> {
        let row=sql::record(&mut self.pool.clone(), "SELECT j.target,j.target_config,j.plan_revision_id,b.source_manifest_id,b.id,b.release_id,b.release_version,b.commit_sha,b.config_snapshot,b.parameters FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1", params![job], &[("target", Type::String),("target_config", Type::String),("plan_revision_id", Type::Uuid),("source_manifest_id", Type::Uuid),("id", Type::Uuid),("release_id",Type::Uuid),("release_version",Type::String),("commit_sha", Type::String),("config_snapshot", Type::String),("parameters", Type::String)]).await?;
        let plan_id: Option<Uuid> = row.decode("plan_revision_id")?;
        let plan = if let Some(id) = plan_id {
            sql::optional_scalar(
                &mut self.pool.clone(),
                "SELECT contents FROM plan_revisions WHERE id=$1",
                params![id],
            )
            .await?
        } else {
            None
        };
        Ok(AssignmentContext {
            release_id: row.decode("release_id")?,
            release_version: row.decode("release_version")?,
            target: row.decode("target")?,
            target_config: row.decode("target_config")?,
            plan_revision_id: row.decode("plan_revision_id")?,
            source_manifest_id: row.decode("source_manifest_id")?,
            id: row.decode("id")?,
            commit_sha: row.decode("commit_sha")?,
            config_snapshot: row.decode("config_snapshot")?,
            parameters: row.decode("parameters")?,
            plan,
        })
    }
}
