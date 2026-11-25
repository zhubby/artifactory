//! [INPUT]: 依赖实际 init 二进制与临时 HTTP 控制面。
//! [OUTPUT]: 验证完成态短路、回执重试和阶段诊断安全投递。
//! [POS]: prepare/commit 任务生命周期回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_source_checkout::PrepareResponse;
use axum::{Json, Router, routing::post};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn material() -> PrepareResponse {
    PrepareResponse {
        source_id: uuid::Uuid::from_u128(1),
        repository_url: "https://git.example/repo.git".into(),
        reference: "HEAD".into(),
        pinned_ref_oid: None,
        frozen_tags: Default::default(),
        manifest: None,
        credentials: Vec::new(),
        completed_result: None,
    }
}
fn command(root: &Path, url: &str, mode: &str) -> tokio::process::Command {
    std::fs::write(root.join("token"), "pod-identity-token").unwrap();
    let mut command =
        tokio::process::Command::new(env!("CARGO_BIN_EXE_artifactory-source-preparer"));
    command
        .arg(mode)
        .env("ARTIFACTORY_SOURCE_PREPARE_URL", url)
        .env(
            "ARTIFACTORY_SOURCE_IDENTITY",
            r#"{"kind":"source-task","task_id":"00000000-0000-0000-0000-000000000001"}"#,
        )
        .env("ARTIFACTORY_POD_TOKEN", root.join("token"))
        .env("POD_UID", "fixture-pod-uid")
        .env("POD_NAME", "fixture-pod")
        .env("POD_NAMESPACE", "test")
        .env("ARTIFACTORY_SOURCE_ROOT", root.join("source"))
        .env("ARTIFACTORY_SOURCE_AUTH_ROOT", root.join("auth"))
        .env("ARTIFACTORY_SOURCE_METADATA", root.join("metadata.json"));
    command
}

#[cfg(unix)]
#[tokio::test]
async fn source_failure_reports_actual_stage_and_a_safe_diagnostic() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(&git,"#!/bin/sh\nprintf 'Authentication failed for https://user:super-secret@private.example\\n' >&2\nexit 128\n").unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
    let progress = Arc::new(Mutex::new(Vec::<Value>::new()));
    let outcome = Arc::new(Mutex::new(Value::Null));
    let seen = progress.clone();
    let saved = outcome.clone();
    let app = Router::new()
        .route("/prepare", post(|| async { Json(material()) }))
        .route(
            "/prepare/progress",
            post(move |Json(body): Json<Value>| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(body["progress"].clone());
                    Json(json!({"accepted":true}))
                }
            }),
        )
        .route(
            "/prepare/commit",
            post(move |Json(body): Json<Value>| {
                let saved = saved.clone();
                async move {
                    *saved.lock().unwrap() = body["result"].clone();
                    Json(json!({"accepted":true}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut process = command(root.path(), &format!("http://{address}/prepare"), "resolve");
    let inherited_paths = std::env::var_os("PATH").unwrap();
    let paths = std::iter::once(bin).chain(std::env::split_paths(&inherited_paths));
    process.env("PATH", std::env::join_paths(paths).unwrap());
    let output = process.output().await.unwrap();
    assert!(!output.status.success());
    let result = outcome.lock().unwrap().clone();
    assert_eq!(
        result["diagnostic"],
        json!({"stage":"resolve_ref","code":"authentication"})
    );
    let records = progress.lock().unwrap();
    assert_eq!(records.last().unwrap()["stage"], "resolve_ref");
    assert_eq!(records.last().unwrap()["state"], "started");
    assert!(
        records
            .windows(2)
            .all(|pair| pair[0]["sequence"].as_u64() < pair[1]["sequence"].as_u64())
    );
    assert!(!result.to_string().contains("super-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("super-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private.example"));
    server.abort();
}
#[tokio::test]
async fn completed_tasks_do_not_checkout_or_commit_again() {
    for mode in ["resolve", "refs"] {
        let root = tempfile::tempdir().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let app = Router::new().route(
            "/prepare",
            post(move |Json(body): Json<Value>| {
                let count = count.clone();
                async move {
                    assert_eq!(body["bootstrap_token"], "pod-identity-token");
                    count.fetch_add(1, Ordering::SeqCst);
                    let mut response = material();
                    response.completed_result =
                        Some(json!({"refs":{"branches":[],"tags":[],"default_ref":null}}));
                    Json(response)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let output = command(root.path(), &format!("http://{address}/prepare"), mode)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(!root.path().join("source").exists());
        assert!(!root.path().join("auth").exists());
        server.abort();
    }
}
#[cfg(unix)]
#[tokio::test]
async fn a_lost_commit_ack_retries_the_same_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let script = bin.join("git");
    let sha = "a".repeat(40);
    std::fs::write(&script,format!("#!/bin/sh\nprintf 'ref: refs/heads/main\\tHEAD\\n{sha}\\tHEAD\\n{sha}\\trefs/heads/main\\n'\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let commits = Arc::new(Mutex::new(Vec::<Value>::new()));
    let committed = commits.clone();
    let app = Router::new()
        .route("/prepare", post(|| async { Json(material()) }))
        .route(
            "/prepare/commit",
            post(move |Json(body): Json<Value>| {
                let committed = committed.clone();
                async move {
                    let mut values = committed.lock().unwrap();
                    values.push(body["result"].clone());
                    (
                        if values.len() == 1 {
                            axum::http::StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            axum::http::StatusCode::OK
                        },
                        Json(json!({"ok":true})),
                    )
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut command = command(root.path(), &format!("http://{address}/prepare"), "refs");
    let paths = std::iter::once(bin)
        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))
        .collect::<Vec<_>>();
    command.env("PATH", std::env::join_paths(paths).unwrap());
    let output = command.output().await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commits = commits.lock().unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0], commits[1]);
    assert_eq!(commits[0]["refs"]["branches"][0]["oid"], sha);
    server.abort();
}

#[tokio::test]
async fn resolve_pins_only_metadata_without_checkout() {
    let root = tempfile::tempdir().unwrap();
    let saved = Arc::new(Mutex::new(Value::Null));
    let seen = saved.clone();
    let mut response = material();
    response.reference = "a".repeat(40);
    let app = Router::new()
        .route(
            "/prepare",
            post(move || {
                let response = response.clone();
                async move { Json(response) }
            }),
        )
        .route(
            "/prepare/progress",
            post(|| async { Json(json!({"ok":true})) }),
        )
        .route(
            "/prepare/pin",
            post(|Json(v): Json<Value>| async move { Json(json!({"oid":v["pin"]["oid"]})) }),
        )
        .route(
            "/prepare/commit",
            post(move |Json(v): Json<Value>| {
                let seen = seen.clone();
                async move {
                    *seen.lock().unwrap() = v["result"].clone();
                    Json(json!({"ok":true}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let output = command(root.path(), &format!("http://{addr}/prepare"), "resolve")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(*saved.lock().unwrap(), json!({"commit_sha":"a".repeat(40)}));
    assert!(!root.path().join("source").exists());
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn preparation_failure_reports_fetch_and_does_not_mark_local_ready() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let script = bin.join("git");
    std::fs::write(&script,"#!/bin/sh\nprintf 'Authentication failed for https://user:super-secret@private.example\\n' >&2\nexit 128\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let outcome = Arc::new(Mutex::new(Value::Null));
    let saved = outcome.clone();
    let ready = Arc::new(AtomicUsize::new(0));
    let ready_count = ready.clone();
    let mut response = material();
    response.pinned_ref_oid = Some("a".repeat(40));
    let app = Router::new()
        .route(
            "/prepare",
            post(move || {
                let response = response.clone();
                async move { Json(response) }
            }),
        )
        .route(
            "/prepare/progress",
            post(|| async { Json(json!({"ok":true})) }),
        )
        .route(
            "/prepare/fail",
            post(move |Json(body): Json<Value>| {
                let saved = saved.clone();
                async move {
                    *saved.lock().unwrap() = body["diagnostic"].clone();
                    Json(json!({"ok":true}))
                }
            }),
        )
        .route(
            "/prepare/ready",
            post(move || {
                let ready = ready_count.clone();
                async move {
                    ready.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"ok":true}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut process = command(root.path(), &format!("http://{address}/prepare"), "prepare");
    let inherited = std::env::var_os("PATH").unwrap();
    let paths = std::iter::once(bin).chain(std::env::split_paths(&inherited));
    process.env("PATH", std::env::join_paths(paths).unwrap());
    let output = process.output().await.unwrap();
    assert!(!output.status.success());
    assert_eq!(
        *outcome.lock().unwrap(),
        json!({"stage":"fetch","code":"authentication"})
    );
    assert_eq!(ready.load(Ordering::SeqCst), 0);
    assert!(!root.path().join("metadata.json").exists());
    assert_eq!(
        std::fs::read_dir(root.path().join("auth")).unwrap().count(),
        0
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("super-secret"));
    server.abort();
}
