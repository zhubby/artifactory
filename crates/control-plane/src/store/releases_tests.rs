//! [INPUT]: 依赖隔离 PostgreSQL 和真实 Release 聚合事务。
//! [OUTPUT]: 验证版本元数据、授权设备准入、构建源码隔离与封存交付围栏。
//! [POS]: Release 领域的数据库回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, ReleaseBuildInput, ReleaseInput, ReleaseUpdate};
use anyhow::{Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
fn release_requests_accept_only_version_metadata() {
    assert!(
        serde_json::from_value::<ReleaseInput>(json!({"version":"1.2.3","notes":"修复启动问题"}))
            .is_ok()
    );
    assert!(serde_json::from_value::<ReleaseInput>(json!({"version":"1.2.3"})).is_ok());
    assert!(serde_json::from_value::<ReleaseUpdate>(json!({"notes":"补充版本说明"})).is_ok());
    assert!(
        serde_json::from_value::<ReleaseInput>(json!({"version":"1.2.3","ref":"main"})).is_err()
    );
}

#[test]
fn build_requests_require_their_own_source_and_targets() {
    let request = json!({"ref":"refs/tags/v1.2.3","goal":"构建命令行程序","target_requirements":[{"kind":"native","key":"cli","os":"linux","arch":"amd64"}],"targets":["cli"]});
    assert!(serde_json::from_value::<ReleaseBuildInput>(request).is_ok());
    assert!(serde_json::from_value::<ReleaseBuildInput>(json!({"parameters":{}})).is_err());
}
struct Database {
    admin: toasty::Db,
    pool: toasty::Db,
    schema: String,
    store: ControlStore,
    project: Uuid,
    actor: Uuid,
}
impl Database {
    async fn new() -> Result<Self> {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
        let schema = format!("release_domain_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await?;
        let pool = connect(
            &with_options(&url, &[("search_path", &schema)])?,
            super::models(),
            PoolSettings::new(10),
        )
        .await?;
        artifactory_database::migrate(&pool, &super::MIGRATIONS).await?;
        let unscoped = ControlStore::new(pool.clone());
        unscoped
            .bootstrap_admin("release-owner".into(), "unused".into())
            .await?;
        let actor: Uuid = sql::scalar(
            &mut pool.clone(),
            "SELECT id FROM users WHERE username='release-owner'",
            params![],
        )
        .await?;
        let tenant = Uuid::from_u128(1);
        let project = Uuid::new_v4();
        sql::execute(&mut pool.clone(),"INSERT INTO projects(id,tenant_id,name,repository_url,pool_ids) VALUES($1,$2,'release-test','https://example.com/repo.git','{00000000-0000-0000-0000-000000000001}')",params![project,tenant]).await?;
        sql::execute(&mut pool.clone(),"INSERT INTO model_profiles(id,tenant_id,name,api,base_url,model,key_ciphertext,is_default) VALUES($1,$2,'test','openai-responses','https://example.com','test','unused',true)",params![Uuid::new_v4(),tenant]).await?;
        for os in ["linux", "darwin"] {
            let node = Uuid::new_v4();
            sql::execute(&mut pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at) VALUES($1,'authorized-test',$2,'amd64','00000000-0000-0000-0000-000000000001',$3,now()+interval '1 day')",params![node,os,node.to_string()]).await?;
        }
        let mut auth = crate::auth::Auth::scoped(
            actor,
            "builder",
            vec![project],
            vec!["read".into(), "build".into(), "release".into()],
        );
        auth.tenant_id = tenant;
        let store = unscoped.for_auth(&auth);
        Ok(Self {
            admin,
            pool,
            schema,
            store,
            project,
            actor,
        })
    }
    async fn release(&self, version: &str) -> Result<Uuid> {
        Ok(self
            .store
            .create_release(
                self.project,
                &ReleaseInput {
                    version: version.into(),
                    notes: "版本说明".into(),
                },
            )
            .await?)
    }
    async fn build(&self, release: Uuid) -> Result<Uuid> {
        self.build_from(release, "main", "cli", "linux").await
    }
    async fn build_from(
        &self,
        release: Uuid,
        reference: &str,
        key: &str,
        os: &str,
    ) -> Result<Uuid> {
        Ok(self.store.create_release_build(release, self.actor, &ReleaseBuildInput {
            reference: reference.into(), goal: "Build the CLI".into(),
            target_requirements: json!([{"kind":"native","key":key,"os":os,"arch":"amd64","format_suffix":".tar.gz"}]),
            targets: vec![key.into()], parameters: json!({}),
        }).await?)
    }
    async fn resolve(
        &self,
        build: Uuid,
        commit: &str,
    ) -> Result<artifactory_source_checkout::SourceManifest> {
        let execution = super::sources::SourceTaskExecution {
            cr_uid: format!("source-{build}"),
            name: "source-pod".into(),
            uid: format!("pod-{build}"),
            attempt: 1,
        };
        self.store
            .bind_source_task(build, &execution.cr_uid)
            .await?;
        self.store
            .source_task_material(&[7; 32], build, &execution)
            .await?;
        self.store
            .pin_source(build, &execution, &json!({"kind":"ref","oid":commit}))
            .await?;
        self.store
            .pin_source(
                build,
                &execution,
                &json!({"kind":"tags","path":"","tags":{}}),
            )
            .await?;
        let repository: String = sql::scalar(
            &mut self.pool.clone(),
            "SELECT source_repository_url FROM builds WHERE id=$1",
            params![build],
        )
        .await?;
        let manifest = artifactory_source_checkout::SourceManifest::new(
            build,
            vec![artifactory_source_checkout::SourceRepository {
                path: String::new(),
                upstream_url: repository,
                commit_sha: commit.into(),
                tags: Default::default(),
                lfs_objects: vec![],
            }],
        )?;
        self.store
            .commit_source_task(build, &execution, &json!({"manifest":manifest}))
            .await?;
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE planning_runs SET state='succeeded',finished_at=now() WHERE build_id=$1",
            params![build],
        )
        .await?;
        Ok(manifest)
    }
    async fn artifact(&self, build: Uuid) -> Result<(Uuid, Uuid)> {
        let source: Uuid = sql::scalar(
            &mut self.pool.clone(),
            "SELECT source_manifest_id FROM builds WHERE id=$1",
            params![build],
        )
        .await?;
        let node = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        let artifact = Uuid::new_v4();
        sql::execute(&mut self.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at) VALUES($1,'test','linux','amd64','00000000-0000-0000-0000-000000000001',$2,now()+interval '1 day')",params![node,node.to_string()]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO jobs(id,build_id,target,target_config,status) SELECT $1,id,selected_targets[1],'{}','succeeded' FROM builds WHERE id=$2",params![job,build]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id,source_manifest_id) VALUES($1,$2,$3,'test',now(),'succeeded',$4,$5)",params![attempt,job,node,Uuid::new_v4(),source]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'cli.tar.gz',$3,4,$4,now())",params![artifact,attempt,artifact.to_string(),"b".repeat(64)]).await?;
        sql::execute(&mut self.pool.clone(),"INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'cli.tar.gz',$3,4,$4)",params![artifact,attempt,artifact.to_string(),"b".repeat(64)]).await?;
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE builds SET status='succeeded' WHERE id=$1",
            params![build],
        )
        .await?;
        Ok((job, artifact))
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
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn native_build_targets_require_current_project_device_grants() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let release = db.release("1.2.3").await?;
        // 离线且暂时排空的设备仍能排队；无授权平台必须在写入构建前拒绝。
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE nodes SET draining=true",
            params![],
        )
        .await?;
        db.build(release).await?;
        let denied = db
            .build_from(release, "main", "windows-cli", "windows")
            .await;
        ensure!(
            denied.is_err(),
            "a platform without an authorized device was accepted"
        );
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE projects SET node_access_mode='nodes',node_ids='{}' WHERE id=$1",
            params![db.project],
        )
        .await?;
        ensure!(
            db.build(release).await.is_err(),
            "pool devices bypassed explicit node grants"
        );
        let node: Uuid = sql::scalar(
            &mut db.pool.clone(),
            "SELECT id FROM nodes WHERE os='linux'",
            params![],
        )
        .await?;
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE projects SET node_ids=ARRAY[$2] WHERE id=$1",
            params![db.project, node],
        )
        .await?;
        db.build(release).await?;
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE nodes SET revoked=true WHERE id=$1",
            params![node],
        )
        .await?;
        ensure!(
            db.build(release).await.is_err(),
            "a revoked device authorized a native target"
        );
        Ok(())
    }
    .await;
    db.close().await?;
    result
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn concurrent_release_builds_have_independent_resolvers() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let release = db.release("1.0.0-rc.1").await?;
        let (a,b) = tokio::join!(db.build_from(release,"refs/heads/main","linux-cli","linux"), db.build_from(release,"refs/tags/v1","mac-cli","darwin"));
        let (a,b) = (a?,b?);
        let tasks: Vec<Value> = sql::scalars(&mut db.pool.clone(), "SELECT to_jsonb(s) FROM source_tasks s JOIN builds b ON b.id=s.build_id WHERE b.release_id=$1", params![release]).await?;
        ensure!(tasks.len()==2, "parallel builds must have independent resolvers");
        ensure!(tasks.iter().any(|t|t["build_id"]==json!(a)&&t["reference"]=="refs/heads/main"), "first build lost its source reference");
        ensure!(tasks.iter().any(|t|t["build_id"]==json!(b)&&t["reference"]=="refs/tags/v1"), "second build lost its source reference");
        db.store.update_release(release, &ReleaseUpdate { notes:"修正文案".into() }).await?;
        let build = db.store.build_detail(a).await?;
        ensure!(build.build.target_requirements[0]["key"]=="linux-cli" && build.build.requested_ref=="refs/heads/main", "notes changed build input");
        let detail = json!(db.store.release(release).await?);
        ensure!(detail["notes"]=="修正文案" && detail.get("requested_ref").is_none() && detail.get("spec_revision").is_none(), "Release still exposes build configuration");
        let other=db.release("1.0.0").await?;
        ensure!(other!=release && db.store.releases(Some(db.project),None).await?[0].version=="1.0.0", "release identity or SemVer ordering changed");
        Ok(())
    }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn new_builds_resolve_moving_refs_and_keep_their_own_source_snapshots() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let release = db.release("2.0.0").await?;
        let first = db.build(release).await?;
        db.resolve(first, &"a".repeat(40)).await?;
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE projects SET repository_url='https://example.com/changed.git' WHERE id=$1",
            params![db.project],
        )
        .await?;
        let second = db.build(release).await?;
        let task: Value = sql::scalar(
            &mut db.pool.clone(),
            "SELECT to_jsonb(s) FROM source_tasks s WHERE id=$1",
            params![second],
        )
        .await?;
        ensure!(
            task["pinned_ref_oid"].is_null()
                && task["repository_url"] == "https://example.com/changed.git",
            "new build inherited another build's source snapshot"
        );
        db.resolve(second, &"b".repeat(40)).await?;
        let old = db.store.build_source_material(&[7; 32], first).await?;
        let new = db.store.build_source_material(&[7; 32], second).await?;
        ensure!(
            old.repository_url == "https://example.com/repo.git"
                && old.manifest.unwrap().root()?.commit_sha == "a".repeat(40),
            "existing build followed project or branch changes"
        );
        ensure!(
            new.repository_url == "https://example.com/changed.git"
                && new.manifest.unwrap().root()?.commit_sha == "b".repeat(40),
            "new build reused the old commit"
        );
        ensure!(
            sql::execute(
                &mut db.pool.clone(),
                "UPDATE builds SET commit_sha=$2 WHERE id=$1",
                params![first, "c".repeat(40)]
            )
            .await
            .is_err(),
            "build source commit was mutable"
        );
        Ok(())
    }
    .await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn publication_freezes_exact_build_and_fences_expired_publishers() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        let release=db.release("3.0.0").await?;
        let build=db.build(release).await?;
        let other=db.build_from(release,"refs/tags/alternate","mac-cli","darwin").await?;
        let source=db.resolve(build,&"a".repeat(40)).await?;
        db.resolve(other,&"c".repeat(40)).await?;
        let (_job,_artifact)=db.artifact(build).await?;
        let (other_job,foreign_artifact)=db.artifact(other).await?;
        sql::execute(&mut db.pool.clone(),"UPDATE jobs SET status='failed' WHERE id=$1",params![other_job]).await?;
        sql::execute(&mut db.pool.clone(),"UPDATE builds SET status='failed' WHERE id=$1",params![other]).await?;
        ensure!(db.store.publish_transaction(build).await?==release,"freeze changed Release identity");
        let manifest=db.store.release(release).await?.delivery_manifest.unwrap();
        ensure!(manifest["schema_version"]==2 && manifest["source_digest"]==source.digest && manifest["target_requirements"][0]["key"]=="cli", "delivery did not freeze the selected build contract");
        ensure!(manifest["files"][0]["delivery_filename"]=="release-test-cli-3.0.0-linux-amd64.tar.gz", "formal filename is not versioned");
        ensure!(sql::execute(&mut db.pool.clone(),"INSERT INTO release_artifacts(release_id,artifact_id,delivery_filename) VALUES($1,$2,'foreign.tar.gz')",params![release,foreign_artifact]).await.is_err(),"another build's artifact entered the delivery");
        ensure!(db.store.publish_transaction(other).await.is_err(),"sealed release switched to another build");
        ensure!(db.build(release).await.is_err(),"sealed release accepted new build");
        let auth=crate::auth::Auth::scoped(db.actor,"builder",vec![db.project],vec!["build".into()]);
        ensure!(db.store.retry_job(other_job,&auth).await.is_err(),"sealed release accepted execution retry");
        db.store.update_release(release,&ReleaseUpdate{notes:"发布中补充说明".into()}).await?;
        let generation=db.store.acquire_release_publisher(release,&"a".repeat(64)).await?.unwrap();
        ensure!(db.store.acquire_release_publisher(release,&"b".repeat(64)).await?.is_none(),"two publishers acquired one delivery");
        sql::execute(&mut db.pool.clone(),"UPDATE release_publication_leases SET expires_at=now()-interval '1 second' WHERE release_id=$1",params![release]).await?;
        ensure!(db.store.complete_release_publication(release,generation).await.is_err(),"expired publisher completed delivery");
        ensure!(db.store.fail_release_publication(release,generation,"stale").await.is_err(),"expired publisher failed delivery");
        let next=db.store.acquire_release_publisher(release,&"c".repeat(64)).await?.unwrap();
        ensure!(next>generation,"publisher generation did not advance");
        db.store.fail_release_publication(release,next,"network failed").await?;
        db.store.finish_release_publisher(release,next).await?;
        db.store.update_release(release,&ReleaseUpdate{notes:"发布失败时补充说明".into()}).await?;
        ensure!(db.store.release(release).await?.status=="publish_failed","failure did not seal delivery");
        ensure!(db.store.publish_transaction(build).await?==release,"retry changed delivery");
        let last=db.store.acquire_release_publisher(release,&"d".repeat(64)).await?.unwrap();
        ensure!(db.store.complete_release_publication(release,generation).await.is_err(),"old generation completed newer owner");
        db.store.complete_release_publication(release,last).await?;
        db.store.complete_release_publication(release,last).await?;
        db.store.update_release(release,&ReleaseUpdate{notes:"发布后修正说明".into()}).await?;
        let done=db.store.release(release).await?;
        ensure!(done.status=="published" && done.notes=="发布后修正说明" && done.delivery_manifest==Some(manifest),"notes changed the frozen delivery");
        let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM publish_attempts WHERE build_id=$1 AND status='succeeded'",params![build]).await?;
        ensure!(count==1,"completion was not idempotent");
        Ok(())
    }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn source_cleanup_skips_locked_release_and_cancels_only_its_build() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let release = db.release("4.0.0").await?;
        let first = db.build(release).await?;
        let second = db.build(release).await?;
        db.store.cancel_build(first).await?;
        let mut pool = db.pool.clone();
        let mut tx = pool.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT id FROM releases WHERE id=$1 FOR UPDATE",
            params![release],
        )
        .await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            db.store.enqueue_source_resolutions(),
        )
        .await??;
        let state: String = sql::scalar(
            &mut tx,
            "SELECT status FROM source_tasks WHERE id=$1",
            params![first],
        )
        .await?;
        ensure!(state == "queued", "cleanup ignored the Release lock");
        tx.commit().await?;
        db.store.enqueue_source_resolutions().await?;
        let canceled: String = sql::scalar(
            &mut db.pool.clone(),
            "SELECT status FROM source_tasks WHERE id=$1",
            params![first],
        )
        .await?;
        let waiting: String = sql::scalar(
            &mut db.pool.clone(),
            "SELECT status FROM source_tasks WHERE id=$1",
            params![second],
        )
        .await?;
        ensure!(
            canceled == "canceled" && waiting == "queued",
            "source cancellation affected another build"
        );
        Ok(())
    }
    .await;
    db.close().await?;
    result
}
