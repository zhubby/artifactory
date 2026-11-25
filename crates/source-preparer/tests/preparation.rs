//! [INPUT]: 依赖真实 init 二进制、Git HTTP/LFS 上游与准备 RPC 夹具。
//! [OUTPUT]: 验证最终嵌套目录、冻结清单、本代就绪和回执重试。
//! [POS]: 同 Pod 源码准备协议的端到端进程回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_source_checkout::{PrepareResponse, SourceManifest};
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    routing::{any, post},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            path.to_str().unwrap(),
            "-c",
            "user.name=Source test",
            "-c",
            "user.email=test@localhost",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
#[derive(Clone)]
struct Fixture {
    root: std::path::PathBuf,
    requests: Arc<AtomicUsize>,
    lfs_objects: Arc<std::collections::BTreeMap<String, Vec<u8>>>,
}
async fn backend(
    State(fixture): State<Fixture>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    use std::io::Write;
    fixture.requests.fetch_add(1, Ordering::SeqCst);
    if matches!(
        uri.path(),
        "/git/info/lfs/objects/batch"
            | "/repo.git/info/lfs/objects/batch"
            | "/child.git/info/lfs/objects/batch"
    ) {
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let host = headers.get("host").unwrap().to_str().unwrap();
        let objects=request["objects"].as_array().unwrap().iter().map(|object|{
            let oid=object["oid"].as_str().unwrap();let data=fixture.lfs_objects.get(oid).unwrap();
            serde_json::json!({"oid":oid,"size":data.len(),"actions":{"download":{"href":format!("http://{host}/lfs/{oid}")}}})
        }).collect::<Vec<_>>();
        let mut response = HeaderMap::new();
        response.insert(
            "content-type",
            "application/vnd.git-lfs+json".parse().unwrap(),
        );
        return (
            StatusCode::OK,
            response,
            serde_json::to_vec(&serde_json::json!({"objects":objects})).unwrap(),
        );
    }
    if let Some(oid) = uri.path().strip_prefix("/lfs/") {
        return (
            StatusCode::OK,
            HeaderMap::new(),
            fixture.lfs_objects.get(oid).unwrap().clone(),
        );
    }
    let output = tokio::task::spawn_blocking(move || {
        let mut command = std::process::Command::new("git");
        command
            .arg("http-backend")
            .env("GIT_PROJECT_ROOT", fixture.root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env(
                "PATH_INFO",
                if uri.path().starts_with("/git/") {
                    format!("/repo.git/{}", uri.path().trim_start_matches("/git/"))
                } else {
                    uri.path().to_owned()
                },
            )
            .env("REQUEST_METHOD", method.as_str())
            .env("QUERY_STRING", uri.query().unwrap_or(""))
            .env(
                "CONTENT_TYPE",
                headers
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(&body).unwrap();
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    assert!(output.status.success());
    let split = output
        .stdout
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap();
    let mut headers = HeaderMap::new();
    for line in String::from_utf8_lossy(&output.stdout[..split]).lines() {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-type")
        {
            headers.insert("content-type", value.trim().parse().unwrap());
        }
    }
    (StatusCode::OK, headers, output.stdout[split + 4..].to_vec())
}

fn command(root: &Path, url: &str) -> tokio::process::Command {
    fs::write(root.join("token"), "pod-identity-token").unwrap();
    let mut command =
        tokio::process::Command::new(env!("CARGO_BIN_EXE_artifactory-source-preparer"));
    command
        .arg("prepare")
        .env("ARTIFACTORY_SOURCE_PREPARE_URL", url)
        .env(
            "ARTIFACTORY_SOURCE_IDENTITY",
            r#"{"run_id":"00000000-0000-0000-0000-000000000001","generation":1}"#,
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

#[derive(Clone, Default)]
struct PreparationState {
    progress: Arc<Mutex<Vec<Value>>>,
    commits: Arc<Mutex<Vec<Value>>>,
    ready: Arc<Mutex<Vec<Value>>>,
    pins: Arc<Mutex<Vec<Value>>>,
    fetches_at_commit: Arc<Mutex<Vec<usize>>>,
}
fn preparation_api(material: PrepareResponse, fixture: Fixture, state: PreparationState) -> Router {
    let progress = state.progress.clone();
    let pins = state.pins.clone();
    let commits = state.commits.clone();
    let ready = state.ready.clone();
    let fetches = state.fetches_at_commit.clone();
    let requests = fixture.requests.clone();
    Router::new()
        .route("/prepare", post(move || { let material=material.clone(); async move { Json(material) } }))
        .route("/prepare/progress", post(move |Json(body): Json<Value>| { let progress=progress.clone(); async move {
            progress.lock().unwrap().push(body["progress"].clone()); Json(json!({"ok":true}))
        }}))
        .route("/prepare/pin", post(move |Json(body): Json<Value>| { let pins=pins.clone(); async move {
            pins.lock().unwrap().push(body["pin"].clone()); Json(json!({"tags":body["pin"]["tags"]}))
        }}))
        .route("/prepare/commit", post(move |Json(body): Json<Value>| {
            let commits=commits.clone(); let fetches=fetches.clone(); let requests=requests.clone(); async move {
                fetches.lock().unwrap().push(requests.load(Ordering::SeqCst));
                let mut values=commits.lock().unwrap(); values.push(body["result"]["manifest"].clone());
                let manifest=body["result"]["manifest"].clone();
                Json(if values.len()==1 { json!({"accepted":false,"pending":true}) } else {
                    json!({"accepted":true,"pending":false,"source_manifest_id":manifest["id"],"source_digest":manifest["digest"]})
                })
            }
        }))
        .route("/prepare/ready", post(move |Json(body): Json<Value>| { let ready=ready.clone(); async move {
            ready.lock().unwrap().push(body); Json(json!({"ok":true}))
        }}))
        .fallback(any(backend)).with_state(fixture)
}

#[tokio::test]
async fn preparation_reuses_final_tree_with_nested_modules_tags_and_lfs() {
    let root = tempfile::tempdir().unwrap();
    let upstream = root.path().join("repo.git");
    let child = root.path().join("child.git");
    let nested = root.path().join("nested.git");
    for path in [&upstream, &child, &nested] {
        fs::create_dir(path).unwrap();
        git(path, &["init", "-b", "main"]);
    }
    fs::write(nested.join("nested.txt"), "nested source").unwrap();
    git(&nested, &["add", "."]);
    git(&nested, &["commit", "-m", "nested"]);
    let nested_commit = git(&nested, &["rev-parse", "HEAD"]);
    fs::write(
        child.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    // SHA-256 of 'abc': LFS pointer is committed without a local LFS store.
    let oid = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    fs::write(
        child.join("payload.bin"),
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 3\n"),
    )
    .unwrap();
    fs::write(
        child.join(".gitmodules"),
        "[submodule \"nested\"]\n path = vendor/nested\n url = ../nested.git\n",
    )
    .unwrap();
    git(&child, &["add", "."]);
    git(
        &child,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{nested_commit},vendor/nested"),
        ],
    );
    git(&child, &["commit", "-m", "child with nested source"]);
    let child_commit = git(&child, &["rev-parse", "HEAD"]);
    fs::write(upstream.join("old.txt"), "history is retained").unwrap();
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "-m", "ancestor"]);
    let ancestor = git(&upstream, &["rev-parse", "HEAD"]);
    git(&upstream, &["tag", "-a", "v1", "-m", "ancestor tag"]);
    fs::write(
        upstream.join(".gitmodules"),
        "[submodule \"child\"]\n path = modules/child\n url = ../child.git\n",
    )
    .unwrap();
    git(&upstream, &["add", "."]);
    git(
        &upstream,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{child_commit},modules/child"),
        ],
    );
    git(&upstream, &["commit", "-m", "root with fixed child"]);
    let commit = git(&upstream, &["rev-parse", "HEAD"]);
    git(&upstream, &["checkout", "--orphan", "unrelated"]);
    git(&upstream, &["rm", "-rf", "."]);
    fs::write(upstream.join("other.txt"), "unrelated").unwrap();
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "-m", "unrelated tag"]);
    git(&upstream, &["tag", "other"]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let material = PrepareResponse {
        source_id: uuid::Uuid::from_u128(1),
        repository_url: format!("http://{address}/repo.git"),
        reference: "main".into(),
        pinned_ref_oid: Some(commit.clone()),
        frozen_tags: Default::default(),
        manifest: None,
        credentials: vec![],
        completed_result: None,
    };
    let fixture = Fixture {
        root: root.path().into(),
        requests: Default::default(),
        lfs_objects: Arc::new(std::collections::BTreeMap::from([(
            oid.into(),
            b"abc".to_vec(),
        )])),
    };
    let state = PreparationState::default();
    let app = preparation_api(material.clone(), fixture.clone(), state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let attempt = root.path().join("attempt");
    fs::create_dir(&attempt).unwrap();
    let output = command(&attempt, &format!("http://{address}/prepare"))
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let source = attempt.join("source");
    assert_eq!(git(&source, &["rev-parse", "HEAD"]), commit);
    assert_eq!(git(&source, &["cat-file", "-t", &ancestor]), "commit");
    assert_eq!(git(&source, &["tag", "--list"]), "v1");
    assert_eq!(
        fs::read(source.join("modules/child/payload.bin")).unwrap(),
        b"abc"
    );
    assert_eq!(
        fs::read_to_string(source.join("modules/child/vendor/nested/nested.txt")).unwrap(),
        "nested source"
    );
    let manifest: SourceManifest =
        serde_json::from_slice(&fs::read(attempt.join("metadata.json")).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.repositories.len(), 3);
    assert_eq!(manifest.repositories[1].lfs_objects[0].oid, oid);
    let values = state.commits.lock().unwrap().clone();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0], values[1]);
    drop(values);
    let fetches = state.fetches_at_commit.lock().unwrap().clone();
    assert_eq!(
        fetches[0], fetches[1],
        "metadata convergence must not fetch again"
    );
    drop(fetches);
    assert_eq!(
        state.ready.lock().unwrap()[0]["source_digest"],
        manifest.digest
    );
    let progress = state.progress.lock().unwrap().clone();
    let completed = progress
        .iter()
        .filter(|p| p["state"] == "completed")
        .map(|p| p["stage"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        completed,
        vec![
            "authorization",
            "fetch",
            "submodules",
            "lfs",
            "verify",
            "manifest",
            "ready"
        ]
    );
    assert!(
        progress
            .windows(2)
            .all(|pair| pair[0]["sequence"].as_u64() < pair[1]["sequence"].as_u64())
    );
    drop(progress);
    assert_eq!(fs::read_dir(attempt.join("auth")).unwrap().count(), 0);
    assert_eq!(state.pins.lock().unwrap().len(), 3);
    // A replacement Pod fetches the same immutable manifest into its own final tree.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_address = listener.local_addr().unwrap();
    let fixed_state = PreparationState::default();
    let mut fixed = material;
    fixed.manifest = Some(manifest.clone());
    fixed.completed_result = Some(json!({"accepted":true}));
    let fixed_app = preparation_api(fixed, fixture.clone(), fixed_state.clone());
    let fixed_server = tokio::spawn(async move { axum::serve(listener, fixed_app).await.unwrap() });
    let replacement = root.path().join("replacement");
    fs::create_dir(&replacement).unwrap();
    let output = command(&replacement, &format!("http://{api_address}/prepare"))
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let regenerated: SourceManifest =
        serde_json::from_slice(&fs::read(replacement.join("metadata.json")).unwrap()).unwrap();
    assert_eq!(regenerated, manifest);
    assert!(fixed_state.pins.lock().unwrap().is_empty());
    assert_eq!(git(&replacement.join("source"), &["tag", "--list"]), "v1");
    assert_eq!(fs::read_dir(replacement.join("auth")).unwrap().count(), 0);
    fixed_server.abort();
    server.abort();
}
