//! [INPUT]: 签名发行、真实HTTP响应与临时OTA目录。
//! [OUTPUT]: 验证资产复用、严格续传、取消和摘要准入。
//! [POS]: OTA下载器跨文件与网络边界回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::{ota, platform};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn interrupted_download_never_replaces_current_binary() {
    let key = SigningKey::from_bytes(&[9; 32]);
    let body = b"complete release binary";
    let (os, arch) = platform();
    let asset = ota::Asset::from_bytes(os, arch, body);
    let manifest = ota::Manifest {
        version: "v-test".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![asset.clone()],
    };
    let raw = serde_json::to_string(&manifest).unwrap();
    for bad in [
        body[..3].to_vec(),
        vec![b'x'; body.len()],
        [body.as_slice(), &[1]].concat(),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let current = directory.path().join("current");
        fs::write(&current, b"known good").unwrap();
        let app = axum::Router::new().route(
            "/binary",
            axum::routing::get(move || {
                let bad = bad.clone();
                async move { bad }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let cfg = ota::OtaConfig {
            directory: directory.path().into(),
            public_keys: vec![STANDARD.encode(key.verifying_key().to_bytes())],
            allow_http: true,
        };
        let request = ota::Request {
            manifest: raw.clone(),
            signature: STANDARD.encode(key.sign(raw.as_bytes()).to_bytes()),
            url: format!("http://{address}/binary"),
            ..Default::default()
        };
        assert!(
            ota::download(&cfg, &request, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(fs::read(&current).unwrap(), b"known good");
        assert!(
            !directory
                .path()
                .join("versions")
                .join(&asset.sha256)
                .join(ota::binary_name())
                .exists()
        );
        server.abort();
    }
}

fn signed_request(body: &[u8], url: String) -> (ota::Request, Vec<String>, ota::Asset) {
    let key = SigningKey::from_bytes(&[9; 32]);
    let (os, arch) = platform();
    let asset = ota::Asset::from_bytes(os, arch, body);
    let manifest = ota::Manifest {
        version: "v-test".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![asset.clone()],
    };
    let raw = serde_json::to_string(&manifest).unwrap();
    (
        ota::Request {
            id: "test-update".into(),
            generation: 2,
            action: "apply".into(),
            signature: STANDARD.encode(key.sign(raw.as_bytes()).to_bytes()),
            manifest: raw,
            url,
        },
        vec![STANDARD.encode(key.verifying_key().to_bytes())],
        asset,
    )
}

#[tokio::test]
async fn verified_cached_asset_does_not_need_the_network() {
    let directory = tempfile::tempdir().unwrap();
    let body = b"verified cached release";
    let (request, keys, asset) = signed_request(body, "http://127.0.0.1:1/offline".into());
    let target = directory
        .path()
        .join("versions")
        .join(&asset.sha256)
        .join(ota::binary_name());
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, body).unwrap();
    let cfg = ota::OtaConfig {
        directory: directory.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    let image = ota::download(&cfg, &request, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(image.path, target.to_string_lossy());
    assert_eq!(fs::read(target).unwrap(), body);
    let canceled = CancellationToken::new();
    canceled.cancel();
    assert!(ota::download(&cfg, &request, &canceled).await.is_err());
}

async fn resume_case(
    range_response: &'static str,
) -> (
    anyhow::Result<ota::Image>,
    Vec<String>,
    tempfile::TempDir,
    ota::Asset,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = b"abcdefghij";
    let directory = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let server = tokio::spawn(async move {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 4096];
            let n = stream.read(&mut bytes).await.unwrap();
            recorded
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&bytes[..n]).to_lowercase());
            if index == 0 {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: \"asset\"\r\nConnection: close\r\n\r\nabc").await.unwrap();
            } else {
                stream.write_all(range_response.as_bytes()).await.unwrap();
            }
        }
    });
    let (request, keys, asset) = signed_request(body, format!("http://{address}/binary"));
    let cfg = ota::OtaConfig {
        directory: directory.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    let result = ota::download(&cfg, &request, &CancellationToken::new()).await;
    server.abort();
    let requests = seen.lock().unwrap().clone();
    (result, requests, directory, asset)
}

#[tokio::test]
async fn interrupted_body_resumes_from_the_written_offset() {
    let (result, requests, directory, asset) = resume_case("HTTP/1.1 206 Partial Content\r\nContent-Length: 7\r\nContent-Range: bytes 3-9/10\r\nETag: \"asset\"\r\nConnection: close\r\n\r\ndefghij").await;
    let image = result.unwrap();
    assert_eq!(fs::read(image.path).unwrap(), b"abcdefghij");
    assert_eq!(requests.len(), 2);
    assert!(requests[1].contains("range: bytes=3-"), "{}", requests[1]);
    assert!(requests[1].contains("if-range: \"asset\""));
    assert!(requests[1].contains("accept-encoding: identity"));
    assert!(
        directory
            .path()
            .join("versions")
            .join(asset.sha256)
            .exists()
    );
}

#[tokio::test]
async fn range_ignored_with_200_restarts_instead_of_appending() {
    let (result, requests, _directory, _) =
        resume_case("HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabcdefghij")
            .await;
    assert_eq!(fs::read(result.unwrap().path).unwrap(), b"abcdefghij");
    assert!(requests[1].contains("range: bytes=3-"));
}

#[tokio::test]
async fn mismatched_content_range_is_rejected_before_install() {
    let (result, requests, directory, asset) = resume_case("HTTP/1.1 206 Partial Content\r\nContent-Length: 7\r\nContent-Range: bytes 2-8/10\r\nConnection: close\r\n\r\ndefghij").await;
    assert!(result.is_err());
    assert_eq!(requests.len(), 2);
    assert!(
        !directory
            .path()
            .join("versions")
            .join(asset.sha256)
            .join(ota::binary_name())
            .exists()
    );
}

#[tokio::test]
async fn changed_etag_during_resume_is_rejected() {
    let (result, requests, directory, asset) = resume_case("HTTP/1.1 206 Partial Content\r\nContent-Length: 7\r\nContent-Range: bytes 3-9/10\r\nETag: \"changed\"\r\nConnection: close\r\n\r\ndefghij").await;
    assert!(result.is_err());
    assert_eq!(requests.len(), 2);
    assert!(
        !directory
            .path()
            .join("versions")
            .join(asset.sha256)
            .join(ota::binary_name())
            .exists()
    );
}

#[tokio::test]
async fn encoded_range_response_is_rejected() {
    let (result, _, directory, asset) = resume_case("HTTP/1.1 206 Partial Content\r\nContent-Encoding: gzip\r\nContent-Length: 7\r\nContent-Range: bytes 3-9/10\r\nConnection: close\r\n\r\ndefghij").await;
    assert!(result.is_err());
    assert!(
        !directory
            .path()
            .join("versions")
            .join(asset.sha256)
            .join(ota::binary_name())
            .exists()
    );
}

#[tokio::test]
async fn expired_url_can_refresh_only_the_same_frozen_request() {
    let root = tempfile::tempdir().unwrap();
    let body = b"refreshed frozen asset";
    let app = axum::Router::new()
        .route(
            "/expired",
            axum::routing::get(|| async { axum::http::StatusCode::FORBIDDEN }),
        )
        .route("/renewed", axum::routing::get(|| async { body.as_slice() }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (request, keys, asset) = signed_request(body, format!("http://{address}/expired"));
    let cfg = ota::OtaConfig {
        directory: root.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    let directory = root.path().to_path_buf();
    let expected = asset.sha256.clone();
    let forward = tokio::spawn(async move {
        loop {
            if let Some(request) = ota::url_refresh_request(&directory).unwrap() {
                assert_eq!(request.update_id, "test-update");
                assert_eq!(request.generation, 2);
                assert_eq!(request.asset_sha256, expected);
                assert_eq!(request.asset_size, body.len() as u64);
                let mut stale = request.clone();
                stale.generation = 1;
                ota::complete_url_refresh(
                    &directory,
                    stale,
                    Some(format!("http://{address}/wrong")),
                )
                .unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                ota::complete_url_refresh(
                    &directory,
                    request,
                    Some(format!("http://{address}/renewed")),
                )
                .unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });
    let image = ota::download(&cfg, &request, &CancellationToken::new())
        .await
        .unwrap();
    forward.await.unwrap();
    server.abort();
    assert_eq!(fs::read(image.path).unwrap(), body);
    let progress = ota::progress_snapshot(root.path()).unwrap().unwrap();
    assert_eq!(progress.subphase, "verified");
    assert_eq!(progress.bytes_done, body.len() as u64);
    assert_eq!(progress.asset_sha256, asset.sha256);
}

#[tokio::test]
async fn cancellation_removes_refresh_mailbox_and_ignores_a_late_reply() {
    let root = tempfile::tempdir().unwrap();
    let app = axum::Router::new().route(
        "/expired",
        axum::routing::get(|| async { axum::http::StatusCode::FORBIDDEN }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (request, keys, _) = signed_request(b"asset", format!("http://{address}/expired"));
    let cfg = ota::OtaConfig {
        directory: root.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let task = tokio::spawn(async move { ota::download(&cfg, &request, &stop).await });
    let frozen = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(request) = ota::url_refresh_request(root.path()).unwrap() {
                break request;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    assert!(task.await.unwrap().is_err());
    assert!(ota::url_refresh_request(root.path()).unwrap().is_none());
    ota::complete_url_refresh(root.path(), frozen, Some("https://example.com/late".into()))
        .unwrap();
    assert!(!root.path().join("url-refresh-reply.json").exists());
    server.abort();
}

#[tokio::test]
async fn canceled_download_never_installs_partial_asset() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        stop.cancel();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    });
    let (request, keys, asset) = signed_request(b"abcdefghij", format!("http://{address}/binary"));
    let cfg = ota::OtaConfig {
        directory: root.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    assert!(ota::download(&cfg, &request, &cancel).await.is_err());
    server.abort();
    assert!(
        !root
            .path()
            .join("versions")
            .join(asset.sha256)
            .join(ota::binary_name())
            .exists()
    );
    assert_eq!(
        ota::progress_snapshot(root.path())
            .unwrap()
            .unwrap()
            .error_code
            .as_deref(),
        Some("canceled")
    );
}

#[tokio::test]
async fn range_416_with_incomplete_file_restarts_instead_of_succeeding() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabc",
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabcdefghij",
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let n = stream.read(&mut request).await.unwrap();
            requests.push(String::from_utf8_lossy(&request[..n]).to_lowercase());
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });
    let (request, keys, _) = signed_request(b"abcdefghij", format!("http://{address}/binary"));
    let cfg = ota::OtaConfig {
        directory: root.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    let image = ota::download(&cfg, &request, &CancellationToken::new())
        .await
        .unwrap();
    let requests = server.await.unwrap();
    assert!(requests[1].contains("range: bytes=3-"));
    assert!(!requests[2].contains("range:"));
    assert_eq!(fs::read(image.path).unwrap(), b"abcdefghij");
    assert_eq!(
        ota::progress_snapshot(root.path())
            .unwrap()
            .unwrap()
            .download_attempt,
        2
    );
}
