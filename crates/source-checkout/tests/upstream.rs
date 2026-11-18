// [INPUT]: 依赖源码边界、真实 Git 与本机 HTTP 夹具。
// [OUTPUT]: 验证每次上游拉取、文件系统权限与内容校验。
// [POS]: 共享检出引擎的真实 Git 回归。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_source_checkout::{
    Checkout, Options, SourceManifest, SourceRepository, managed_path,
};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    routing::any,
};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

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
#[cfg(unix)]
struct ReadOnlySource(Vec<(std::path::PathBuf, fs::Permissions)>);
#[cfg(unix)]
impl ReadOnlySource {
    fn new(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        fn protect(path: &Path, entries: &mut Vec<(std::path::PathBuf, fs::Permissions)>) {
            let metadata = fs::symlink_metadata(path).unwrap();
            if metadata.is_symlink() {
                return;
            }
            entries.push((path.to_owned(), metadata.permissions()));
            if metadata.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    protect(&entry.unwrap().path(), entries);
                }
            }
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(metadata.permissions().mode() & !0o222),
            )
            .unwrap();
        }
        let mut entries = Vec::new();
        protect(path, &mut entries);
        Self(entries)
    }
}
#[cfg(unix)]
impl Drop for ReadOnlySource {
    fn drop(&mut self) {
        for (path, permissions) in &self.0 {
            fs::set_permissions(path, permissions.clone()).unwrap();
        }
    }
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
        "/git/info/lfs/objects/batch" | "/git.git/info/lfs/objects/batch"
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
#[tokio::test]
async fn every_checkout_fetches_upstream_and_isolates_attempts() {
    let root = tempfile::tempdir().unwrap();
    let upstream = root.path().join("repo.git");
    fs::create_dir(&upstream).unwrap();
    git(&upstream, &["init", "-b", "source"]);
    fs::write(upstream.join("main.c"), "int main(void) { return 0; }\n").unwrap();
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "-m", "immutable fixture"]);
    let commit = git(&upstream, &["rev-parse", "HEAD"]);
    git(&upstream, &["tag", "-a", "v1", "-m", "frozen tag"]);
    let tag = git(&upstream, &["rev-parse", "refs/tags/v1"]);
    let requests = Arc::new(AtomicUsize::new(0));
    let app = Router::new().fallback(any(backend)).with_state(Fixture {
        root: root.path().into(),
        requests: requests.clone(),
        lfs_objects: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let checkout = Checkout::new(Options {
        auth_root: root.path().join("auth"),
        ..Default::default()
    });
    let manifest = SourceManifest::new(
        uuid::Uuid::new_v4(),
        vec![SourceRepository {
            path: String::new(),
            upstream_url: format!("http://{address}/git"),
            commit_sha: commit,
            tags: std::collections::BTreeMap::from([("v1".into(), tag)]),
            lfs_objects: Vec::new(),
        }],
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let first = root.path().join("first");
    let second = root.path().join("second");
    checkout
        .checkout(&manifest, &[], &first, &cancel)
        .await
        .unwrap();
    let initial = requests.load(Ordering::SeqCst);
    assert!(initial > 0);
    checkout
        .checkout(&manifest, &[], &second, &cancel)
        .await
        .unwrap();
    assert!(
        requests.load(Ordering::SeqCst) > initial,
        "second task must fetch directly from upstream"
    );
    assert_eq!(git(&second, &["describe", "--tags"]), "v1");
    fs::write(first.join("main.c"), "changed\n").unwrap();
    assert!(
        checkout
            .verify(&manifest, &first, &cancel)
            .await
            .unwrap_err()
            .to_string()
            .contains("source_modified")
    );
    checkout.verify(&manifest, &second, &cancel).await.unwrap();
    git(&first, &["reset", "--hard", "HEAD"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(first.join("main.c"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(checkout.verify(&manifest, &first, &cancel).await.is_err());
    }
    fs::write(second.join("generated.tmp"), "build output").unwrap();
    checkout.verify(&manifest, &second, &cancel).await.unwrap();
    for relative in ["../x", "a/../x", ".git/x", "/x", "C:/x", "a\\x"] {
        assert!(managed_path(&second, relative).is_err());
    }
    server.abort();
}
#[tokio::test]
async fn verification_preserves_permissions_and_ignores_mutable_index_flags() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "source"]);
    let script = root.path().join("build.sh");
    fs::write(&script, "#!/bin/sh\nprintf 'source\\n'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(root.path(), &["add", "build.sh"]);
    git(root.path(), &["update-index", "--chmod=+x", "build.sh"]);
    git(root.path(), &["commit", "-m", "executable source"]);
    let commit = git(root.path(), &["rev-parse", "HEAD"]);
    let manifest = SourceManifest::new(
        uuid::Uuid::new_v4(),
        vec![SourceRepository {
            path: String::new(),
            upstream_url: "https://example.com/source.git".into(),
            commit_sha: commit,
            tags: Default::default(),
            lfs_objects: Vec::new(),
        }],
    )
    .unwrap();
    let cancel = CancellationToken::new();
    artifactory_source_checkout::verify(&manifest, root.path(), &cancel)
        .await
        .expect("a clean executable snapshot must verify on its native filesystem");
    #[cfg(unix)]
    {
        let _readonly = ReadOnlySource::new(root.path());
        artifactory_source_checkout::verify(&manifest, root.path(), &cancel)
            .await
            .expect("verification must not write to a read-only executable source tree");
    }
    for (enable, disable) in [
        ("--assume-unchanged", "--no-assume-unchanged"),
        ("--skip-worktree", "--no-skip-worktree"),
    ] {
        git(root.path(), &["update-index", enable, "build.sh"]);
        fs::write(&script, "changed source\n").unwrap();
        let error = artifactory_source_checkout::verify(&manifest, root.path(), &cancel)
            .await
            .expect_err("mutable index flags must not hide modified tracked source");
        assert!(error.to_string().contains("source_modified"), "{error:#}");
        git(root.path(), &["update-index", disable, "build.sh"]);
        git(root.path(), &["reset", "--hard", "HEAD"]);
    }
}

#[tokio::test]
async fn submodules_use_fixed_gitlinks_and_cannot_be_omitted() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("repo.git");
    let child = root.path().join("child.git");
    for path in [&parent, &child] {
        fs::create_dir(path).unwrap();
        git(path, &["init", "-b", "main"]);
    }
    fs::write(child.join("child.txt"), "fixed child").unwrap();
    git(&child, &["add", "."]);
    git(&child, &["commit", "-m", "child"]);
    let child_sha = git(&child, &["rev-parse", "HEAD"]);
    fs::write(
        parent.join(".gitmodules"),
        "[submodule \"child\"]\n path = modules/child\n url = ../child.git\n",
    )
    .unwrap();
    git(&parent, &["add", ".gitmodules"]);
    git(
        &parent,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{child_sha},modules/child"),
        ],
    );
    git(&parent, &["commit", "-m", "fixed child link"]);
    let commit = git(&parent, &["rev-parse", "HEAD"]);
    let app = Router::new().fallback(any(backend)).with_state(Fixture {
        root: root.path().to_owned(),
        requests: Default::default(),
        lfs_objects: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root_repo = SourceRepository {
        path: String::new(),
        upstream_url: format!("http://{address}/git"),
        commit_sha: commit,
        tags: Default::default(),
        lfs_objects: Vec::new(),
    };
    let child_repo = SourceRepository {
        path: "modules/child".into(),
        upstream_url: format!("http://{address}/child.git"),
        commit_sha: child_sha,
        tags: Default::default(),
        lfs_objects: Vec::new(),
    };
    let manifest =
        SourceManifest::new(uuid::Uuid::new_v4(), vec![root_repo.clone(), child_repo]).unwrap();
    let checkout = Checkout::new(Options {
        auth_root: root.path().join("auth"),
        ..Default::default()
    });
    let cancel = CancellationToken::new();
    let workspace = root.path().join("complete");
    checkout
        .checkout(&manifest, &[], &workspace, &cancel)
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(workspace.join("modules/child/child.txt")).unwrap(),
        "fixed child"
    );
    #[cfg(unix)]
    {
        let _readonly = ReadOnlySource::new(&workspace);
        checkout
            .verify(&manifest, &workspace, &cancel)
            .await
            .expect("verification must not write to read-only submodules");
    }
    let incomplete = SourceManifest::new(uuid::Uuid::new_v4(), vec![root_repo]).unwrap();
    assert!(
        checkout
            .checkout(&incomplete, &[], &root.path().join("incomplete"), &cancel)
            .await
            .unwrap_err()
            .to_string()
            .contains("omits")
    );
    assert_eq!(
        fs::read_dir(root.path().join("auth")).unwrap().count(),
        0,
        "authentication directories remain"
    );
    server.abort();
}

#[tokio::test]
async fn lfs_is_fetched_and_checked_without_shared_storage() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let upstream = root.path().join("repo.git");
    fs::create_dir(&upstream).unwrap();
    git(&upstream, &["init", "-b", "main"]);
    let data = b"large artifact source bytes".to_vec();
    let oid = hex::encode(Sha256::digest(&data));
    fs::write(
        upstream.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    fs::write(
        upstream.join("source.bin"),
        format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            data.len()
        ),
    )
    .unwrap();
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "-m", "LFS source"]);
    let commit = git(&upstream, &["rev-parse", "HEAD"]);
    let app = Router::new().fallback(any(backend)).with_state(Fixture {
        root: root.path().to_owned(),
        requests: Default::default(),
        lfs_objects: Arc::new(std::collections::BTreeMap::from([(
            oid.clone(),
            data.clone(),
        )])),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let manifest = SourceManifest::new(
        uuid::Uuid::new_v4(),
        vec![SourceRepository {
            path: String::new(),
            upstream_url: format!("http://{address}/git"),
            commit_sha: commit,
            tags: Default::default(),
            lfs_objects: vec![artifactory_source_checkout::LfsObject {
                path: "source.bin".into(),
                oid,
                size: data.len() as u64,
            }],
        }],
    )
    .unwrap();
    let checkout = Checkout::new(Options {
        auth_root: root.path().join("auth"),
        ..Default::default()
    });
    let workspace = root.path().join("checkout");
    let cancel = CancellationToken::new();
    checkout
        .checkout(&manifest, &[], &workspace, &cancel)
        .await
        .unwrap();
    assert_eq!(fs::read(workspace.join("source.bin")).unwrap(), data);
    #[cfg(unix)]
    {
        let _readonly = ReadOnlySource::new(&workspace);
        checkout
            .verify(&manifest, &workspace, &cancel)
            .await
            .expect("verification must keep LFS temporary writes outside read-only source");
    }
    fs::write(workspace.join("source.bin"), vec![b'x'; data.len()]).unwrap();
    assert!(
        checkout
            .verify(&manifest, &workspace, &cancel)
            .await
            .is_err()
    );
    assert_eq!(fs::read_dir(root.path().join("auth")).unwrap().count(), 0);
    server.abort();
}
