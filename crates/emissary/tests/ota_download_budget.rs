//! [INPUT]: 真实下载器、签名资产、回环HTTP与受控Tokio时钟。
//! [OUTPUT]: 验证重试、续传和URL续签共用一次300秒预算。
//! [POS]: OTA授权总期限回归；只使用临时目录，不安装服务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::{ota, platform};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use std::{
    fs,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::{Instant, advance},
};
use tokio_util::sync::CancellationToken;

fn signed_request(url: String) -> (ota::Request, Vec<String>, ota::Asset) {
    let key = SigningKey::from_bytes(&[19; 32]);
    let (os, arch) = platform();
    let asset = ota::Asset::from_bytes(os, arch, b"abcdefghij");
    let manifest = ota::Manifest {
        version: "v-budget".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![asset.clone()],
    };
    let manifest = serde_json::to_string(&manifest).unwrap();
    (
        ota::Request {
            id: "first-authorized-update".into(),
            generation: 2,
            action: "apply".into(),
            signature: STANDARD.encode(key.sign(manifest.as_bytes()).to_bytes()),
            manifest,
            url,
        },
        vec![STANDARD.encode(key.verifying_key().to_bytes())],
        asset,
    )
}

// 只推进显式要求的时间；持续yield阻止空闲运行时自动跳到网络超时。
async fn observe_until(mut ready: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "fixture observation timed out"
        );
        tokio::task::yield_now().await;
    }
}

async fn advance_until(mut ready: impl FnMut() -> bool) {
    for _ in 0..100 {
        for _ in 0..100 {
            if ready() {
                return;
            }
            tokio::task::yield_now().await;
        }
        advance(Duration::from_millis(20)).await;
    }
    assert!(ready(), "bounded retry clock did not reach observation");
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        let mut bytes = [0; 1024];
        let count = stream.read(&mut bytes).await.unwrap();
        assert!(count > 0, "HTTP request ended before headers");
        request.extend_from_slice(&bytes[..count]);
        assert!(request.len() <= 8192, "fixture request too large");
    }
    String::from_utf8(request).unwrap().to_lowercase()
}

#[tokio::test(start_paused = true)]
async fn retries_resume_and_url_refresh_share_one_total_budget() {
    let keep_clock = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let root = tempfile::tempdir().unwrap();
    let current = root.path().join("current");
    fs::write(&current, b"known good").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = seen.clone();
    let server = tokio::spawn(async move {
        let responses = [
            "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: \"asset\"\r\nConnection: close\r\n\r\nabc",
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 7\r\nContent-Range: bytes 3-9/10\r\nETag: \"asset\"\r\n\r\n",
        ];
        let mut held = Vec::new();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            if request.starts_with("get /observer-health ") {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await
                    .unwrap();
                continue;
            }
            let index = {
                let mut recorded = recorded.lock().unwrap();
                recorded.push(request);
                recorded.len() - 1
            };
            if index == 0 {
                tokio::time::sleep(Duration::from_secs(200)).await;
            }
            let response = responses.get(index).copied().unwrap_or("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            stream.write_all(response.as_bytes()).await.unwrap();
            if index == 2 {
                // 独立保存慢正文连接；接收循环继续观察错误的额外GET。
                held.push(stream);
            }
        }
    });
    let (request, keys, asset) = signed_request(format!("http://{address}/expired"));
    let cfg = ota::OtaConfig {
        directory: root.path().into(),
        public_keys: keys,
        allow_http: true,
    };
    let run_cfg = cfg.clone();
    let run_request = request.clone();
    let started = Instant::now();
    let first = tokio::spawn(async move {
        ota::download(&run_cfg, &run_request, &CancellationToken::new()).await
    });
    observe_until(|| seen.lock().unwrap().len() == 1).await;
    advance(Duration::from_secs(200)).await;
    observe_until(|| {
        fs::read_dir(root.path().join("versions").join(&asset.sha256))
            .unwrap()
            .any(|entry| {
                let path = entry.unwrap().path();
                path.extension().is_some_and(|v| v == "part")
                    && fs::metadata(path).unwrap().len() == 3
            })
    })
    .await;
    advance_until(|| ota::url_refresh_request(root.path()).unwrap().is_some()).await;
    let frozen = ota::url_refresh_request(root.path()).unwrap().unwrap();
    assert_eq!(frozen.update_id, request.id);
    assert_eq!(frozen.generation, request.generation);
    assert_eq!(frozen.asset_sha256, asset.sha256);
    advance(Duration::from_secs(8)).await;
    ota::complete_url_refresh(
        root.path(),
        frozen,
        Some(format!("http://{address}/renewed")),
    )
    .unwrap();
    advance(Duration::from_millis(100)).await;
    observe_until(|| seen.lock().unwrap().len() == 3).await;
    let remaining = Duration::from_secs(299) - started.elapsed();
    advance(remaining).await;
    assert!(!first.is_finished(), "download ended before total budget");
    advance(Duration::from_secs(1)).await;
    observe_until(|| first.is_finished()).await;
    let error = first.await.unwrap().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("release download deadline exceeded"),
        "{error:#}"
    );
    assert_eq!(started.elapsed(), Duration::from_secs(300));
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains("range: bytes=3-"));
    assert!(requests[2].contains("get /renewed "));
    assert!(requests[2].contains("range: bytes=3-"));
    assert!(requests[2].contains("if-range: \"asset\""));
    assert_eq!(fs::read(&current).unwrap(), b"known good");
    let assets = root.path().join("versions").join(&asset.sha256);
    assert!(!assets.join(ota::binary_name()).exists());
    assert!(fs::read_dir(&assets).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|v| v != "part")
    }));
    assert!(ota::url_refresh_request(root.path()).unwrap().is_none());
    assert!(!root.path().join("url-refresh-reply.json").exists());
    let failed = ota::progress_snapshot(root.path()).unwrap().unwrap();
    assert_eq!(failed.update_id, request.id);
    assert_eq!(failed.bytes_done, 3);
    assert_eq!(failed.error_code.as_deref(), Some("download_failed"));
    advance(Duration::from_secs(600)).await;
    let health = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                b"GET /observer-health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        assert!(response.ends_with(b"\r\n\r\nok"));
    });
    observe_until(|| health.is_finished()).await;
    health.await.unwrap();
    assert_eq!(
        seen.lock().unwrap().len(),
        3,
        "GET continued after deadline"
    );
    server.abort();
    let _ = server.await;

    // 新管理员执行身份进入新调用；原执行没有获得延长或自动重试。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted, mut received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(read_request(&mut stream).await.starts_with("get /binary "));
        accepted.send(()).unwrap();
        tokio::time::sleep(Duration::from_secs(299)).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabcdefghij",
            )
            .await
            .unwrap();
    });
    let mut retry = request;
    retry.id = "new-admin-retry".into();
    retry.generation += 1;
    retry.url = format!("http://{address}/binary");
    let next_started = Instant::now();
    let second =
        tokio::spawn(async move { ota::download(&cfg, &retry, &CancellationToken::new()).await });
    observe_until(|| received.try_recv().is_ok()).await;
    advance(Duration::from_secs(299)).await;
    observe_until(|| second.is_finished()).await;
    let image = second.await.unwrap().unwrap();
    assert_eq!(next_started.elapsed(), Duration::from_secs(299));
    assert_eq!(fs::read(image.path).unwrap(), b"abcdefghij");
    assert_eq!(fs::read(current).unwrap(), b"known good");
    let success = ota::progress_snapshot(root.path()).unwrap().unwrap();
    assert_eq!(success.update_id, "new-admin-retry");
    assert_eq!(success.generation, 3);
    assert_eq!(success.subphase, "verified");
    assert!(success.error_code.is_none());
    observe_until(|| server.is_finished()).await;
    server.await.unwrap();
    keep_clock.abort();
}
