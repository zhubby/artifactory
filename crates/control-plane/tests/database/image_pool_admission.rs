//! [INPUT]: 依赖隔离 PostgreSQL、真实提案入口和有暂停开关的 HTTP 查池接口。
//! [OUTPUT]: 验证查池不占行锁，临时故障不落回执，重试能返回缺池错误。
//! [POS]: 镜像准入跨数据库与 Kubernetes 的并发回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn image_pool_lookup_does_not_hold_build_or_planning_row_locks() {
    const CHILD: &str = "ARTIFACTORY_IMAGE_POOL_PREFLIGHT_CHILD";
    if let Ok(origin) = std::env::var(CHILD) {
        let db = Database::with_tenant().await;
        std::fs::write(
            std::env::var("ARTIFACTORY_POOL_TEST_SCHEMA_FILE").unwrap(),
            &db.schema,
        )
        .unwrap();
        let (_, build, _) = db
            .fixture_with_requirements(
                json!([{"key":"app","kind":"image","platforms":["linux/amd64"]}]),
            )
            .await;
        sql::execute(
            &mut db.pool.clone(),
            "DELETE FROM jobs WHERE build_id=$1",
            params![build],
        )
        .await
        .unwrap();
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET status='planning' WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        let run = Uuid::new_v4();
        sql::execute(&mut db.pool.clone(), "INSERT INTO planning_runs(id,build_id,purpose,model_config,deadline_at) VALUES($1,$2,'initial','{}',now()+interval '1 minute')", params![run,build]).await.unwrap();
        let digest: String = sql::scalar(&mut db.pool.clone(), "SELECT s.digest FROM source_manifests s JOIN builds b ON b.source_manifest_id=s.id WHERE b.id=$1", params![build]).await.unwrap();
        let state = db.state.clone();
        let proposal = json!({"event_id":Uuid::new_v4(),"run_id":run,"kind":"proposal","payload":{"generation":1,"kind":"submit_plan","source_digest":digest,"deadline_ms":chrono::Utc::now().timestamp_millis()+60000,"payload":{"plan":{"summary":"build image","evidence":[{"path":"Dockerfile","reason":"image entry"}],"recipe":{"version":1,"targets":{"app":{"executor":"buildkit","os":"linux","arch":"amd64","application_platform":null,"image":{"name":"app","pool":"ci-images","platforms":["linux/amd64"]}}}}}}}});
        let pending_proposal = proposal.clone();
        let pending = tokio::spawn(async move {
            artifactory_control_plane::planning_bridge::accept_event(&state, pending_proposal).await
        });
        let http = reqwest::Client::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let state: Value = http
                    .get(format!("{origin}/lookup-state"))
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                if state["started"] == true {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("proposal must reach the blocked BuilderPool GET");
        let mut pool = db.pool.clone();
        let mut tx = pool.transaction().await.unwrap();
        for (table, id) in [("builds", build), ("planning_runs", run)] {
            sql::scalar::<Uuid>(
                &mut tx,
                &format!("SELECT id FROM {table} WHERE id=$1 FOR UPDATE NOWAIT"),
                params![id],
            )
            .await
            .expect("BuilderPool HTTP wait must leave build and run unlocked");
        }
        tx.rollback().await.unwrap();
        http.post(format!("{origin}/release-lookup"))
            .send()
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(result["error"].as_str().unwrap().contains("does not exist"));
        assert_eq!(
            sql::scalar::<i64>(
                &mut db.pool.clone(),
                "SELECT count(*) FROM jobs WHERE build_id=$1",
                params![build]
            )
            .await
            .unwrap(),
            0
        );
        for (index, code) in [403, 503].into_iter().enumerate() {
            http.post(format!("{origin}/lookup-code/{code}"))
                .send()
                .await
                .unwrap();
            let event = Uuid::new_v4();
            let mut retryable = proposal.clone();
            retryable["event_id"] = json!(event);
            let failure = artifactory_control_plane::planning_bridge::accept_event(
                &db.state,
                retryable.clone(),
            )
            .await;
            assert!(
                failure.is_err(),
                "HTTP {code} must leave the event retryable"
            );
            let error = failure.unwrap_err().to_string();
            assert!(
                error.contains(&code.to_string()) || code == 503 && error.contains("timed out"),
                "HTTP {code} must stay a lookup error or bounded retry timeout: {error}"
            );
            assert!(!error.contains("private API detail"));
            assert_eq!(
                sql::scalar::<i64>(
                    &mut db.pool.clone(),
                    "SELECT count(*) FROM planning_receipts WHERE event_id=$1",
                    params![event]
                )
                .await
                .unwrap(),
                0,
                "infrastructure errors must not persist a receipt"
            );
            assert_eq!(
                sql::scalar::<i64>(
                    &mut db.pool.clone(),
                    "SELECT count(*) FROM planning_events WHERE run_id=$1",
                    params![run]
                )
                .await
                .unwrap(),
                1 + index as i64,
                "infrastructure errors must not become permanent planning results"
            );
            http.post(format!("{origin}/lookup-code/404"))
                .send()
                .await
                .unwrap();
            let recovered =
                artifactory_control_plane::planning_bridge::accept_event(&db.state, retryable)
                    .await
                    .unwrap();
            assert!(
                recovered["error"]
                    .as_str()
                    .unwrap()
                    .contains("does not exist")
            );
        }
        db.close().await;
        return;
    }
    #[derive(Clone, Default)]
    struct Lookup {
        started: Arc<AtomicBool>,
        release: Arc<tokio::sync::Notify>,
        code: Arc<std::sync::atomic::AtomicU16>,
    }
    let lookup = Lookup::default();
    let app = axum::Router::new().fallback(|axum::extract::State(lookup): axum::extract::State<Lookup>, uri: axum::http::Uri| async move {
        match uri.path() {
            "/v1/get" => (StatusCode::OK, axum::Json(json!({"lifecycle":{"generation":1}}))),
            "/lookup-state" => (StatusCode::OK, axum::Json(json!({"started":lookup.started.load(Ordering::SeqCst)}))),
            "/release-lookup" => {lookup.release.notify_one(); (StatusCode::OK, axum::Json(json!({})))},
            path if path.starts_with("/lookup-code/") => {
                lookup.code.store(path.rsplit('/').next().unwrap().parse().unwrap(), Ordering::SeqCst);
                (StatusCode::OK, axum::Json(json!({})))
            },
            path if path.ends_with("/builderpools/ci-images") => {
                lookup.started.store(true, Ordering::SeqCst);
                if lookup.code.load(Ordering::SeqCst) == 0 { lookup.release.notified().await; }
                let code = match lookup.code.load(Ordering::SeqCst) { 0 => 404, code => code };
                (StatusCode::from_u16(code).unwrap(), axum::Json(json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":if code == 404 { "NotFound" } else { "Injected" },"code":code,"message":"private API detail"})))
            },
            _ => (StatusCode::NOT_FOUND, axum::Json(json!({}))),
        }
    }).with_state(lookup);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let temp = tempfile::tempdir().unwrap();
    let schema_file = temp.path().join("schema.txt");
    let config = temp.path().join("kubeconfig.json");
    std::fs::write(&config, json!({"apiVersion":"v1","kind":"Config","clusters":[{"name":"test","cluster":{"server":origin}}],"users":[{"name":"test","user":{}}],"contexts":[{"name":"test","context":{"cluster":"test","user":"test","namespace":"images"}}],"current-context":"test"}).to_string()).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(90),
        tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "image_pool_admission::image_pool_lookup_does_not_hold_build_or_planning_row_locks",
                "--ignored",
                "--nocapture",
            ])
            .env(CHILD, &origin)
            .env("ARTIFACTORY_POOL_TEST_SCHEMA_FILE", &schema_file)
            .env("KUBECONFIG", &config)
            .env("ARTIFACTORY_PLANNING_URL", &origin)
            .env(
                "ARTIFACTORY_SERVICE_TOKEN",
                "pool-test-service-identity-0000000000000000",
            )
            .env("ARTIFACTORY_INSECURE_DEV", "true")
            .env("ARTIFACTORY_IMAGE_NAMESPACE", "images")
            .env_remove("ARTIFACTORY_SERVICE_CA")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    server.abort();
    if let Ok(schema) = std::fs::read_to_string(&schema_file) {
        assert!(
            schema.starts_with("test_")
                && schema
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        );
        let admin = connect(
            &std::env::var("ARTIFACTORY_TEST_DATABASE_URL").unwrap(),
            Default::default(),
            PoolSettings::new(2),
        )
        .await
        .unwrap();
        sql::execute(
            &mut admin.clone(),
            &format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
            params![],
        )
        .await
        .unwrap();
    }
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
