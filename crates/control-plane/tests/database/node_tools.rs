//! [INPUT]: node_metadata 的租户、规划租约和 mTLS 夹具。
//! [OUTPUT]: 验证 Runtime 节点工具及真实 MCP SDK 授权。
//! [POS]: 分别验证 Control 与 Runtime 的租约和项目范围。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL and ARTIFACTORY_SERVICE_TOKEN"]
async fn runtime_node_tools_authenticate_and_intersect_current_project_scope() {
    let database = Database::with_tenant().await;
    let (_, build, _) = database.fixture().await;
    let node = database.node().await;
    let run = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(),"UPDATE builds SET status='planning',planning_context=jsonb_build_object('pool_ids',jsonb_build_array('00000000-0000-0000-0000-000000000001'),'signing_profiles','[]'::jsonb) WHERE id=$1",params![build]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO planning_runs(id,build_id,purpose,model_config) VALUES($1,$2,'initial','{}')",
        params![run, build],
    )
    .await
    .unwrap();
    let token = std::env::var("ARTIFACTORY_SERVICE_TOKEN")
        .expect("isolated Runtime test needs ARTIFACTORY_SERVICE_TOKEN");
    let router = api::router(database.state.clone());
    let body = json!({"run_id":run,"name":"get_node","arguments":{"node_id":node}});
    for authorized in [false, true] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/internal/runtime/tool")
            .header("content-type", "application/json");
        if authorized {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if authorized {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
        if authorized {
            let value: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 131072)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(value["nodes"][0]["node_id"], json!(node));
        }
    }
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE projects SET pool_ids='{}' WHERE id=(SELECT project_id FROM builds WHERE id=$1)",
        params![build],
    )
    .await
    .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri("/internal/runtime/tool")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 131072)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["nodes"], json!([]));
    database.close().await;
}

async fn mcp_probe(config: &Value) -> Value {
    use tokio::io::AsyncWriteExt;
    let mut probe = tokio::process::Command::new(
        std::env::var("ARTIFACTORY_NODE").unwrap_or_else(|_| "node".into()),
    )
    .arg(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/agent-worker/scripts/node-mcp-probe.mjs"
    ))
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
    probe
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(config).unwrap())
        .await
        .unwrap();
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), probe.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "MCP probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["initialized"], "artifactory-nodes");
    assert_eq!(
        report["tools"],
        json!(["summarize_nodes", "query_nodes", "get_node"])
    );
    report
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL, Node.js 24, built Agent Worker and ARTIFACTORY_SERVICE_TOKEN"]
async fn official_mcp_sdk_queries_over_real_runtime_and_rejects_stale_leases() {
    use artifactory_planner_runtime::{server, store::RuntimeStore};
    use artifactory_runtime_protocol::{Execution, ModelConfiguration, RunIntent, service::Client};
    let database = Database::with_tenant().await;
    let (project, build, _) = database.fixture().await;
    let node = database.node().await;
    let run = Uuid::new_v4();
    let token = std::env::var("ARTIFACTORY_SERVICE_TOKEN").unwrap();
    sql::execute(&mut database.pool.clone(),"UPDATE builds SET status='planning',planning_context=jsonb_build_object('pool_ids',jsonb_build_array('00000000-0000-0000-0000-000000000001'),'signing_profiles','[]'::jsonb) WHERE id=$1",params![build]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO planning_runs(id,build_id,purpose,model_config) VALUES($1,$2,'initial','{}')",
        params![run, build],
    )
    .await
    .unwrap();
    let schema = format!("runtime_mcp_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut database.pool.clone(),
        &format!("CREATE SCHEMA {schema}"),
        params![],
    )
    .await
    .unwrap();
    let url = with_options(
        &std::env::var("ARTIFACTORY_TEST_DATABASE_URL").unwrap(),
        &[("search_path", &schema)],
    )
    .unwrap();
    let runtime_db = connect(
        &url,
        artifactory_planner_runtime::store::models(),
        PoolSettings::new(4),
    )
    .await
    .unwrap();
    artifactory_database::migrate(&runtime_db, &artifactory_planner_runtime::store::MIGRATIONS)
        .await
        .unwrap();
    let control_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let control_origin = format!("http://{}", control_listener.local_addr().unwrap());
    let app = api::router(database.state.clone());
    let control = tokio::spawn(async move {
        axum::serve(control_listener, app).await.unwrap();
    });
    let mut upstream = Client::new("https://control.invalid".into(), token.clone()).unwrap();
    // Only disposable loopback fixtures use HTTP; service authorization remains active.
    upstream.origin = control_origin;
    let runtime_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", runtime_listener.local_addr().unwrap());
    let key = Arc::new([7; 32]);
    let runtime = server::Runtime {
        store: RuntimeStore::new(runtime_db.clone(), key.clone()),
        key,
        control: upstream,
        model_http: reqwest::Client::new(),
        public_url: origin.clone(),
    };
    let intent = RunIntent {
        tenant_id: Uuid::from_u128(1),
        run_id: run,
        build_id: build,
        project_id: project,
        version: 1,
        source_manifest_id: Some(Uuid::new_v4()),
        source_digest: Some("a".repeat(64)),
        pool: "default".into(),
        context: json!({}),
        model: ModelConfiguration {
            id: Uuid::new_v4(),
            api: "openai-completions".into(),
            base_url: "https://model.invalid/v1".into(),
            model: "fixture".into(),
            api_key: "fixture-private-key".into(),
            parameters: json!({}),
            max_calls: 2,
            timeout_seconds: 60,
        },
    };
    server::put_intent(&runtime, serde_json::to_value(intent).unwrap())
        .await
        .unwrap();
    let execution = Execution {
        run_id: run,
        generation: 1,
        uid: "fixture-pod".into(),
        name: "fixture-pod".into(),
        namespace: "isolated".into(),
    };
    server::register_execution(&runtime, execution.clone())
        .await
        .unwrap();
    sql::execute(
        &mut runtime_db.clone(),
        "UPDATE runtime_runs SET status='provisioning' WHERE id=$1",
        params![run],
    )
    .await
    .unwrap();
    let acquired = runtime
        .store
        .acquire_execution(execution, &origin)
        .await
        .unwrap();
    let mut stale = acquired["lease"].clone();
    stale["generation"] = json!(0);
    let app = server::router(runtime, token);
    let runner = tokio::spawn(async move {
        axum::serve(runtime_listener, app).await.unwrap();
    });
    let config = json!({"transport":"runtime","endpoint":origin,"lease":acquired["lease"],"stale_lease":stale,"queries":[
        {"name":"summarize_nodes","arguments":{}},
        {"name":"query_nodes","arguments":{"os":"linux"}},
        {"name":"get_node","arguments":{"node_id":node}}
    ]});
    let report = mcp_probe(&config).await;
    assert_eq!(report["staleLeaseRejected"], true);
    assert_eq!(report["results"][1]["nodes"][0]["node_id"], json!(node));
    assert_eq!(report["results"][2]["nodes"][0]["metadata"], json!({}));
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE projects SET pool_ids='{}' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    let report = mcp_probe(&config).await;
    for result in report["results"].as_array().unwrap() {
        assert_eq!(
            result["nodes"],
            json!([]),
            "current project authorization is enforced through Runtime"
        );
    }
    runner.abort();
    let _ = runner.await;
    control.abort();
    let _ = control.await;
    drop(runtime_db);
    sql::execute(
        &mut database.pool.clone(),
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await
    .unwrap();
    database.close().await;
}
