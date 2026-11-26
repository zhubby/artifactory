//! [INPUT]: 真实supervisor、受控运行进程、回环HTTP与状态写入故障。
//! [OUTPUT]: 验证保存/邮箱故障下的进程保全、候选恢复和重连健康计数。
//! [POS]: 固定启动器持久化回归；不安装系统服务或更换设备身份。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(unix)]
use artifactory_emissary::{config::atomic_json, ota, platform};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use futures::FutureExt;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.try_exists().unwrap() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn download_result_write_failure_keeps_current_runtime_and_retries() {
    exercise(false, "state.json").await;
}

#[tokio::test]
async fn verified_candidate_waits_for_durable_state_before_draining() {
    exercise(true, "state.json").await;
}

#[tokio::test]
async fn download_protection_cleanup_failure_keeps_current_runtime_and_retries() {
    exercise(false, "active-download.json").await;
}

#[tokio::test]
async fn activation_write_failure_keeps_current_runtime_until_state_is_durable() {
    activation_fault("state.json", CommitFault::None).await;
}

#[tokio::test]
async fn activation_storage_lock_failure_keeps_current_runtime_and_retries() {
    activation_fault("storage.lock", CommitFault::None).await;
}

#[tokio::test]
async fn health_commit_write_failure_keeps_healthy_candidate_and_retries() {
    activation_fault("state.json", CommitFault::Persist).await;
}

#[tokio::test]
async fn health_commit_lock_failure_keeps_healthy_candidate_and_retries() {
    activation_fault("storage.lock", CommitFault::Persist).await;
}

#[tokio::test]
async fn candidate_exit_during_commit_failure_restores_current_before_saving_terminal() {
    activation_fault("state.json", CommitFault::CandidateExit).await;
}

#[tokio::test]
async fn reconnect_during_health_commit_retry_requires_three_new_session_acks() {
    activation_fault("state.json", CommitFault::Reconnect).await;
}

#[tokio::test]
async fn unavailable_request_lock_does_not_block_healthy_candidate_commit() {
    activation_fault("request.lock", CommitFault::Mailbox).await;
}

#[tokio::test]
async fn candidate_start_failure_restores_current_before_saving_terminal() {
    recovery_fault(false).await;
}

#[tokio::test]
async fn candidate_health_timeout_restores_current_before_saving_terminal() {
    recovery_fault(true).await;
}

async fn recovery_fault(health_timeout: bool) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("ota");
    fs::create_dir(&directory).unwrap();
    let config = root.path().join("node.json");
    let runtime = root.path().join("runtime");
    let saved = root.path().join("saved-state.json");
    let candidate_script = b"#!/bin/sh\nprintf started > \"$3.candidate.started\"\nread line\nprintf stopped > \"$3.candidate.stopped\"\n";
    let (os, arch) = platform();
    let asset = ota::Asset::from_bytes(os, arch, candidate_script);
    let candidate = directory
        .join("versions")
        .join(&asset.sha256)
        .join(ota::binary_name());
    // 原程序收到EOF时，激活意图已保存。仅在首次退出制造启动和保存故障。
    let fault_script = if health_timeout {
        String::new()
    } else {
        format!(
            "if [ ! -e \"$3.stopped\" ]; then\nmv '{}' '{}' || exit 21\nmkdir '{}' || exit 22\nrm '{}' || exit 23\nfi\n",
            directory.join("state.json").display(),
            saved.display(),
            directory.join("state.json").display(),
            candidate.display(),
        )
    };
    fs::write(
        &runtime,
        format!(
            "#!/bin/sh\nprintf started >> \"$3.started\"\nread line\n{fault_script}printf stopped >> \"$3.stopped\"\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let key = SigningKey::from_bytes(&[73; 32]);
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
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
            assert!(request.len() <= 8192);
        }
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    candidate_script.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(candidate_script).await.unwrap();
    });
    let manifest = serde_json::to_string(&ota::Manifest {
        version: "candidate".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![asset],
    })
    .unwrap();
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let running_config = config.clone();
    let mut supervisor =
        tokio::spawn(async move { ota::supervise(&running_config, "test", token).await });
    let outcome = std::panic::AssertUnwindSafe(async {
        wait_file(&config.with_extension("json.started")).await;
        ota::submit(
            &directory,
            &ota::Request {
                id: "recovery-save".into(),
                generation: 1,
                action: "apply".into(),
                signature: STANDARD.encode(key.sign(manifest.as_bytes()).to_bytes()),
                manifest,
                url: format!("http://{address}/binary"),
            },
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while ota::snapshot(&directory).unwrap().phase != "draining" {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(fs::read(&candidate).unwrap(), candidate_script);
        atomic_json(
            &directory.join("drained.json"),
            &serde_json::json!({"id":"recovery-save"}),
        )
        .unwrap();
        if health_timeout {
            wait_file(&config.with_extension("json.candidate.started")).await;
            assert_eq!(ota::snapshot(&directory).unwrap().phase, "activating");
            fs::rename(directory.join("state.json"), &saved).unwrap();
            fs::create_dir(directory.join("state.json")).unwrap();
        }
        tokio::time::timeout(
            Duration::from_secs(if health_timeout { 130 } else { 5 }),
            async {
                while fs::read(config.with_extension("json.started")).unwrap() != b"startedstarted"
                {
                    assert!(
                        !supervisor.is_finished(),
                        "supervisor exited before recovery"
                    );
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            },
        )
        .await
        .unwrap();
        // 原程序必须在保存路径仍不可写时恢复，耐久指向仍保持激活前状态。
        assert!(directory.join("state.json").is_dir());
        let durable: ota::State = serde_json::from_slice(&fs::read(&saved).unwrap()).unwrap();
        assert_eq!(durable.phase, "activating");
        assert_eq!(durable.current.path, runtime.to_string_lossy());
        assert_eq!(durable.candidate.path, candidate.to_string_lossy());
        if health_timeout {
            assert!(
                chrono::Utc::now()
                    .signed_duration_since(durable.started_at)
                    .num_seconds()
                    >= 120
            );
            assert!(config.with_extension("json.candidate.stopped").exists());
        } else {
            assert!(!candidate.exists());
            assert!(!config.with_extension("json.candidate.started").exists());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(1500), &mut supervisor)
                .await
                .is_err(),
            "terminal save failure stopped recovered runtime"
        );
        assert_eq!(
            fs::read(config.with_extension("json.stopped")).unwrap(),
            b"stopped"
        );
        fs::remove_dir(directory.join("state.json")).unwrap();
        fs::rename(&saved, directory.join("state.json")).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while ota::snapshot(&directory).unwrap().phase != "rolled_back" {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let state = ota::snapshot(&directory).unwrap();
        assert_eq!(state.current.path, runtime.to_string_lossy());
        assert!(state.candidate.path.is_empty());
        assert_eq!(
            state.error,
            if health_timeout {
                "candidate health confirmation timed out after 120 seconds"
            } else {
                "candidate failed to start"
            }
        );
    })
    .catch_unwind()
    .await;
    cancel.cancel();
    // 失败断言也先等待监督器回收其确切子进程。
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
    assert_eq!(
        fs::read(config.with_extension("json.stopped")).unwrap(),
        b"stoppedstopped"
    );
}

#[derive(PartialEq, Eq)]
enum CommitFault {
    None,
    Persist,
    CandidateExit,
    Reconnect,
    Mailbox,
}

async fn activation_fault(fault: &str, mode: CommitFault) {
    let health_commit = mode != CommitFault::None;
    let candidate_exit = mode == CommitFault::CandidateExit;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("ota");
    fs::create_dir(&directory).unwrap();
    let config = root.path().join("node.json");
    let runtime = root.path().join("runtime");
    let original_script = b"#!/bin/sh\nprintf started >> \"$3.started\"\nread line\nprintf stopped > \"$3.stopped\"\n";
    let candidate_script = b"#!/bin/sh\nprintf '%s' \"$$\" > \"$3.candidate.pid\"\nprintf started > \"$3.candidate.started\"\nread line\nprintf stopped > \"$3.candidate.stopped\"\n";
    fs::write(&runtime, original_script).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    // 从已耐久保存的draining状态启动会按恢复语义失败，先运行后再提交真实下载。
    let key = SigningKey::from_bytes(&[72; 32]);
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
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
            assert!(request.len() <= 8192);
        }
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    candidate_script.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(candidate_script).await.unwrap();
    });
    let (os, arch) = platform();
    let manifest = serde_json::to_string(&ota::Manifest {
        version: "candidate".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![ota::Asset::from_bytes(os, arch, candidate_script)],
    })
    .unwrap();
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let running_config = config.clone();
    let mut supervisor =
        tokio::spawn(async move { ota::supervise(&running_config, "test", token).await });
    let outcome = std::panic::AssertUnwindSafe(async {
        wait_file(&config.with_extension("json.started")).await;
        ota::submit(
            &directory,
            &ota::Request {
                id: "activation-save".into(),
                generation: 1,
                action: "apply".into(),
                signature: STANDARD.encode(key.sign(manifest.as_bytes()).to_bytes()),
                manifest,
                url: format!("http://{address}/binary"),
            },
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while ota::snapshot(&directory).unwrap().phase != "draining" {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let saved = root.path().join("saved-state.json");
        if !health_commit {
            fs::rename(directory.join(fault), &saved).unwrap();
            fs::create_dir(directory.join(fault)).unwrap();
        }
        atomic_json(
            &directory.join("drained.json"),
            &serde_json::json!({"id":"activation-save"}),
        )
        .unwrap();
        if !health_commit {
            assert!(
                tokio::time::timeout(Duration::from_millis(1500), &mut supervisor)
                    .await
                    .is_err(),
                "activation state save failure stopped supervisor"
            );
            assert!(
                !config.with_extension("json.stopped").exists(),
                "current runtime stopped before durable activation intent"
            );
            assert!(!config.with_extension("json.candidate.started").exists());
            fs::remove_dir(directory.join(fault)).unwrap();
            fs::rename(&saved, directory.join(fault)).unwrap();
        }
        wait_file(&config.with_extension("json.candidate.started")).await;
        assert_eq!(ota::snapshot(&directory).unwrap().phase, "activating");
        assert!(config.with_extension("json.stopped").exists());
        if health_commit {
            if mode == CommitFault::Mailbox {
                ota::submit(
                    &directory,
                    &ota::Request {
                        id: "activation-save".into(),
                        generation: 1,
                        action: "apply".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
            fs::rename(directory.join(fault), &saved).unwrap();
            fs::create_dir(directory.join(fault)).unwrap();
        }
        for id in ["one", "two", "three"] {
            ota::confirm(&directory, "candidate-session", id).unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if mode == CommitFault::Mailbox {
            tokio::time::timeout(Duration::from_secs(5), async {
                while ota::snapshot(&directory).unwrap().phase != "succeeded" {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("unrelated mailbox lock prevented healthy candidate commit");
            assert!(directory.join(fault).is_dir());
            fs::remove_dir(directory.join(fault)).unwrap();
            fs::rename(&saved, directory.join(fault)).unwrap();
        } else if health_commit {
            assert!(
                tokio::time::timeout(Duration::from_millis(1500), &mut supervisor)
                    .await
                    .is_err(),
                "health commit persistence failure stopped healthy candidate"
            );
            assert!(!config.with_extension("json.candidate.stopped").exists());
            let durable: ota::State = serde_json::from_slice(
                &fs::read(if fault == "state.json" {
                    saved.clone()
                } else {
                    directory.join("state.json")
                })
                .unwrap(),
            )
            .unwrap();
            assert_eq!(durable.phase, "activating");
            assert_eq!(durable.current.path, runtime.to_string_lossy());
            assert!(!durable.candidate.path.is_empty());
            if candidate_exit {
                let pid = fs::read_to_string(config.with_extension("json.candidate.pid")).unwrap();
                assert!(
                    std::process::Command::new("/bin/kill")
                        .args(["-KILL", pid.trim()])
                        .status()
                        .unwrap()
                        .success()
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(1500), &mut supervisor)
                        .await
                        .is_err(),
                    "terminal save failure prevented original runtime recovery"
                );
                tokio::time::timeout(Duration::from_secs(5), async {
                    while fs::read(config.with_extension("json.started")).unwrap()
                        != b"startedstarted"
                    {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap();
            }
            // 已取得的三次确认不能因磁盘恢复较慢而丢失。
            atomic_json(
                &directory.join("health.json"),
                &ota::Health {
                    session: "candidate-session".into(),
                    id: "three".into(),
                    time: chrono::Utc::now() - chrono::Duration::seconds(26),
                },
            )
            .unwrap();
            if mode == CommitFault::Reconnect {
                ota::confirm(&directory, "reconnected-session", "new-one").unwrap();
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            fs::remove_dir(directory.join(fault)).unwrap();
            fs::rename(saved, directory.join(fault)).unwrap();
            if mode == CommitFault::Reconnect {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                assert_eq!(
                    ota::snapshot(&directory).unwrap().phase,
                    "activating",
                    "reconnected session reused previous session health confirmation"
                );
                for id in ["new-two", "new-three"] {
                    ota::confirm(&directory, "reconnected-session", id).unwrap();
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            }
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while ota::snapshot(&directory).unwrap().phase
                != if candidate_exit {
                    "rolled_back"
                } else {
                    "succeeded"
                }
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let state = ota::snapshot(&directory).unwrap();
        if candidate_exit {
            assert_eq!(state.current.path, runtime.to_string_lossy());
            assert!(state.candidate.path.is_empty());
            assert!(state.error.contains("candidate exited"));
        } else {
            assert_eq!(fs::read(&state.current.path).unwrap(), candidate_script);
            assert_eq!(state.previous.path, runtime.to_string_lossy());
        }
    })
    .catch_unwind()
    .await;
    cancel.cancel();
    if !supervisor.is_finished() {
        tokio::time::timeout(Duration::from_secs(10), &mut supervisor)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    outcome.unwrap();
    wait_file(&config.with_extension(if candidate_exit {
        "json.stopped"
    } else {
        "json.candidate.stopped"
    }))
    .await;
}

async fn exercise(success: bool, fault: &str) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("ota");
    fs::create_dir(&directory).unwrap();
    let config = root.path().join("node.json");
    let runtime = root.path().join("runtime");
    fs::write(
        &runtime,
        b"#!/bin/sh\nprintf started > \"$3.started\"\ncat > /dev/null\nprintf stopped > \"$3.stopped\"\n",
    )
    .unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let original = ota::Image {
        version: "current".into(),
        path: runtime.to_string_lossy().into(),
    };
    let key = SigningKey::from_bytes(&[71; 32]);
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
            current: original.clone(),
            phase: "idle".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (observed, accepted) = tokio::sync::oneshot::channel();
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
        let response: &[u8] = if success {
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\ncandidate"
        } else {
            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        };
        stream.write_all(response).await.unwrap();
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
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let running_config = config.clone();
    let mut supervisor =
        tokio::spawn(async move { ota::supervise(&running_config, "test", token).await });
    let result = async {
        wait_file(&config.with_extension("json.started")).await;
        ota::submit(
            &directory,
            &ota::Request {
                id: "failed-download".into(),
                generation: 1,
                action: "apply".into(),
                signature: STANDARD.encode(key.sign(manifest.as_bytes()).to_bytes()),
                manifest,
                url: format!("http://{address}/binary"),
            },
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), accepted)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ota::snapshot(&directory).unwrap().phase, "downloading");
        let saved = root.path().join("saved-record.json");
        fs::rename(directory.join(fault), &saved).unwrap();
        fs::create_dir(directory.join(fault)).unwrap();
        // 提前放置真实排空确认，验证保存失败仍阻止候选激活。
        if success {
            atomic_json(
                &directory.join("drained.json"),
                &serde_json::json!({"id":"failed-download"}),
            )
            .unwrap();
        }
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if ota::progress_snapshot(&directory)
                    .unwrap()
                    .is_some_and(|p| {
                        if success {
                            p.subphase == "verified"
                        } else {
                            p.error_code.as_deref() == Some("download_failed")
                        }
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut supervisor)
                .await
                .is_err(),
            "terminal state write failure stopped supervisor/current runtime"
        );
        assert!(!config.with_extension("json.stopped").exists());
        assert!(directory.join("active-download.json").exists());
        if success {
            fs::remove_file(directory.join("drained.json")).unwrap();
        }
        fs::remove_dir(directory.join(fault)).unwrap();
        fs::rename(saved, directory.join(fault)).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let state = ota::snapshot(&directory).unwrap();
                if state.phase == if success { "draining" } else { "failed" }
                    && !directory.join("active-download.json").exists()
                {
                    assert_eq!(state.current.path, original.path);
                    if success {
                        assert_eq!(fs::read(&state.candidate.path).unwrap(), b"candidate");
                        assert!(state.error.is_empty());
                    } else {
                        assert!(state.candidate.path.is_empty());
                        assert!(state.error.contains("HTTP 400"));
                    }
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    };
    // 失败断言也先关闭运行进程，防止测试留下子进程。
    let outcome = std::panic::AssertUnwindSafe(result).catch_unwind().await;
    cancel.cancel();
    if !supervisor.is_finished() {
        tokio::time::timeout(Duration::from_secs(10), &mut supervisor)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    if outcome.is_err() {
        server.abort();
        let _ = server.await;
    } else {
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
    }
    outcome.unwrap();
    wait_file(&config.with_extension("json.stopped")).await;
}
