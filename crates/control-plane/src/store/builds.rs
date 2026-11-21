//! ```text
//! [INPUT]: 依赖租户构建、镜像准入和执行结果索引。
//! [OUTPUT]: 读取 Release 构建快照、正式交付名与执行结果。
//! [POS]: 构建结果查询与失败任务重试边界；对象正文由接口层读取。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
use super::ControlStore;
use super::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
};
use artifactory_database::{params, sql};
use serde::Deserialize;
use serde_json::Value;
use toasty::stmt::Type;

use uuid::Uuid;

pub struct ArtifactDownload {
    pub delivery_filename: Option<String>,
    pub object_key: String,
    pub project_id: Uuid,
}
#[derive(Deserialize)]
pub struct LogChunk {
    pub id: i64,
    pub attempt_id: Uuid,
    pub step: String,
    pub object_key: String,
}

impl ControlStore {
    pub async fn build_by_number(&self, project: Uuid, number: i64) -> Result<Uuid> {
        sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT id FROM builds WHERE tenant_id=$1 AND project_id=$2 AND number=$3",
            params![self.tenant()?, project, number],
        )
        .await?
        .ok_or(ApiError::NotFound)
    }

    pub async fn build_route(&self, id: Uuid) -> Result<Value> {
        Ok(sql::scalar(&mut self.pool.clone(), "SELECT jsonb_build_object('id',b.id,'number',b.number,'project_slug',p.slug) FROM builds b JOIN projects p ON p.id=b.project_id WHERE b.id=$1 AND b.tenant_id=$2", params![id,self.tenant()?]).await?)
    }

    pub async fn builds(
        &self,
        project: Option<Uuid>,
        visible: Option<Vec<Uuid>>,
    ) -> Result<Vec<dto::BuildSummary>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT (to_jsonb(b)-'config_yaml'-'config_snapshot'-'model_config_snapshot') || jsonb_build_object('project_name',p.name,'project_slug',p.slug,'targets',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',j.id,'target',j.target,'status',j.status)),'[]') FROM jobs j WHERE j.build_id=b.id)) FROM builds b JOIN projects p ON p.id=b.project_id WHERE ($1::uuid IS NULL OR b.project_id=$1) AND ($2::uuid[] IS NULL OR b.project_id=ANY($2)) ORDER BY b.created_at DESC LIMIT 100", params![project, visible]).await?)
    }

    pub async fn artifacts(
        &self,
        project: Option<Uuid>,
        visible: Option<Vec<Uuid>>,
    ) -> Result<Vec<dto::ArtifactSummary>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT (to_jsonb(ar)-'object_key') || jsonb_build_object('target',j.target,'build_id',b.id,'project_name',p.name,'project_id',p.id,'release_id',b.release_id,'release_version',b.release_version,'delivery_filename',(SELECT ra.delivery_filename FROM release_artifacts ra JOIN releases r ON r.id=ra.release_id WHERE ra.artifact_id=ar.id AND r.status='published' LIMIT 1)) FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=b.project_id WHERE ($1::uuid IS NULL OR p.id=$1) AND ($2::uuid[] IS NULL OR p.id=ANY($2)) ORDER BY ar.created_at DESC LIMIT 100", params![project, visible]).await?)
    }

    pub async fn build_project(&self, id: Uuid) -> Result<Uuid> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT project_id FROM builds WHERE id=$1",
            params![id],
        )
        .await?)
    }

    pub async fn build_detail(&self, id: Uuid) -> Result<dto::BuildDetail> {
        let build:dto::BuildEnvelope=sql::scalar(&mut self.pool.clone(), "SELECT (to_jsonb(b)-'model_config_snapshot') || jsonb_build_object('project_name',p.name,'project_slug',p.slug) FROM builds b JOIN projects p ON p.id=b.project_id WHERE b.id=$1", params![id]).await?;
        let jobs:Vec<dto::Job>=sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(j) || jsonb_build_object('placement',(SELECT to_jsonb(p)-'job_id' FROM job_placements p WHERE p.job_id=j.id),'attempts',(SELECT coalesce(jsonb_agg((to_jsonb(a)-'lease_token'-'command_id') || jsonb_build_object('node_name',n.name,'steps',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY s.step_index),'[]') FROM step_runs s WHERE s.attempt_id=a.id)) ORDER BY a.created_at),'[]') FROM attempts a JOIN nodes n ON n.id=a.node_id WHERE a.job_id=j.id)) FROM jobs j WHERE j.build_id=$1 ORDER BY j.target", params![id]).await?;
        let artifacts:Vec<dto::BuildArtifact>=sql::scalars(&mut self.pool.clone(), "SELECT (to_jsonb(ar)-'object_key') || jsonb_build_object('target',j.target,'attempt_status',a.status,'release_id',b.release_id,'release_version',b.release_version) FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE j.build_id=$1 ORDER BY ar.created_at", params![id]).await?;
        let publish_attempts = sql::scalars::<dto::PublishAttempt>(
            &mut self.pool.clone(),
            "SELECT to_jsonb(p) FROM publish_attempts p WHERE build_id=$1 ORDER BY created_at DESC",
            params![id],
        )
        .await?;
        Ok(dto::BuildDetail {
            build,
            jobs,
            artifacts,
            publish_attempts,
            image_executions: self.image_history(id).await?,
            image_registry: std::env::var("ARTIFACTORY_REGISTRY_URL").ok(),
        })
    }

    pub async fn cancel_build(&self, id: Uuid) -> Result<()> {
        let mut transaction_db = self.pool.clone();
        let mut transaction = transaction_db.transaction().await?;
        let status: String = sql::scalar(
            &mut transaction,
            "SELECT status FROM builds WHERE id=$1 FOR UPDATE",
            params![id],
        )
        .await?;
        if ["succeeded", "failed", "lost", "canceled"].contains(&status.as_str()) {
            return Err(ApiError::Conflict("build is already terminal".into()));
        }
        sql::execute(
            &mut transaction,
            "UPDATE builds SET cancel_requested=true,status='canceling' WHERE id=$1",
            params![id],
        )
        .await?;
        sql::execute(
            &mut transaction,
            "UPDATE jobs SET status='canceled' WHERE build_id=$1 AND status='queued'",
            params![id],
        )
        .await?;
        sql::execute(&mut transaction, "UPDATE planning_runs SET state='canceled',finished_at=now() WHERE build_id=$1 AND state IN ('queued','leased')", params![id]).await?;
        if ["resolving", "planning", "replanning", "needs_input"].contains(&status.as_str()) {
            sql::execute(
                &mut transaction,
                "UPDATE builds SET status='canceled',finished_at=now() WHERE id=$1",
                params![id],
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn retry_job(&self, id: Uuid, auth: &Auth) -> Result<()> {
        let mut transaction_db = self.pool.clone();
        let mut transaction = transaction_db.transaction().await?;
        let release: Uuid = sql::scalar(
            &mut transaction,
            "SELECT b.release_id FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1",
            params![id],
        )
        .await?;
        let draft: Option<Uuid> = sql::optional_scalar(
            &mut transaction,
            "SELECT id FROM releases WHERE id=$1 AND status='draft' FOR UPDATE",
            params![release],
        )
        .await?;
        if draft.is_none() {
            return Err(ApiError::Conflict("release is sealed".into()));
        }
        sql::execute(&mut transaction,"SELECT require_active_project(b.project_id) FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1",params![id]).await?;
        let row=sql::record(&mut transaction, "SELECT j.status,j.target_config->>'executor' AS executor,b.status AS build_status,b.id,b.project_id,b.release_version,EXISTS(SELECT 1 FROM releases r WHERE r.id=b.release_id AND r.status<>'draft') AS published FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1 FOR UPDATE OF j,b", params![id], &[("status", Type::String),("executor", Type::String),("build_status", Type::String),("id", Type::Uuid),("project_id", Type::Uuid),("release_version", Type::String),("published", Type::Bool)]).await?;
        auth.project(row.decode("project_id")?, "build")?;
        if row.decode::<Option<String>>("executor")?.as_deref() == Some("buildkit")
            && !crate::images::enabled()
        {
            return Err(ApiError::Conflict(crate::images::DISABLED_MESSAGE.into()));
        }
        if row.decode::<bool>("published")?
            || ["planning", "replanning", "needs_input"]
                .contains(&row.decode::<String>("build_status")?.as_str())
            || !["failed", "lost", "canceled"].contains(&row.decode::<String>("status")?.as_str())
        {
            return Err(ApiError::Conflict(
                "only failed, lost or canceled unpublished jobs can be retried".into(),
            ));
        }
        sql::execute(
            &mut transaction,
            "UPDATE jobs SET status='queued' WHERE id=$1",
            params![id],
        )
        .await?;
        sql::execute(&mut transaction, "UPDATE builds SET status='queued',cancel_requested=false,finished_at=NULL,publish_status='not_requested',publish_error=NULL WHERE id=$1", params![row.decode::<Uuid>("id")?]).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn artifact_download(&self, id: Uuid) -> Result<ArtifactDownload> {
        let row=sql::record(&mut self.pool.clone(), "SELECT ar.object_key,b.project_id,(SELECT ra.delivery_filename FROM release_artifacts ra JOIN releases r ON r.id=ra.release_id WHERE ra.artifact_id=ar.id AND r.status='published' LIMIT 1) AS delivery_filename FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE ar.id=$1", params![id], &[("object_key", Type::String),("project_id", Type::Uuid),("delivery_filename",Type::String)]).await?;
        Ok(ArtifactDownload {
            delivery_filename: row.decode("delivery_filename")?,
            object_key: row.decode("object_key")?,
            project_id: row.decode("project_id")?,
        })
    }

    pub async fn release_artifacts(&self, id: Uuid) -> Result<Vec<dto::ReleaseArtifact>> {
        Ok(sql::scalars::<dto::ReleaseArtifact>(&mut self.pool.clone(), "SELECT (to_jsonb(ar)-'object_key') || jsonb_build_object('target',j.target,'delivery_filename',ra.delivery_filename,'release_id',ra.release_id,'release_version',r.version) FROM release_artifacts ra JOIN releases r ON r.id=ra.release_id JOIN artifacts ar ON ar.id=ra.artifact_id JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id WHERE ra.release_id=$1 ORDER BY j.target,ar.path", params![id]).await?)
    }

    pub async fn release_images(&self, id: Uuid) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(i) || jsonb_build_object('target',j.target,'name',e.spec->>'name','execution_id',e.id,'repository',r.repository,'version',r.version,'release_id',r.release_id,'release_version',r.version,'digest',r.root_digest,'root_digest',r.root_digest,'manifest_digests',r.manifest_digests) FROM release_images r JOIN image_artifacts i ON i.id=r.image_id JOIN jobs j ON j.id=i.job_id JOIN image_executions e ON e.id=i.execution_id WHERE r.release_id=$1 ORDER BY j.target", params![id]).await?)
    }

    pub async fn log_chunks(&self, id: Uuid, cursor: i64) -> Result<Vec<LogChunk>> {
        let rows=sql::records(&mut self.pool.clone(), "SELECT l.id,l.attempt_id,l.step,l.object_key FROM log_chunks l JOIN attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=a.job_id WHERE j.build_id=$1 AND l.id>$2 ORDER BY l.id LIMIT 100", params![id, cursor], &[("id", Type::I64),("attempt_id", Type::Uuid),("step", Type::String),("object_key", Type::String)]).await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                Ok::<_, artifactory_database::Error>(LogChunk {
                    id: row.decode("id")?,
                    attempt_id: row.decode("attempt_id")?,
                    step: row.decode("step")?,
                    object_key: row.decode("object_key")?,
                })
            })
            .collect::<artifactory_database::Result<_>>()?)
    }

    pub async fn historical_log_chunks(
        &self,
        build: Uuid,
        attempt: Option<Uuid>,
        step: Option<&str>,
        before: i64,
    ) -> Result<Vec<LogChunk>> {
        let rows: Vec<Value> = sql::scalars(&mut self.pool.clone(),
            "SELECT jsonb_build_object('id',l.id,'attempt_id',l.attempt_id,'step',l.step,'object_key',l.object_key) FROM log_chunks l JOIN attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=a.job_id WHERE j.build_id=$1 AND ($2::uuid IS NULL OR a.id=$2) AND ($3::text IS NULL OR l.step=$3) AND l.id<$4 ORDER BY l.id DESC LIMIT 101",
            params![build, attempt, step, before]).await?;
        Ok(rows
            .into_iter()
            .map(serde_json::from_value)
            .collect::<std::result::Result<_, _>>()
            .map_err(anyhow::Error::from)?)
    }
}
