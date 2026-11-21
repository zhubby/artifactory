//! [INPUT]: 依赖隔离数据库、镜像夹具与工作区租约。
//! [OUTPUT]: 验证材料回收、失败诊断、日志隔离和 Release 分发快照。
//! [POS]: 镜像审查问题的跨边界回归验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_failure_preserves_safe_summary_and_separates_publisher_logs() {
    let db = Database::with_tenant().await;
    let (_, _, job, _) = image(&db).await;
    let id = Uuid::new_v4();
    let store = &db.state.store;
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    for (publisher, platform, allowed) in [
        (true, "publish", true),
        (true, "linux/amd64", false),
        (false, "publish", false),
        (false, "linux/amd64", true),
    ] {
        let report = Report::Log {
            platform: platform.into(),
            sequence: 1,
            text: "safe diagnostics".into(),
        };
        assert_eq!(
            store
                .image_report(
                    id,
                    publisher,
                    &report,
                    Some(("fixture-log", "fixture-hash"))
                )
                .await
                .is_ok(),
            allowed,
            "publisher={publisher}, platform={platform}"
        );
    }
    for message in ["x".repeat(1025), "contains\ncontrol".into(), "".into()] {
        assert!(
            store
                .image_report(id, true, &Report::Failed { message }, None)
                .await
                .is_err()
        );
    }
    let summary = "image publication failed: registry returned HTTP 407; [REDACTED]";
    store
        .image_report(
            id,
            true,
            &Report::Failed {
                message: summary.into(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        store.image_execution(id).await.unwrap().status,
        "publish_failed"
    );
    store.finish_image(id, "failed").await.unwrap();
    let saved: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT error FROM image_executions WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(saved, summary);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn successful_image_cleanup_preserves_success_and_deletes_only_its_secret() {
    use axum::{
        Router, body::Body, extract::State, http::Request, response::IntoResponse, routing::any,
    };
    use std::sync::Mutex;
    type Requests = Arc<Mutex<Vec<(String, String, serde_json::Value)>>>;
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
    store
        .image_report(id, true, &Report::Published { digest }, None)
        .await
        .unwrap();
    confirm_image(store, id).await;
    let requests: Requests = Arc::new(Mutex::new(Vec::new()));
    let secret = format!("image-{}-task", id.simple());
    let secret_path = format!("/api/v1/namespaces/test/secrets/{secret}");
    let app=Router::new().fallback(any({let secret_path=secret_path.clone();let secret=secret.clone();move|State(seen):State<Requests>,request:Request<Body>|{let secret_path=secret_path.clone();let secret=secret.clone();async move {
        let method=request.method().as_str().to_string();let path=request.uri().path().to_string();let bytes=axum::body::to_bytes(request.into_body(),8192).await.unwrap();
        let body=if bytes.is_empty(){json!(null)}else{serde_json::from_slice(&bytes).unwrap()};seen.lock().unwrap().push((method.clone(),path.clone(),body));
        if method=="GET"&&path==secret_path {return axum::Json(json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":secret,"namespace":"test","uid":"secret-cleanup-uid","labels":{"artifactory.dev/execution":id.to_string()}}})).into_response();}
        if method=="DELETE"&&path==secret_path {return axum::Json(json!({"apiVersion":"v1","kind":"Status","status":"Success"})).into_response();}
        if path.contains("/imagebuilds/") && matches!(method.as_str(),"GET"|"PATCH") {return axum::Json(json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":format!("image-{}",id.simple()),"namespace":"test","uid":"build-cleanup-uid","labels":{"artifactory.dev/execution":id.to_string()}},"status":{"phase":"Succeeded"}})).into_response();}
        (StatusCode::NOT_FOUND,axum::Json(json!({"apiVersion":"v1","kind":"Status","code":404,"reason":"NotFound","message":"unexpected Kubernetes request"}))).into_response()
    }}})).with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = kube::Client::try_from(kube::Config::new(
        format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap(),
    ))
    .unwrap();
    let server = tokio::spawn(axum::serve(listener, app).into_future());
    let config = artifactory_control_plane::images::Config {
        namespace: "test".into(),
        registry_url: "https://registry.invalid".into(),
        control_url: "https://control.invalid".into(),
        service: "registry".into(),
        issuer: "fixture".into(),
        signing_key: Default::default(),
        signing_cert: Default::default(),
    };
    artifactory_control_plane::images::tick(&db.state, &config, &client)
        .await
        .unwrap();
    let execution = store.image_execution(id).await.unwrap();
    assert!(
        execution.task_envelope.is_none(),
        "completed image retained task materials"
    );
    assert_eq!(execution.status, "succeeded");
    assert!(!execution.cancel_requested);
    {
        let seen = requests.lock().unwrap();
        assert_eq!(
            seen.iter()
                .map(|r| (r.0.as_str(), r.1.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("GET", secret_path.as_str()),
                ("DELETE", secret_path.as_str())
            ],
            "success must not patch cancelRequested"
        );
        assert_eq!(seen[1].2["preconditions"]["uid"], "secret-cleanup-uid");
    }
    server.abort();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn failed_or_canceled_execution_stops_an_active_imagebuild_before_cleanup() {
    use axum::{Router, body::Body, http::Request, response::IntoResponse, routing::any};
    use std::sync::Mutex;
    for status in ["failed", "publish_failed", "canceled"] {
        let db = Database::with_tenant().await;
        let (_, _, job, _) = image(&db).await;
        let id = Uuid::new_v4();
        let store = &db.state.store;
        store
            .claim_image(job, id, "build", "publish")
            .await
            .unwrap();
        store.save_image_task(id, "sealed").await.unwrap();
        if status == "canceled" {
            store.finish_image(id, "canceled").await.unwrap();
        } else {
            store
                .image_report(
                    id,
                    status == "publish_failed",
                    &Report::Failed {
                        message: "fixture failure before Kubernetes Pod termination".into(),
                    },
                    None,
                )
                .await
                .unwrap();
        }
        assert_eq!(store.image_execution(id).await.unwrap().status, status);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().fallback(any({
            let requests = requests.clone();
            move |request: Request<Body>| {
                let requests = requests.clone();
                async move {
                    let method = request.method().as_str().to_string();
                    let path = request.uri().path().to_string();
                    let bytes = axum::body::to_bytes(request.into_body(), 8192).await.unwrap();
                    let body: serde_json::Value = if bytes.is_empty() { json!(null) } else { serde_json::from_slice(&bytes).unwrap() };
                    requests.lock().unwrap().push((method.clone(), path.clone(), body));
                    if path.contains("/imagebuilds/") {
                        return axum::Json(json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":format!("image-{}",id.simple()),"namespace":"test","uid":"active-build-uid","labels":{"artifactory.dev/execution":id.to_string()}},"spec":{"cancelRequested":false},"status":{"phase":"Running"}})).into_response();
                    }
                    if method == "GET" && path.contains("/secrets/") {
                        return axum::Json(json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":format!("image-{}-task",id.simple()),"namespace":"test","uid":"active-secret-uid","labels":{"artifactory.dev/execution":id.to_string()}}})).into_response();
                    }
                    axum::Json(json!({"apiVersion":"v1","kind":"Status","status":"Success"})).into_response()
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = kube::Client::try_from(kube::Config::new(
            format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        ))
        .unwrap();
        let server = tokio::spawn(axum::serve(listener, app).into_future());
        let config = artifactory_control_plane::images::Config {
            namespace: "test".into(),
            registry_url: "https://registry.invalid".into(),
            control_url: "https://control.invalid".into(),
            service: "registry".into(),
            issuer: "fixture".into(),
            signing_key: Default::default(),
            signing_cert: Default::default(),
        };
        artifactory_control_plane::images::tick(&db.state, &config, &client)
            .await
            .unwrap();
        {
            let requests = requests.lock().unwrap();
            assert_eq!(
                requests.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
                vec!["GET", "PATCH", "GET", "DELETE"],
                "{status} must stop the active CR before deleting its materials"
            );
            assert_eq!(requests[1].2["spec"]["cancelRequested"], true);
            assert_eq!(requests[3].2["preconditions"]["uid"], "active-secret-uid");
        }
        let execution = store.image_execution(id).await.unwrap();
        assert_eq!(
            execution.status, status,
            "cleanup changed the durable terminal outcome"
        );
        assert_eq!(execution.cancel_requested, status == "canceled");
        assert!(execution.task_envelope.is_none());
        server.abort();
        db.close().await;
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_cleanup_classifies_success_separately_from_failure_cancellation_and_expiry() {
    let db = Database::with_tenant().await;
    for status in ["succeeded", "failed", "canceled", "publish_failed"] {
        let (_, _, job, _) = image(&db).await;
        let id = Uuid::new_v4();
        let store = &db.state.store;
        store
            .claim_image(job, id, "build", "publish")
            .await
            .unwrap();
        store.save_image_task(id, "sealed").await.unwrap();
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE image_executions SET status=$2 WHERE id=$1",
            params![id, status],
        )
        .await
        .unwrap();
        let stop = store.image_stop_status(id).await.unwrap().unwrap();
        let expected = if status == "succeeded" {
            artifactory_control_plane::store::images::ImageStop::Cleanup
        } else {
            artifactory_control_plane::store::images::ImageStop::Terminate(
                if status == "canceled" {
                    "canceled"
                } else {
                    "failed"
                },
            )
        };
        assert_eq!(stop, expected, "terminal {status} cleanup classification");
    }
    for canceled in [false, true] {
        let (_, build, job, _) = image(&db).await;
        let id = Uuid::new_v4();
        let store = &db.state.store;
        store
            .claim_image(job, id, "build", "publish")
            .await
            .unwrap();
        store.save_image_task(id, "sealed").await.unwrap();
        assert!(store.image_stop_status(id).await.unwrap().is_none());
        if canceled {
            store.cancel_build(build).await.unwrap();
        } else {
            sql::execute(
                &mut db.pool.clone(),
                "UPDATE image_executions SET deadline_at=now()-interval '1 second' WHERE id=$1",
                params![id],
            )
            .await
            .unwrap();
        }
        let stop = store.image_stop_status(id).await.unwrap().unwrap();
        assert_eq!(
            stop,
            artifactory_control_plane::store::images::ImageStop::Terminate(if canceled {
                "canceled"
            } else {
                "failed"
            })
        );
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn deletion_sweeps_orphan_image_logs_after_grace_and_resumes_storage_failure() {
    use axum::{
        Router,
        body::Body,
        http::{Request, Response, StatusCode},
        routing::any,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    let objects = Arc::new(Mutex::new(
        std::collections::BTreeMap::<String, Vec<u8>>::new(),
    ));
    let fail_list = Arc::new(AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app=Router::new().fallback(any({
        let objects=objects.clone(); let fail_list=fail_list.clone();
        move |request:Request<Body>| {
            let objects=objects.clone(); let fail_list=fail_list.clone();
            async move {
                let key=request.uri().path().trim_start_matches("/test/").to_owned();
                let prefix=url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes()).find(|(k,_)|k=="prefix").map(|(_,v)|v.into_owned());
                if let Some(prefix)=prefix {
                    if fail_list.swap(false,Ordering::SeqCst) { return Response::builder().status(StatusCode::SERVICE_UNAVAILABLE).body(Body::empty()).unwrap(); }
                    let contents=objects.lock().unwrap().keys().filter(|k|k.starts_with(&prefix)).map(|key|format!("<Contents><Key>{key}</Key><Size>1</Size><ETag>x</ETag><LastModified>2026-11-17T00:00:00Z</LastModified></Contents>")).collect::<String>();
                    return Response::new(Body::from(format!("<ListBucketResult><IsTruncated>false</IsTruncated>{contents}</ListBucketResult>")));
                }
                match request.method().as_str() {
                    "PUT"=> { let bytes=axum::body::to_bytes(request.into_body(),65536).await.unwrap(); objects.lock().unwrap().insert(key,bytes.to_vec()); Response::new(Body::empty()) }
                    "DELETE"=> {objects.lock().unwrap().remove(&key); Response::new(Body::empty())}
                    _=>match objects.lock().unwrap().get(&key) {Some(bytes)=>Response::new(Body::from(bytes.clone())),None=>Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap()}
                }
            }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut db = Database::with_tenant().await;
    let mut settings = (*db.state.settings).clone();
    settings.s3_endpoint = endpoint;
    settings.s3_bucket = "test".into();
    db.state.storage =
        Arc::new(artifactory_control_plane::storage::Storage::new(&settings).unwrap());
    let (project, _, job, _) = image(&db).await;
    let tenant = Uuid::from_u128(1);
    let id = Uuid::new_v4();
    let store = &db.state.store;
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    let prefix = format!("{tenant}/{project}/image-logs/");
    let indexed = format!("{prefix}{id}/linux/amd64/1-first");
    let orphan = format!("{prefix}{id}/linux/amd64/1-conflict");
    let late = format!("{prefix}{id}/linux/arm64/2-late");
    let survivor = format!("{tenant}/{}/image-logs/other", Uuid::new_v4());
    let other_tenant = format!("{}/{project}/image-logs/other", Uuid::new_v4());
    for key in [&indexed, &orphan, &survivor, &other_tenant] {
        db.state.storage.put(key, b"x".to_vec()).await.unwrap();
    }
    let report = Report::Log {
        platform: "linux/amd64".into(),
        sequence: 1,
        text: "x".into(),
    };
    store
        .image_report(id, false, &report, Some((&indexed, "first")))
        .await
        .unwrap();
    assert!(
        store
            .image_report(id, false, &report, Some((&orphan, "conflict")))
            .await
            .is_err()
    );
    let digest = platform_results(store, id).await;
    store
        .image_report(id, true, &Report::Published { digest }, None)
        .await
        .unwrap();
    confirm_image(store, id).await;
    store.refresh_builds().await.unwrap();
    let owner: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT owner_id FROM tenants WHERE id=$1",
        params![tenant],
    )
    .await
    .unwrap();
    let mut auth =
        artifactory_control_plane::auth::Auth::scoped(owner, "admin", vec![project], vec![]);
    auth.token_projects = None;
    store.authorize_tenant(&mut auth, tenant).await.unwrap();
    let operation = store
        .start_lifecycle(
            &auth,
            Some(project),
            "delete",
            Uuid::new_v4(),
            Some("test"),
            Some("unused"),
        )
        .await
        .unwrap();
    let op = Uuid::parse_str(operation["id"].as_str().unwrap()).unwrap();
    let lease = store
        .lease_lifecycle(Uuid::new_v4())
        .await
        .unwrap()
        .unwrap();
    store.advance_lifecycle_drain(&lease).await.unwrap();
    artifactory_control_plane::lifecycle::tick(&db.state)
        .await
        .unwrap();
    assert_eq!(
        store.lifecycle_operation(owner, op).await.unwrap()["phase"],
        "objects"
    );
    assert!(
        objects.lock().unwrap().contains_key(&indexed),
        "image logs wait for in-flight object writes"
    );
    db.state.storage.put(&late, b"late".to_vec()).await.unwrap();
    sql::execute(&mut db.pool.clone(),"UPDATE lifecycle_operations SET manifest=jsonb_set(manifest,'{uploads_expire_at}',to_jsonb((now()-interval '1 minute')::text)),next_run_at=now() WHERE id=$1",params![op]).await.unwrap();
    fail_list.store(true, Ordering::SeqCst);
    artifactory_control_plane::lifecycle::tick(&db.state)
        .await
        .unwrap();
    assert_eq!(
        store.lifecycle_operation(owner, op).await.unwrap()["state"],
        "failed"
    );
    store.retry_lifecycle(owner, op).await.unwrap();
    for _ in 0..8 {
        artifactory_control_plane::lifecycle::tick(&db.state)
            .await
            .unwrap();
    }
    assert_eq!(
        store.lifecycle_operation(owner, op).await.unwrap()["state"],
        "succeeded"
    );
    let remaining = objects.lock().unwrap().clone();
    for key in [&indexed, &orphan, &late] {
        assert!(
            !remaining.contains_key(key),
            "orphan image log remains: {key}"
        );
    }
    for key in [&survivor, &other_tenant] {
        assert!(remaining.contains_key(key));
    }
    db.close().await;
    server.abort();
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn external_history_keeps_public_registry_origin_after_revocation() {
    let db = Database::with_tenant().await;
    let (project, build, job, _) = image(&db).await;
    let store = publication_store(&db, project);
    let id = Uuid::new_v4();
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    let digest = platform_results(&store, id).await;
    store
        .image_report(id, true, &Report::Published { digest }, None)
        .await
        .unwrap();
    confirm_image(&store, id).await;
    freeze_published_image(&db, build, id).await;
    let credential = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO image_credentials(id,tenant_id,project_id,name,kind,envelope,registry_origin) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,'destination','registry','private-envelope','https://external.example.com:5443')",params![credential,project]).await.unwrap();
    let external = Uuid::new_v4();
    store
        .request_image_publication(
            id,
            external,
            Some(credential),
            Some("release/app"),
            &[],
            "external",
        )
        .await
        .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE image_credentials SET enabled=false,envelope='rotated-secret',registry_origin='https://rotated.example.com' WHERE id=$1",
        params![credential],
    )
    .await
    .unwrap();
    let history = store.image_history(build).await.unwrap();
    let external = history
        .iter()
        .find(|row| row["id"] == external.to_string())
        .unwrap();
    assert_eq!(
        external["destination_registry"],
        "https://external.example.com:5443"
    );
    let public = serde_json::to_string(&history).unwrap();
    for secret in [
        "private-envelope",
        "rotated-secret",
        "build_token_hash",
        "publish_token_hash",
        "task_envelope",
    ] {
        assert!(!public.contains(secret));
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dispatch_deadline_keeps_complete_platforms_retryable() {
    let db = Database::with_tenant().await;
    let (project, _, job, _) = image(&db).await;
    let store = publication_store(&db, project);
    let id = Uuid::new_v4();
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    platform_results(&store, id).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE image_executions SET deadline_at=now()-interval '1 second' WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    let status = store.image_stop_status(id).await.unwrap().unwrap();
    assert_eq!(
        status,
        artifactory_control_plane::store::images::ImageStop::Terminate("failed")
    );
    store.finish_image(id, "failed").await.unwrap();
    let execution = store.image_execution(id).await.unwrap();
    assert_eq!(execution.status, "publish_failed");
    assert!(!execution.cancel_requested);
    assert!(
        store
            .request_image_publication(id, Uuid::new_v4(), None, None, &[], "retry")
            .await
            .is_err()
    );
    assert_eq!(execution.results.len(), 2);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn draining_preserves_accepted_images_and_rejects_new_work() {
    for tenant_drain in [false, true] {
        let db = Database::with_tenant().await;
        let (project, build, job, _manifest) = image(&db).await;
        let store = &db.state.store;
        let queued = Uuid::new_v4();
        sql::execute(&mut db.pool.clone(), "INSERT INTO jobs(id,build_id,target,target_config) SELECT $1,build_id,'queued-image',target_config FROM jobs WHERE id=$2", params![queued,job]).await.unwrap();
        let id = Uuid::new_v4();
        store
            .claim_image(job, id, "build", "publish")
            .await
            .unwrap();
        store.save_image_task(id, "sealed").await.unwrap();
        let tenant = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let owner: Uuid = sql::scalar(
            &mut db.pool.clone(),
            "SELECT owner_id FROM tenants WHERE id=$1",
            params![tenant],
        )
        .await
        .unwrap();
        let mut auth = artifactory_control_plane::auth::Auth::scoped(
            owner,
            "admin",
            vec![project],
            vec!["admin".into(), "release".into()],
        );
        auth.token_projects = None;
        store.authorize_tenant(&mut auth, tenant).await.unwrap();
        let operation = store
            .start_lifecycle(
                &auth,
                if tenant_drain { None } else { Some(project) },
                "archive",
                Uuid::new_v4(),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(
            store.image_live(id).await.unwrap(),
            "draining must keep accepted workers authorized"
        );
        assert!(store.image_live(id).await.unwrap());
        store
            .image_report(
                id,
                false,
                &Report::Heartbeat {
                    platform: "linux/amd64".into(),
                    pod_uid: Uuid::new_v4().to_string(),
                },
                None,
            )
            .await
            .unwrap();
        assert!(store.image_candidates().await.unwrap().contains(&queued));
        let queued_id = Uuid::new_v4();
        assert!(
            store
                .claim_image(queued, queued_id, "queued", "queued-publish")
                .await
                .unwrap()
        );
        store
            .save_image_task(queued_id, "queued-sealed")
            .await
            .unwrap();
        for execution in [id, queued_id] {
            let digest = platform_results(store, execution).await;
            store
                .image_report(execution, true, &Report::Published { digest }, None)
                .await
                .unwrap();
            confirm_image(store, execution).await;
        }
        assert!(
            store
                .for_auth(&auth)
                .request_image_publication(id, Uuid::new_v4(), None, None, &[], "retry")
                .await
                .is_err()
        );
        let input = serde_json::from_value(crate::fixture::build_input()).unwrap();
        let release: Uuid = sql::scalar(
            &mut db.pool.clone(),
            "SELECT release_id FROM builds WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        assert!(
            store
                .for_auth(&auth)
                .create_release_build(release, owner, &input)
                .await
                .is_err()
        );
        store.refresh_builds().await.unwrap();
        assert_eq!(
            sql::scalar::<String>(
                &mut db.pool.clone(),
                "SELECT status FROM builds WHERE id=$1",
                params![build]
            )
            .await
            .unwrap(),
            "succeeded"
        );
        let lease = store
            .lease_lifecycle(Uuid::new_v4())
            .await
            .unwrap()
            .unwrap();
        store.advance_lifecycle_drain(&lease).await.unwrap();
        assert_eq!(
            store
                .lifecycle_operation(
                    owner,
                    Uuid::parse_str(operation["id"].as_str().unwrap()).unwrap()
                )
                .await
                .unwrap()["state"],
            "succeeded"
        );
        db.close().await;
    }
}
