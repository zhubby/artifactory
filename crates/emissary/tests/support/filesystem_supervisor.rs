//! [INPUT]: 父驱动私有小卷、真实supervisor与首字节后暂停的HTTP。
//! [OUTPUT]: 验证真实满盘下载失败保持当前进程，并读回终态保存时机。
//! [POS]: 文件系统故障门禁的启动器组合；不安装系统服务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::{config::atomic_json, ota};
use futures::FutureExt;
use serde_json::{Value, json};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

async fn wait_for(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

pub async fn exercise(root: &Path, mount: &Path) -> Value {
    let directory = mount.join("supervisor-ota");
    fs::create_dir(&directory).unwrap();
    let config_root = root.join("supervisor-config");
    fs::create_dir(&config_root).unwrap();
    let config = config_root.join("node.json");
    let runtime = directory.join("runtime");
    let runtime_bytes = b"#!/bin/sh\nprintf started > \"$3.started\"\ncat > /dev/null\nprintf stopped > \"$3.stopped\"\n";
    fs::write(&runtime, runtime_bytes).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let body = vec![0x62; 16 << 20];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request, keys, asset) = super::request(&body, format!("http://{address}/binary"));
    atomic_json(
        &config_root.join("ota.json"),
        &ota::OtaConfig {
            directory: directory.clone(),
            public_keys: keys,
            allow_http: true,
        },
    )
    .unwrap();
    atomic_json(
        &directory.join("state.json"),
        &ota::State {
            phase: "idle".into(),
            current: ota::Image {
                version: "current".into(),
                path: runtime.to_string_lossy().into(),
            },
            ..Default::default()
        },
    )
    .unwrap();
    fs::write(directory.join("unconfirmed-source"), b"original bytes").unwrap();
    let (release, ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(stream.read_u8().await.unwrap());
            assert!(headers.len() <= 8192);
        }
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(&body[..1]).await.unwrap();
        ready.await.unwrap();
        let _ = stream.write_all(&body[1..]).await;
    });
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let running_config = config.clone();
    let mut supervisor =
        tokio::spawn(async move { ota::supervise(&running_config, "test", token).await });
    let result = async {
        wait_for(|| config.with_extension("json.started").exists()).await;
        ota::submit(&directory, &request).unwrap();
        let asset_dir = directory.join("versions").join(&asset.sha256);
        let part = || {
            fs::read_dir(&asset_dir).ok().and_then(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .find(|p| {
                        p.extension().is_some_and(|v| v == "part")
                            && fs::metadata(p).is_ok_and(|v| v.len() == 1)
                    })
            })
        };
        wait_for(|| part().is_some()).await;
        let partial = part().unwrap();
        // 保留已分配块，避免下载器删除临时文件后恰好腾出状态保存空间。
        let retained_part = directory.join("retained-part");
        fs::hard_link(&partial, &retained_part).unwrap();
        let original = fs::read(directory.join("state.json")).unwrap();
        assert_eq!(ota::snapshot(&directory).unwrap().phase, "downloading");
        let pressure = mount.join("supervisor-pressure");
        let mut file = fs::File::create(&pressure).unwrap();
        let mut writes = 0;
        let mut one_byte_enospc = false;
        for size in [1 << 20, 4096, 1] {
            let block = vec![0x47; size];
            loop {
                writes += 1;
                assert!(writes <= 1024, "pressure exceeded private volume");
                if let Err(error) = file.write_all(&block) {
                    assert!(super::caused_by_enospc(&error, 16), "{error}");
                    one_byte_enospc |= size == 1;
                    break;
                }
            }
        }
        drop(file);
        assert!(one_byte_enospc);
        let state = ota::snapshot(&directory).unwrap();
        let state_write_enospc = match atomic_json(&directory.join("state.json"), &state) {
            Ok(()) => false,
            Err(error) => {
                assert!(
                    error
                        .chain()
                        .any(|cause| super::caused_by_enospc(cause, 16)),
                    "state write failed without ENOSPC: {error:#}"
                );
                true
            }
        };
        let free = fs2::available_space(mount).unwrap();
        release.send(()).unwrap();
        // 实际下载临时文件删除，证明写入失败已离开下载器。
        wait_for(|| !partial.exists()).await;
        assert!(
            tokio::time::timeout(Duration::from_secs(3), &mut supervisor)
                .await
                .is_err(),
            "ENOSPC stopped supervisor/current runtime"
        );
        assert!(!config.with_extension("json.stopped").exists());
        let full_state = ota::snapshot(&directory).unwrap();
        let terminal_saved_while_full = full_state.phase == "failed";
        if !terminal_saved_while_full {
            assert_eq!(fs::read(directory.join("state.json")).unwrap(), original);
            assert!(directory.join("active-download.json").is_file());
        }
        assert!(!asset_dir.join(ota::binary_name()).exists());
        assert_eq!(fs::read(&runtime).unwrap(), runtime_bytes);
        assert_eq!(
            fs::read(directory.join("unconfirmed-source")).unwrap(),
            b"original bytes"
        );
        fs::remove_file(pressure).unwrap();
        fs::remove_file(retained_part).unwrap();
        wait_for(|| {
            ota::snapshot(&directory).is_ok_and(|state| {
                state.phase == "failed" && !directory.join("active-download.json").exists()
            })
        })
        .await;
        let state = ota::snapshot(&directory).unwrap();
        assert!(
            state.error.contains("release write failed"),
            "{}",
            state.error
        );
        assert_eq!(state.current.path, runtime.to_string_lossy());
        assert!(state.candidate.path.is_empty());
        assert!(!config.with_extension("json.stopped").exists());
        json!({"one_byte_write_enospc":true,"state_write_probe_enospc":state_write_enospc,
            "terminal_saved_while_full":terminal_saved_while_full,
            "free_bytes_at_failure":free,"real_supervisor":true,
            "download_failed_after_first_persisted_byte":true,
            "current_runtime_kept_running":true,"pending_state_protection_checked":true,
            "failed_state_saved_after_space_release":true,
            "protection_released_after_recovery":true,"target_not_installed":true,
            "original_asset_and_source_preserved":true})
    };
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
    let mut proof = outcome.unwrap();
    wait_for(|| config.with_extension("json.stopped").exists()).await;
    proof["normal_stop_confirmed"] = json!(true);
    proof
}
