//! [INPUT]: 依赖真实 PostgreSQL 和 Coordinator 业务提交事务。
//! [OUTPUT]: 提供隔离夹具，验证真实 outbox 统计和取消围栏。
//! [POS]: Coordinator 事件进入业务构建状态的数据库回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, PlanningEventCommit};
use anyhow::Result;
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use serde_json::{Value, json};
use uuid::Uuid;

#[path = "image_revision_tests.rs"]
mod image_revisions;
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

    async fn planning_fixture(&self) -> Result<(Uuid, Uuid, Uuid, Uuid)> {
        let project = Uuid::new_v4();
        let build = Uuid::new_v4();
        let run = Uuid::new_v4();
        let source = Uuid::new_v4();
        sql::execute(&mut self.pool.clone(), "INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','statistics','https://example.com/repo.git')", params![project]).await?;
        sql::execute(&mut self.pool.clone(), "INSERT INTO source_manifests(id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,'{}',$4)", params![source,project,"0".repeat(40),"a".repeat(64)]).await?;
        sql::execute(&mut self.pool.clone(), "WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version,source_manifest_id,goal,model_config_snapshot) VALUES($1,$2,'main','planning','https://example.com/repo.git',(SELECT id FROM fixture_release),(SELECT version FROM fixture_release),$3,'Build both targets','{}')", params![build,project,source]).await?;
        sql::execute(&mut self.pool.clone(), "INSERT INTO planning_runs(id,build_id,purpose,model_config,deadline_at) VALUES($1,$2,'initial','{}',now()+interval '1 minute')", params![run,build]).await?;
        Ok((project, build, run, source))
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn coordinator_outbox_statistics_reach_failed_input_accepted_and_rejected_runs() -> Result<()>
{
    use artifactory_planning_coordinator::store::{ModelRequest, PlanningStore};
    use artifactory_planning_protocol::{Execution, Lease, ModelConfiguration, RunIntent};
    use artifactory_service_transport::crypto;
    use std::sync::Arc;

    let db = Database::new().await?;
    let schema = format!("coordinator_statistics_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut db.admin.clone(),
        &format!("CREATE SCHEMA {schema}"),
        params![],
    )
    .await?;
    let url = with_options(
        &std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?,
        &[("search_path", &schema)],
    )?;
    let pool = connect(
        &url,
        artifactory_planning_coordinator::store::models(),
        PoolSettings::new(4),
    )
    .await?;
    artifactory_database::migrate(&pool, &artifactory_planning_coordinator::store::MIGRATIONS)
        .await?;
    let key = Arc::new([8; 32]);
    let coordinator = PlanningStore::new(pool, key.clone());
    let tenant = Uuid::parse_str("00000000-0000-0000-0000-000000000001")?;
    let usage =
        json!({"input":7,"output":3,"cacheRead":0,"cacheWrite":0,"totalTokens":10,"cost":0.25});
    let plan = json!({"summary":"compile app","evidence":[{"path":"Makefile","reason":"build definition"}],"recipe":{"version":1,"targets":{"app":{"os":"linux","arch":"amd64","application_platform":null,"steps":[{"name":"compile","run":"make"}],"artifacts":["dist/app"]}}}});
    for mode in ["failed", "needs_input", "accepted", "rejected"] {
        let (project, build, run, source) = db.planning_fixture().await?;
        let intent = RunIntent {
            tenant_id: tenant,
            run_id: run,
            build_id: build,
            version: 1,
            project_id: project,
            pool: "default".into(),
            source_manifest_id: Some(source),
            source_digest: Some("a".repeat(64)),
            context: json!({}),
            model: ModelConfiguration {
                id: Uuid::new_v4(),
                api: "openai-completions".into(),
                base_url: "https://model.invalid/v1".into(),
                model: "test".into(),
                api_key: "private-test-key".into(),
                parameters: json!({}),
                max_calls: 5,
                timeout_seconds: 600,
            },
        };
        let data = serde_json::to_vec(&intent)?;
        coordinator
            .put_intent(
                run,
                1,
                crypto::hash(&data),
                false,
                Some(intent),
                Some(crypto::seal(&key, &data)?),
            )
            .await?;
        coordinator.queue("default", 100, 100).await?;
        let execution = Execution {
            run_id: run,
            generation: 1,
            uid: format!("pod-{run}"),
            name: format!("pi-{run}"),
            namespace: "test".into(),
        };
        coordinator.register_execution(execution.clone()).await?;
        let acquired = coordinator
            .acquire_execution(execution, "https://planning.invalid")
            .await?;
        let lease: Lease = serde_json::from_value(acquired["lease"].clone())?;
        let request = json!({"model":"test","messages":[]});
        coordinator
            .reserve_model(ModelRequest {
                run,
                generation: 1,
                lease: &lease,
                request_id: Uuid::new_v4(),
                path: "v1/chat/completions",
                value: &request,
                hash: crypto::hash(serde_json::to_vec(&request)?),
            })
            .await?;
        coordinator.report_event(json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":1,"kind":"usage","payload":usage})).await?;
        if mode == "failed" {
            coordinator.report_event(json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":2,"kind":"failed","payload":{"error":"model authentication rejected"}})).await?;
        } else {
            let (name, arguments) = if mode == "needs_input" {
                ("request_input", json!({"question":"Which package?"}))
            } else {
                ("submit_plan", json!({"plan":plan}))
            };
            coordinator
                .tool_call(lease, name, Uuid::new_v4(), &json!({"arguments":arguments}))
                .await?;
        }
        let event = coordinator
            .deliveries()
            .await?
            .into_iter()
            .find(|event| event.run_id == run)
            .unwrap();
        let submit = || {
            db.store.commit_planning_event(
                PlanningEventCommit {
                    event: event.id,
                    run,
                    build,
                    hash: "durable-statistics".into(),
                    kind: &event.kind,
                    payload: &event.payload,
                    validation: (mode == "rejected").then(|| "invalid recipe".into()),
                },
                |_| async { crate::execution_plan::ExecutionPlan::parse(plan.clone()) },
            )
        };
        let result = submit().await?;
        assert_eq!(
            result,
            submit().await?,
            "duplicate delivery reuses its receipt"
        );
        let mut auth =
            crate::auth::Auth::scoped(Uuid::new_v4(), "admin", vec![project], vec!["read".into()]);
        auth.tenant_id = tenant;
        let visible = serde_json::to_value(db.store.plans(build, &auth).await?)?;
        assert_eq!(visible["runs"][0]["calls"], 1, "{mode}");
        assert_eq!(visible["runs"][0]["usage"], usage, "{mode}");
        assert_eq!(event.payload["calls"], 1);
        assert_eq!(event.payload["usage"], usage);
        assert_eq!(event.payload["generation"], 1);
        assert_eq!(
            visible["runs"][0]["state"],
            match mode {
                "failed" => "failed",
                "needs_input" => "needs_input",
                "accepted" => "succeeded",
                _ => "queued",
            }
        );
        if mode != "rejected" {
            let late = json!({"error":"late failure","calls":99,"usage":{"totalTokens":999},"generation":1});
            db.store
                .commit_planning_event(
                    PlanningEventCommit {
                        event: Uuid::new_v4(),
                        run,
                        build,
                        hash: "late-statistics".into(),
                        kind: "failed",
                        payload: &late,
                        validation: None,
                    },
                    |_| async { panic!("late failure cannot prepare a plan") },
                )
                .await?;
            let after = serde_json::to_value(db.store.plans(build, &auth).await?)?;
            assert_eq!(after["runs"][0]["calls"], 1);
            assert_eq!(after["runs"][0]["usage"], usage);
            assert_eq!(after["runs"][0]["state"], visible["runs"][0]["state"]);
        }
        coordinator
            .acknowledge_delivery(event.id, run, &event.kind, &event.payload, &result)
            .await?;
    }
    sql::execute(
        &mut db.admin.clone(),
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    db.close().await
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn planning_statistics_do_not_regress_or_change_after_cancellation() -> Result<()> {
    let db = Database::new().await?;
    let (_, build, run, _) = db.planning_fixture().await?;
    let submit = |payload: Value| {
        let store = db.store.clone();
        async move {
            store
                .commit_planning_event(
                    PlanningEventCommit {
                        event: Uuid::new_v4(),
                        run,
                        build,
                        hash: "statistics".into(),
                        kind: "proposal",
                        payload: &payload,
                        validation: Some("invalid recipe".into()),
                    },
                    |_| async { panic!("rejected proposals do not prepare a plan") },
                )
                .await
        }
    };
    let proposal = json!({"kind":"submit_plan","payload":{},"generation":2,"deadline_ms":chrono::Utc::now().timestamp_millis()+60_000,"calls":2,"usage":{"input":14,"output":6,"totalTokens":20,"cost":0.5}});
    submit(proposal.clone()).await?;
    for (calls, tokens, cost) in [(1, 100, 5.0), (2, 10, 0.1)] {
        let mut stale = proposal.clone();
        stale["calls"] = json!(calls);
        stale["usage"] = json!({"totalTokens":tokens,"cost":cost});
        submit(stale).await?;
    }
    let mut old_generation = proposal.clone();
    old_generation["generation"] = json!(1);
    old_generation["calls"] = json!(3);
    old_generation["usage"] = json!({"totalTokens":30});
    let result = submit(old_generation).await?;
    assert!(
        result["error"]
            .as_str()
            .is_some_and(|error| error.contains("generation"))
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='canceled',cancel_requested=true WHERE id=$1",
        params![build],
    )
    .await?;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE planning_runs SET state='canceled' WHERE id=$1",
        params![run],
    )
    .await?;
    let mut canceled = proposal;
    canceled["calls"] = json!(4);
    canceled["usage"] = json!({"totalTokens":40});
    submit(canceled).await?;
    let after: Value = sql::scalar(&mut db.pool.clone(), "SELECT jsonb_build_object('state',state,'calls',calls,'usage',usage) FROM planning_runs WHERE id=$1", params![run]).await?;
    assert_eq!(after["state"], "canceled");
    assert_eq!(after["calls"], 2);
    assert_eq!(
        after["usage"],
        json!({"input":14,"output":6,"totalTokens":20,"cost":0.5})
    );
    db.close().await
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
    sql::execute(&mut db.pool.clone(),"WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version) VALUES($1,$2,'main','queued','https://example.com/repo.git', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))",params![build,project]).await?;
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
        sql::execute(&mut db.pool.clone(), "WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,cancel_requested,source_repository_url,release_id,release_version) VALUES($1,$2,'main',$3,$4,'https://example.com/repo.git', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))", params![build,project,build_status,canceled]).await?;
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
