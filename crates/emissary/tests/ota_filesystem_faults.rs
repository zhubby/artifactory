//! [INPUT]: 父驱动创建的私有HFS+/APFS/ext4镜像卷、签名资产和回环HTTP。
//! [OUTPUT]: 验证磁盘故障保留原资产；可选原队列重开并由真实事务确认。
//! [POS]: Unix普通进程磁盘故障门禁；不安装服务或占满宿主磁盘。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
#[path = "support/filesystem_sources.rs"]
mod filesystem_sources;
#[path = "support/filesystem_supervisor.rs"]
mod filesystem_supervisor;
#[path = "native_service/queues.rs"]
mod queues;
mod support;
#[path = "support/telemetry_gate.rs"]
mod telemetry_gate;
#[path = "support/telemetry_sources.rs"]
mod telemetry_sources;
use artifactory_emissary::{config::atomic_json, ota, platform};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

fn roots() -> (PathBuf, PathBuf, String) {
    let root = PathBuf::from(
        std::env::var_os("ARTIFACTORY_OTA_FAULT_DIR").expect("private parent directory required"),
    );
    assert_eq!(
        root.parent().unwrap().canonicalize().unwrap(),
        Path::new("/tmp").canonicalize().unwrap()
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("emissary-ota-fs-")
    );
    let mount = root.join("mount");
    let nonce = fs::read_to_string(root.join("nonce")).unwrap();
    assert_eq!(fs::read_to_string(mount.join("owner")).unwrap(), nonce);
    let total = fs2::total_space(&mount).unwrap();
    assert!(
        (100 << 20..=200 << 20).contains(&total),
        "unexpected filesystem size"
    );
    use std::os::unix::fs::MetadataExt;
    assert_ne!(
        fs::metadata(&mount).unwrap().dev(),
        fs::metadata(&root).unwrap().dev(),
        "fixture is not a separate filesystem"
    );
    (root, mount, nonce)
}

fn asset(directory: &Path, bytes: &[u8]) -> ota::Image {
    let (os, arch) = platform();
    let asset = ota::Asset::from_bytes(os, arch, bytes);
    let target = directory
        .join("versions")
        .join(asset.sha256)
        .join(ota::binary_name());
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, bytes).unwrap();
    ota::Image {
        version: "known-good".into(),
        path: target.to_string_lossy().into(),
    }
}

fn request(body: &[u8], url: String) -> (ota::Request, Vec<String>, ota::Asset) {
    let key = SigningKey::from_bytes(&[29; 32]);
    let (os, arch) = platform();
    let asset = ota::Asset::from_bytes(os, arch, body);
    let manifest = serde_json::to_string(&ota::Manifest {
        version: "v-fs-fault".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![asset.clone()],
    })
    .unwrap();
    (
        ota::Request {
            id: "filesystem-fault".into(),
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

fn fill_to(mount: &Path, remaining: u64) {
    let mut file = fs::File::create(mount.join("pressure")).unwrap();
    let block = vec![0xa5; 1 << 20];
    let mut written = 0;
    loop {
        let free = fs2::available_space(mount).unwrap();
        if free <= remaining + 4096 {
            break;
        }
        let count = (free - remaining).min(block.len() as u64) as usize;
        if let Err(error) = file.write_all(&block[..count]) {
            assert!(
                caused_by_enospc(&error, 16),
                "pressure write failed: {error}"
            );
            eprintln!(
                "fixture pressure reached ENOSPC with {} bytes available",
                fs2::available_space(mount).unwrap()
            );
            break;
        }
        written += count;
        assert!(written <= 200 << 20, "fixture fill exceeded owned volume");
    }
    if let Err(error) = file.sync_all() {
        assert!(
            caused_by_enospc(&error, 16),
            "pressure sync failed: {error}"
        );
    }
}

fn caused_by_enospc(error: &(dyn std::error::Error + 'static), depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        // tempfile保留ErrorKind，但其路径包装不暴露内层raw_os_error。
        if error.raw_os_error() == Some(28) || error.kind() == std::io::ErrorKind::StorageFull {
            return true;
        }
        if error
            .get_ref()
            .is_some_and(|inner| caused_by_enospc(inner, depth - 1))
        {
            return true;
        }
    }
    error
        .source()
        .is_some_and(|inner| caused_by_enospc(inner, depth - 1))
}

fn retained(directory: &Path, state: &[u8], images: &[ota::Image]) {
    assert_eq!(fs::read(directory.join("state.json")).unwrap(), state);
    for (index, image) in images.iter().enumerate() {
        assert_eq!(
            fs::read(&image.path).unwrap(),
            format!("known-{index}").as_bytes()
        );
    }
    assert_eq!(
        fs::read(directory.join("telemetry-source")).unwrap(),
        b"unacknowledged source"
    );
}

#[tokio::test]
#[ignore = "requires parent-owned small mounted filesystem; explicit driver only"]
async fn actual_filesystem_faults_preserve_original_assets() {
    let result = tokio::time::timeout(Duration::from_secs(300), exercise()).await;
    result.expect("filesystem fault acceptance timed out");
}

async fn exercise() {
    let case = std::env::var("ARTIFACTORY_OTA_FAULT_CASE").unwrap();
    if matches!(case.as_str(), "reserve_database" | "cleanup_database") {
        database_action(&case).await;
        return;
    }
    let (root, mount, nonce) = roots();
    if case == "supervisor_enospc" {
        let spool = std::env::var_os("ARTIFACTORY_OTA_FAULT_DATABASE").is_some();
        let source = spool.then(|| filesystem_sources::retained(&root, &mount, "all").unwrap());
        let proof = filesystem_supervisor::exercise(&root, &mount).await;
        if let Some(original) = &source {
            assert_eq!(
                *original,
                filesystem_sources::retained(&root, &mount, "all").unwrap()
            );
        }
        atomic_json(
            &root.join("supervisor_enospc.json"),
            &json!({"passed":true,"nonce":nonce,"case":case,"supervisor":proof,
                "real_source_records_preserved":source.is_some(),"queue_records":source,
                "native_service_executed":false}),
        )
        .unwrap();
        return;
    }
    if case == "seed_spool" {
        filesystem_sources::prepare(&root, &mount, "all")
            .await
            .unwrap();
        let records = filesystem_sources::retained(&root, &mount, "all").unwrap();
        atomic_json(
            &root.join("seed_spool.json"),
            &json!({"passed":true,"nonce":nonce,"queue_records":records}),
        )
        .unwrap();
        return;
    }
    if let Some(original_case) = case.strip_prefix("recover_") {
        let name = std::env::var("ARTIFACTORY_OTA_FAULT_DATABASE").unwrap();
        let run_root = root.clone();
        let run_mount = mount.clone();
        let original_case = original_case.to_owned();
        let proof =
            durable_receiver::isolated_named(Duration::from_secs(270), &name, |url| async move {
                filesystem_sources::recover(&run_root, &run_mount, &original_case, &url).await
            })
            .await
            .unwrap();
        atomic_json(
            &root.join(format!("{case}.json")),
            &json!({"passed":true,"nonce":nonce,"recovery":proof}),
        )
        .unwrap();
        return;
    }
    let spool = std::env::var_os("ARTIFACTORY_OTA_FAULT_DATABASE").is_some();
    let directory = mount.join("ota");
    if case == "prepare_readonly" {
        fs::create_dir_all(&directory).unwrap();
        let images = (0..3)
            .map(|i| asset(&directory, format!("known-{i}").as_bytes()))
            .collect::<Vec<_>>();
        atomic_json(
            &directory.join("state.json"),
            &ota::State {
                current: images[0].clone(),
                previous: images[1].clone(),
                candidate: images[2].clone(),
                ..Default::default()
            },
        )
        .unwrap();
        fs::write(directory.join("telemetry-source"), b"unacknowledged source").unwrap();
        fs::write(directory.join("storage.lock"), b"").unwrap();
        atomic_json(
            &root.join("prepare_readonly.json"),
            &json!({"passed":true,"nonce":nonce}),
        )
        .unwrap();
        return;
    }
    if case != "readonly" {
        fs::create_dir_all(&directory).unwrap();
        let images = (0..3)
            .map(|i| asset(&directory, format!("known-{i}").as_bytes()))
            .collect::<Vec<_>>();
        atomic_json(
            &directory.join("state.json"),
            &ota::State {
                current: images[0].clone(),
                previous: images[1].clone(),
                candidate: images[2].clone(),
                ..Default::default()
            },
        )
        .unwrap();
        fs::write(directory.join("telemetry-source"), b"unacknowledged source").unwrap();
    }
    let original = fs::read(directory.join("state.json")).unwrap();
    let state: ota::State = serde_json::from_slice(&original).unwrap();
    let images = vec![state.current, state.previous, state.candidate];
    // 本机APFS在ENOSPC时仍报告约4.5MiB；资产必须超过该余量。
    let body = vec![0x3b; 16 << 20];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request, keys, target) = request(&body, format!("http://{address}/binary"));
    let cfg = ota::OtaConfig {
        directory: directory.clone(),
        public_keys: keys,
        allow_http: true,
    };
    let mut free_before = fs2::available_space(&directory).unwrap();
    let mut free_after = free_before;
    let mut actual_enospc = false;
    let mut terminal_progress_persisted = None;
    if case == "low_space" {
        fill_to(&mount, 32 << 20);
        free_before = fs2::available_space(&directory).unwrap();
        free_after = free_before;
        assert!(free_before < ota::required_space(target.size, 0).unwrap());
        let error = ota::download(&cfg, &request, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("insufficient_space"),
            "{error:#}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "low space issued HTTP request"
        );
        assert_eq!(
            ota::progress_snapshot(&directory)
                .unwrap()
                .unwrap()
                .error_code
                .as_deref(),
            Some("insufficient_space")
        );
    } else if case == "after_precheck" {
        assert!(free_before > ota::required_space(target.size, 0).unwrap());
        let sent = body.clone();
        let (resume, ready) = tokio::sync::oneshot::channel();
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
                        sent.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(&sent[..1]).await.unwrap();
            ready.await.unwrap();
            let _ = stream.write_all(&sent[1..]).await;
        });
        let running_cfg = cfg.clone();
        let running_request = request.clone();
        let download = tokio::spawn(async move {
            ota::download(&running_cfg, &running_request, &CancellationToken::new()).await
        });
        let asset_dir = directory.join("versions").join(&target.sha256);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if asset_dir.is_dir()
                    && fs::read_dir(&asset_dir).unwrap().any(|entry| {
                        let p = entry.unwrap().path();
                        p.extension().is_some_and(|v| v == "part")
                            && fs::metadata(p).unwrap().len() == 1
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        // 首字节落盘证明两次真实空间预检均已通过。
        // 压力文件预留512KiB元数据空间；实际ENOSPC也终止填充。
        fill_to(&mount, 512 << 10);
        free_after = fs2::available_space(&directory).unwrap();
        assert!(free_after < target.size);
        resume.send(()).unwrap();
        let error = download.await.unwrap().unwrap_err();
        assert!(
            error.to_string().contains("release write failed"),
            "{error:#}"
        );
        actual_enospc = caused_by_enospc(error.as_ref(), 16);
        assert!(actual_enospc, "write failed without ENOSPC: {error:#}");
        server.await.unwrap();
        let progress = ota::progress_snapshot(&directory).unwrap().unwrap();
        terminal_progress_persisted =
            Some(progress.error_code.as_deref() == Some("download_failed"));
        if terminal_progress_persisted == Some(false) {
            // 满盘也会阻止失败进度原子写入；旧进度不能声称已校验或成功。
            assert_eq!(progress.error_code, None);
            assert_eq!(progress.subphase, "download");
            assert!(progress.bytes_done < progress.bytes_total);
        }
    } else {
        assert_eq!(case, "readonly");
        let probe = fs::File::create(directory.join("readonly-probe")).unwrap_err();
        assert_eq!(probe.raw_os_error(), Some(30), "volume is not read-only");
        assert!(
            ota::download(&cfg, &request, &CancellationToken::new())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "read-only volume issued HTTP request"
        );
    }
    retained(&directory, &original, &images);
    let source_proof = spool.then(|| filesystem_sources::retained(&root, &mount, "all").unwrap());
    let target_path = directory
        .join("versions")
        .join(&target.sha256)
        .join(ota::binary_name());
    assert!(!target_path.exists());
    if mount.join("pressure").exists() {
        fs::remove_file(mount.join("pressure")).unwrap();
    }
    let parts = directory.join("versions").join(&target.sha256);
    if parts.exists() {
        assert!(fs::read_dir(parts).unwrap().all(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_none_or(|v| v != "part")
        }));
    }
    atomic_json(
        &root.join(format!("{case}.json")),
        &json!({
            "passed":true,"nonce":nonce,"case":case,"actual_mounted_filesystem":true,
            "initial_available_bytes":free_before,"after_pressure_available_bytes":free_after,
            "actual_enospc":actual_enospc,"state_and_protected_assets_preserved":true,
            "terminal_progress_persisted":terminal_progress_persisted,
            "unconfirmed_data_sentinel_preserved":true,"target_not_installed":true,
            "real_source_records_preserved":source_proof.is_some(),"queue_records":source_proof,
            "partial_files_removed":true,"native_service_executed":false
        }),
    )
    .unwrap();
}

async fn database_action(case: &str) {
    let root = PathBuf::from(std::env::var_os("ARTIFACTORY_OTA_FAULT_DIR").unwrap());
    assert_eq!(
        root.parent().unwrap().canonicalize().unwrap(),
        Path::new("/tmp").canonicalize().unwrap()
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("emissary-ota-fs-")
    );
    let nonce = fs::read_to_string(root.join("nonce")).unwrap();
    let name = std::env::var("ARTIFACTORY_OTA_FAULT_DATABASE").unwrap();
    if case == "reserve_database" {
        durable_receiver::assert_absent(&name).await.unwrap();
        atomic_json(
            &root.join("reserved.json"),
            &json!({"name":name,"nonce":nonce,"absent_before_run":true}),
        )
        .unwrap();
    } else {
        let reserved: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("reserved.json")).unwrap()).unwrap();
        assert_eq!(
            reserved,
            json!({"name":name,"nonce":nonce,"absent_before_run":true})
        );
        durable_receiver::cleanup_owned(&name).await.unwrap();
    }
    atomic_json(
        &root.join(format!("{case}.json")),
        &json!({"passed":true,"nonce":nonce,"name":name}),
    )
    .unwrap();
}
