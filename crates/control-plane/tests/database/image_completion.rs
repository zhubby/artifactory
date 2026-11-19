//! [INPUT]: 依赖隔离数据库、候选回执和模拟 Kubernetes。
//! [OUTPUT]: 验证候选提交与失败诊断的执行身份围栏。
//! [POS]: Control 唯一成功写入者的跨边界验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use axum::{Router, extract::State, response::IntoResponse, routing::any};
use std::{sync::Mutex, time::Duration};

struct Kubernetes {
    build: Arc<Mutex<serde_json::Value>>,
    on_secret_write: Arc<Mutex<Option<serde_json::Value>>>,
    client: kube::Client,
    server: tokio::task::JoinHandle<()>,
}

impl Kubernetes {
    async fn new(build: serde_json::Value) -> Self {
        type Objects = (
            Arc<Mutex<serde_json::Value>>,
            Arc<Mutex<Option<serde_json::Value>>>,
            Arc<Mutex<Option<serde_json::Value>>>,
        );
        let build = Arc::new(Mutex::new(build));
        let on_secret_write = Arc::new(Mutex::new(None));
        let objects: Objects = (
            build.clone(),
            Arc::new(Mutex::new(None)),
            on_secret_write.clone(),
        );
        let app = Router::new().fallback(any(|State(objects): State<Objects>, request: Request<Body>| async move {
            use base64::Engine;
            let method = request.method().clone();
            let path = request.uri().path().to_string();
            let bytes = axum::body::to_bytes(request.into_body(), 65536).await.unwrap();
            if path.contains("/imagebuilds/") {
                return axum::Json(objects.0.lock().unwrap().clone()).into_response();
            }
            let mut secret_storage = objects.1.lock().unwrap();
            if path.ends_with("/secrets") && method == "POST" {
                if let Some(changed) = objects.2.lock().unwrap().take() {
                    *objects.0.lock().unwrap() = changed;
                }
                if secret_storage.is_some() {
                    return (StatusCode::CONFLICT, axum::Json(json!({"apiVersion":"v1","kind":"Status","code":409,"reason":"AlreadyExists","message":"secret exists"}))).into_response();
                }
                let mut secret: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                secret["metadata"]["uid"] = json!("task-uid");
                let data = secret["stringData"].as_object().unwrap().iter().map(|(key,value)| (key.clone(), json!(base64::engine::general_purpose::STANDARD.encode(value.as_str().unwrap())))).collect::<serde_json::Map<_,_>>();
                secret.as_object_mut().unwrap().remove("stringData");
                secret["data"] = json!(data);
                *secret_storage = Some(secret.clone());
                return axum::Json(secret).into_response();
            }
            if path.contains("/secrets/") && method == "GET" && let Some(secret) = &*secret_storage {
                return axum::Json(secret.clone()).into_response();
            }
            if path.contains("/secrets/") && method == "DELETE" {
                *secret_storage = None;
                return axum::Json(json!({"apiVersion":"v1","kind":"Status","status":"Success"})).into_response();
            }
            (StatusCode::NOT_FOUND, axum::Json(json!({"apiVersion":"v1","kind":"Status","code":404,"reason":"NotFound","message":"object absent"}))).into_response()
        })).with_state(objects.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = kube::Client::try_from(kube::Config::new(
            format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        ))
        .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            build,
            on_secret_write,
            client,
            server,
        }
    }

    async fn tick(&self, db: &Database) {
        let config = artifactory_control_plane::images::Config {
            namespace: "test".into(),
            registry_url: "https://registry.invalid".into(),
            control_url: "https://control.invalid".into(),
            service: "registry".into(),
            issuer: "fixture".into(),
            signing_key: Default::default(),
            signing_cert: Default::default(),
        };
        artifactory_control_plane::images::tick(&db.state, &config, &self.client)
            .await
            .unwrap();
    }
}

impl Drop for Kubernetes {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn dispatched(db: &Database) -> (Uuid, Uuid, Uuid, Kubernetes) {
    let (_, build, job, _) = image(db).await;
    let id = Uuid::new_v4();
    let store = &db.state.store;
    store
        .claim_image(job, id, &crypto::hash("build"), &crypto::hash("publish"))
        .await
        .unwrap();
    let materials = serde_json::to_vec(&json!({"build.json":"{}","publish.json":"{}"})).unwrap();
    let envelope =
        crypto::seal_context(&db.state.settings.master_key, &materials, id.as_bytes()).unwrap();
    store.save_image_task(id, &envelope).await.unwrap();
    let execution = store.image_execution(id).await.unwrap();
    let digest: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT digest FROM source_manifests WHERE id=$1",
        params![execution.source_manifest_id],
    )
    .await
    .unwrap();
    let object = json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":format!("image-{}",id.simple()),"namespace":"test","uid":"build-uid","generation":1,"labels":{"artifactory.dev/execution":id.to_string()}},"spec":{"executionId":id,"tenantId":execution.tenant_id,"projectId":execution.project_id,"pool":execution.spec.pool,"sourceManifestId":execution.source_manifest_id,"sourceDigest":digest,"taskSecret":format!("image-{}-task",id.simple()),"platforms":execution.spec.platforms,"allowEmulation":execution.spec.allow_emulation,"publishOnly":false,"timeoutSeconds":execution.task_timeout_seconds,"cancelRequested":false},"status":{"phase":"Running","observedGeneration":1}});
    let kubernetes = Kubernetes::new(object).await;
    kubernetes.tick(db).await;
    assert_eq!(
        store
            .image_execution(id)
            .await
            .unwrap()
            .image_build_uid
            .as_deref(),
        Some("build-uid")
    );
    (id, build, job, kubernetes)
}

async fn publish_candidate(db: &Database, id: Uuid) -> String {
    let digest = platform_results(&db.state.store, id).await;
    db.state
        .store
        .image_report(
            id,
            true,
            &Report::Published {
                digest: digest.clone(),
            },
            None,
        )
        .await
        .unwrap();
    digest
}

async fn artifact_count(db: &Database, id: Uuid) -> i64 {
    sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM image_artifacts WHERE execution_id=$1",
        params![id],
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn published_ack_keeps_execution_running_without_artifact() {
    let db = Database::with_tenant().await;
    let (_, _, job, _) = image(&db).await;
    let id = Uuid::new_v4();
    let store = &db.state.store;
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    let digest = platform_results(store, id).await;
    let report = Report::Published { digest };
    store.image_report(id, true, &report, None).await.unwrap();
    store.image_report(id, true, &report, None).await.unwrap();
    assert_eq!(store.image_execution(id).await.unwrap().status, "running");
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM image_artifacts WHERE execution_id=$1",
            params![id]
        )
        .await
        .unwrap(),
        0
    );
    assert!(
        store
            .image_report(
                id,
                true,
                &Report::Published {
                    digest: sha256(b"changed")
                },
                None
            )
            .await
            .is_err()
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn control_rechecks_cr_identity_spec_generation_and_success_before_commit() {
    let db = Database::with_tenant().await;
    let (id, _, _, kubernetes) = dispatched(&db).await;
    let digest = publish_candidate(&db, id).await;
    let mut success = kubernetes.build.lock().unwrap().clone();
    success["status"]["phase"] = json!("Succeeded");
    for (path, changed) in [
        ("/metadata/uid", json!("replacement-uid")),
        ("/spec/sourceDigest", json!(sha256(b"other"))),
        ("/spec/tenantId", json!(Uuid::new_v4())),
        ("/spec/platforms", json!(["linux/amd64"])),
        ("/spec/taskSecret", json!("other-task")),
        ("/spec/timeoutSeconds", json!(1)),
        ("/spec/publishOnly", json!(true)),
        ("/status/observedGeneration", json!(0)),
        ("/status/observedGeneration", json!(null)),
        ("/metadata/generation", json!(2)),
        ("/metadata/generation", json!(null)),
        ("/status/phase", json!("Running")),
    ] {
        let mut object = success.clone();
        *object.pointer_mut(path).unwrap() = changed;
        *kubernetes.build.lock().unwrap() = object;
        kubernetes.tick(&db).await;
        assert_eq!(
            db.state.store.image_execution(id).await.unwrap().status,
            "running",
            "accepted changed {path}"
        );
        assert_eq!(
            artifact_count(&db, id).await,
            0,
            "artifact for changed {path}"
        );
    }
    *kubernetes.build.lock().unwrap() = success;
    // 已保存候选跨 Control 调度重放；Publisher 不需要重新推送。
    kubernetes.tick(&db).await;
    let completed = db.state.store.image_execution(id).await.unwrap();
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.root_digest.as_deref(), Some(digest.as_str()));
    assert_eq!(artifact_count(&db, id).await, 1);
    kubernetes.tick(&db).await;
    assert!(
        db.state
            .store
            .image_execution(id)
            .await
            .unwrap()
            .task_envelope
            .is_none()
    );
    assert_eq!(artifact_count(&db, id).await, 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn operator_success_requires_all_business_candidates() {
    let db = Database::with_tenant().await;
    let (id, _, _, kubernetes) = dispatched(&db).await;
    kubernetes.build.lock().unwrap()["status"]["phase"] = json!("Succeeded");
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "running"
    );
    platform_results(&db.state.store, id).await;
    kubernetes.tick(&db).await;
    assert_eq!(artifact_count(&db, id).await, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn operator_failure_wins_over_previously_acknowledged_candidates() {
    let db = Database::with_tenant().await;
    let (id, _, _, kubernetes) = dispatched(&db).await;
    publish_candidate(&db, id).await;
    kubernetes.build.lock().unwrap()["status"]["phase"] = json!("Failed");
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "publish_failed"
    );
    kubernetes.build.lock().unwrap()["status"]["phase"] = json!("Succeeded");
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "publish_failed"
    );
    assert_eq!(artifact_count(&db, id).await, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn failure_diagnostics_require_current_uid_spec_generation_and_survive_cleanup() {
    let db = Database::with_tenant().await;
    let (id, _, _, kubernetes) = dispatched(&db).await;
    let mut failure = kubernetes.build.lock().unwrap().clone();
    failure["status"]["phase"] = json!("Failed");
    failure["status"]["message"] = json!(
        "CleanupPending: waiting for Pod deletion\nImage failure: platform=linux/amd64 pod=Evicted"
    );
    for (path, changed) in [
        ("/metadata/uid", json!("replacement-uid")),
        ("/spec/sourceDigest", json!(sha256(b"changed"))),
        ("/status/observedGeneration", json!(0)),
        ("/status/observedGeneration", json!(null)),
        ("/metadata/generation", json!(2)),
    ] {
        let mut stale = failure.clone();
        *stale.pointer_mut(path).unwrap() = changed;
        *kubernetes.build.lock().unwrap() = stale;
        kubernetes.tick(&db).await;
        assert_eq!(
            db.state.store.image_execution(id).await.unwrap().status,
            "running",
            "accepted stale {path}"
        );
    }
    *kubernetes.build.lock().unwrap() = failure;
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "failed"
    );
    let saved: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT error FROM image_executions WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(saved, "Image failure: platform=linux/amd64 pod=Evicted");
    kubernetes.tick(&db).await;
    assert!(
        db.state
            .store
            .image_execution(id)
            .await
            .unwrap()
            .task_envelope
            .is_none()
    );
    let after: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT error FROM image_executions WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(after, saved);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn control_reads_current_cr_again_after_task_material_delivery() {
    let db = Database::with_tenant().await;
    let (id, _, _, kubernetes) = dispatched(&db).await;
    publish_candidate(&db, id).await;
    let mut success = kubernetes.build.lock().unwrap().clone();
    success["status"]["phase"] = json!("Succeeded");
    let mut failed = success.clone();
    failed["status"]["phase"] = json!("Failed");
    *kubernetes.build.lock().unwrap() = success;
    *kubernetes.on_secret_write.lock().unwrap() = Some(failed);
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "running"
    );
    assert_eq!(artifact_count(&db, id).await, 0);
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "publish_failed"
    );
    db.close().await;
}

async fn waiting(db: &Database, holder: i32) {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let blocked:bool = sql::scalar(&mut db.pool.clone(),"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND wait_event_type='Lock')",params![holder]).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("completion must wait for the held transaction lock");
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn completion_rechecks_cancel_failure_and_deadline_after_database_lock_wait() {
    let db = Database::with_tenant().await;
    for change in ["cancel", "failure", "deadline"] {
        let (id, build, _, _kubernetes) = dispatched(&db).await;
        publish_candidate(&db, id).await;
        let mut connection = db.pool.clone();
        let mut holder = connection.transaction().await.unwrap();
        let pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![])
            .await
            .unwrap();
        sql::execute(
            &mut holder,
            "SELECT id FROM builds WHERE id=$1 FOR UPDATE",
            params![build],
        )
        .await
        .unwrap();
        let completion = tokio::spawn({
            let store = db.state.store.clone();
            async move { store.complete_image(id, "build-uid").await }
        });
        waiting(&db, pid).await;
        match change {
            "cancel" => {
                sql::execute(
                    &mut holder,
                    "UPDATE builds SET cancel_requested=true WHERE id=$1",
                    params![build],
                )
                .await
                .unwrap();
            }
            "failure" => {
                sql::execute(
                    &mut holder,
                    "UPDATE image_executions SET status='failed' WHERE id=$1",
                    params![id],
                )
                .await
                .unwrap();
            }
            _ => {
                sql::execute(&mut holder,"UPDATE image_executions SET deadline_at=clock_timestamp()-interval '1 second' WHERE id=$1",params![id]).await.unwrap();
            }
        }
        holder.commit().await.unwrap();
        assert!(
            completion.await.unwrap().is_err(),
            "{change} overwritten by success"
        );
        assert_ne!(
            db.state.store.image_execution(id).await.unwrap().status,
            "succeeded"
        );
        assert_eq!(artifact_count(&db, id).await, 0);
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn completion_rolls_back_artifact_if_deadline_expires_during_insert() {
    let db = Database::with_tenant().await;
    let (id, _, job, _kubernetes) = dispatched(&db).await;
    publish_candidate(&db, id).await;
    sql::execute(&mut db.pool.clone(),"UPDATE image_executions SET deadline_at=clock_timestamp()+interval '500 milliseconds' WHERE id=$1",params![id]).await.unwrap();
    let mut connection = db.pool.clone();
    let mut holder = connection.transaction().await.unwrap();
    let pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![])
        .await
        .unwrap();
    sql::execute(
        &mut holder,
        "LOCK TABLE image_artifacts IN ACCESS EXCLUSIVE MODE",
        params![],
    )
    .await
    .unwrap();
    let completion = tokio::spawn({
        let store = db.state.store.clone();
        async move { store.complete_image(id, "build-uid").await }
    });
    waiting(&db, pid).await;
    tokio::time::sleep(Duration::from_millis(550)).await;
    holder.commit().await.unwrap();
    assert!(completion.await.unwrap().is_err());
    assert_eq!(artifact_count(&db, id).await, 0);
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM jobs WHERE id=$1",
            params![job]
        )
        .await
        .unwrap(),
        "running"
    );
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "running"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn lost_published_ack_is_idempotent_after_success_and_expiry() {
    let db = Database::with_tenant().await;
    let (id, _, _, kubernetes) = dispatched(&db).await;
    let digest = publish_candidate(&db, id).await;
    kubernetes.build.lock().unwrap()["status"]["phase"] = json!("Succeeded");
    kubernetes.tick(&db).await;
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "succeeded"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE image_executions SET deadline_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    let router = api::router(db.state.clone());
    for (token, report, expected) in [
        (
            "publish",
            Report::Published {
                digest: digest.clone(),
            },
            StatusCode::OK,
        ),
        (
            "publish",
            Report::Published {
                digest: sha256(b"changed"),
            },
            StatusCode::CONFLICT,
        ),
        (
            "build",
            Report::Published {
                digest: digest.clone(),
            },
            StatusCode::FORBIDDEN,
        ),
        (
            "invalid",
            Report::Published {
                digest: digest.clone(),
            },
            StatusCode::UNAUTHORIZED,
        ),
        (
            "publish",
            Report::Heartbeat {
                platform: "publish".into(),
                pod_uid: Uuid::new_v4().to_string(),
            },
            StatusCode::CONFLICT,
        ),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/image-tasks/{id}/report"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(serde_json::to_vec(&report).unwrap()))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected, "token={token}");
    }
    assert_eq!(artifact_count(&db, id).await, 1);
    assert_eq!(
        db.state.store.image_execution(id).await.unwrap().status,
        "succeeded"
    );
    db.close().await;
}
