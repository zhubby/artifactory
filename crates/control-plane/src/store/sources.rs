//! [INPUT]: 依赖源码清单、任务身份和项目凭据引用。
//! [OUTPUT]: 固定单次构建的解析身份、部分 pin 和源码清单。
//! [POS]: 源码任务以 Build 归属；按版本、构建、任务顺序加锁，隔离解析结果。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_source_checkout::{
    GitCredential, PrepareResponse, SourceCredential, SourceManifest,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceTaskExecution {
    pub cr_uid: String,
    pub name: String,
    pub uid: String,
    pub attempt: u32,
}

fn source_execution(task: &Value, execution: &SourceTaskExecution) -> Result<()> {
    ensure!(
        (1..=3).contains(&execution.attempt)
            && !execution.name.is_empty()
            && !execution.uid.is_empty(),
        "invalid source execution identity"
    );
    ensure!(
        task["source_task_uid"].as_str() == Some(execution.cr_uid.as_str()),
        "SourceTask UID changed"
    );
    Ok(())
}

fn current_source_execution(task: &Value, execution: &SourceTaskExecution) -> Result<()> {
    source_execution(task, execution)?;
    ensure!(
        task["source_execution"] == serde_json::to_value(execution)?,
        "source execution superseded"
    );
    Ok(())
}

impl ControlStore {
    pub(crate) async fn enqueue_source_resolutions(&self) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let releases:Vec<Uuid>=sql::scalars(&mut tx,"SELECT r.id FROM releases r WHERE EXISTS(SELECT 1 FROM builds b WHERE b.release_id=r.id AND b.status='resolving') OR EXISTS(SELECT 1 FROM source_tasks s JOIN builds b ON b.id=s.build_id WHERE b.release_id=r.id AND s.status IN ('queued','running')) ORDER BY r.id FOR UPDATE SKIP LOCKED",params![]).await?;
        sql::execute(&mut tx,"INSERT INTO source_tasks(id,tenant_id,project_id,build_id,purpose,repository_url,reference,credential_id,bindings,deadline_at) SELECT b.id,b.tenant_id,b.project_id,b.id,'resolve',b.source_repository_url,b.requested_ref,b.source_credential_id,coalesce(b.planning_context->'source_credentials','[]'),now()+interval '15 minutes' FROM builds b JOIN releases r ON r.id=b.release_id JOIN projects p ON p.id=b.project_id JOIN tenants t ON t.id=b.tenant_id WHERE b.release_id=ANY($1) AND b.status='resolving' AND NOT b.cancel_requested AND r.status='draft' AND p.status='active' AND t.status='active' ON CONFLICT(id) DO NOTHING",params![&releases]).await?;
        sql::execute(&mut tx, "UPDATE source_tasks s SET status='canceled' WHERE (s.build_id IS NULL OR EXISTS(SELECT 1 FROM builds b WHERE b.id=s.build_id AND b.release_id=ANY($1))) AND s.status IN ('queued','running') AND ((s.purpose='resolve' AND NOT EXISTS(SELECT 1 FROM builds b WHERE b.id=s.build_id AND b.status='resolving' AND NOT b.cancel_requested)) OR NOT EXISTS(SELECT 1 FROM projects p JOIN tenants t ON t.id=p.tenant_id WHERE p.id=s.project_id AND p.status IN ('active','draining') AND t.status IN ('active','draining')))", params![&releases]).await?;
        sql::execute(&mut tx, "UPDATE source_tasks s SET status='failed',error='source preparation deadline reached' WHERE (s.build_id IS NULL OR EXISTS(SELECT 1 FROM builds b WHERE b.id=s.build_id AND b.release_id=ANY($1))) AND s.status IN ('queued','running') AND s.deadline_at<=clock_timestamp()", params![&releases]).await?;
        sql::execute(&mut tx, "UPDATE builds b SET status='failed',error=t.error,finished_at=now() FROM source_tasks t WHERE b.release_id=ANY($1) AND t.build_id=b.id AND t.status IN ('failed','canceled') AND b.status='resolving' AND NOT b.cancel_requested", params![&releases]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn source_tasks_pending(&self) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(t) FROM source_tasks t WHERE status IN ('queued','running') OR (status IN ('failed','canceled') AND NOT cleanup_requested) ORDER BY created_at LIMIT 64", params![]).await?)
    }

    pub(crate) async fn source_task_dispatched(&self, id: Uuid, cleanup: bool) -> Result<()> {
        sql::execute(&mut self.pool.clone(), "UPDATE source_tasks SET status=CASE WHEN status='queued' THEN 'running' ELSE status END,cleanup_requested=cleanup_requested OR $2 WHERE id=$1", params![id,cleanup]).await?;
        Ok(())
    }

    pub(crate) async fn bind_source_task(&self, id: Uuid, uid: &str) -> Result<()> {
        ensure!(!uid.is_empty(), "SourceTask UID required");
        let count = sql::execute(&mut self.pool.clone(), "UPDATE source_tasks SET source_task_uid=$2 WHERE id=$1 AND (source_task_uid IS NULL OR source_task_uid=$2)", params![id,uid]).await?;
        ensure!(count == 1, "SourceTask UID changed");
        Ok(())
    }

    pub(crate) async fn source_task_failed(&self, id: Uuid, message: &str) -> Result<()> {
        sql::execute(&mut self.pool.clone(), "UPDATE source_tasks SET status='failed',error=$2 WHERE id=$1 AND status IN ('queued','running')", params![id,message.chars().take(2000).collect::<String>()]).await?;
        Ok(())
    }

    pub(crate) async fn create_refs_task(&self, project: Uuid) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let count=sql::execute(&mut self.pool.clone(), "INSERT INTO source_tasks(id,tenant_id,project_id,purpose,repository_url,reference,credential_id,bindings,deadline_at) SELECT $1,tenant_id,id,'refs',repository_url,'HEAD',credential_id,source_credentials,now()+interval '30 seconds' FROM projects WHERE id=$2 AND tenant_id=$3 AND status='active'", params![id,project,self.tenant()?]).await?;
        ensure!(count == 1, "project unavailable");
        Ok(id)
    }

    pub(crate) async fn source_task_result(&self, id: Uuid) -> Result<Value> {
        Ok(sql::scalar(&mut self.pool.clone(), "SELECT jsonb_build_object('status',status,'result',result,'error',error) FROM source_tasks WHERE id=$1", params![id]).await?)
    }

    pub(crate) async fn source_task_context(&self, id: Uuid) -> Result<Value> {
        Ok(sql::scalar(&mut self.pool.clone(), "SELECT to_jsonb(s) FROM source_tasks s JOIN projects p ON p.id=s.project_id JOIN tenants t ON t.id=s.tenant_id WHERE s.id=$1 AND p.status IN ('active','draining') AND t.status IN ('active','draining') AND (s.status='succeeded' OR (s.status IN ('queued','running') AND s.deadline_at>clock_timestamp() AND (s.purpose='refs' OR EXISTS(SELECT 1 FROM builds b WHERE b.id=s.build_id AND NOT b.cancel_requested AND b.status='resolving'))))", params![id]).await?)
    }

    pub(crate) async fn pin_source(
        &self,
        id: Uuid,
        execution: &SourceTaskExecution,
        pin: &Value,
    ) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let current = lock_source_task(&mut tx, id).await?;
        current_source_execution(&current, execution)?;
        ensure_live_source_task(&mut tx, id, &current).await?;
        ensure!(
            current["purpose"] == "resolve",
            "only source resolution pins objects"
        );
        let result = match pin["kind"].as_str() {
            Some("ref") => {
                let oid = pin["oid"].as_str().context("missing ref object")?;
                ensure!(
                    artifactory_source_checkout::valid_commit(oid),
                    "invalid ref object"
                );
                let fixed:String=sql::scalar(&mut tx,"UPDATE source_tasks SET pinned_ref_oid=coalesce(pinned_ref_oid,$2) WHERE id=$1 RETURNING pinned_ref_oid",params![id,oid]).await?;
                json!({"oid":fixed})
            }
            Some("tags") => {
                let path = pin["path"].as_str().context("missing repository path")?;
                ensure!(
                    path.len() <= 4096 && !path.contains(['\0', '\n', '\r']),
                    "invalid repository path"
                );
                let tags: std::collections::BTreeMap<String, String> =
                    serde_json::from_value(pin["tags"].clone())?;
                ensure!(
                    tags.len() <= 100_000
                        && tags
                            .values()
                            .all(|v| artifactory_source_checkout::valid_commit(v)),
                    "invalid frozen tags"
                );
                let mut frozen: Value = current["frozen_tags"].clone();
                if frozen.get(path).is_none() {
                    frozen[path] = serde_json::to_value(tags)?;
                }
                sql::execute(
                    &mut tx,
                    "UPDATE source_tasks SET frozen_tags=$2 WHERE id=$1",
                    params![id, &frozen],
                )
                .await?;
                json!({"tags":frozen[path]})
            }
            _ => anyhow::bail!("unknown source pin"),
        };
        tx.commit().await?;
        Ok(result)
    }

    pub(crate) async fn commit_source_task(
        &self,
        id: Uuid,
        execution: &SourceTaskExecution,
        result: &Value,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let task = lock_source_task(&mut tx, id).await?;
        source_execution(&task, execution)?;
        if task["status"] == "succeeded" {
            ensure!(task["result"] == *result, "source result changed");
            tx.commit().await?;
            return Ok(());
        }
        current_source_execution(&task, execution)?;
        ensure_live_source_task(&mut tx, id, &task).await?;
        if let Some(error) = result["error"].as_str() {
            ensure!(error.len() <= 2000, "source error exceeds limit");
            sql::execute(
                &mut tx,
                "UPDATE source_tasks SET status='failed',error=$2 WHERE id=$1",
                params![id, error],
            )
            .await?;
            tx.commit().await?;
            return Ok(());
        }
        if task["purpose"] == "resolve" {
            let manifest: SourceManifest = serde_json::from_value(result["manifest"].clone())?;
            manifest.validate()?;
            ensure!(
                manifest.id == id && task["pinned_ref_oid"].is_string(),
                "source identity not pinned"
            );
            let root = manifest.root()?;
            ensure!(
                task["repository_url"] == root.upstream_url
                    && task["pinned_ref_oid"] == root.commit_sha,
                "source repository changed"
            );
            for repo in &manifest.repositories {
                ensure!(
                    task["frozen_tags"][&repo.path] == serde_json::to_value(&repo.tags)?,
                    "source tags changed"
                );
            }
            let build_id = task["build_id"]
                .as_str()
                .context("resolution build missing")?
                .parse::<Uuid>()?;
            let build: Value = sql::scalar(&mut tx, "SELECT to_jsonb(b) FROM builds b WHERE id=$1 AND status='resolving' AND NOT cancel_requested", params![build_id]).await?;
            ensure!(
                build["model_config_snapshot"].is_object(),
                "configure an enabled model profile before building"
            );
            sql::execute(&mut tx,"INSERT INTO source_manifests(id,tenant_id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,$5,$6)",params![id,task["tenant_id"].as_str().context("tenant missing")?.parse::<Uuid>()?,task["project_id"].as_str().context("project missing")?.parse::<Uuid>()?,&root.commit_sha,serde_json::to_value(&manifest)?,&manifest.digest]).await?;
            let changed = sql::execute(&mut tx,"UPDATE builds SET commit_sha=$2,source_manifest_id=$3,status='planning' WHERE id=$1 AND status='resolving' AND NOT cancel_requested AND source_manifest_id IS NULL",params![build_id,&root.commit_sha,id]).await?;
            ensure!(changed == 1, "build source resolution superseded");
            super::models::PlanningRun::create()
                .id(Uuid::new_v4())
                .build_id(build_id)
                .purpose("initial")
                .model_config(toasty::Json(build["model_config_snapshot"].clone()))
                .exec(&mut tx)
                .await?;
        } else {
            ensure!(
                result["refs"].is_object() && serde_json::to_vec(result)?.len() <= 4 * 1024 * 1024,
                "invalid reference result"
            );
        }
        sql::execute(
            &mut tx,
            "UPDATE source_tasks SET status='succeeded',result=$2 WHERE id=$1",
            params![id, result],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn source_manifest(&self, id: Uuid) -> Result<SourceManifest> {
        let value: Value = sql::scalar(
            &mut self.pool.clone(),
            "SELECT manifest FROM source_manifests WHERE id=$1",
            params![id],
        )
        .await?;
        let manifest: SourceManifest = serde_json::from_value(value)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) async fn source_task_material(
        &self,
        master: &[u8; 32],
        task_id: Uuid,
        execution: &SourceTaskExecution,
    ) -> Result<PrepareResponse> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let task = lock_source_task(&mut tx, task_id).await?;
        source_execution(&task, execution)?;
        let repository = task["repository_url"]
            .as_str()
            .context("repository missing")?;
        let completed = (task["status"] == "succeeded").then(|| task["result"].clone());
        let credentials = if completed.is_some() {
            Vec::new()
        } else {
            ensure_live_source_task(&mut tx, task_id, &task).await?;
            if !task["source_execution"].is_null() {
                let current: SourceTaskExecution =
                    serde_json::from_value(task["source_execution"].clone())?;
                ensure!(
                    current == *execution || current.attempt < execution.attempt,
                    "source execution superseded"
                );
            }
            sql::execute(
                &mut tx,
                "UPDATE source_tasks SET source_execution=$2 WHERE id=$1",
                params![task_id, serde_json::to_value(execution)?],
            )
            .await?;
            let tenant = task["tenant_id"]
                .as_str()
                .context("tenant missing")?
                .parse::<Uuid>()?;
            let credentials = source_credentials_with(
                &mut tx,
                master,
                tenant,
                repository,
                task["credential_id"]
                    .as_str()
                    .map(Uuid::parse_str)
                    .transpose()?,
                task["bindings"].clone(),
            )
            .await?;
            ensure_live_source_task(&mut tx, task_id, &task).await?;
            credentials
        };
        let material = PrepareResponse {
            source_id: task_id,
            repository_url: repository.into(),
            reference: task["reference"].as_str().unwrap_or("HEAD").into(),
            pinned_ref_oid: task["pinned_ref_oid"].as_str().map(str::to_owned),
            frozen_tags: if completed.is_some() {
                Default::default()
            } else {
                serde_json::from_value(task["frozen_tags"].clone())?
            },
            manifest: None,
            completed_result: completed,
            credentials,
        };
        tx.commit().await?;
        Ok(material)
    }
    pub(crate) async fn build_source_material(
        &self,
        master: &[u8; 32],
        build: Uuid,
    ) -> Result<PrepareResponse> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        lock_build_source(&mut tx, build).await?;
        let material = build_source_material_with(&mut tx, master, build)
            .await
            .context("load fixed source material")?;
        tx.commit().await?;
        Ok(material)
    }
    pub(crate) async fn native_source_material(
        &self,
        master: &[u8; 32],
        node: Uuid,
        attempt: Uuid,
        token: &str,
        digest: &str,
    ) -> Result<PrepareResponse> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let build:Uuid=sql::scalar(&mut tx,"SELECT b.id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE a.id=$1 AND a.node_id=$2",params![attempt,node]).await?;
        lock_build_source(&mut tx, build).await?;
        sql::execute(&mut tx,"SELECT a.id FROM attempts a JOIN jobs j ON j.id=a.job_id WHERE a.id=$1 FOR SHARE OF a,j",params![attempt]).await?;
        sql::execute(
            &mut tx,
            "SELECT id FROM nodes WHERE id=$1 FOR SHARE",
            params![node],
        )
        .await?;
        native_source_build(&mut tx, node, attempt, token, digest)
            .await
            .context("native source authorization")?;
        let material = build_source_material_with(&mut tx, master, build)
            .await
            .context("load fixed source material")?;
        native_source_build(&mut tx, node, attempt, token, digest)
            .await
            .context("native source authorization")?;
        tx.commit().await?;
        Ok(material)
    }
    pub(crate) async fn image_source_material(
        &self,
        master: &[u8; 32],
        execution: Uuid,
        platform: &str,
    ) -> Result<PrepareResponse> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let build: Uuid = sql::scalar(
            &mut tx,
            "SELECT j.build_id FROM image_executions e JOIN jobs j ON j.id=e.job_id WHERE e.id=$1",
            params![execution],
        )
        .await?;
        lock_build_source(&mut tx, build).await?;
        sql::execute(&mut tx,"SELECT j.id FROM jobs j JOIN image_executions e ON e.job_id=j.id WHERE e.id=$1 FOR SHARE OF j",params![execution]).await?;
        sql::execute(
            &mut tx,
            "SELECT id FROM image_executions WHERE id=$1 FOR SHARE",
            params![execution],
        )
        .await?;
        image_source_build(&mut tx, execution, platform).await?;
        let material = build_source_material_with(&mut tx, master, build)
            .await
            .context("load fixed source material")?;
        image_source_build(&mut tx, execution, platform).await?;
        tx.commit().await?;
        Ok(material)
    }
}

async fn lock_source_task(tx: &mut dyn toasty::Executor, id: Uuid) -> Result<Value> {
    let scope:Value=sql::scalar(tx,"SELECT jsonb_build_object('tenant',s.tenant_id,'project',s.project_id,'build',s.build_id,'release',b.release_id) FROM source_tasks s LEFT JOIN builds b ON b.id=s.build_id WHERE s.id=$1",params![id]).await?;
    let tenant = scope["tenant"]
        .as_str()
        .context("source tenant missing")?
        .parse::<Uuid>()?;
    let project = scope["project"]
        .as_str()
        .context("source project missing")?
        .parse::<Uuid>()?;
    sql::execute(
        tx,
        "SELECT id FROM tenants WHERE id=$1 FOR SHARE",
        params![tenant],
    )
    .await?;
    sql::execute(
        tx,
        "SELECT id FROM projects WHERE id=$1 FOR SHARE",
        params![project],
    )
    .await?;
    if let Some(release) = scope["release"].as_str() {
        sql::execute(
            tx,
            "SELECT id FROM releases WHERE id=$1 FOR UPDATE",
            params![release.parse::<Uuid>()?],
        )
        .await?;
    }
    if let Some(build) = scope["build"].as_str() {
        sql::execute(
            tx,
            "SELECT id FROM builds WHERE id=$1 FOR UPDATE",
            params![build.parse::<Uuid>()?],
        )
        .await?;
    }
    Ok(sql::scalar(
        tx,
        "SELECT to_jsonb(s) FROM source_tasks s WHERE id=$1 FOR UPDATE",
        params![id],
    )
    .await?)
}

async fn ensure_live_source_task(
    tx: &mut dyn toasty::Executor,
    id: Uuid,
    task: &Value,
) -> Result<()> {
    ensure!(
        matches!(task["status"].as_str(), Some("queued" | "running")),
        "source task stopped"
    );
    let live:bool=sql::scalar(tx,"SELECT s.deadline_at>clock_timestamp() AND p.status IN ('active','draining') AND t.status IN ('active','draining') AND (s.purpose='refs' OR EXISTS(SELECT 1 FROM builds b WHERE b.id=s.build_id AND NOT b.cancel_requested AND b.status='resolving')) FROM source_tasks s JOIN projects p ON p.id=s.project_id JOIN tenants t ON t.id=s.tenant_id WHERE s.id=$1",params![id]).await?;
    ensure!(live, "source task canceled or expired");
    Ok(())
}

pub(super) async fn source_credentials_with(
    tx: &mut dyn toasty::Executor,
    master: &[u8; 32],
    tenant: Uuid,
    repository: &str,
    primary: Option<Uuid>,
    bindings: Value,
) -> Result<Vec<SourceCredential>> {
    let bindings: Vec<crate::source::CredentialBinding> = serde_json::from_value(bindings)?;
    let mut references = std::collections::BTreeMap::new();
    if let Some(id) = primary {
        references.insert(repository.to_owned(), id);
    }
    for binding in bindings {
        references.insert(binding.repository_url, binding.credential_id);
    }
    let mut credentials = Vec::new();
    for (upstream_url, id) in references {
        let ciphertext: String = sql::scalar(
            tx,
            "SELECT ciphertext FROM credentials WHERE id=$1 AND tenant_id=$2",
            params![id, tenant],
        )
        .await?;
        let credential: GitCredential =
            serde_json::from_slice(&crate::crypto::open(master, &ciphertext)?)?;
        credentials.push(SourceCredential {
            upstream_url,
            credential,
        });
    }
    Ok(credentials)
}

async fn build_source_material_with(
    tx: &mut dyn toasty::Executor,
    master: &[u8; 32],
    build: Uuid,
) -> Result<PrepareResponse> {
    let row:Value=sql::scalar(tx,"SELECT to_jsonb(b) FROM builds b JOIN projects p ON p.id=b.project_id JOIN tenants t ON t.id=b.tenant_id WHERE b.id=$1 AND NOT b.cancel_requested AND b.status IN ('planning','replanning','queued','running') AND p.status IN ('active','draining') AND t.status IN ('active','draining')",params![build]).await?;
    let manifest_id = row["source_manifest_id"]
        .as_str()
        .context("source missing")?
        .parse::<Uuid>()?;
    let manifest: Value = sql::scalar(
        tx,
        "SELECT manifest FROM source_manifests WHERE id=$1",
        params![manifest_id],
    )
    .await?;
    let manifest: SourceManifest = serde_json::from_value(manifest)?;
    manifest.validate()?;
    let repository = row["source_repository_url"]
        .as_str()
        .context("repository missing")?;
    let bindings: Vec<crate::source::CredentialBinding> =
        serde_json::from_value(row["planning_context"]["source_credentials"].clone())?;
    let bindings: Vec<_> = bindings
        .into_iter()
        .filter(|binding| {
            manifest
                .repositories
                .iter()
                .any(|repo| repo.upstream_url == binding.repository_url)
        })
        .collect();
    let credentials = source_credentials_with(
        tx,
        master,
        row["tenant_id"]
            .as_str()
            .context("tenant missing")?
            .parse()?,
        repository,
        row["source_credential_id"]
            .as_str()
            .map(Uuid::parse_str)
            .transpose()?,
        serde_json::to_value(bindings)?,
    )
    .await?;
    Ok(PrepareResponse {
        source_id: manifest.id,
        repository_url: repository.into(),
        reference: row["requested_ref"].as_str().unwrap_or("HEAD").into(),
        pinned_ref_oid: None,
        frozen_tags: Default::default(),
        manifest: Some(manifest),
        completed_result: None,
        credentials,
    })
}

async fn native_source_build(
    tx: &mut dyn toasty::Executor,
    node: Uuid,
    attempt: Uuid,
    token: &str,
    digest: &str,
) -> Result<Uuid> {
    let build:Uuid=sql::scalar(tx,"SELECT b.id FROM attempts a JOIN nodes n ON n.id=a.node_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id JOIN source_manifests s ON s.id=b.source_manifest_id WHERE a.id=$1 AND a.node_id=$2 AND a.lease_token=$3 AND a.status IN ('leased','running') AND a.lease_expires_at>clock_timestamp() AND NOT n.revoked AND n.certificate_expires_at>clock_timestamp() AND NOT b.cancel_requested AND j.status IN ('leased','running') AND a.source_manifest_id=b.source_manifest_id AND a.plan_revision_id IS NOT DISTINCT FROM j.plan_revision_id AND build_node_allowed(b.id,n.id) AND s.digest=$4 AND s.tenant_id=n.tenant_id",params![attempt,node,token,digest]).await?;
    Ok(build)
}

async fn image_source_build(
    tx: &mut dyn toasty::Executor,
    execution: Uuid,
    platform: &str,
) -> Result<Uuid> {
    Ok(sql::scalar(tx,"SELECT b.id FROM image_executions e JOIN jobs j ON j.id=e.job_id JOIN builds b ON b.id=j.build_id WHERE e.id=$1 AND e.operation='build' AND e.status IN ('preparing','running') AND NOT e.cancel_requested AND NOT b.cancel_requested AND e.deadline_at>clock_timestamp() AND b.source_manifest_id=e.source_manifest_id AND j.plan_revision_id IS NOT DISTINCT FROM e.plan_revision_id AND e.spec->'platforms' ? $2",params![execution,platform]).await?)
}
async fn lock_build_source(tx: &mut dyn toasty::Executor, build: Uuid) -> Result<()> {
    let scope:Value=sql::scalar(tx,"SELECT jsonb_build_object('tenant',tenant_id,'project',project_id) FROM builds WHERE id=$1",params![build]).await?;
    for (query, key) in [
        ("SELECT id FROM tenants WHERE id=$1 FOR SHARE", "tenant"),
        ("SELECT id FROM projects WHERE id=$1 FOR SHARE", "project"),
    ] {
        let id = scope[key]
            .as_str()
            .context("source scope missing")?
            .parse::<Uuid>()?;
        sql::execute(tx, query, params![id]).await?;
    }
    sql::execute(
        tx,
        "SELECT id FROM builds WHERE id=$1 FOR SHARE",
        params![build],
    )
    .await?;
    Ok(())
}
