//! [INPUT]: 真实监督器、原runtime、签名下载及私有请求/状态路径故障。
//! [OUTPUT]: 验证请求耐久接受、交接锁和取消清理重试保持进程。
//! [POS]: OTA请求接受回归；不安装系统服务或改动设备身份。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(unix)]
use artifactory_emissary::{config::atomic_json, ota, platform};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use futures::FutureExt;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[test]
fn submit_cannot_replace_mailbox_during_supervisor_handoff() {
    let root = tempfile::tempdir().unwrap();
    let request = ota::Request {
        id: "original".into(),
        generation: 1,
        action: "cancel".into(),
        ..Default::default()
    };
    ota::submit(root.path(), &request).unwrap();
    let original = fs::read(root.path().join("request.json")).unwrap();
    let lock = fs::File::create(root.path().join("request.lock")).unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    let newer = ota::Request {
        id: "newer".into(),
        generation: 2,
        ..request
    };
    assert!(
        ota::submit(root.path(), &newer).is_err(),
        "submission replaced mailbox while handoff held the lock"
    );
    assert_eq!(
        fs::read(root.path().join("request.json")).unwrap(),
        original
    );
    fs2::FileExt::unlock(&lock).unwrap();
    ota::submit(root.path(), &newer).unwrap();
    let stored: ota::Request =
        serde_json::from_slice(&fs::read(root.path().join("request.json")).unwrap()).unwrap();
    assert_eq!(stored.id, "newer");
    assert_eq!(stored.generation, 2);
}

#[tokio::test]
async fn apply_state_write_failure_keeps_runtime_and_unconsumed_request() {
    request_fault(false, "state.json", false).await;
}

#[tokio::test]
async fn cancel_state_write_failure_keeps_runtime_and_unconsumed_request() {
    request_fault(true, "state.json", false).await;
}

#[tokio::test]
async fn cancel_protection_cleanup_failure_keeps_runtime_and_retries() {
    request_fault(true, "active-download.json", false).await;
}

#[tokio::test]
async fn cleanup_retry_preserves_newer_mailbox_generation() {
    request_fault(true, "active-download.json", true).await;
}

async fn request_fault(cancel_request: bool, fault: &str, newer_request: bool) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("ota");
    fs::create_dir(&directory).unwrap();
    let config = root.path().join("node.json");
    let runtime = root.path().join("runtime");
    fs::write(
        &runtime,
        b"#!/bin/sh\nprintf started > \"$3.started\"\nread line\nprintf stopped > \"$3.stopped\"\n",
    )
    .unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let key = SigningKey::from_bytes(&[74; 32]);
    atomic_json(
        &root.path().join("ota.json"),
        &ota::OtaConfig {
            directory: directory.clone(),
            public_keys: vec![STANDARD.encode(key.verifying_key().to_bytes())],
            allow_http: true,
        },
    )
    .unwrap();
    atomic_json(
        &directory.join("state.json"),
        &ota::State {
            current: ota::Image {
                version: "current".into(),
                path: runtime.to_string_lossy().into(),
            },
            phase: "idle".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (observed, mut accepted) = tokio::sync::oneshot::channel();
    let (release, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
            assert!(request.len() <= 8192);
        }
        observed.send(()).unwrap();
        ready.await.unwrap();
        let _ = stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\ncandidate",
            )
            .await;
    });
    let (os, arch) = platform();
    let manifest = serde_json::to_string(&ota::Manifest {
        version: "candidate".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![ota::Asset::from_bytes(os, arch, b"candidate")],
    })
    .unwrap();
    let mut request = ota::Request {
        id: "request-save".into(),
        generation: 1,
        action: "apply".into(),
        signature: STANDARD.encode(key.sign(manifest.as_bytes()).to_bytes()),
        manifest,
        url: format!("http://{address}/binary"),
    };
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let running_config = config.clone();
    let mut supervisor =
        tokio::spawn(async move { ota::supervise(&running_config, "test", token).await });
    let outcome = std::panic::AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !config.with_extension("json.started").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        if cancel_request {
            ota::submit(&directory, &request).unwrap();
            tokio::time::timeout(Duration::from_secs(5), &mut accepted)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(ota::snapshot(&directory).unwrap().phase, "downloading");
            assert!(directory.join("active-download.json").exists());
            request.action = "cancel".into();
        }
        let saved = root.path().join("saved-record.json");
        fs::rename(directory.join(fault), &saved).unwrap();
        fs::create_dir(directory.join(fault)).unwrap();
        ota::submit(&directory, &request).unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            !supervisor.is_finished(),
            "request persistence failure stopped supervisor"
        );
        assert!(!config.with_extension("json.stopped").exists());
        if fault == "state.json" {
            let retained: ota::Request =
                serde_json::from_slice(&fs::read(directory.join("request.json")).unwrap()).unwrap();
            assert_eq!(retained.id, request.id);
            assert_eq!(retained.action, request.action);
            let durable: ota::State = serde_json::from_slice(&fs::read(&saved).unwrap()).unwrap();
            assert_eq!(
                durable.phase,
                if cancel_request {
                    "downloading"
                } else {
                    "idle"
                }
            );
        }
        if !cancel_request {
            assert!(matches!(
                accepted.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
        }
        if newer_request {
            // 已接受请求清理受阻时，新邮箱不能被旧请求消费。
            request.id = "newer-cancel".into();
            request.generation = 2;
            ota::submit(&directory, &request).unwrap();
        }
        fs::remove_dir(directory.join(fault)).unwrap();
        fs::rename(&saved, directory.join(fault)).unwrap();
        if !cancel_request {
            tokio::time::timeout(Duration::from_secs(5), &mut accepted)
                .await
                .unwrap()
                .unwrap();
        }
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let state = ota::snapshot(&directory).unwrap();
                if state.phase
                    == if cancel_request {
                        "canceled"
                    } else {
                        "draining"
                    }
                    && state.generation == request.generation
                    && !directory.join("active-download.json").exists()
                    && !directory.join("request.json").exists()
                {
                    assert_eq!(state.current.path, runtime.to_string_lossy());
                    assert_eq!(state.update_id, request.id);
                    if cancel_request {
                        assert!(state.candidate.path.is_empty());
                    } else {
                        assert_eq!(fs::read(&state.candidate.path).unwrap(), b"candidate");
                    }
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    })
    .catch_unwind()
    .await;
    cancel.cancel();
    let stopped = tokio::time::timeout(Duration::from_secs(10), &mut supervisor).await;
    if outcome.is_err() {
        server.abort();
        let _ = server.await;
    } else {
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }
    assert!(stopped.is_ok(), "supervisor cleanup timed out");
    outcome.unwrap();
    stopped.unwrap().unwrap().unwrap();
    assert!(config.with_extension("json.stopped").exists());
}
