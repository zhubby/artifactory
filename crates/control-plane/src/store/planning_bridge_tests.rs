//! [INPUT]: 依赖真实 PostgreSQL 和 Coordinator 业务提交事务。
//! [OUTPUT]: 验证规划失败回执、取消围栏及重排不变约束。
//! [POS]: Coordinator 事件进入业务构建状态的数据库回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, PlanningEventCommit};
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
async fn planning_placement_commit_preserves_scripts_resources_and_versions() -> Result<()> {
    let db = Database::new().await?;
    let project = Uuid::new_v4();
    let build = Uuid::new_v4();
    let job = Uuid::new_v4();
    let run = Uuid::new_v4();
    let placement = json!({"candidates":[{"node_id":Uuid::new_v4(),"metadata_digest":"a".repeat(64),"reason":"fixture"}],"resources":{"min_available_memory_bytes":1024}});
    let target = json!({"os":"linux","arch":"amd64","steps":[{"name":"compile","run":"make"}]});
    sql::execute(&mut db.pool.clone(),"INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','placement','https://example.com/repo.git')",params![project]).await?;
    sql::execute(&mut db.pool.clone(),"WITH fixture_release AS (INSERT INTO releases(id,project_id,version,requested_ref,goal,target_requirements,targets,source_resolution_id) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-',''),'main','test','[]','{}',$1) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,spec_revision,release_version) VALUES($1,$2,'main','queued','https://example.com/repo.git', (SELECT id FROM fixture_release),1,(SELECT version FROM fixture_release))",params![build,project]).await?;
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
                .commit_planning_event(
                    PlanningEventCommit {
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

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn planning_execution_failure_finishes_the_build_once_and_preserves_cancellation()
-> Result<()> {
    let db = Database::new().await?;
    let project = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','failure','https://example.com/repo.git')", params![project]).await?;
    for canceled in [false, true] {
        let build = Uuid::new_v4();
        let run = Uuid::new_v4();
        let event = Uuid::new_v4();
        let build_status = if canceled { "canceled" } else { "planning" };
        let run_state = if canceled { "canceled" } else { "queued" };
        sql::execute(&mut db.pool.clone(), "WITH fixture_release AS (INSERT INTO releases(id,project_id,version,requested_ref,goal,target_requirements,targets,source_resolution_id) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-',''),'main','test','[]','{}',$1) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,cancel_requested,source_repository_url,release_id,spec_revision,release_version) VALUES($1,$2,'main',$3,$4,'https://example.com/repo.git', (SELECT id FROM fixture_release),1,(SELECT version FROM fixture_release))", params![build,project,build_status,canceled]).await?;
        sql::execute(&mut db.pool.clone(), "INSERT INTO planning_runs(id,build_id,purpose,state,model_config,deadline_at) VALUES($1,$2,'initial',$3,'{}',now()+interval '1 minute')", params![run,build,run_state]).await?;
        let payload =
            json!({"generation":1,"error":"source preparation container exited with code 1"});
        let commit = || {
            db.store.commit_planning_event(
                PlanningEventCommit {
                    event,
                    run,
                    build,
                    hash: "fixed-failure-event".into(),
                    kind: "failed",
                    payload: &payload,
                    validation: None,
                },
                |_| async { panic!("failure never prepares an executable plan") },
            )
        };
        let (first, replay) = tokio::join!(commit(), commit());
        let first = first?;
        assert_eq!(
            first, replay?,
            "concurrent duplicate failure must reuse the receipt"
        );
        let after: Value = sql::scalar(&mut db.pool.clone(), "SELECT jsonb_build_object('build_status',b.status,'run_state',r.state,'build_finished',b.finished_at IS NOT NULL,'run_finished',r.finished_at IS NOT NULL,'error',b.error) FROM builds b JOIN planning_runs r ON r.build_id=b.id WHERE b.id=$1", params![build]).await?;
        if canceled {
            assert!(
                first.get("error").is_some(),
                "canceled build rejects failure receipt"
            );
            assert_eq!(after["build_status"], "canceled");
            assert_eq!(after["run_state"], "canceled");
            assert!(after["error"].is_null());
        } else {
            assert_eq!(first["ok"], true);
            assert_eq!(after["build_status"], "failed");
            assert_eq!(after["run_state"], "failed");
            assert_eq!(after["build_finished"], true);
            assert_eq!(after["run_finished"], true);
            assert_eq!(after["error"], payload["error"]);
        }
        let count: i64 = sql::scalar(
            &mut db.pool.clone(),
            "SELECT count(*) FROM planning_receipts WHERE event_id=$1",
            params![event],
        )
        .await?;
        assert_eq!(
            count, 1,
            "duplicate failure creates exactly one business receipt"
        );
    }
    db.close().await
}
