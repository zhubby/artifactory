//! [INPUT]: 依赖固定 Job、租户授权和加密任务材料。
//! [OUTPUT]: 保存候选与 CR UID，事务确认成功并登记唯一制品。
//! [POS]: Control 是成功写入者；网络校验在事务外完成。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::{
    crypto,
    error::{ApiError, Result},
};
use anyhow::ensure;
use artifactory_database::{params, sql};
use artifactory_image_protocol::{ImageSpec, PlatformResult, Report};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Deserialize)]
pub struct ImageExecution {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub job_id: Uuid,
    pub source_manifest_id: Uuid,
    pub plan_revision_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub operation: String,
    pub status: String,
    pub spec: ImageSpec,
    pub candidate_repository: String,
    pub destination_repository: String,
    pub destination_credential: Option<Uuid>,
    pub destination_registry: Option<String>,
    pub tags: Vec<String>,
    pub root_digest: Option<String>,
    pub published_digest: Option<String>,
    pub image_build_uid: Option<String>,
    pub task_envelope: Option<String>,
    pub results: Vec<PlatformResult>,
    pub cancel_requested: bool,
    pub timeout_seconds: u32,
    #[serde(default)]
    pub task_timeout_seconds: u32,
}
#[derive(Debug, PartialEq, Eq)]
/// 成功只回收材料；失败和取消主动终止残留任务。
pub enum ImageStop {
    Cleanup,
    Terminate(&'static str),
}
const ROW: &str = "SELECT to_jsonb(e)||jsonb_build_object('timeout_seconds',greatest(1,extract(epoch FROM e.deadline_at-now())::int),'task_timeout_seconds',greatest(1,extract(epoch FROM e.deadline_at-e.created_at)::int)) FROM image_executions e";
// 构建锁先于执行锁，顺序与取消、重试一致。
const LIVE: &str = "SELECT e.id FROM image_executions e JOIN jobs j ON j.id=e.job_id JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=e.project_id JOIN tenants t ON t.id=e.tenant_id WHERE e.id=$1 AND e.status IN ('preparing','running') AND NOT e.cancel_requested AND NOT b.cancel_requested AND e.deadline_at>clock_timestamp() AND b.source_manifest_id=e.source_manifest_id AND j.plan_revision_id IS NOT DISTINCT FROM e.plan_revision_id AND p.status IN ('active','draining') AND t.status IN ('active','draining') AND (e.operation='publish' OR j.status IN ('leased','running'))";
impl ControlStore {
    pub async fn image_execution(&self, id: Uuid) -> anyhow::Result<ImageExecution> {
        Ok(serde_json::from_value(
            sql::scalar::<Value>(
                &mut self.pool.clone(),
                &format!("{ROW} WHERE e.id=$1"),
                params![id],
            )
            .await?,
        )?)
    }
    pub async fn image_live(&self, id: Uuid) -> anyhow::Result<bool> {
        Ok(
            sql::optional_scalar::<Uuid>(&mut self.pool.clone(), LIVE, params![id])
                .await?
                .is_some(),
        )
    }
    pub async fn image_stop_status(&self, id: Uuid) -> anyhow::Result<Option<ImageStop>> {
        let row = sql::optional_record(&mut self.pool.clone(), &format!("SELECT e.status,e.cancel_requested OR b.cancel_requested OR b.source_manifest_id IS DISTINCT FROM e.source_manifest_id OR j.plan_revision_id IS DISTINCT FROM e.plan_revision_id OR p.status NOT IN ('active','draining') OR t.status NOT IN ('active','draining') AS canceled FROM image_executions e JOIN jobs j ON j.id=e.job_id JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=e.project_id JOIN tenants t ON t.id=e.tenant_id WHERE e.id=$1 AND NOT EXISTS({LIVE})"), params![id], &[("status",toasty::stmt::Type::String),("canceled",toasty::stmt::Type::Bool)]).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let status = row.decode::<String>("status")?;
        if status == "succeeded" {
            return Ok(Some(ImageStop::Cleanup));
        }
        Ok(Some(ImageStop::Terminate(
            if status == "canceled" || row.decode::<bool>("canceled")? {
                "canceled"
            } else {
                "failed"
            },
        )))
    }
    pub async fn image_pending(&self) -> anyhow::Result<Vec<Uuid>> {
        Ok(sql::scalars(&mut self.pool.clone(),"WITH pending AS (SELECT e.id FROM image_executions e JOIN jobs j ON j.id=e.job_id JOIN builds b ON b.id=j.build_id WHERE e.status='running' OR (e.status='preparing' AND e.created_at<now()-interval '5 minutes') OR (e.status NOT IN ('preparing','running') AND e.task_envelope IS NOT NULL) ORDER BY (e.cancel_requested OR b.cancel_requested OR e.deadline_at<=clock_timestamp() OR e.status NOT IN ('preparing','running')) DESC,e.last_dispatched_at NULLS FIRST,e.created_at,e.id LIMIT 32 FOR UPDATE OF e SKIP LOCKED) UPDATE image_executions e SET last_dispatched_at=clock_timestamp() FROM pending p WHERE e.id=p.id RETURNING e.id",params![]).await?)
    }
    pub async fn clear_image_task(&self, id: Uuid) -> anyhow::Result<()> {
        sql::execute(&mut self.pool.clone(),"UPDATE image_executions SET task_envelope=NULL WHERE id=$1 AND status NOT IN ('preparing','running')",params![id]).await?;
        Ok(())
    }
    pub async fn image_candidates(&self) -> anyhow::Result<Vec<Uuid>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT j.id FROM jobs j JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=b.project_id JOIN tenants t ON t.id=b.tenant_id WHERE j.status='queued' AND j.target_config->>'executor'='buildkit' AND NOT b.cancel_requested AND b.source_manifest_id IS NOT NULL AND p.status IN ('active','draining') AND t.status IN ('active','draining') ORDER BY b.created_at LIMIT 16",params![]).await?)
    }
    pub async fn claim_image(
        &self,
        job: Uuid,
        id: Uuid,
        build_hash: &str,
        publish_hash: &str,
    ) -> anyhow::Result<bool> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // 工作区锁先于构建锁；排空只封锁新准入，已接受的 Job 继续执行。
        let scope: Option<Value> = sql::optional_scalar(&mut tx,"SELECT jsonb_build_object('tenant',b.tenant_id,'project',b.project_id) FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1",params![job]).await?;
        let Some(scope) = scope else { return Ok(false) };
        let tenant: Uuid = serde_json::from_value(scope["tenant"].clone())?;
        let project: Uuid = serde_json::from_value(scope["project"].clone())?;
        for (query, id) in [
            (
                "SELECT id FROM tenants WHERE id=$1 AND status IN ('active','draining') FOR SHARE",
                tenant,
            ),
            (
                "SELECT id FROM projects WHERE id=$1 AND status IN ('active','draining') FOR SHARE",
                project,
            ),
        ] {
            if sql::optional_scalar::<Uuid>(&mut tx, query, params![id])
                .await?
                .is_none()
            {
                return Ok(false);
            }
        }
        let row:Option<Value>=sql::optional_scalar(&mut tx,"SELECT jsonb_build_object('tenant',b.tenant_id,'project',b.project_id,'snapshot',b.source_manifest_id,'plan',j.plan_revision_id,'config',j.target_config) FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1 AND j.status='queued' AND j.target_config->>'executor'='buildkit' AND b.source_manifest_id IS NOT NULL AND NOT b.cancel_requested FOR UPDATE OF b,j SKIP LOCKED",params![job]).await?;
        let Some(row) = row else { return Ok(false) };
        let tenant: Uuid = serde_json::from_value(row["tenant"].clone())?;
        let project: Uuid = serde_json::from_value(row["project"].clone())?;
        let target: crate::recipe::Target = serde_json::from_value(row["config"].clone())?;
        let spec = target
            .image
            .ok_or_else(|| anyhow::anyhow!("image specification required"))?;
        spec.validate()?;
        let candidate = format!("{tenant}/{project}/builds/{}", id.simple());
        let destination = format!("{tenant}/{project}/images/{}", spec.name);
        sql::execute(&mut tx,"INSERT INTO image_executions(id,tenant_id,project_id,job_id,source_manifest_id,plan_revision_id,build_token_hash,publish_token_hash,spec,candidate_repository,destination_repository,tags,deadline_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,now()+($13::int+600)*interval '1 second')",params![id,tenant,project,job,row["snapshot"].as_str().unwrap().parse::<Uuid>()?,row["plan"].as_str().map(Uuid::parse_str).transpose()?,build_hash,publish_hash,serde_json::to_value(&spec)?,candidate,destination,&spec.tags,target.timeout_seconds as i32]).await?;
        sql::execute(
            &mut tx,
            "UPDATE jobs SET status='running' WHERE id=$1",
            params![job],
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn save_image_task(&self, id: Uuid, envelope: &str) -> anyhow::Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(&mut tx,"SELECT b.id FROM builds b JOIN jobs j ON j.build_id=b.id JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR UPDATE OF b,j",params![id]).await?;
        let live =
            sql::optional_scalar::<Uuid>(&mut tx, &format!("{LIVE} FOR UPDATE OF e"), params![id])
                .await?;
        ensure!(live.is_some(), "image execution is stale");
        sql::execute(&mut tx,"UPDATE image_executions SET task_envelope=$2,status='running' WHERE id=$1 AND task_envelope IS NULL",params![id,envelope]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn image_token(&self, id: Uuid, token: &str) -> anyhow::Result<Option<bool>> {
        let hash = crypto::hash(token);
        Ok(sql::optional_scalar(&mut self.pool.clone(),"SELECT publish_token_hash=$2 FROM image_executions WHERE id=$1 AND (publish_token_hash=$2 OR build_token_hash=$2)",params![id,hash]).await?)
    }
    /// 固定首次交付的 CR UID；同名 CR 重建不能接管执行。
    pub async fn bind_image_build(&self, id: Uuid, uid: &str) -> anyhow::Result<()> {
        ensure!(!uid.is_empty(), "ImageBuild UID required");
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(&mut tx,"SELECT b.id FROM builds b JOIN jobs j ON j.build_id=b.id JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR UPDATE OF b,j",params![id]).await?;
        ensure!(
            sql::optional_scalar::<Uuid>(&mut tx, &format!("{LIVE} FOR UPDATE OF e"), params![id])
                .await?
                .is_some(),
            "image execution is stale or canceled"
        );
        let n = sql::execute(&mut tx,"UPDATE image_executions SET image_build_uid=$2 WHERE id=$1 AND (image_build_uid IS NULL OR image_build_uid=$2)",params![id,uid]).await?;
        ensure!(n == 1, "ImageBuild UID changed");
        tx.commit().await?;
        Ok(())
    }
    /// CR 当前身份、spec 和成功状态复核后，原子提交候选。
    pub async fn complete_image(&self, id: Uuid, uid: &str) -> anyhow::Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(&mut tx,"SELECT b.id FROM builds b JOIN jobs j ON j.build_id=b.id JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR UPDATE OF b,j",params![id]).await?;
        let row: Value = sql::scalar(
            &mut tx,
            &format!("{ROW} WHERE e.id=$1 FOR UPDATE OF e"),
            params![id],
        )
        .await?;
        let execution: ImageExecution = serde_json::from_value(row)?;
        ensure!(
            execution.image_build_uid.as_deref() == Some(uid) && !uid.is_empty(),
            "ImageBuild UID changed"
        );
        if execution.status == "succeeded" {
            return Ok(());
        }
        ensure!(
            sql::optional_scalar::<Uuid>(&mut tx, LIVE, params![id])
                .await?
                .is_some(),
            "image execution is stale or canceled"
        );
        let digest = artifactory_image_protocol::sha256(&artifactory_image_protocol::index(
            &execution.spec.platforms,
            &execution.results,
        )?);
        ensure!(
            execution.published_digest.as_deref() == Some(&digest),
            "published candidate required"
        );
        ensure!(
            execution
                .root_digest
                .as_ref()
                .is_none_or(|root| root == &digest),
            "publication source changed"
        );
        let parent = execution.parent_id.unwrap_or(id);
        if execution.destination_credential.is_none() {
            let n = sql::execute(&mut tx,"INSERT INTO image_artifacts(id,tenant_id,project_id,job_id,execution_id,repository,digest,platforms) SELECT $1,tenant_id,project_id,job_id,id,destination_repository,$3,results FROM image_executions WHERE id=$2 ON CONFLICT(execution_id) DO UPDATE SET digest=image_artifacts.digest WHERE image_artifacts.digest=excluded.digest AND image_artifacts.repository=excluded.repository AND image_artifacts.platforms=excluded.platforms",params![Uuid::new_v4(),parent,&digest]).await?;
            ensure!(n == 1, "image artifact changed");
            if parent != id {
                sql::execute(&mut tx,"UPDATE image_executions SET status='succeeded',root_digest=$2,error=NULL,finished_at=now() WHERE id=$1",params![parent,&digest]).await?;
            }
            sql::execute(
                &mut tx,
                "UPDATE jobs SET status='succeeded' WHERE id=$1",
                params![execution.job_id],
            )
            .await?;
        }
        let n = sql::execute(&mut tx,"UPDATE image_executions SET status='succeeded',root_digest=$2,error=NULL,finished_at=now() WHERE id=$1 AND status IN ('preparing','running') AND NOT cancel_requested AND deadline_at>clock_timestamp()",params![id,&digest]).await?;
        ensure!(n == 1, "image execution expired before commit");
        tx.commit().await?;
        Ok(())
    }
    pub async fn image_report(
        &self,
        id: Uuid,
        publisher: bool,
        report: &Report,
        log: Option<(&str, &str)>,
    ) -> anyhow::Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // 所有回执遵循构建、Job、执行锁顺序。
        sql::execute(&mut tx,"SELECT b.id FROM builds b JOIN jobs j ON j.build_id=b.id JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR UPDATE OF b,j",params![id]).await?;
        let row: Value = sql::scalar(
            &mut tx,
            "SELECT to_jsonb(e) FROM image_executions e WHERE id=$1 FOR UPDATE",
            params![id],
        )
        .await?;
        if let Report::Published { digest } = report
            && publisher
            && row["published_digest"] == *digest
        {
            return Ok(());
        }
        ensure!(
            sql::optional_scalar::<Uuid>(&mut tx, LIVE, params![id])
                .await?
                .is_some(),
            "image execution is stale or canceled"
        );
        let execution: ImageExecution = serde_json::from_value({
            let mut r = row.clone();
            r["timeout_seconds"] = json!(1);
            r
        })?;
        match report {
            Report::Heartbeat { platform, pod_uid } => {
                ensure!(
                    (publisher && platform == "publish")
                        || (!publisher && execution.spec.platforms.contains(platform)),
                    "unauthorized platform"
                );
                let pod = Uuid::parse_str(pod_uid)?;
                let n=sql::execute(&mut tx,"INSERT INTO image_platforms(execution_id,platform,pod_uid) VALUES($1,$2,$3) ON CONFLICT(execution_id,platform) DO UPDATE SET heartbeat_at=now() WHERE image_platforms.pod_uid=excluded.pod_uid",params![id,platform,pod]).await?;
                ensure!(n == 1, "worker identity changed");
            }
            Report::Log {
                platform,
                sequence,
                text,
            } => {
                ensure!(
                    !publisher
                        && execution.spec.platforms.contains(platform)
                        && *sequence > 0
                        && text.len() <= 16384,
                    "invalid log report"
                );
                let (key, hash) = log.ok_or_else(|| anyhow::anyhow!("log object required"))?;
                let n=sql::execute(&mut tx,"INSERT INTO image_logs(execution_id,platform,sequence,object_key,content_hash) VALUES($1,$2,$3,$4,$5) ON CONFLICT(execution_id,platform,sequence) DO UPDATE SET object_key=image_logs.object_key WHERE image_logs.content_hash=excluded.content_hash",params![id,platform,*sequence,key,hash]).await?;
                ensure!(n == 1, "log sequence changed");
            }
            Report::Platform { result } => {
                ensure!(
                    !publisher && execution.spec.platforms.contains(&result.platform),
                    "unauthorized platform result"
                );
                result.validate()?;
                let mut results = execution.results;
                if let Some(existing) = results.iter().find(|r| r.platform == result.platform) {
                    ensure!(existing == result, "platform result changed");
                } else {
                    results.push(result.clone());
                }
                sql::execute(
                    &mut tx,
                    "UPDATE image_executions SET results=$2 WHERE id=$1",
                    params![id, serde_json::to_value(results)?],
                )
                .await?;
            }
            Report::Failed { .. } => {
                let status = if publisher {
                    "publish_failed"
                } else {
                    "failed"
                };
                sql::execute(
                    &mut tx,
                    "UPDATE image_executions SET status=$2,error=$3,finished_at=now() WHERE id=$1",
                    params![
                        id,
                        status,
                        if publisher {
                            "image publication failed"
                        } else {
                            "image build failed"
                        }
                    ],
                )
                .await?;
                if execution.destination_credential.is_none() {
                    sql::execute(
                        &mut tx,
                        "UPDATE jobs SET status='failed' WHERE id=$1",
                        params![execution.job_id],
                    )
                    .await?;
                }
            }
            Report::Published { digest } => {
                ensure!(
                    publisher && artifactory_image_protocol::digest(digest),
                    "unauthorized publication"
                );
                let expected = if let Some(root) = &execution.root_digest {
                    root.clone()
                } else {
                    artifactory_image_protocol::sha256(&artifactory_image_protocol::index(
                        &execution.spec.platforms,
                        &execution.results,
                    )?)
                };
                ensure!(digest == &expected, "publication digest changed");
                ensure!(
                    execution
                        .published_digest
                        .as_ref()
                        .is_none_or(|saved| saved == digest),
                    "published candidate changed"
                );
                sql::execute(
                    &mut tx,
                    "UPDATE image_executions SET published_digest=$2 WHERE id=$1",
                    params![id, digest],
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn finish_image(&self, id: Uuid, status: &str) -> anyhow::Result<()> {
        ensure!(
            ["failed", "canceled"].contains(&status),
            "invalid terminal state"
        );
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(&mut tx,"SELECT b.id FROM builds b JOIN jobs j ON j.build_id=b.id JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR UPDATE OF b,j",params![id]).await?;
        sql::execute(&mut tx,"UPDATE jobs SET status=$2 WHERE id=(SELECT job_id FROM image_executions WHERE id=$1 AND status IN ('preparing','running') AND destination_credential IS NULL)",params![id,status]).await?;
        sql::execute(&mut tx,"UPDATE image_executions SET status=CASE WHEN $2='failed' AND jsonb_array_length(results)=jsonb_array_length(spec->'platforms') THEN 'publish_failed' ELSE $2 END,cancel_requested=($2='canceled'),error=$3,finished_at=now() WHERE id=$1 AND status IN ('preparing','running')",params![id,status,if status=="failed"{"image Job failed or timed out"}else{"image execution canceled"}]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn image_credentials(&self, project: Uuid) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT to_jsonb(c)-'envelope' FROM image_credentials c WHERE project_id=$1 AND tenant_id=$2 ORDER BY created_at",params![project,self.tenant()?]).await?)
    }
    pub async fn save_image_credential(
        &self,
        project: Uuid,
        id: Uuid,
        name: &str,
        registry_origin: Option<&str>,
        envelope: &str,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_project_admin(&mut tx, project).await?;
        let kind = if registry_origin.is_some() {
            "registry"
        } else {
            "secret"
        };
        sql::execute(&mut tx,"INSERT INTO image_credentials(id,tenant_id,project_id,name,kind,envelope,registry_origin) VALUES($1,$2,$3,$4,$5,$6,$7)",params![id,self.tenant()?,project,name,kind,envelope,registry_origin]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn image_credential(
        &self,
        project: Uuid,
        id: Uuid,
        kind: &str,
    ) -> anyhow::Result<String> {
        sql::optional_scalar(&mut self.pool.clone(),"SELECT envelope FROM image_credentials WHERE id=$1 AND project_id=$2 AND kind=$3 AND enabled",params![id,project,kind]).await?.ok_or_else(||anyhow::anyhow!("image credential unavailable for project"))
    }
    pub async fn disable_image_credential(&self, project: Uuid, id: Uuid) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_project_admin(&mut tx, project).await?;
        sql::execute(&mut tx,"UPDATE image_credentials SET enabled=false WHERE id=$1 AND project_id=$2 AND tenant_id=$3",params![id,project,self.tenant()?]).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn images(&self, visible: &[Uuid], project: Option<Uuid>) -> Result<Vec<Value>> {
        let registry = std::env::var("ARTIFACTORY_REGISTRY_URL")
            .ok()
            .filter(|endpoint| {
                artifactory_image_protocol::RegistryLogin {
                    endpoint: endpoint.clone(),
                    username: String::new(),
                    password: String::new(),
                }
                .validate(true)
                .is_ok()
            });
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT to_jsonb(i)||jsonb_build_object('target',j.target,'build_id',j.build_id,'build_number',b.number,'project_slug',p.slug,'name',e.spec->>'name','registry',$4::text) FROM image_artifacts i JOIN image_executions e ON e.id=i.execution_id JOIN jobs j ON j.id=i.job_id JOIN builds b ON b.id=j.build_id JOIN projects p ON p.id=i.project_id WHERE i.tenant_id=$1 AND i.project_id=ANY($2) AND ($3::uuid IS NULL OR i.project_id=$3) ORDER BY i.created_at DESC LIMIT 100",params![self.tenant()?,visible,project,registry]).await?)
    }
    pub async fn image_history(&self, build: Uuid) -> anyhow::Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT to_jsonb(e)-'task_envelope'-'build_token_hash'-'publish_token_hash' FROM image_executions e JOIN jobs j ON j.id=e.job_id WHERE j.build_id=$1 ORDER BY e.created_at",params![build]).await?)
    }
    pub async fn image_log_keys(&self, id: Uuid, after: i64) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT to_jsonb(l) FROM image_logs l JOIN image_executions e ON e.id=l.execution_id WHERE l.execution_id=$1 AND e.tenant_id=$2 AND l.id>$3 ORDER BY l.id LIMIT 100",params![id,self.tenant()?,after]).await?)
    }
    pub async fn request_image_publication(
        &self,
        parent: Uuid,
        id: Uuid,
        destination: Option<Uuid>,
        repository: Option<&str>,
        tags: &[String],
        hash: &str,
    ) -> Result<()> {
        if destination.is_none() && (repository.is_some() || !tags.is_empty()) {
            return Err(ApiError::BadRequest(
                "internal publication destination is immutable".into(),
            ));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let project: Uuid = sql::optional_scalar(
            &mut tx,
            "SELECT project_id FROM image_executions WHERE id=$1 AND tenant_id=$2",
            params![parent, self.tenant()?],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        sql::execute(
            &mut tx,
            "SELECT require_active_project($1)",
            params![project],
        )
        .await?;
        sql::execute(&mut tx,"SELECT b.id FROM builds b JOIN jobs j ON j.build_id=b.id JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR UPDATE OF b,j",params![parent]).await?;
        let row:Value=sql::optional_scalar(&mut tx,"SELECT to_jsonb(e) FROM image_executions e WHERE id=$1 AND tenant_id=$2 AND operation='build' AND NOT cancel_requested AND EXISTS(SELECT 1 FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=e.job_id AND NOT b.cancel_requested AND j.plan_revision_id IS NOT DISTINCT FROM e.plan_revision_id AND ($3::uuid IS NOT NULL OR (j.status='failed' AND e.id=(SELECT id FROM image_executions WHERE job_id=j.id AND operation='build' ORDER BY created_at DESC,id DESC LIMIT 1)))) FOR UPDATE",params![parent,self.tenant()?,destination]).await?.ok_or(ApiError::NotFound)?;
        let project: Uuid =
            serde_json::from_value(row["project_id"].clone()).map_err(anyhow::Error::from)?;
        self.scope
            .as_ref()
            .ok_or(ApiError::Unauthorized)?
            .project(project, "release")?;
        if destination.is_some() {
            let credential: Option<Uuid> = sql::optional_scalar(&mut tx,"SELECT id FROM image_credentials WHERE id=$1 AND project_id=$2 AND tenant_id=$3 AND kind='registry' AND enabled FOR SHARE",params![destination,project,self.tenant()?]).await?;
            if credential.is_none() {
                return Err(ApiError::BadRequest(
                    "image credential unavailable for project".into(),
                ));
            }
            if row["status"] != "succeeded" {
                return Err(ApiError::Conflict(
                    "publish requires a verified image".into(),
                ));
            }
        } else if row["status"] != "publish_failed" {
            return Err(ApiError::Conflict(
                "only a failed publication can be retried".into(),
            ));
        }
        let n=sql::execute(&mut tx,"INSERT INTO image_executions(id,tenant_id,project_id,job_id,source_manifest_id,plan_revision_id,parent_id,operation,build_token_hash,publish_token_hash,spec,results,candidate_repository,destination_repository,destination_credential,destination_registry,tags,root_digest,deadline_at) SELECT $2,tenant_id,project_id,job_id,source_manifest_id,plan_revision_id,id,'publish','',$3,spec,results,CASE WHEN $4::uuid IS NULL THEN candidate_repository ELSE destination_repository END,CASE WHEN $4::uuid IS NULL THEN destination_repository ELSE $5 END,$4,(SELECT registry_origin FROM image_credentials WHERE id=$4),CASE WHEN $4::uuid IS NULL THEN tags ELSE $6 END,CASE WHEN $4::uuid IS NULL THEN NULL ELSE root_digest END,now()+interval '1 hour' FROM image_executions WHERE id=$1",params![parent,id,hash,destination,repository,tags]).await?;
        if n != 1 {
            return Err(ApiError::NotFound);
        }
        if destination.is_none() {
            sql::execute(&mut tx,"UPDATE jobs SET status='running' WHERE id=(SELECT job_id FROM image_executions WHERE id=$1)",params![parent]).await?;
            sql::execute(&mut tx,"UPDATE builds SET status='running',finished_at=NULL WHERE id=(SELECT build_id FROM jobs WHERE id=(SELECT job_id FROM image_executions WHERE id=$1))",params![parent]).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
