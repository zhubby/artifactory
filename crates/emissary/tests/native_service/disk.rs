//! [INPUT]: 父驱动自有ext4挂载、仍运行的原生服务和暂拒上传的接收端。
//! [OUTPUT]: 填满私有卷，验证同进程及会话保持、真实采集失败与释放后恢复。
//! [POS]: 原生持久补报前的磁盘故障阶段，不创建服务或额外发送器。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Machine, command, support};
use anyhow::{Context, Result, ensure};
use artifactory_emissary::{config::Config, ota};
use artifactory_emissary_protocol::telemetry::Batch;
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, path::PathBuf, process::Command, time::Duration};

pub(super) fn mount() -> Result<PathBuf> {
    ensure!(
        cfg!(target_os = "linux"),
        "native disk fault requires Linux"
    );
    let path = PathBuf::from(
        std::env::var_os("ARTIFACTORY_NATIVE_FAULT_MOUNT").context("private mount required")?,
    )
    .canonicalize()?;
    let nonce = std::env::var("ARTIFACTORY_NATIVE_FAULT_NONCE")?;
    ensure!(
        nonce.len() == 32 && nonce.bytes().all(|c| c.is_ascii_hexdigit()),
        "private mount nonce invalid"
    );
    ensure!(
        fs::read_to_string(path.join("owner"))? == nonce,
        "private mount owner differs"
    );
    let mut cmd = Command::new("findmnt");
    cmd.args(["--json", "--mountpoint"])
        .arg(&path)
        .args(["--output", "SOURCE,FSTYPE,TARGET"]);
    let output = command::run(cmd, Duration::from_secs(10))?;
    ensure!(output.status.success(), "private mount lookup failed");
    let data: Value = serde_json::from_slice(&output.stdout)?;
    let entries = data["filesystems"]
        .as_array()
        .context("mount inventory missing")?;
    ensure!(entries.len() == 1, "mount inventory ambiguous");
    let entry = &entries[0];
    let source = entry["source"].as_str().context("mount source missing")?;
    ensure!(
        source
            .strip_prefix("/dev/loop")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
            && entry["fstype"] == "ext4"
            && entry["target"].as_str() == path.to_str(),
        "native disk target must be the private ext4 loop mount"
    );
    ensure!(
        fs2::total_space(&path)? <= 1 << 30,
        "private fault volume exceeds 1GiB"
    );
    Ok(path)
}

pub(super) struct Resumed {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub batch: Batch,
    pub proof: Value,
    pub released: chrono::DateTime<chrono::Utc>,
}

pub(super) async fn exercise(
    machine: &Machine,
    cfg: &Config,
    connection: &mut support::Connection,
    retained: impl Fn() -> Result<()>,
) -> Result<Resumed> {
    let mount = mount()?;
    ensure!(
        machine.directory.starts_with(&mount),
        "service is outside fault volume"
    );
    let pid = machine.running().await;
    let health_path = cfg.root.join("ota/health.json");
    let health: ota::Health = machine.read_json(&health_path);
    let capabilities: ota::LauncherCapabilities =
        machine.read_json(&cfg.root.join("ota/capabilities.json"));
    let identity: Vec<_> = [
        &machine.config,
        &cfg.ca_file,
        &cfg.certificate_file,
        &cfg.key_file,
    ]
    .into_iter()
    .map(|p| (p.clone(), machine.read_bytes(p), machine.permissions(p)))
    .collect();
    let service = machine.service_contract();
    // 压力文件属于测试账户，始终留在父驱动已验证的私有卷。
    let pressure = mount.join(format!("native-pressure-{}", uuid::Uuid::new_v4().simple()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pressure)?;
    use std::io::Write;
    let mut attempts = 0;
    let mut one_byte_enospc = false;
    for size in [1 << 20, 4096, 1] {
        let block = vec![0x74; size];
        loop {
            attempts += 1;
            ensure!(
                attempts <= 2048,
                "native pressure exceeded private volume budget"
            );
            match file.write_all(&block) {
                Ok(()) => {}
                Err(error) => {
                    ensure!(
                        error.raw_os_error() == Some(28),
                        "native pressure failed without ENOSPC"
                    );
                    one_byte_enospc |= size == 1;
                    break;
                }
            }
        }
    }
    drop(file);
    ensure!(
        one_byte_enospc,
        "native pressure did not reach one-byte ENOSPC"
    );
    let free = fs2::available_space(&mount)?;
    let observed = tokio::time::timeout(Duration::from_secs(50), async {
        loop {
            let event = connection.event("heartbeat").await;
            let payload: Value = serde_json::from_str(&event.payload_json)?;
            let telemetry = &payload["diagnostics"]["metrics"]["telemetry"];
            retained()?;
            ensure!(
                machine.running().await == pid,
                "native service restarted while disk full"
            );
            if telemetry["disk_errors"].as_u64().unwrap_or(0) >= 2 {
                ensure!(
                    telemetry["dropped_batches"].as_u64().unwrap_or(0) >= 2,
                    "native disk failures hid gaps"
                );
                return Ok::<_, anyhow::Error>(telemetry["disk_errors"].as_u64().unwrap());
            }
        }
    })
    .await
    .context("native service did not report repeated collection failures")??;
    retained()?;
    fs::remove_file(&pressure)?;
    let released = chrono::Utc::now();
    let fresh = cfg.root.join("telemetry/fresh/pending.batch");
    let (bytes, batch) = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if machine
                .command(true, "test", &[OsStr::new("-f"), fresh.as_os_str()])
                .status
                .success()
            {
                let bytes = machine.read_bytes(&fresh);
                let batch: Batch = serde_json::from_slice(&bytes)?;
                batch.validate()?;
                if batch.sampled_at >= released && batch.host.is_some() {
                    ensure!(
                        batch.dropped_batches >= 2,
                        "native resumed sample hid disk gaps"
                    );
                    return Ok::<_, anyhow::Error>((bytes, batch));
                }
            }
            retained()?;
            ensure!(
                machine.running().await == pid,
                "native service restarted after space release"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("native service did not resume collection after space release")??;
    let after = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let after: ota::Health = machine.read_json(&health_path);
            ensure!(
                after.session == health.session,
                "native runtime session changed after disk fault"
            );
            if after.time >= released && !after.id.is_empty() {
                break Ok::<_, anyhow::Error>(after);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("native health confirmation did not persist after space release")??;
    let after_capabilities: ota::LauncherCapabilities =
        machine.read_json(&cfg.root.join("ota/capabilities.json"));
    ensure!(
        after.session == health.session,
        "native runtime reconnected or restarted during disk fault"
    );
    ensure!(
        after_capabilities.instance_id == capabilities.instance_id,
        "launcher changed during disk fault"
    );
    ensure!(
        machine.running().await == pid,
        "native launcher PID changed during disk fault"
    );
    ensure!(
        machine.service_contract() == service,
        "native service account or definition changed"
    );
    for (path, bytes, permissions) in identity {
        ensure!(
            machine.read_bytes(&path) == bytes && machine.permissions(&path) == permissions,
            "native identity or permissions changed"
        );
    }
    retained()?;
    Ok(Resumed {
        path: fresh,
        bytes,
        proof: json!({
            "passed":true,"native_service_executed":true,"actual_enospc":true,
            "one_byte_write_enospc":true,"free_bytes_at_failure":free,
            "observed_disk_errors":observed,"repeated_collection_failures_observed":true,
            "service_pid_unchanged":true,"runtime_session_unchanged":true,
            "launcher_instance_unchanged":true,"identity_and_permissions_unchanged":true,
            "health_confirmation_resumed":true,
            "original_sources_retained":true,"same_service_resumed_after_space_release":true,
            "boundary":"Linux systemd service on private ext4; no physical power loss"
        }),
        batch,
        released,
    })
}
