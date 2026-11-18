//! [INPUT]: 依赖真实 PostgreSQL 和 Runtime 业务提交事务。
//! [OUTPUT]: 验证重排不改变脚本、资源和已更新版本。
//! [POS]: 取代已删除 Poll RPC 的 Placement 业务回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, RuntimeEventCommit};
use anyhow::Result;
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use serde_json::{Value, json};
use uuid::Uuid;
struct Database {
    admin: toasty::Db,
    pool: toasty::Db,
    schema: String,
    store: ControlStore,
}

impl Database {
    async fn new() -> Result<Self> {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
        let schema = format!("placement_commit_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await?;
        let scoped = with_options(&url, &[("search_path", &schema)])?;
        let pool = connect(&scoped, super::super::models(), PoolSettings::new(8)).await?;
        artifactory_database::migrate(&pool, &super::super::MIGRATIONS).await?;
        let store = ControlStore::new(pool.clone());
        store
            .bootstrap_admin("placement-owner".into(), "unused".into())
            .await?;
        Ok(Self {
            admin,
            pool,
            schema,
            store,
        })
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
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn runtime_placement_commit_preserves_scripts_resources_and_versions() -> Result<()> {
    let db = Database::new().await?;
    let project = Uuid::new_v4();
    let build = Uuid::new_v4();
    let job = Uuid::new_v4();
    let run = Uuid::new_v4();
    let placement = json!({"candidates":[{"node_id":Uuid::new_v4(),"metadata_digest":"a".repeat(64),"reason":"fixture"}],"resources":{"min_available_memory_bytes":1024}});
    let target = json!({"os":"linux","arch":"amd64","steps":[{"name":"compile","run":"make"}]});
    sql::execute(&mut db.pool.clone(),"INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','placement','https://example.com/repo.git')",params![project]).await?;
    sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url) VALUES($1,$2,'main','queued','https://example.com/repo.git')",params![build,project]).await?;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO jobs(id,build_id,target,target_config) VALUES($1,$2,'app',$3)",
        params![job, build, &target],
    )
    .await?;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO job_placements(job_id,placement,version) VALUES($1,$2,1)",
        params![job, &placement],
    )
    .await?;
    sql::execute(&mut db.pool.clone(),"INSERT INTO planning_runs(id,build_id,purpose,model_config,deadline_at) VALUES($1,$2,'placement','{}',now()+interval '1 minute')",params![run,build]).await?;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO placement_rounds(build_id,round,run_id,versions) VALUES($1,1,$2,$3)",
        params![build, run, json!({"app":{"job_id":job,"version":1}})],
    )
    .await?;
    let submit = |args: Value| {
        let store = db.store.clone();
        async move {
            let payload = json!({"kind":"submit_placements","payload":args,"deadline_ms":chrono::Utc::now().timestamp_millis()+60_000});
            store
                .commit_runtime_event(
                    RuntimeEventCommit {
                        event: Uuid::new_v4(),
                        run,
                        build,
                        hash: Uuid::new_v4().to_string(),
                        kind: "proposal",
                        payload: &payload,
                        validation: None,
                    },
                    |_| async { panic!("placement never prepares executable scripts") },
                )
                .await
        }
    };
    let rejected = submit(
        json!({"placements":{"app":placement},"scripts":[{"name":"compile","content":"changed"}]}),
    )
    .await?;
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|s| s.contains("unknown field")),
        "{rejected}"
    );
    let mut relaxed = placement.clone();
    relaxed["resources"]["min_available_memory_bytes"] = json!(0);
    let rejected = submit(json!({"placements":{"app":relaxed}})).await?;
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|s| s.contains("resource requirements")),
        "{rejected}"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE job_placements SET version=2 WHERE job_id=$1",
        params![job],
    )
    .await?;
    let rejected = submit(json!({"placements":{"app":placement}})).await?;
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|s| s.contains("version superseded")),
        "{rejected}"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE job_placements SET version=1 WHERE job_id=$1",
        params![job],
    )
    .await?;
    let accepted = submit(json!({"placements":{"app":placement}})).await?;
    assert_eq!(accepted["accepted"], true, "{accepted}");
    let after: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT target_config FROM jobs WHERE id=$1",
        params![job],
    )
    .await?;
    assert_eq!(target, after);
    db.close().await
}
