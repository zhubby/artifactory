//! [INPUT]: 依赖租户状态、工作区任务、镜像执行和设备终端租约。
//! [OUTPUT]: 持久化归档、删除租约、终端排空及写入宽限期。
//! [POS]: 在清理前等待构建结束与设备终端的回收安全窗口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use uuid::Uuid;

const PUBLIC: &str =
    "to_jsonb(o)-'manifest'-'lease_owner'-'lease_until'-'generation'-'object_cursor'";

/// 撤权最多五秒，最后一份设备执行租约再存活四十五秒。
pub(super) const TERMINAL_DRAIN_SECONDS: i64 =
    artifactory_emissary_protocol::terminal::LEASE_SECONDS as i64 + 5;

const DRAIN_TERMINALS: &str = r#"
WITH stopped AS (
    UPDATE terminal_sessions SET
        state=CASE WHEN generation=0 AND attached_at IS NULL THEN 'closed' ELSE 'closing' END,
        finished_at=CASE WHEN generation=0 AND attached_at IS NULL THEN clock_timestamp() ELSE NULL END,
        updated_at=clock_timestamp(),reason='tenant_draining',ticket_hash=NULL,ticket_expires_at=NULL
    WHERE tenant_id=$1 AND finished_at IS NULL
    RETURNING id,tenant_id,reason,exit_code,exit_signal,finished_at
)
INSERT INTO audit_events(tenant_id,actor,action,resource_id,details)
SELECT tenant_id,'terminal-runtime','terminal_closed',id::text,
    jsonb_build_object('reason',reason,'code',exit_code,'signal',exit_signal,
        'account',NULL,'input_bytes',0,'output_bytes',0)
FROM stopped WHERE finished_at IS NOT NULL
"#;

const CLOSE_DRAINED_TERMINALS: &str = r#"
WITH stopped AS (
    UPDATE terminal_sessions SET state='closed',finished_at=clock_timestamp(),
        updated_at=clock_timestamp(),reason='tenant_draining',ticket_hash=NULL,ticket_expires_at=NULL
    WHERE tenant_id=$1 AND finished_at IS NULL
    RETURNING id,tenant_id,reason,exit_code,exit_signal
)
INSERT INTO audit_events(tenant_id,actor,action,resource_id,details)
SELECT tenant_id,'terminal-runtime','terminal_closed',id::text,
    jsonb_build_object('reason',reason,'code',exit_code,'signal',exit_signal)
FROM stopped
"#;

#[derive(Debug, serde::Deserialize)]
pub struct Lease {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Option<Uuid>,
    pub action: String,
    pub phase: String,
    pub generation: i64,
    pub manifest: Value,
    pub object_cursor: i64,
}

impl ControlStore {
    pub async fn lifecycle_impact(&self, tenant: Uuid, project: Option<Uuid>) -> Result<Value> {
        Ok(sql::scalar(&mut self.pool.clone(),"SELECT jsonb_build_object('projects',(SELECT count(*) FROM projects WHERE tenant_id=$1 AND ($2::uuid IS NULL OR id=$2)),'builds',(SELECT count(*) FROM builds WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)),'artifacts',(SELECT count(*) FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2)),'devices',(SELECT count(*) FROM nodes WHERE tenant_id=$1 AND $2::uuid IS NULL))",params![tenant,project]).await?)
    }
    pub async fn ensure_writable(
        &self,
        tenant: Uuid,
        kind: Option<&str>,
        id: Option<Uuid>,
    ) -> Result<()> {
        let active: bool = sql::scalar(
            &mut self.pool.clone(),
            "SELECT coalesce((SELECT status='active' FROM tenants WHERE id=$1),false)",
            params![tenant],
        )
        .await?;
        if !active {
            return Err(ApiError::Conflict("workspace is read only".into()));
        }
        let query = match kind {
            Some("projects") => Some("SELECT status='active' FROM projects WHERE id=$1"),
            Some("builds") => Some(
                "SELECT p.status='active' FROM builds b JOIN projects p ON p.id=b.project_id WHERE b.id=$1",
            ),
            Some("jobs") => Some(
                "SELECT p.status='active' FROM jobs j JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=b.project_id WHERE j.id=$1",
            ),
            _ => None,
        };
        if let (Some(query), Some(id)) = (query, id) {
            let active: bool = sql::optional_scalar(&mut self.pool.clone(), query, params![id])
                .await?
                .unwrap_or(false);
            if !active {
                return Err(ApiError::Conflict("workspace is read only".into()));
            }
        }
        Ok(())
    }
    pub async fn rename_tenant(&self, name: &str) -> Result<Value> {
        let tenant = self.tenant()?;
        let value: Value=sql::scalar(&mut self.pool.clone(),"UPDATE tenants SET name=$2 WHERE id=$1 AND status='active' RETURNING to_jsonb(tenants)",params![tenant,name]).await?;
        Ok(value)
    }

    pub async fn lifecycle_operations(&self, actor: Uuid) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),&format!("SELECT {PUBLIC} FROM lifecycle_operations o WHERE actor_id=$1 ORDER BY created_at DESC LIMIT 100"),params![actor]).await?)
    }

    pub async fn lifecycle_operation(&self, actor: Uuid, id: Uuid) -> Result<Value> {
        sql::optional_scalar(
            &mut self.pool.clone(),
            &format!("SELECT {PUBLIC} FROM lifecycle_operations o WHERE id=$1 AND actor_id=$2"),
            params![id, actor],
        )
        .await?
        .ok_or(ApiError::NotFound)
    }

    /// Parent lock serializes overlapping tenant/project operations and task admission.
    pub async fn start_lifecycle(
        &self,
        auth: &Auth,
        project: Option<Uuid>,
        action: &str,
        key: Uuid,
        slug_confirmation: Option<&str>,
        verified_password: Option<&str>,
    ) -> Result<Value> {
        auth.session_role("viewer")?;
        if let Some(project) = project {
            auth.project(project, "admin")?;
        }
        if !matches!(action, "archive" | "restore" | "delete") {
            return Err(ApiError::BadRequest("invalid lifecycle action".into()));
        }
        let tenant = auth.tenant_id;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // Idempotency is account-wide, including after the target has been purged.
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,96))",
            params![format!("{}:{key}", auth.user_id)],
        )
        .await?;
        if let Some(existing)=sql::optional_scalar::<Value>(&mut tx,"SELECT to_jsonb(o) FROM lifecycle_operations o WHERE actor_id=$1 AND idempotency_key=$2",params![auth.user_id,key]).await? {
            if existing["tenant_id"]!=json!(tenant) || existing["project_id"]!=json!(project) || existing["action"]!=action { return Err(ApiError::Conflict("idempotency key was used for a different operation".into())); }
            let id=existing["id"].as_str().and_then(|v|Uuid::parse_str(v).ok()).ok_or(ApiError::NotFound)?;
            tx.commit().await?;
            return self.lifecycle_operation(auth.user_id,id).await;
        }
        let parent: Value = sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(t) FROM tenants t WHERE id=$1 FOR UPDATE",
            params![tenant],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        let role: Option<String> = sql::optional_scalar(
            &mut tx,
            "SELECT role FROM tenant_members WHERE tenant_id=$1 AND user_id=$2",
            params![tenant, auth.user_id],
        )
        .await?;
        if project.is_none()
            && (role.as_deref() != Some("owner") || parent["owner_id"] != json!(auth.user_id))
        {
            return Err(ApiError::Forbidden);
        }
        if project.is_some() && !matches!(role.as_deref(), Some("owner" | "admin")) {
            let allowed:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM project_members WHERE tenant_id=$1 AND project_id=$2 AND user_id=$3 AND role='admin')",params![tenant,project,auth.user_id]).await?;
            if !allowed {
                return Err(ApiError::Forbidden);
            }
        }
        let target = if let Some(project) = project {
            if parent["status"] != "active" {
                return Err(ApiError::Conflict("tenant is read only".into()));
            }
            sql::optional_scalar::<Value>(
                &mut tx,
                "SELECT to_jsonb(p) FROM projects p WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
                params![project, tenant],
            )
            .await?
            .ok_or(ApiError::NotFound)?
        } else {
            parent
        };
        let overlap:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM lifecycle_operations WHERE tenant_id=$1 AND state IN ('waiting','running','failed') AND ($2::uuid IS NULL OR project_id IS NULL OR project_id=$2))",params![tenant,project]).await?;
        if overlap {
            return Err(ApiError::Conflict(
                "another workspace operation must finish first".into(),
            ));
        }
        let previous = target["status"].as_str().ok_or(ApiError::NotFound)?;
        if !matches!(previous, "active" | "archived")
            || (action == "archive" && previous != "active")
            || (action == "restore" && previous != "archived")
        {
            return Err(ApiError::Conflict(
                "operation is not available in this workspace state".into(),
            ));
        }
        let slug = target["slug"].as_str().ok_or(ApiError::NotFound)?;
        if action == "delete" {
            if slug_confirmation != Some(slug) {
                return Err(ApiError::BadRequest(
                    "enter the complete workspace slug".into(),
                ));
            }
            let encoded: String = sql::scalar(
                &mut tx,
                "SELECT password_hash FROM users WHERE id=$1 FOR SHARE",
                params![auth.user_id],
            )
            .await?;
            if verified_password != Some(encoded.as_str()) {
                return Err(ApiError::BadRequest(
                    "current password changed; authenticate again".into(),
                ));
            }
        }
        let id = Uuid::new_v4();
        let counts:Value=sql::scalar(&mut tx,"SELECT jsonb_build_object('projects',(SELECT count(*) FROM projects WHERE tenant_id=$1 AND ($2::uuid IS NULL OR id=$2)),'builds',(SELECT count(*) FROM builds WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)),'artifacts',(SELECT count(*) FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2)),'devices',CASE WHEN $2::uuid IS NULL THEN (SELECT count(*) FROM nodes WHERE tenant_id=$1) ELSE 0 END)",params![tenant,project]).await?;
        let restored = action == "restore";
        sql::execute(&mut tx,"INSERT INTO lifecycle_operations(id,actor_id,tenant_id,project_id,slug,action,state,phase,previous_status,idempotency_key,counts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",params![id,auth.user_id,tenant,project,slug,action,if restored{"succeeded"}else{"waiting"},if restored{"complete"}else{"draining"},previous,key,counts]).await?;
        let table = if project.is_some() {
            "projects"
        } else {
            "tenants"
        };
        sql::execute(&mut tx,&format!("UPDATE {table} SET status=$2,archived_at=CASE WHEN $2='active' THEN NULL ELSE archived_at END WHERE id=$1"),params![project.unwrap_or(tenant),if restored{"active"}else{"draining"}]).await?;
        if project.is_none() && !restored {
            // 先围栏租户准入，再保留曾附着终端的关闭记录。
            sql::execute(&mut tx, DRAIN_TERMINALS, params![tenant]).await?;
        }
        tx.commit().await?;
        self.lifecycle_operation(auth.user_id, id).await
    }

    pub async fn cancel_lifecycle(&self, actor: Uuid, id: Uuid) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // Match the worker's parent-before-operation lock ordering.
        let tenant: Uuid = sql::optional_scalar(
            &mut tx,
            "SELECT tenant_id FROM lifecycle_operations WHERE id=$1 AND actor_id=$2",
            params![id, actor],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        sql::execute(
            &mut tx,
            "SELECT id FROM tenants WHERE id=$1 FOR UPDATE",
            params![tenant],
        )
        .await?;
        let op: Value = sql::scalar(
            &mut tx,
            "SELECT to_jsonb(o) FROM lifecycle_operations o WHERE id=$1 AND actor_id=$2 FOR UPDATE",
            params![id, actor],
        )
        .await?;
        if op["state"] != "waiting" || op["phase"] != "draining" {
            return Err(ApiError::Conflict(
                "cleanup has started; this operation can no longer be canceled".into(),
            ));
        }
        let project = op["project_id"]
            .as_str()
            .and_then(|v| Uuid::parse_str(v).ok());
        let table = if project.is_some() {
            "projects"
        } else {
            "tenants"
        };
        sql::execute(
            &mut tx,
            &format!("UPDATE {table} SET status=$2 WHERE id=$1"),
            params![
                project.unwrap_or(tenant),
                op["previous_status"].as_str().ok_or(ApiError::NotFound)?
            ],
        )
        .await?;
        sql::execute(&mut tx,"UPDATE lifecycle_operations SET state='canceled',phase='complete',blockers='[]',generation=generation+1,lease_owner=NULL,lease_until=NULL,updated_at=clock_timestamp() WHERE id=$1",params![id]).await?;
        tx.commit().await?;
        self.lifecycle_operation(actor, id).await
    }

    pub async fn retry_lifecycle(&self, actor: Uuid, id: Uuid) -> Result<Value> {
        let changed=sql::execute(&mut self.pool.clone(),"UPDATE lifecycle_operations SET state=CASE WHEN phase='draining' THEN 'waiting' ELSE 'running' END,error=NULL,next_run_at=clock_timestamp(),lease_owner=NULL,lease_until=NULL,generation=generation+1,updated_at=clock_timestamp() WHERE id=$1 AND actor_id=$2 AND state='failed'",params![id,actor]).await?;
        if changed == 0 {
            self.lifecycle_operation(actor, id).await?;
            return Err(ApiError::Conflict(
                "only failed operations can be retried".into(),
            ));
        }
        self.lifecycle_operation(actor, id).await
    }

    pub async fn lease_lifecycle(&self, owner: Uuid) -> Result<Option<Lease>> {
        let row:Option<Value>=sql::optional_scalar(&mut self.pool.clone(),"UPDATE lifecycle_operations SET lease_owner=$1,lease_until=clock_timestamp()+interval '120 seconds',generation=generation+1 WHERE id=(SELECT id FROM lifecycle_operations WHERE state IN ('waiting','running') AND next_run_at<=clock_timestamp() AND (lease_until IS NULL OR lease_until<clock_timestamp()) ORDER BY next_run_at,created_at LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING to_jsonb(lifecycle_operations)",params![owner]).await?;
        row.map(|v| serde_json::from_value(v).map_err(|e| ApiError::Internal(e.into())))
            .transpose()
    }

    pub async fn lifecycle_blockers(
        &self,
        tenant: Uuid,
        project: Option<Uuid>,
    ) -> Result<Vec<Value>> {
        let mut blockers = sql::scalars(&mut self.pool.clone(),"WITH target AS (SELECT id FROM builds WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)) SELECT jsonb_build_object('kind','build','id',b.id,'status',b.status,'tenant_slug',tn.slug,'project_slug',p.slug,'number',b.number) FROM builds b JOIN target t ON t.id=b.id JOIN projects p ON p.id=b.project_id JOIN tenants tn ON tn.id=b.tenant_id WHERE b.status NOT IN ('succeeded','failed','canceled','lost') UNION ALL SELECT jsonb_build_object('kind','publication','id',b.id,'status',b.publish_status) FROM builds b JOIN target t ON t.id=b.id WHERE b.publish_status='pending' AND b.status NOT IN ('failed','canceled','lost') UNION ALL SELECT jsonb_build_object('kind','planning','id',r.id,'status',r.state) FROM planning_runs r JOIN target t ON t.id=r.build_id WHERE r.state IN ('queued','leased') UNION ALL SELECT jsonb_build_object('kind','attempt','id',a.id,'status',a.status) FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN target t ON t.id=j.build_id WHERE a.status IN ('leased','running') UNION ALL SELECT jsonb_build_object('kind','image_execution','id',e.id,'status',e.status) FROM image_executions e WHERE e.tenant_id=$1 AND ($2::uuid IS NULL OR e.project_id=$2) AND e.status IN ('preparing','running') UNION ALL SELECT jsonb_build_object('kind','device_update','id',u.id,'status',u.status) FROM emissary_updates u WHERE u.tenant_id=$1 AND ($2::uuid IS NULL OR EXISTS(SELECT 1 FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN target t ON t.id=j.build_id WHERE a.node_id=u.node_id)) AND u.status NOT IN ('succeeded','failed','rolled_back','canceled') UNION ALL SELECT jsonb_build_object('kind',kind,'id',id,'status','running') FROM workspace_tasks WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2) AND expires_at>clock_timestamp()",params![tenant,project]).await?;
        if project.is_none() {
            // Control 的 closed 不是设备回收确认；删除前保守等待最后一份租约。
            let terminals = sql::scalars(
                &mut self.pool.clone(),
                r#"
                SELECT jsonb_build_object('kind','terminal','id',s.id,'status','lease_expiring')
                FROM terminal_sessions s JOIN lifecycle_operations o ON o.tenant_id=s.tenant_id
                WHERE s.tenant_id=$1 AND (s.generation>0 OR s.attached_at IS NOT NULL)
                    AND (s.finished_at IS NULL OR s.finished_at>=o.created_at-$2::bigint*interval '1 second')
                    AND o.project_id IS NULL AND o.action IN ('archive','delete')
                    AND o.state IN ('waiting','running','failed')
                    AND (o.manifest->>'terminal_drain_until' IS NULL
                        OR (o.manifest->>'terminal_drain_until')::timestamptz>clock_timestamp())
            "#,
                params![tenant, TERMINAL_DRAIN_SECONDS],
            )
            .await?;
            blockers.extend(terminals);
        }
        Ok(blockers)
    }

    pub async fn advance_lifecycle_drain(&self, lease: &Lease) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT id FROM tenants WHERE id=$1 FOR UPDATE",
            params![lease.tenant_id],
        )
        .await?;
        let valid:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM lifecycle_operations WHERE id=$1 AND generation=$2 AND state='waiting' AND phase='draining' AND lease_until>clock_timestamp())",params![lease.id,lease.generation]).await?;
        if !valid {
            return Ok(());
        }
        if lease.project_id.is_none() {
            // 首次观察已提交的租户围栏后计时；锁等待和提交延迟不消耗回收窗口。
            sql::execute(&mut tx,r#"
                UPDATE lifecycle_operations o SET manifest=jsonb_set(manifest,'{terminal_drain_until}',
                    to_jsonb((clock_timestamp()+$2::bigint*interval '1 second')::text))
                WHERE o.id=$1 AND NOT o.manifest ? 'terminal_drain_until'
                    AND EXISTS(SELECT 1 FROM terminal_sessions s WHERE s.tenant_id=o.tenant_id
                        AND (s.generation>0 OR s.attached_at IS NOT NULL)
                        AND (s.finished_at IS NULL OR s.finished_at>=o.created_at-$2::bigint*interval '1 second'))
            "#,params![lease.id,TERMINAL_DRAIN_SECONDS]).await?;
        }
        // Admission is already fenced. Ongoing work can only decrease; the final transaction
        // still rechecks the durable task rows before changing the workspace state.
        let blockers = self
            .lifecycle_blockers(lease.tenant_id, lease.project_id)
            .await?;
        if !blockers.is_empty() {
            sql::execute(&mut tx,"UPDATE lifecycle_operations SET blockers=$3,lease_owner=NULL,lease_until=NULL,next_run_at=clock_timestamp()+interval '2 seconds',updated_at=clock_timestamp() WHERE id=$1 AND generation=$2",params![lease.id,lease.generation,json!(blockers)]).await?;
        } else if lease.action == "archive" {
            if lease.project_id.is_none() {
                sql::execute(&mut tx, CLOSE_DRAINED_TERMINALS, params![lease.tenant_id]).await?;
            }
            let table = if lease.project_id.is_some() {
                "projects"
            } else {
                "tenants"
            };
            sql::execute(
                &mut tx,
                &format!(
                    "UPDATE {table} SET status='archived',archived_at=clock_timestamp() WHERE id=$1"
                ),
                params![lease.project_id.unwrap_or(lease.tenant_id)],
            )
            .await?;
            sql::execute(&mut tx,"UPDATE lifecycle_operations SET state='succeeded',phase='complete',blockers='[]',lease_owner=NULL,lease_until=NULL,updated_at=clock_timestamp() WHERE id=$1 AND generation=$2",params![lease.id,lease.generation]).await?;
        } else {
            if lease.project_id.is_none() {
                sql::execute(&mut tx, CLOSE_DRAINED_TERMINALS, params![lease.tenant_id]).await?;
            }
            let table = if lease.project_id.is_some() {
                "projects"
            } else {
                "tenants"
            };
            sql::execute(
                &mut tx,
                &format!("UPDATE {table} SET status='deleting' WHERE id=$1"),
                params![lease.project_id.unwrap_or(lease.tenant_id)],
            )
            .await?;
            // Hold the parent admission lock while recording a complete, durable inventory.
            let mut manifest:Value=sql::scalar(&mut tx,"SELECT jsonb_build_object('projects',(SELECT coalesce(jsonb_agg(id),'[]') FROM projects WHERE tenant_id=$1 AND ($2::uuid IS NULL OR id=$2)),'nodes',(SELECT coalesce(jsonb_agg(id),'[]') FROM nodes WHERE tenant_id=$1 AND $2::uuid IS NULL),'builds',(SELECT coalesce(jsonb_agg(id),'[]') FROM builds WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)),'objects',(SELECT coalesce(jsonb_agg(DISTINCT key),'[]') FROM (SELECT u.object_key AS key FROM uploads u JOIN attempts a ON a.id=u.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2) UNION SELECT 'artifacts/'||u.id::text FROM uploads u JOIN attempts a ON a.id=u.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2) UNION SELECT ar.object_key FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2) UNION SELECT l.object_key FROM image_logs l JOIN image_executions e ON e.id=l.execution_id WHERE e.tenant_id=$1 AND ($2::uuid IS NULL OR e.project_id=$2) UNION SELECT l.object_key FROM log_chunks l JOIN attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2)) keys),'uploads_expire_at',(SELECT coalesce(max(u.expires_at),clock_timestamp()) FROM uploads u JOIN attempts a ON a.id=u.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2)))",params![lease.tenant_id,lease.project_id]).await?;
            let terminal_drain: Value = sql::scalar(&mut tx,
                "SELECT coalesce(manifest->'terminal_drain_until','null'::jsonb) FROM lifecycle_operations WHERE id=$1",
                params![lease.id]).await?;
            if !terminal_drain.is_null() {
                manifest["terminal_drain_until"] = terminal_drain;
            }
            manifest["attempts"]=sql::scalar(&mut tx,"SELECT coalesce(jsonb_agg(a.id),'[]') FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.project_id=$2)",params![lease.tenant_id,lease.project_id]).await?;
            manifest["image_log_projects"]=sql::scalar(&mut tx,"SELECT coalesce(jsonb_agg(DISTINCT project_id),'[]') FROM image_executions WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)",params![lease.tenant_id,lease.project_id]).await?;
            manifest["audit_ids"] = sql::scalar(&mut tx,"WITH target AS (SELECT id FROM builds WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)), resources AS (SELECT id FROM projects WHERE tenant_id=$1 AND ($2::uuid IS NULL OR id=$2) UNION SELECT id FROM target UNION SELECT j.id FROM jobs j JOIN target t ON t.id=j.build_id UNION SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN target t ON t.id=j.build_id UNION SELECT ar.id FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN target t ON t.id=j.build_id UNION SELECT id FROM releases WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2) UNION SELECT id FROM planning_runs WHERE build_id IN (SELECT id FROM target) UNION SELECT id FROM source_manifests WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id=$2)) SELECT coalesce(jsonb_agg(id),'[]') FROM resources",params![lease.tenant_id,lease.project_id]).await?;
            manifest["ota"]=sql::scalar(&mut tx,"SELECT coalesce(jsonb_agg(jsonb_build_object('hash',manifest_hash,'manifest',manifest)),'[]') FROM emissary_releases WHERE tenant_id=$1 AND $2::uuid IS NULL",params![lease.tenant_id,lease.project_id]).await?;
            manifest["planning"] = json!(std::env::var("ARTIFACTORY_PLANNING_URL").is_ok());
            if manifest["attempts"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
                || manifest["image_log_projects"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
            {
                let uploads_expire_at: String = sql::scalar(&mut tx,"SELECT greatest(($1::jsonb->>'uploads_expire_at')::timestamptz,clock_timestamp()+interval '5 minutes')::text",params![&manifest]).await?;
                manifest["uploads_expire_at"] = json!(uploads_expire_at);
            }
            sql::execute(&mut tx,"UPDATE lifecycle_operations SET state='running',phase='services',manifest=$3,blockers='[]',lease_owner=NULL,lease_until=NULL,updated_at=clock_timestamp() WHERE id=$1 AND generation=$2",params![lease.id,lease.generation,manifest]).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn checkpoint_lifecycle(&self, lease: &Lease, phase: &str) -> Result<bool> {
        Ok(sql::execute(&mut self.pool.clone(),"UPDATE lifecycle_operations SET phase=$3,object_cursor=0,lease_owner=NULL,lease_until=NULL,next_run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE id=$1 AND generation=$2 AND lease_until>clock_timestamp() AND state='running'",params![lease.id,lease.generation,phase]).await?>0)
    }

    pub async fn renew_lifecycle(&self, lease: &Lease) -> Result<bool> {
        Ok(sql::execute(&mut self.pool.clone(), "UPDATE lifecycle_operations SET lease_until=clock_timestamp()+interval '120 seconds' WHERE id=$1 AND generation=$2 AND lease_until>clock_timestamp() AND state IN ('waiting','running')", params![lease.id,lease.generation]).await? > 0)
    }

    pub async fn advance_lifecycle_cursor(&self, lease: &Lease, cursor: i64) -> Result<()> {
        sql::execute(&mut self.pool.clone(), "UPDATE lifecycle_operations SET object_cursor=$3,lease_owner=NULL,lease_until=NULL,next_run_at=clock_timestamp(),updated_at=clock_timestamp() WHERE id=$1 AND generation=$2 AND lease_until>clock_timestamp() AND state='running'", params![lease.id,lease.generation,cursor]).await?;
        Ok(())
    }

    pub async fn fail_lifecycle(&self, lease: &Lease) -> Result<()> {
        // Store a safe message; transport errors can contain signed URLs or credentials.
        sql::execute(&mut self.pool.clone(),"UPDATE lifecycle_operations SET state='failed',error='Cleanup step failed. Retry after service recovery.',lease_owner=NULL,lease_until=NULL,updated_at=clock_timestamp() WHERE id=$1 AND generation=$2 AND state IN ('waiting','running')",params![lease.id,lease.generation]).await?;
        Ok(())
    }
}
