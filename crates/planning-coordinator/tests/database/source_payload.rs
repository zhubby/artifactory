//! [INPUT]: 依赖真实 PostgreSQL、HTTP Router 和固定源码清单。
//! [OUTPUT]: 验证大清单、准备故障重试及材料出口的身份复核。
//! [POS]: 固定源码准备的传输与并发授权回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_source_checkout::{LfsObject, SourceManifest, SourceRepository};
use axum::{Json, Router, extract::State, routing::post};

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn source_preparation_storage_faults_return_retryable_503_before_and_after_material() {
    use artifactory_service_transport::error::ApiError;
    use axum::response::IntoResponse;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;
    for after_material in [false, true] {
        let s = state().await;
        let run = Uuid::new_v4();
        server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
            .await
            .unwrap();
        s.store.queue("default", 1, 1).await.unwrap();
        let execution = Execution {
            run_id: run,
            generation: 1,
            uid: "fault-pod".into(),
            name: "fault".into(),
            namespace: "test".into(),
        };
        s.store.register_execution(execution.clone()).await.unwrap();
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/internal/planning/source-material",
            post({
                let entered = entered.clone();
                let release = release.clone();
                let calls = calls.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    let calls = calls.clone();
                    async move {
                        let n = calls.fetch_add(1, Ordering::SeqCst);
                        if after_material && n == 0 {
                            entered.notify_one();
                            release.notified().await;
                        }
                        Json(json!({"manifest":"material-fixture","credentials":[]}))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut local = s.clone();
        local.control.origin = format!("http://{}", listener.local_addr().unwrap());
        let control = tokio::spawn(axum::serve(listener, app).into_future());
        // 与正式 prepare 路由共用材料授权函数，省去外部 TokenReview。
        let app =
            Router::new()
                .route(
                    "/prepare",
                    post(
                        |State(coordinator): State<Coordinator>,
                         Json(execution): Json<Execution>| async move {
                            server::prepare_material(
                                &coordinator,
                                &execution,
                                "artifactory-planning-worker",
                            )
                            .await
                            .map(Json)
                        },
                    ),
                )
                .with_state(local.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/prepare", listener.local_addr().unwrap());
        let service = tokio::spawn(axum::serve(listener, app).into_future());
        if !after_material {
            sql::execute(
                &mut s.pool.clone(),
                "ALTER TABLE planning_runs RENAME TO unavailable_runs",
                params![],
            )
            .await
            .unwrap();
        }
        let request = {
            let url = url.clone();
            let execution = execution.clone();
            tokio::spawn(async move {
                reqwest::Client::new()
                    .post(url)
                    .json(&execution)
                    .send()
                    .await
                    .unwrap()
            })
        };
        if after_material {
            tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
                .await
                .unwrap();
            sql::execute(
                &mut s.pool.clone(),
                "ALTER TABLE planning_runs RENAME TO unavailable_runs",
                params![],
            )
            .await
            .unwrap();
            release.notify_one();
        }
        let response = request.await.unwrap();
        sql::execute(
            &mut s.pool.clone(),
            "ALTER TABLE unavailable_runs RENAME TO planning_runs",
            params![],
        )
        .await
        .unwrap();
        assert_eq!(
            response.status(),
            503,
            "storage fault must remain retryable, after_material={after_material}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(after_material));
        let response = reqwest::Client::new()
            .post(&url)
            .json(&execution)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["manifest"], "material-fixture");
        for stale in [
            Execution {
                run_id: Uuid::new_v4(),
                ..execution.clone()
            },
            Execution {
                generation: 2,
                ..execution.clone()
            },
            Execution {
                uid: "wrong-pod".into(),
                ..execution.clone()
            },
        ] {
            let response = reqwest::Client::new()
                .post(&url)
                .json(&stale)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 403);
        }
        server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
            .await
            .unwrap();
        assert!(matches!(
            server::prepare_material(&local, &execution, "artifactory-planning-worker").await,
            Err(ApiError::Forbidden)
        ));
        assert_eq!(ApiError::Unavailable.into_response().status(), 503);
        service.abort();
        control.abort();
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn source_preparation_registration_pending_returns_409_without_fetching_material() {
    use artifactory_service_transport::error::ApiError;
    let s = state().await;
    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    s.store.queue("default", 1, 1).await.unwrap();
    let execution = Execution {
        run_id: run,
        generation: 1,
        uid: "pending-pod".into(),
        name: "pending".into(),
        namespace: "test".into(),
    };
    assert!(matches!(
        server::prepare_material(&s, &execution, "artifactory-planning-worker").await,
        Err(ApiError::Conflict(_))
    ));
}

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
    let coordinator = tokio::spawn(
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
            "/internal/planning/source-material",
            post(|State(v): State<Value>| async move { Json(v) }),
        )
        .with_state(material.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let control = format!("http://{}", listener.local_addr().unwrap());
    let control_server = tokio::spawn(axum::serve(listener, app).into_future());
    let mut local = s.clone();
    local.control.origin = control;
    let prepared =
        server::prepare_material(&local, &execution, "artifactory-planning-worker").await;
    let prepared = match prepared {
        Ok(material) => material,
        Err(_) => panic!("valid preparation was rejected"),
    };
    assert_eq!(prepared, material);
    let acquired = s.store.acquire_execution(execution, &url).await.unwrap();
    assert_eq!(acquired["task"]["context"]["source_manifest"], manifest);
    coordinator.abort();
    control_server.abort();
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn source_material_wait_rechecks_cancellation_and_generation() {
    use artifactory_service_transport::error::ApiError;
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
            "/internal/planning/source-material",
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
            server::prepare_material(&local, &execution, "artifactory-planning-worker").await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), entered.notified())
            .await
            .unwrap();
        if cancel {
            server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
                .await
                .unwrap();
        } else {
            sql::execute(&mut s.pool.clone(),"UPDATE planning_runs SET lifecycle=jsonb_set(lifecycle,'{generation}','2'),execution=NULL WHERE id=$1",params![run]).await.unwrap();
        }
        release.notify_one();
        assert!(
            matches!(request.await.unwrap(), Err(ApiError::Forbidden)),
            "stale generation received source credentials"
        );
        control.abort();
    }
}
