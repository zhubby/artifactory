//! [INPUT]: 隔离 PostgreSQL、新初始化基线和真实源码事务。
//! [OUTPUT]: 验证 CR UID、阶段诊断、固定对象与锁等待后的授权。
//! [POS]: 源码元数据存储的并发与权限回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, sources::SourceTaskExecution};
use anyhow::{Context, Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use artifactory_source_checkout::{GitCredential, SourceManifest, SourceRepository};
use serde_json::json;
use uuid::Uuid;

#[path = "source_preparation_tests.rs"]
mod preparation;

#[path = "source_progress_tests.rs"]
mod progress;

fn execution() -> SourceTaskExecution {
    SourceTaskExecution {
        cr_uid: "source-current".into(),
        name: "source-attempt-1".into(),
        uid: "source-pod-1".into(),
        attempt: 1,
    }
}

struct Database {
    admin: toasty::Db,
    pool: toasty::Db,
    schema: String,
    store: ControlStore,
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies first-writer CR UID binding"]
async fn source_task_cr_uid_is_pinned_once_even_with_concurrent_dispatch() -> Result<()> {
    let db = Database::new().await?;
    let outcome = async {
        let (id, _) = db.unbound_task().await?;
        let (first, second) = tokio::join!(
            db.store.bind_source_task(id, "first-cr"),
            db.store.bind_source_task(id, "second-cr")
        );
        ensure!(
            first.is_ok() != second.is_ok(),
            "two CRs acquired one source task"
        );
        let current = db.store.source_task_context(id).await?;
        let uid = current["source_task_uid"]
            .as_str()
            .context("CR UID missing")?;
        db.store.bind_source_task(id, uid).await?;
        ensure!(
            db.store
                .bind_source_task(id, "replacement-cr")
                .await
                .is_err(),
            "same-name replacement acquired source task"
        );
        ensure!(
            db.store.bind_source_task(id, "").await.is_err(),
            "empty CR UID accepted"
        );
        Ok(())
    }
    .await;
    db.close().await?;
    outcome
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies monotonic source attempt fencing"]
async fn source_attempt_binding_cannot_regress_or_accept_another_pod_uid() -> Result<()> {
    let db = Database::new().await?;
    let outcome = async {
        let (id, manifest) = db.task().await?;
        db.pin(id).await?;
        let first = execution();
        let second = SourceTaskExecution {
            name: "source-attempt-2".into(),
            uid: "source-pod-2".into(),
            attempt: 2,
            ..first.clone()
        };
        let third = SourceTaskExecution {
            name: "source-attempt-3".into(),
            uid: "source-pod-3".into(),
            attempt: 3,
            ..first.clone()
        };
        let (_, third_result) = tokio::join!(
            db.store.source_task_material(&[7; 32], id, &second),
            db.store.source_task_material(&[7; 32], id, &third),
        );
        third_result?;
        let current = db.store.source_task_context(id).await?;
        ensure!(
            current["source_execution"] == serde_json::to_value(&third)?,
            "source attempt moved backwards"
        );
        ensure!(
            current["pinned_ref_oid"] == "a".repeat(40),
            "retry changed fixed source"
        );
        ensure!(
            db.store
                .source_task_material(&[7; 32], id, &first)
                .await
                .is_err(),
            "old attempt acquired materials"
        );
        let replaced = SourceTaskExecution {
            uid: "same-attempt-other-pod".into(),
            ..third.clone()
        };
        ensure!(
            db.store
                .source_task_material(&[7; 32], id, &replaced)
                .await
                .is_err(),
            "same attempt accepted another Pod"
        );
        ensure!(
            db.store
                .pin_source(id, &first, &json!({"kind":"ref","oid":"b".repeat(40)}))
                .await
                .is_err(),
            "old attempt pinned source"
        );
        let result = json!({"manifest":manifest});
        ensure!(
            db.store
                .commit_source_task(id, &first, &result)
                .await
                .is_err(),
            "old attempt committed source"
        );
        db.store.commit_source_task(id, &third, &result).await?;
        db.store.commit_source_task(id, &first, &result).await?;
        let receipt = db.store.source_task_material(&[7; 32], id, &first).await?;
        ensure!(
            receipt.completed_result == Some(result) && receipt.credentials.is_empty(),
            "completed retry reissued credentials"
        );
        Ok(())
    }
    .await;
    db.close().await?;
    outcome
}

impl Database {
    async fn new() -> Result<Self> {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
        let schema = format!("source_fences_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await?;
        let scoped = with_options(&url, &[("search_path", &schema)])?;
        let pool = connect(&scoped, super::models(), PoolSettings::new(8)).await?;
        artifactory_database::migrate(&pool, &super::MIGRATIONS).await?;
        let store = ControlStore::new(pool.clone());
        store
            .bootstrap_admin("source-test-owner".into(), "unused".into())
            .await?;
        Ok(Self {
            admin,
            pool,
            schema,
            store,
        })
    }
    async fn unbound_task(&self) -> Result<(Uuid, SourceManifest)> {
        let project = Uuid::new_v4();
        let build = Uuid::new_v4();
        sql::execute(&mut self.pool.clone(),"INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','source-test','https://example.invalid/source.git')",params![project]).await?;
        sql::execute(&mut self.pool.clone(),r#"WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,model_config_snapshot,planning_context,release_id,release_version) VALUES($1,$2,'main','resolving','https://example.invalid/source.git','{}','{"source_credentials":[]}', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))"#,params![build,project]).await?;
        self.store.enqueue_source_resolutions().await?;
        let manifest = SourceManifest::new(
            build,
            vec![SourceRepository {
                path: String::new(),
                upstream_url: "https://example.invalid/source.git".into(),
                commit_sha: "a".repeat(40),
                tags: Default::default(),
                lfs_objects: Vec::new(),
            }],
        )?;
        Ok((build, manifest))
    }
    async fn task(&self) -> Result<(Uuid, SourceManifest)> {
        let task = self.unbound_task().await?;
        self.store
            .bind_source_task(task.0, "source-current")
            .await?;
        self.store
            .source_task_material(&[7; 32], task.0, &execution())
            .await?;
        Ok(task)
    }
    async fn pin(&self, id: Uuid) -> Result<()> {
        self.store
            .pin_source(
                id,
                &execution(),
                &json!({"kind":"ref","oid":"a".repeat(40)}),
            )
            .await?;
        self.store
            .pin_source(
                id,
                &execution(),
                &json!({"kind":"tags","path":"","tags":{}}),
            )
            .await?;
        Ok(())
    }
    async fn counts(&self, id: Uuid) -> Result<(i64, i64)> {
        Ok(sql::tuple(&mut self.pool.clone(),"SELECT (SELECT count(*) FROM source_manifests WHERE id=$1),(SELECT count(*) FROM planning_runs WHERE build_id=$1)",params![id]).await?)
    }
    async fn close(self) -> Result<()> {
        sql::execute(
            &mut self.admin.clone(),
            &format!("DROP SCHEMA {} CASCADE", self.schema),
            params![],
        )
        .await?;
        Ok(())
    }

    async fn native(&self) -> Result<(Uuid, Uuid, SourceManifest)> {
        let (build, manifest) = self.task().await?;
        self.pin(build).await?;
        self.store
            .commit_source_task(build, &execution(), &json!({"manifest":manifest}))
            .await?;
        let node = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let pool: Uuid = "00000000-0000-0000-0000-000000000001".parse()?;
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE projects SET pool_ids=$2 WHERE id=(SELECT project_id FROM builds WHERE id=$1)",
            params![build, vec![pool]],
        )
        .await?;
        sql::execute(&mut self.pool.clone(),"UPDATE builds SET status='queued',planning_context=jsonb_set(planning_context,'{node_access}',jsonb_build_object('mode','pools','ids',$2::jsonb)) WHERE id=$1",params![build,json!([pool])]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at) VALUES($1,'source-test','linux','amd64',$2,$3,now()+interval '1 day')",params![node,pool,node.to_string()]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO jobs(id,build_id,target,target_config,status) VALUES($1,$2,'native','{}','leased')",params![job,build]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id,source_manifest_id) VALUES($1,$2,$3,'source-test',now()+interval '1 day','leased',$4,$5)",params![attempt,job,node,Uuid::new_v4(),manifest.id]).await?;
        Ok((node, attempt, manifest))
    }
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies material issuance after authorization lock waits"]
async fn native_source_material_rechecks_lock_waits() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        for change in ["lease","revoked","cancel"] {
            let (node,attempt,manifest)=db.native().await?;
            db.store.native_source_material(&[7;32],node,attempt,"source-test",&manifest.digest).await?;
            let mut connection=db.pool.clone();let mut holder=connection.transaction().await?;
            let pid:i32=sql::scalar(&mut holder,"SELECT pg_backend_pid()",params![]).await?;
            let (table,id)=match change {"lease"=>("attempts",attempt),"revoked"=>("nodes",node),_=>("builds",manifest.id)};
            sql::execute(&mut holder,&format!("SELECT id FROM {table} WHERE id=$1 FOR UPDATE"),params![id]).await?;
            let store=db.store.clone();let digest=manifest.digest.clone();
            let acquire=tokio::spawn(async move{store.native_source_material(&[7;32],node,attempt,"source-test",&digest).await});
            let waiting=tokio::time::timeout(std::time::Duration::from_secs(30),async {
                loop {
                    let blocked:bool=sql::scalar(&mut db.pool.clone(),"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",params![pid]).await?;
                    if blocked{return Ok::<_,anyhow::Error>(());}
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }).await;
            let update=match change {"lease"=>"UPDATE attempts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1","revoked"=>"UPDATE nodes SET revoked=true WHERE id=$1",_=>"UPDATE builds SET cancel_requested=true WHERE id=$1"};
            sql::execute(&mut holder,update,params![id]).await?;holder.commit().await?;
            waiting.with_context(||format!("{change}: material acquire did not wait on authorization"))??;
            ensure!(tokio::time::timeout(std::time::Duration::from_secs(5),acquire).await??.is_err(),"{change}: material issued after authorization changed");
        }
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies image material issuance after authorization lock waits"]
async fn image_source_material_rechecks_lock_waits() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        for change in ["deadline", "cancel", "terminal"] {
            let (build, manifest) = db.task().await?;
            db.pin(build).await?;
            db.store.commit_source_task(build, &execution(), &json!({"manifest":manifest})).await?;
            let job = Uuid::new_v4();
            let execution = Uuid::new_v4();
            sql::execute(&mut db.pool.clone(), "INSERT INTO jobs(id,build_id,target,target_config,status) VALUES($1,$2,'image','{}','running')", params![job,build]).await?;
            sql::execute(&mut db.pool.clone(), "INSERT INTO image_executions(id,tenant_id,project_id,job_id,source_manifest_id,build_token_hash,publish_token_hash,spec,candidate_repository,destination_repository,tags,deadline_at) SELECT $1,b.tenant_id,b.project_id,$2,$3,'unused','unused','{\"platforms\":[\"linux/amd64\"]}','candidate','destination','{}',clock_timestamp()+interval '1 day' FROM builds b WHERE b.id=$3", params![execution,job,build]).await?;
            db.store.image_source_material(&[7;32],execution,"linux/amd64").await?;
            ensure!(db.store.image_source_material(&[7;32],execution,"linux/arm64").await.is_err(),"unassigned platform received source material");
            let mut connection = db.pool.clone();
            let mut holder = connection.transaction().await?;
            let pid: i32 = sql::scalar(&mut holder,"SELECT pg_backend_pid()",params![]).await?;
            let (table,id) = if change == "cancel" { ("builds",build) } else { ("image_executions",execution) };
            sql::execute(&mut holder,&format!("SELECT id FROM {table} WHERE id=$1 FOR UPDATE"),params![id]).await?;
            let store = db.store.clone();
            let acquire = tokio::spawn(async move {store.image_source_material(&[7;32],execution,"linux/amd64").await});
            let waiting = tokio::time::timeout(std::time::Duration::from_secs(5),async {
                loop {
                    let blocked: bool = sql::scalar(&mut db.pool.clone(),"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",params![pid]).await?;
                    if blocked { return Ok::<_,anyhow::Error>(()); }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }).await;
            let update = match change {
                "deadline" => "UPDATE image_executions SET deadline_at=clock_timestamp()-interval '1 second' WHERE id=$1",
                "cancel" => "UPDATE builds SET cancel_requested=true WHERE id=$1",
                _ => "UPDATE image_executions SET status='succeeded' WHERE id=$1",
            };
            sql::execute(&mut holder,update,params![id]).await?;
            holder.commit().await?;
            waiting.context("image material acquire did not wait on authorization")??;
            ensure!(tokio::time::timeout(std::time::Duration::from_secs(5),acquire).await??.is_err(),"{change}: image material issued after authorization changed");
        }
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies large fixed manifests are not duplicated in material replies"]
async fn large_source_material_keeps_one_manifest_copy() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let (build, mut manifest) = db.task().await?;
        manifest.repositories[0].tags = (0..65_000)
            .map(|index| (format!("release-{index:06}"), "a".repeat(40)))
            .collect();
        manifest = SourceManifest::new(build, manifest.repositories)?;
        ensure!(
            serde_json::to_vec(&manifest)?.len() > 3 * 1024 * 1024,
            "fixture does not exercise the transport boundary"
        );
        db.store
            .pin_source(
                build,
                &execution(),
                &json!({"kind":"ref","oid":"a".repeat(40)}),
            )
            .await?;
        db.store
            .pin_source(
                build,
                &execution(),
                &json!({"kind":"tags","path":"","tags":manifest.repositories[0].tags}),
            )
            .await?;
        db.store
            .commit_source_task(build, &execution(), &json!({"manifest":manifest}))
            .await?;
        let receipt = db
            .store
            .source_task_material(&[7; 32], build, &execution())
            .await?;
        ensure!(
            receipt
                .completed_result
                .as_ref()
                .and_then(|value| value.get("manifest"))
                .is_some(),
            "completed task lost its immutable receipt"
        );
        ensure!(
            receipt.manifest.is_none() && receipt.frozen_tags.is_empty(),
            "receipt duplicated the source manifest or tags"
        );
        let fixed = db.store.build_source_material(&[7; 32], build).await?;
        ensure!(
            fixed.manifest == Some(manifest) && fixed.frozen_tags.is_empty(),
            "fixed material duplicated or changed its manifest"
        );
        for material in [receipt, fixed] {
            ensure!(
                serde_json::to_vec(&material)?.len()
                    < artifactory_service_transport::service::SERVICE_JSON_LIMIT,
                "legal source manifest exceeds the material transport limit"
            );
        }
        Ok(())
    }
    .await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies completed-task receipts and least-privilege credentials"]
async fn completed_source_material_does_not_reissue_credentials() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        let (build,manifest)=db.task().await?;db.pin(build).await?;
        db.store.commit_source_task(build, &execution(),&json!({"manifest":manifest})).await?;
        let material=db.store.source_task_material(&[7;32],build,&execution()).await?;
        ensure!(material.credentials.is_empty() && material.completed_result.is_some(),"completed task reissued credentials instead of a receipt");
        let unused=Uuid::new_v4();
        sql::execute(&mut db.pool.clone(),"INSERT INTO credentials(id,tenant_id,name,kind,ciphertext) VALUES($1,'00000000-0000-0000-0000-000000000001','unused','https','deliberately not decryptable')",params![unused]).await?;
        let bound=Uuid::new_v4();
        sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,release_id,release_version,requested_ref,status,source_repository_url,source_manifest_id,commit_sha,model_config_snapshot,planning_context) SELECT $2,project_id,release_id,release_version,requested_ref,status,source_repository_url,source_manifest_id,commit_sha,model_config_snapshot,jsonb_set(planning_context,'{source_credentials}',$3) FROM builds WHERE id=$1",params![build,bound,json!([{"repository_url":"https://elsewhere.invalid/unused.git","credential_id":unused}])]).await?;
        let material=db.store.build_source_material(&[7;32],bound).await?;
        ensure!(material.credentials.is_empty(),"unrelated repository credential was disclosed");
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies concurrent CAS and atomic planning admission"]
async fn source_pins_and_commit_are_idempotent() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let (id, mut manifest) = db.task().await?;
        let a = json!({"kind":"ref","oid":"a".repeat(40)});
        let b = json!({"kind":"ref","oid":"b".repeat(40)});
        let identity = execution();
        let (a, b) = tokio::join!(
            db.store.pin_source(id, &identity, &a),
            db.store.pin_source(id, &identity, &b)
        );
        let pinned = a?;
        ensure!(
            pinned == b?,
            "concurrent resolvers observed different pinned objects"
        );
        manifest.repositories[0].commit_sha =
            pinned["oid"].as_str().context("pinned oid missing")?.into();
        manifest = SourceManifest::new(id, manifest.repositories)?;
        let first = db
            .store
            .pin_source(
                id,
                &execution(),
                &json!({"kind":"tags","path":"","tags":{}}),
            )
            .await?;
        let second = db
            .store
            .pin_source(
                id,
                &execution(),
                &json!({"kind":"tags","path":"","tags":{"new":"b".repeat(40)}}),
            )
            .await?;
        ensure!(first == second, "a retry followed moving tags");
        let durable:serde_json::Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('oid',pinned_ref_oid,'tags',frozen_tags) FROM source_tasks WHERE build_id=$1",params![id]).await?;
        ensure!(durable["oid"]==pinned["oid"]&&durable["tags"][""]==first["tags"],"partial source pins were not retained by Release");
        let result = json!({"manifest":manifest});
        let (a, b) = tokio::join!(
            db.store.commit_source_task(id, &identity, &result),
            db.store.commit_source_task(id, &identity, &result)
        );
        a?;
        b?;
        ensure!(
            db.counts(id).await? == (1, 1),
            "duplicate result created another manifest or planning run"
        );
        ensure!(
            db.store.source_task_context(id).await?["status"] == "succeeded",
            "completed task cannot acknowledge an earlier commit"
        );
        ensure!(
            db.store
                .commit_source_task(id, &execution(), &json!({"manifest":{}}))
                .await
                .is_err(),
            "completed result was replaced"
        );
        Ok(())
    }
    .await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies invalid and stopped preparations leave no partial state"]
async fn rejected_source_results_do_not_admit_planning() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        for change in ["digest","canceled","expired","archived"] {
            let (id,mut manifest)=db.task().await?;db.pin(id).await?;
            match change {
                "digest"=>manifest.digest="b".repeat(64),
                "canceled"=>{sql::execute(&mut db.pool.clone(),"UPDATE builds SET cancel_requested=true WHERE id=$1",params![id]).await?;},
                "expired"=>{sql::execute(&mut db.pool.clone(),"UPDATE source_tasks SET deadline_at=clock_timestamp()-interval '1 second' WHERE id=$1",params![id]).await?;},
                _=>{sql::execute(&mut db.pool.clone(),"UPDATE projects SET status='archived' WHERE id=(SELECT project_id FROM builds WHERE id=$1)",params![id]).await?;},
            }
            ensure!(db.store.commit_source_task(id, &execution(),&json!({"manifest":manifest})).await.is_err(),"{change} source result was accepted");
            ensure!(db.counts(id).await?==(0,0),"{change} result partially committed");
            if change!="digest" {ensure!(db.store.pin_source(id, &execution(),&json!({"kind":"ref","oid":"c".repeat(40)})).await.is_err(),"{change} source pin was accepted");}
        }
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies cancellation that commits during a source lock wait"]
async fn source_commit_rechecks_cancellation_after_build_lock() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        let (id,manifest)=db.task().await?;db.pin(id).await?;
        let mut connection=db.pool.clone();let mut holder=connection.transaction().await?;
        let pid:i32=sql::scalar(&mut holder,"SELECT pg_backend_pid()",params![]).await?;
        sql::execute(&mut holder,"SELECT id FROM builds WHERE id=$1 FOR UPDATE",params![id]).await?;
        let store=db.store.clone();
        let submit=tokio::spawn(async move{store.commit_source_task(id, &execution(),&json!({"manifest":manifest})).await});
        let waiting=tokio::time::timeout(std::time::Duration::from_secs(5),async {
            loop {
                let blocked:bool=sql::scalar(&mut db.pool.clone(),"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",params![pid]).await?;
                if blocked{return Ok::<_,anyhow::Error>(());}
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await;
        sql::execute(&mut holder,"UPDATE builds SET cancel_requested=true WHERE id=$1",params![id]).await?;
        holder.commit().await?;
        waiting.context("source submit did not wait on build")??;
        ensure!(tokio::time::timeout(std::time::Duration::from_secs(5),submit).await??.is_err(),"waiting source commit ignored cancellation");
        ensure!(db.counts(id).await?==(0,0),"waiting source commit admitted planning");
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies task-bound credential lookup cannot cross tenants"]
async fn source_credentials_require_the_bound_tenant() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        let tenant:Uuid="00000000-0000-0000-0000-000000000001".parse()?;
        let credential=GitCredential{kind:"https".into(),username:"git".into(),secret:"fixture-only".into(),known_hosts:String::new()};
        let ciphertext=crate::crypto::seal(&[7;32],&serde_json::to_vec(&credential)?)?;
        let id=Uuid::new_v4();
        sql::execute(&mut db.pool.clone(),"INSERT INTO credentials(id,tenant_id,name,kind,ciphertext) VALUES($1,$2,'source-test','https',$3)",params![id,tenant,ciphertext]).await?;
        let material=super::sources::source_credentials_with(&mut db.pool.clone(),&[7;32],tenant,"https://example.invalid/source.git",Some(id),json!([])).await?;
        ensure!(material.len()==1 && material[0].credential.secret=="fixture-only","bound credential unavailable");
        ensure!(super::sources::source_credentials_with(&mut db.pool.clone(),&[7;32],Uuid::new_v4(),"https://example.invalid/source.git",Some(id),json!([])).await.is_err(),"credential crossed tenant boundary");
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; independent build source identity"]
async fn independent_source_survives_another_build_cancellation() -> Result<()> {
    let db = Database::new().await?;
    let outcome=async {
  let (first,manifest)=db.task().await?;let second=Uuid::new_v4();
  sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,release_id,release_version,requested_ref,source_repository_url,model_config_snapshot,status) SELECT $2,project_id,release_id,release_version,requested_ref,source_repository_url,model_config_snapshot,'resolving' FROM builds WHERE id=$1",params![first,second]).await?;
  db.store.enqueue_source_resolutions().await?;
  db.store.bind_source_task(second,"source-current").await?;
  db.store.source_task_material(&[7;32],second,&execution()).await?;
  db.store.cancel_build(first).await?;
  db.store.enqueue_source_resolutions().await?;
  ensure!(db.pin(first).await.is_err(),"canceled build still accepted source pins");
  ensure!(db.store.commit_source_task(first,&execution(),&json!({"manifest":manifest})).await.is_err(),"canceled build still committed source");
  db.pin(second).await?;
  let own=SourceManifest::new(second,manifest.repositories)?;
  db.store.commit_source_task(second,&execution(),&json!({"manifest":own})).await?;
  let source:Uuid=sql::scalar(&mut db.pool.clone(),"SELECT source_manifest_id FROM builds WHERE id=$1",params![second]).await?;
  ensure!(source==second,"build used another build's resolver source");
  let state:String=sql::scalar(&mut db.pool.clone(),"SELECT status FROM builds WHERE id=$1",params![first]).await?;
  ensure!(state=="canceled","canceled build was revived");
  let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM source_tasks",params![]).await?;
  ensure!(count==2,"independent source tasks were collapsed");Ok(())
 }.await;
    db.close().await?;
    outcome
}
