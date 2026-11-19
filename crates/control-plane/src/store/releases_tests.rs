//! [INPUT]: 依赖隔离 PostgreSQL 和真实 Release 聚合事务。
//! [OUTPUT]: 验证并发源码复用、修订快照与封存交付围栏。
//! [POS]: Release 领域的数据库回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, ReleaseBuildInput, ReleaseInput, ReleaseUpdate};
use anyhow::{Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use serde_json::{Value, json};
use uuid::Uuid;
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
        Ok(self.store.create_release(self.project,&ReleaseInput{version:version.into(),reference:"main".into(),goal:"Build the CLI".into(),target_requirements:json!([{"kind":"native","key":"cli","os":"linux","arch":"amd64","format_suffix":".tar.gz"}]),targets:vec!["cli".into()]}).await?)
    }
    async fn build(&self, release: Uuid) -> Result<Uuid> {
        Ok(self
            .store
            .create_release_build(
                release,
                self.actor,
                &ReleaseBuildInput {
                    parameters: json!({}),
                    goal: None,
                },
            )
            .await?)
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
async fn concurrent_release_builds_share_one_resolver_and_snapshot_revision() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let release = db.release("1.0.0-rc.1").await?;
        let (a, b) = tokio::join!(db.build(release), db.build(release));
        let a = a?;
        let b = b?;
        let count: i64 = sql::scalar(
            &mut db.pool.clone(),
            "SELECT count(*) FROM source_tasks WHERE release_id=$1",
            params![release],
        )
        .await?;
        ensure!(count == 1, "parallel builds created multiple resolvers");
        db.store
            .update_release(
                release,
                &ReleaseUpdate {
                    spec_revision: 1,
                    goal: Some("Build revised CLI".into()),
                    target_requirements: None,
                    targets: None,
                },
            )
            .await?;
        ensure!(
            db.store
                .update_release(
                    release,
                    &ReleaseUpdate {
                        spec_revision: 1,
                        goal: Some("stale".into()),
                        target_requirements: None,
                        targets: None
                    }
                )
                .await
                .is_err(),
            "stale revision accepted"
        );
        let revision: i32 = sql::scalar(
            &mut db.pool.clone(),
            "SELECT spec_revision FROM builds WHERE id=$1",
            params![a],
        )
        .await?;
        ensure!(revision == 1, "existing build snapshot changed");
        let c = db.build(release).await?;
        let revision: i32 = sql::scalar(
            &mut db.pool.clone(),
            "SELECT spec_revision FROM builds WHERE id=$1",
            params![c],
        )
        .await?;
        ensure!(revision == 2, "new build did not snapshot latest revision");
        let other = db.release("1.0.0").await?;
        ensure!(other != release && a != b, "identities collapsed");
        let list = db.store.releases(Some(db.project), None).await?;
        ensure!(
            list[0].version == "1.0.0",
            "release list did not use SemVer order"
        );
        ensure!(
            db.release("1.0.0").await.is_err(),
            "duplicate version accepted"
        );
        Ok(())
    }
    .await;
    db.close().await?;
    result
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn source_retry_keeps_partial_pin_and_fixed_source_is_reused() -> Result<()> {
    let db = Database::new().await?;
    let result=async{
  let release=db.release("2.0.0").await?;
  let credential_a=Uuid::new_v4();let credential_b=Uuid::new_v4();
  for (credential,secret) in [(credential_a,"frozen-a"),(credential_b,"project-b")] {
    let material=artifactory_source_checkout::GitCredential{kind:"https".into(),username:"git".into(),secret:secret.into(),known_hosts:String::new()};let ciphertext=crate::crypto::seal(&[7;32],&serde_json::to_vec(&material)?)?;
    sql::execute(&mut db.pool.clone(),"INSERT INTO credentials(id,tenant_id,name,kind,ciphertext) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,'https',$3)",params![credential,credential.to_string(),ciphertext]).await?;
  }
  let bindings_a=json!([{"repository_url":"https://example.com/dependency-a.git","credential_id":credential_a}]);let bindings_b=json!([{"repository_url":"https://example.com/dependency-b.git","credential_id":credential_b}]);
  sql::execute(&mut db.pool.clone(),"UPDATE projects SET repository_url='https://example.com/first-build.git',credential_id=$2,source_credentials=$3 WHERE id=$1",params![db.project,credential_a,&bindings_a]).await?;
  let first=db.build(release).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE projects SET repository_url='https://example.com/changed.git',credential_id=$2,source_credentials=$3 WHERE id=$1",params![db.project,credential_b,&bindings_b]).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE releases SET pinned_ref_oid=$2,frozen_tags=$3 WHERE id=$1",params![release,"a".repeat(40),json!({"":{"v1":"a".repeat(40)}})]).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE source_tasks SET status='failed' WHERE id=$1",params![first]).await?;
  let second=db.build(release).await?;
  let old_status:String=sql::scalar(&mut db.pool.clone(),"SELECT status FROM builds WHERE id=$1",params![first]).await?;ensure!(old_status=="failed","source retry revived a failed resolver build");
  let task:Value=sql::scalar(&mut db.pool.clone(),"SELECT to_jsonb(s) FROM source_tasks s WHERE id=$1",params![second]).await?;ensure!(task["pinned_ref_oid"]=="a".repeat(40)&&task["frozen_tags"][""]["v1"]=="a".repeat(40),"retry changed partial source pin");
  ensure!(task["repository_url"]=="https://example.com/first-build.git"&&task["credential_id"]==json!(credential_a)&&task["bindings"]==bindings_a,"source retry followed changed project repository or credentials");
  let manifest=artifactory_source_checkout::SourceManifest::new(second,vec![artifactory_source_checkout::SourceRepository{path:String::new(),upstream_url:"https://example.com/first-build.git".into(),commit_sha:"a".repeat(40),tags:std::collections::BTreeMap::from([("v1".into(),"a".repeat(40))]),lfs_objects:vec![]}])?;
  sql::execute(&mut db.pool.clone(),"INSERT INTO source_manifests(id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,$5)",params![second,db.project,"a".repeat(40),json!(manifest),&manifest.digest]).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE releases SET source_manifest_id=$2,commit_sha=$3 WHERE id=$1",params![release,second,"a".repeat(40)]).await?;
  let third=db.build(release).await?;let source:Uuid=sql::scalar(&mut db.pool.clone(),"SELECT source_manifest_id FROM builds WHERE id=$1",params![third]).await?;ensure!(source==second,"new build resolved moving source again");
  let snapshot:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('url',source_repository_url,'credential',source_credential_id,'bindings',planning_context->'source_credentials') FROM builds WHERE id=$1",params![third]).await?;
  ensure!(snapshot["url"]=="https://example.com/first-build.git"&&snapshot["credential"]==json!(credential_a)&&snapshot["bindings"]==bindings_a,"fixed source build followed changed project source references");
  let material=db.store.build_source_material(&[7;32],third).await?;
  ensure!(material.repository_url=="https://example.com/first-build.git"&&material.credentials.len()==1&&material.credentials[0].credential.secret=="frozen-a","fixed material used changed project repository credentials");
  let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM source_tasks WHERE release_id=$1",params![release]).await?;ensure!(count==2,"fixed source created a new resolver");Ok(())
 }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn publication_freezes_exact_build_and_fences_expired_publishers() -> Result<()> {
    let db = Database::new().await?;
    let result=async{
  let release=db.release("3.0.0").await?;let build=db.build(release).await?;
  let source=artifactory_source_checkout::SourceManifest::new(build,vec![artifactory_source_checkout::SourceRepository{path:String::new(),upstream_url:"https://example.com/repo.git".into(),commit_sha:"a".repeat(40),tags:Default::default(),lfs_objects:vec![]}])?;
  sql::execute(&mut db.pool.clone(),"INSERT INTO source_manifests(id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,$5)",params![build,db.project,"a".repeat(40),json!(source),&source.digest]).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE releases SET source_manifest_id=$2,commit_sha=$3 WHERE id=$1",params![release,build,"a".repeat(40)]).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE source_tasks SET status='succeeded' WHERE release_id=$1",params![release]).await?;
  sql::execute(&mut db.pool.clone(),"UPDATE builds SET status='succeeded',source_manifest_id=$1,commit_sha=$2 WHERE id=$1",params![build,"a".repeat(40)]).await?;
  let node=Uuid::new_v4();let job=Uuid::new_v4();let attempt=Uuid::new_v4();let artifact=Uuid::new_v4();
  sql::execute(&mut db.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at) VALUES($1,'test','linux','amd64','00000000-0000-0000-0000-000000000001',$2,now()+interval '1 day')",params![node,node.to_string()]).await?;
  sql::execute(&mut db.pool.clone(),"INSERT INTO jobs(id,build_id,target,target_config,status) VALUES($1,$2,'cli','{}','succeeded')",params![job,build]).await?;
  sql::execute(&mut db.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id,source_manifest_id) VALUES($1,$2,$3,'test',now(),'succeeded',$4,$5)",params![attempt,job,node,Uuid::new_v4(),build]).await?;
  sql::execute(&mut db.pool.clone(),"INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'cli.tar.gz',$3,4,$4,now())",params![artifact,attempt,artifact.to_string(),"b".repeat(64)]).await?;
  sql::execute(&mut db.pool.clone(),"INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'cli.tar.gz',$3,4,$4)",params![artifact,attempt,artifact.to_string(),"b".repeat(64)]).await?;
  ensure!(db.store.publish_transaction(build).await?==release,"freeze changed Release identity");
  let frozen=db.store.release(release).await?;let manifest=frozen.delivery_manifest.unwrap();ensure!(manifest["files"][0]["delivery_filename"]=="release-test-cli-3.0.0-linux-amd64.tar.gz","formal filename is not versioned");ensure!(manifest["source_digest"]==source.digest,"source digest missing");
  ensure!(db.build(release).await.is_err(),"sealed release accepted new build");
  let generation=db.store.acquire_release_publisher(release,&"a".repeat(64)).await?.unwrap();
  ensure!(db.store.acquire_release_publisher(release,&"b".repeat(64)).await?.is_none(),"two publishers acquired one delivery");
  sql::execute(&mut db.pool.clone(),"UPDATE release_publication_leases SET expires_at=now()-interval '1 second' WHERE release_id=$1",params![release]).await?;
  ensure!(db.store.complete_release_publication(release,generation).await.is_err(),"expired publisher completed delivery");ensure!(db.store.fail_release_publication(release,generation,"stale").await.is_err(),"expired publisher failed delivery");
  let next=db.store.acquire_release_publisher(release,&"c".repeat(64)).await?.unwrap();ensure!(next>generation,"publisher generation did not advance");
  db.store.fail_release_publication(release,next,"network failed").await?;db.store.finish_release_publisher(release,next).await?;
  ensure!(db.store.release(release).await?.status=="publish_failed","failure did not seal delivery");
  ensure!(db.store.publish_transaction(build).await?==release,"retry changed delivery");let last=db.store.acquire_release_publisher(release,&"d".repeat(64)).await?.unwrap();
  ensure!(db.store.complete_release_publication(release,generation).await.is_err(),"old generation completed newer owner");
  db.store.complete_release_publication(release,last).await?;db.store.complete_release_publication(release,last).await?;
  let done=db.store.release(release).await?;ensure!(done.status=="published"&&done.delivery_manifest==Some(manifest),"retry changed frozen manifest");let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM publish_attempts WHERE build_id=$1 AND status='succeeded'",params![build]).await?;ensure!(count==1,"completion was not idempotent");Ok(())
 }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn source_cleanup_skips_release_locked_for_a_new_waiter() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
  let release=db.release("4.0.0").await?;let first=db.build(release).await?;db.store.cancel_build(first).await?;
  let mut pool=db.pool.clone();let mut tx=pool.transaction().await?;
  sql::execute(&mut tx,"SELECT id FROM releases WHERE id=$1 FOR UPDATE",params![release]).await?;
  let waiter=Uuid::new_v4();
  sql::execute(&mut tx,"INSERT INTO builds(id,project_id,release_id,release_version,spec_revision,requested_ref,source_repository_url,source_credential_id,planning_context,model_config_snapshot,goal,target_requirements,selected_targets) SELECT $2,project_id,release_id,release_version,spec_revision,requested_ref,source_repository_url,source_credential_id,planning_context,model_config_snapshot,goal,target_requirements,selected_targets FROM builds WHERE id=$1",params![first,waiter]).await?;
  tokio::time::timeout(std::time::Duration::from_secs(2),db.store.enqueue_source_resolutions()).await??;
  let state:String=sql::scalar(&mut tx,"SELECT status FROM source_tasks WHERE id=$1",params![first]).await?;ensure!(state=="queued","cleanup canceled a task while a new waiter held Release lock");
  tx.commit().await?;db.store.enqueue_source_resolutions().await?;
  let state:String=sql::scalar(&mut db.pool.clone(),"SELECT status FROM source_tasks WHERE id=$1",params![first]).await?;ensure!(state=="queued","cleanup did not see committed waiter");
  let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM source_tasks WHERE release_id=$1",params![release]).await?;ensure!(count==1,"cleanup race duplicated resolver");Ok(())
 }.await;
    db.close().await?;
    result
}
