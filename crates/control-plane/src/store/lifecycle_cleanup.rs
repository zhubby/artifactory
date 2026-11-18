//! [INPUT]: 依赖工作区删除租约和资源清单。
//! [OUTPUT]: 删除项目状态与租户签名密文。
//! [POS]: 持久化生命周期清理的数据库阶段。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, lifecycle::Lease};
use crate::error::{ApiError, Result};
use artifactory_database::{params, sql};
use serde_json::json;
use uuid::Uuid;

impl ControlStore {
    pub async fn begin_workspace_task(&self, kind: &str, project: Option<Uuid>) -> Result<Uuid> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT require_active_tenant($1)",
            params![self.tenant()?],
        )
        .await?;
        if let Some(p) = project {
            sql::execute(&mut tx, "SELECT require_active_project($1)", params![p]).await?;
        }
        let id = Uuid::new_v4();
        sql::execute(&mut tx,"INSERT INTO workspace_tasks(id,tenant_id,project_id,kind,expires_at) VALUES($1,$2,$3,$4,clock_timestamp()+interval '15 minutes')",params![id,self.tenant()?,project,kind]).await?;
        tx.commit().await?;
        Ok(id)
    }
    pub async fn end_workspace_task(&self, id: Uuid) -> Result<()> {
        sql::execute(
            &mut self.pool.clone(),
            "DELETE FROM workspace_tasks WHERE id=$1",
            params![id],
        )
        .await?;
        Ok(())
    }
    pub async fn bind_task_object(&self, task: Uuid, hash: &str) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,97))",
            params![hash],
        )
        .await?;
        let deleting: bool = sql::scalar(
            &mut tx,
            "SELECT EXISTS(SELECT 1 FROM shared_object_deletions WHERE hash=$1)",
            params![hash],
        )
        .await?;
        if deleting {
            return Err(ApiError::Conflict(
                "release assets are being cleaned; retry the import".into(),
            ));
        }
        let updated=sql::execute(&mut tx,"UPDATE workspace_tasks SET object_hash=$2 WHERE id=$1 AND expires_at>clock_timestamp()",params![task,hash]).await?;
        if updated == 0 {
            return Err(ApiError::Conflict("management task expired".into()));
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn revoke_lifecycle_identities(&self, lease: &Lease) -> Result<()> {
        if lease.project_id.is_some() {
            return Ok(());
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let valid:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM lifecycle_operations WHERE id=$1 AND generation=$2 AND state='running' AND lease_until>clock_timestamp() FOR UPDATE)",params![lease.id,lease.generation]).await?;
        if !valid {
            return Err(ApiError::Conflict("operation lease expired".into()));
        }
        sql::execute(&mut tx,"UPDATE nodes SET revoked=true,active_session=NULL,connection_owner=NULL WHERE tenant_id=$1",params![lease.tenant_id]).await?;
        sql::execute(
            &mut tx,
            "UPDATE api_tokens SET revoked=true WHERE tenant_id=$1",
            params![lease.tenant_id],
        )
        .await?;
        sql::execute(&mut tx,"UPDATE tenant_invitations SET revoked_at=coalesce(revoked_at,clock_timestamp()) WHERE tenant_id=$1",params![lease.tenant_id]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn lifecycle_uploads_expired(&self, lease: &Lease) -> Result<bool> {
        Ok(sql::scalar(&mut self.pool.clone(),"SELECT (manifest->>'uploads_expire_at')::timestamptz<clock_timestamp() FROM lifecycle_operations WHERE id=$1 AND generation=$2",params![lease.id,lease.generation]).await?)
    }
    pub async fn defer_lifecycle(&self, lease: &Lease) -> Result<()> {
        sql::execute(&mut self.pool.clone(),"UPDATE lifecycle_operations SET lease_owner=NULL,lease_until=NULL,next_run_at=clock_timestamp()+interval '15 seconds',updated_at=clock_timestamp() WHERE id=$1 AND generation=$2",params![lease.id,lease.generation]).await?;
        Ok(())
    }
    pub async fn claim_ota_cleanup(&self, hash: &str, lease: &Lease) -> Result<Option<bool>> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,97))",
            params![hash],
        )
        .await?;
        let shared:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM emissary_releases r JOIN tenants t ON t.id=r.tenant_id WHERE r.manifest_hash=$1 AND r.tenant_id<>$2 AND t.status<>'deleting')",params![hash,lease.tenant_id]).await?;
        if shared {
            return Ok(Some(false));
        }
        let busy: bool = sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM workspace_tasks WHERE object_hash=$1 AND expires_at>clock_timestamp()) OR EXISTS(SELECT 1 FROM shared_object_deletions WHERE hash=$1 AND operation_id<>$2)",params![hash,lease.id]).await?;
        if busy {
            return Ok(None);
        }
        sql::execute(&mut tx,"INSERT INTO shared_object_deletions(hash,operation_id) VALUES($1,$2) ON CONFLICT DO NOTHING",params![hash,lease.id]).await?;
        tx.commit().await?;
        Ok(Some(true))
    }
    pub async fn purge_lifecycle_control(&self, lease: &Lease) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT id FROM tenants WHERE id=$1 FOR UPDATE",
            params![lease.tenant_id],
        )
        .await?;
        let valid:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM lifecycle_operations WHERE id=$1 AND generation=$2 AND state='running' AND lease_until>clock_timestamp() FOR UPDATE)",params![lease.id,lease.generation]).await?;
        if !valid {
            return Err(ApiError::Conflict("operation lease expired".into()));
        }
        let projects: Vec<Uuid> = serde_json::from_value(lease.manifest["projects"].clone())
            .map_err(anyhow::Error::from)?;
        // Persist link tombstones before removing identities; all statements commit atomically.
        sql::execute(&mut tx,"INSERT INTO deleted_projects(id,tenant_id,slug) SELECT id,tenant_id,slug FROM projects WHERE id=ANY($1) ON CONFLICT DO NOTHING",params![&projects]).await?;
        for statement in [
            "DELETE FROM release_images WHERE release_id IN(SELECT id FROM releases WHERE project_id=ANY($1))",
            "DELETE FROM image_artifacts WHERE project_id=ANY($1)",
            "DELETE FROM image_executions WHERE project_id=ANY($1)",
            "DELETE FROM image_credentials WHERE project_id=ANY($1)",
            "DELETE FROM release_artifacts WHERE release_id IN(SELECT id FROM releases WHERE project_id=ANY($1))",
            "DELETE FROM releases WHERE project_id=ANY($1)",
            "DELETE FROM publish_attempts WHERE build_id IN(SELECT id FROM builds WHERE project_id=ANY($1))",
            "DELETE FROM runtime_receipts WHERE run_id IN(SELECT r.id FROM planning_runs r JOIN builds b ON b.id=r.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM runtime_intents WHERE run_id IN(SELECT r.id FROM planning_runs r JOIN builds b ON b.id=r.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM planning_events WHERE run_id IN(SELECT r.id FROM planning_runs r JOIN builds b ON b.id=r.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM device_commands WHERE attempt_id IN(SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM artifacts WHERE attempt_id IN(SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM uploads WHERE attempt_id IN(SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM log_chunks WHERE attempt_id IN(SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM step_runs WHERE attempt_id IN(SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM attempt_events WHERE attempt_id IN(SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM attempts WHERE job_id IN(SELECT j.id FROM jobs j JOIN builds b ON b.id=j.build_id WHERE b.project_id=ANY($1))",
            "DELETE FROM jobs WHERE build_id IN(SELECT id FROM builds WHERE project_id=ANY($1))",
            "UPDATE builds SET current_plan_id=NULL WHERE project_id=ANY($1)",
            "DELETE FROM plan_revisions WHERE build_id IN(SELECT id FROM builds WHERE project_id=ANY($1))",
            "DELETE FROM planning_runs WHERE build_id IN(SELECT id FROM builds WHERE project_id=ANY($1))",
            "DELETE FROM source_tasks WHERE project_id=ANY($1)",
            "DELETE FROM builds WHERE project_id=ANY($1)",
            "DELETE FROM source_manifests WHERE project_id=ANY($1)",
            "DELETE FROM audit_events WHERE tenant_id=(SELECT tenant_id FROM projects WHERE id=ANY($1) LIMIT 1) AND resource_id IN(SELECT jsonb_array_elements_text($2::jsonb->'audit_ids'))",
            "DELETE FROM projects WHERE id=ANY($1)",
        ] {
            let values = if statement.contains("$2") {
                params![&projects, &lease.manifest]
            } else {
                params![&projects]
            };
            sql::execute(&mut tx, statement, values).await?;
        }
        // Remove project permissions from surviving tenant tokens without revoking other projects.
        sql::execute(&mut tx,"UPDATE api_tokens SET project_ids=ARRAY(SELECT p FROM unnest(project_ids) p WHERE NOT p=ANY($1)) WHERE tenant_id=$2 AND project_ids && $1",params![&projects,lease.tenant_id]).await?;
        if lease.project_id.is_none() {
            for table in [
                "signing_profiles",
                "signing_credentials",
                "emissary_updates",
                "enrollment_tokens",
                "nodes",
                "node_pools",
                "emissary_releases",
                "model_profiles",
                "credentials",
                "api_tokens",
                "tenant_invitations",
                "tenant_members",
                "audit_events",
            ] {
                sql::execute(
                    &mut tx,
                    &format!("DELETE FROM {table} WHERE tenant_id=$1"),
                    params![lease.tenant_id],
                )
                .await?;
            }
            sql::execute(&mut tx,"INSERT INTO deleted_tenants(id,slug) SELECT id,slug FROM tenants WHERE id=$1 ON CONFLICT DO NOTHING",params![lease.tenant_id]).await?;
            sql::execute(
                &mut tx,
                "DELETE FROM tenants WHERE id=$1",
                params![lease.tenant_id],
            )
            .await?;
        }
        sql::execute(&mut tx,"UPDATE lifecycle_operations SET state='succeeded',phase='complete',manifest='{}',blockers='[]',lease_owner=NULL,lease_until=NULL,updated_at=clock_timestamp() WHERE id=$1 AND generation=$2",params![lease.id,lease.generation]).await?;
        sql::execute(
            &mut tx,
            "DELETE FROM shared_object_deletions WHERE operation_id=$1",
            params![lease.id],
        )
        .await?;
        sql::execute(&mut tx,"DELETE FROM workspace_tasks WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)",params![lease.tenant_id,lease.project_id]).await?;
        sql::execute(&mut tx,"INSERT INTO audit_events(actor,action,resource_id,details) SELECT actor_id::text,'workspace_deleted',id::text,$2 FROM lifecycle_operations WHERE id=$1",params![lease.id,json!({"tenant_id":lease.tenant_id,"project_id":lease.project_id})]).await?;
        tx.commit().await?;
        Ok(())
    }
}
