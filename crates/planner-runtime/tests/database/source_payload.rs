//! [INPUT]: 依赖真实 PostgreSQL、HTTP Router 和固定源码清单。
//! [OUTPUT]: 验证大清单链路和材料出口的 generation 拒绝。
//! [POS]: 固定源码准备的传输与并发授权回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_source_checkout::{LfsObject, SourceManifest, SourceRepository};
use axum::{Json, Router, extract::State, routing::post};

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn large_source_manifest_survives_intent_acquire_and_material_http() {
    let s = state().await;
    let run = Uuid::new_v4();
    let source = SourceManifest::new(
        Uuid::new_v4(),
        vec![SourceRepository {
            path: String::new(),
            upstream_url: "https://example.invalid/repo.git".into(),
            commit_sha: "a".repeat(40),
            tags: Default::default(),
            lfs_objects: (0..20_000)
                .map(|n| LfsObject {
                    path: format!("objects/asset-{n:05}-{}.bin", "x".repeat(24)),
                    oid: "b".repeat(64),
                    size: 1024,
                })
                .collect(),
        }],
    )
    .unwrap();
    let manifest = serde_json::to_value(&source).unwrap();
    let size = serde_json::to_vec(&manifest).unwrap().len();
    assert!(size > 2 * 1024 * 1024 && size <= 4 * 1024 * 1024);
    let mut input = intent(run);
    input.source_manifest_id = Some(source.id);
    input.source_digest = Some(source.digest);
    input.context = json!({"source_manifest":manifest});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let runtime = tokio::spawn(
        axum::serve(listener, server::router(s.clone(), "x".repeat(64))).into_future(),
    );
    let response = reqwest::Client::new()
        .post(format!("{url}/v1/intent"))
        .bearer_auth("x".repeat(64))
        .json(&input)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    s.store.queue("default", 1, 1).await.unwrap();
    let execution = Execution {
        run_id: run,
        generation: 1,
        uid: "large-source-pod".into(),
        name: "large-source".into(),
        namespace: "test".into(),
    };
    s.store.register_execution(execution.clone()).await.unwrap();
    s.store.prepare_execution(&execution).await.unwrap();
    let material = json!({"manifest":manifest,"credentials":[]});
    let app = Router::new()
        .route(
            "/internal/runtime/source-material",
            post(|State(v): State<Value>| async move { Json(v) }),
        )
        .with_state(material.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let control = format!("http://{}", listener.local_addr().unwrap());
    let control_server = tokio::spawn(axum::serve(listener, app).into_future());
    let mut local = s.clone();
    local.control.origin = control;
    let prepared = server::prepare_material(&local, &execution, "artifactory-pi").await;
    let prepared = match prepared {
        Ok(material) => material,
        Err(_) => panic!("valid preparation was rejected"),
    };
    assert_eq!(prepared, material);
    let acquired = s.store.acquire_execution(execution, &url).await.unwrap();
    assert_eq!(acquired["task"]["context"]["source_manifest"], manifest);
    runtime.abort();
    control_server.abort();
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn source_material_wait_rechecks_cancellation_and_generation() {
    use artifactory_runtime_protocol::error::ApiError;
    use tokio::sync::Notify;
    let s = state().await;
    for cancel in [true, false] {
        let run = Uuid::new_v4();
        server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
            .await
            .unwrap();
        s.store.queue("default", 1, 1).await.unwrap();
        let execution = Execution {
            run_id: run,
            generation: 1,
            uid: "source-wait-pod".into(),
            name: "source-wait".into(),
            namespace: "test".into(),
        };
        s.store.register_execution(execution.clone()).await.unwrap();
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let app = Router::new().route(
            "/internal/runtime/source-material",
            post({
                let entered = entered.clone();
                let release = release.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        Json(json!({"credentials":["private-source-key"]}))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut local = s.clone();
        local.control.origin = format!("http://{}", listener.local_addr().unwrap());
        let control = tokio::spawn(axum::serve(listener, app).into_future());
        let request = tokio::spawn(async move {
            server::prepare_material(&local, &execution, "artifactory-pi").await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
            .await
            .unwrap();
        if cancel {
            server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
                .await
                .unwrap();
        } else {
            sql::execute(&mut s.pool.clone(),"UPDATE runtime_runs SET lifecycle=jsonb_set(lifecycle,'{generation}','2'),execution=NULL WHERE id=$1",params![run]).await.unwrap();
        }
        release.notify_one();
        assert!(
            matches!(request.await.unwrap(), Err(ApiError::Forbidden)),
            "stale generation received source credentials"
        );
        control.abort();
    }
}
