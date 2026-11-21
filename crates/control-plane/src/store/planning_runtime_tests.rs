//! [INPUT]: 依赖隔离 PostgreSQL、真实 Coordinator 调用预留与项目授权。
//! [OUTPUT]: 验证提案前统计、安全摘要、终态隔离和查询降级。
//! [POS]: 规划查询跨业务与运行时边界的回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use artifactory_planning_coordinator::store::{ModelRequest, PlanningStore};
use artifactory_planning_protocol::{Execution, Lease, ModelConfiguration, RunIntent};
use artifactory_service_transport::crypto;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    admin: toasty::Db,
    pool: toasty::Db,
    schema: String,
    coordinator_schema: String,
    store: ControlStore,
    coordinator: PlanningStore,
    auth: Auth,
    build: Uuid,
    run: Uuid,
}

impl Fixture {
    async fn new() -> anyhow::Result<Self> {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
        let schema = format!("runtime_business_{}", Uuid::new_v4().simple());
        let coordinator_schema = format!("runtime_coordinator_{}", Uuid::new_v4().simple());
        for name in [&schema, &coordinator_schema] {
            sql::execute(
                &mut admin.clone(),
                &format!("CREATE SCHEMA {name}"),
                params![],
            )
            .await?;
        }
        let pool = connect(
            &with_options(&url, &[("search_path", &schema)])?,
            crate::store::models(),
            PoolSettings::new(4),
        )
        .await?;
        artifactory_database::migrate(&pool, &crate::store::MIGRATIONS).await?;
        let store = ControlStore::new(pool.clone());
        store
            .bootstrap_admin("runtime-owner".into(), "unused".into())
            .await?;
        let runtime_pool = connect(
            &with_options(&url, &[("search_path", &coordinator_schema)])?,
            artifactory_planning_coordinator::store::models(),
            PoolSettings::new(4),
        )
        .await?;
        artifactory_database::migrate(
            &runtime_pool,
            &artifactory_planning_coordinator::store::MIGRATIONS,
        )
        .await?;
        let key = Arc::new([7; 32]);
        let coordinator = PlanningStore::new(runtime_pool, key.clone());
        let project = Uuid::new_v4();
        let build = Uuid::new_v4();
        let run = Uuid::new_v4();
        let source = Uuid::new_v4();
        let tenant = Uuid::from_u128(1);
        sql::execute(&mut pool.clone(), "INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,$2,'runtime','https://example.com/repo.git')", params![project,tenant]).await?;
        sql::execute(&mut pool.clone(), "WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version) VALUES($1,$2,'main','planning','https://example.com/repo.git',(SELECT id FROM fixture_release),(SELECT version FROM fixture_release))", params![build,project]).await?;
        sql::execute(&mut pool.clone(), "INSERT INTO planning_runs(id,build_id,purpose,model_config) VALUES($1,$2,'initial','{}')", params![run,build]).await?;
        let intent = RunIntent {
            tenant_id: tenant,
            run_id: run,
            build_id: build,
            project_id: project,
            version: 1,
            source_manifest_id: Some(source),
            source_digest: Some("a".repeat(64)),
            context: json!({}),
            pool: "default".into(),
            model: ModelConfiguration {
                id: Uuid::new_v4(),
                api: "openai-completions".into(),
                base_url: "https://model.invalid/v1".into(),
                model: "test".into(),
                api_key: "private-provider-key".into(),
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
            uid: "pod-runtime".into(),
            name: "pi-runtime".into(),
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
        coordinator.report_event(json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":1,"kind":"usage","payload":{"input":7,"output":3,"cacheRead":0,"totalTokens":10,"cost":0.25,"secret":"must-never-leak"}})).await?;
        let mut auth = Auth::scoped(Uuid::new_v4(), "admin", vec![project], vec!["read".into()]);
        auth.tenant_id = tenant;
        Ok(Self {
            admin,
            pool,
            schema,
            coordinator_schema,
            store,
            coordinator,
            auth,
            build,
            run,
        })
    }

    async fn visible(&self) -> anyhow::Result<Value> {
        use artifactory_planning_coordinator::server::{self, Coordinator};
        use artifactory_service_transport::service::Client;
        let token = "runtime-fixture-service-token-00000000".to_string();
        let coordinator = Coordinator {
            store: self.coordinator.clone(),
            key: Arc::new([7; 32]),
            control: Client::new("https://control.invalid".into(), token.clone())?,
            model_http: reqwest::Client::new(),
            public_url: "https://planning.invalid".into(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let mut client = Client::new("https://planning.invalid".into(), token.clone())?;
        client.origin = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move {
            axum::serve(listener, server::router(coordinator, token))
                .await
                .unwrap();
        });
        let result = plans(&self.store, self.build, &self.auth, |run| async move {
            client.post("/v1/get", &json!({"run_id":run})).await
        })
        .await;
        server.abort();
        Ok(serde_json::to_value(result?)?)
    }

    async fn close(self) -> anyhow::Result<()> {
        for name in [&self.schema, &self.coordinator_schema] {
            sql::execute(
                &mut self.admin.clone(),
                &format!("DROP SCHEMA {name} CASCADE"),
                params![],
            )
            .await?;
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn model_reservations_are_visible_before_any_proposal_without_replacing_confirmed_values()
-> anyhow::Result<()> {
    let f = Fixture::new().await?;
    assert!(f.coordinator.deliveries().await?.is_empty());
    let visible = f.visible().await?;
    assert!(visible["plans"].as_array().unwrap().is_empty());
    assert_eq!(visible["runs"][0]["state"], "queued");
    assert_eq!(visible["runs"][0]["calls"], 0);
    assert_eq!(visible["runs"][0]["usage"], json!({}));
    assert_eq!(
        visible["runs"][0]["runtime"],
        json!({"status":"running","generation":1,"calls":1,"usage":{"input":7,"output":3,"cacheRead":0,"totalTokens":10,"cost":0.25}})
    );
    let serialized = visible.to_string();
    for secret in [
        "private-provider-key",
        "must-never-leak",
        "grant",
        "intent",
        "execution",
        "owner",
    ] {
        assert!(!serialized.contains(secret), "{secret}");
    }
    f.close().await
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn inactive_runs_skip_runtime_and_unavailable_reads_preserve_confirmed_statistics()
-> anyhow::Result<()> {
    let f = Fixture::new().await?;
    let reads = AtomicUsize::new(0);
    for state in ["canceled", "succeeded", "failed", "needs_input"] {
        sql::execute(
            &mut f.pool.clone(),
            "UPDATE planning_runs SET state=$2,calls=2,usage='{\"totalTokens\":20}' WHERE id=$1",
            params![f.run, state],
        )
        .await?;
        let visible = serde_json::to_value(
            plans(&f.store, f.build, &f.auth, |_| async {
                reads.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("unavailable")
            })
            .await?,
        )?;
        assert!(visible["runs"][0].get("runtime").is_none(), "{state}");
        assert!(
            visible["runs"][0].get("runtime_unavailable").is_none(),
            "{state}"
        );
        assert_eq!(visible["runs"][0]["state"], state);
        assert_eq!(visible["runs"][0]["calls"], 2);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    sql::execute(
        &mut f.pool.clone(),
        "UPDATE planning_runs SET state='leased' WHERE id=$1",
        params![f.run],
    )
    .await?;
    for result in [
        Err(anyhow::anyhow!("unavailable")),
        Ok(json!({"status":"running","lifecycle":{"generation":"invalid","calls":1}})),
    ] {
        let visible =
            serde_json::to_value(plans(&f.store, f.build, &f.auth, |_| async { result }).await?)?;
        assert_eq!(visible["runs"][0]["runtime_unavailable"], true);
        assert!(visible["runs"][0].get("runtime").is_none());
        assert_eq!(visible["runs"][0]["state"], "leased");
        assert_eq!(visible["runs"][0]["calls"], 2);
        assert_eq!(visible["runs"][0]["usage"], json!({"totalTokens":20}));
    }
    let mut denied = f.auth.clone();
    denied.project_roles.clear();
    assert!(
        plans(&f.store, f.build, &denied, |_| async {
            reads.fetch_add(1, Ordering::SeqCst);
            Ok(json!({}))
        })
        .await
        .is_err()
    );
    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "project authorization precedes runtime reads"
    );
    sql::execute(
        &mut f.pool.clone(),
        "UPDATE planning_runs SET state='succeeded' WHERE id=$1",
        params![f.run],
    )
    .await?;
    let latest = Uuid::new_v4();
    sql::execute(&mut f.pool.clone(),"INSERT INTO planning_runs(id,build_id,purpose,model_config,created_at) VALUES($1,$2,'revision','{}',now()+interval '1 second')",params![latest,f.build]).await?;
    let visible = serde_json::to_value(
        plans(&f.store, f.build, &f.auth, |run| async move {
            assert_eq!(run, latest);
            Ok(json!({"status":"queued","lifecycle":{"generation":2,"calls":3},"usage":{}}))
        })
        .await?,
    )?;
    assert!(visible["runs"][0].get("runtime").is_none());
    assert_eq!(visible["runs"][1]["runtime"]["calls"], 3);
    sql::execute(
        &mut f.pool.clone(),
        "UPDATE planning_runs SET state='canceled' WHERE id=$1",
        params![latest],
    )
    .await?;
    let visible = serde_json::to_value(
        plans(&f.store, f.build, &f.auth, |_| async {
            reads.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("latest canceled run must not read runtime")
        })
        .await?,
    )?;
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(
        visible["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|run| run.get("runtime").is_none() && run.get("runtime_unavailable").is_none())
    );
    f.close().await
}
