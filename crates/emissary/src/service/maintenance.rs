//! [INPUT]: 管理员签名发行、原身份与私有围栏；Windows ACL使用独立模块路径。
//! [OUTPUT]: 以可写句柄刷盘备份并替换固定启动器，失败恢复原文件与服务。
//! [POS]: 原生服务维护事务；不注册新身份或修改runtime版本指针。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    config::{Config, atomic_json, read_json},
    ota,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
#[cfg(unix)]
mod fence;

pub struct MaintenanceOptions<'a> {
    pub config: &'a Path,
    pub installed_binary: &'a Path,
    pub candidate: &'a Path,
    pub manifest: &'a Path,
    pub signature: &'a Path,
    pub service_stopped: bool,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    schema_version: u32,
    phase: String,
    identity: String,
    original_sha256: String,
    candidate_sha256: String,
    service: serde_json::Value,
    backup: String,
}

/// 只能从管理员安装入口调用；旧launcher没有新鲜活动快照时必须先停服务。
pub fn maintain_launcher(options: MaintenanceOptions<'_>) -> Result<PathBuf> {
    maintain_with(options, &Native)
}
trait Host {
    fn snapshot(&self, binary: &Path, config: &Path) -> Result<serde_json::Value>;
    fn manage(&self, action: &str, config: &Path) -> Result<()>;
    fn private_backup(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
    fn replace(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        replace(path, bytes)
    }
}
struct Native;
impl Host for Native {
    fn snapshot(&self, binary: &Path, config: &Path) -> Result<serde_json::Value> {
        native_snapshot(binary, config)
    }
    fn manage(&self, action: &str, config: &Path) -> Result<()> {
        super::manage(action, config, None)
    }
    fn private_backup(&self, path: &Path) -> Result<()> {
        #[cfg(windows)]
        {
            let status = std::process::Command::new("icacls.exe")
                .arg(path)
                .args([
                    "/inheritance:r",
                    "/grant:r",
                    "*S-1-5-18:(OI)(CI)F",
                    "*S-1-5-32-544:(OI)(CI)F",
                ])
                .status()?;
            ensure!(status.success(), "private maintenance backup ACL failed");
        }
        #[cfg(not(windows))]
        let _ = path;
        Ok(())
    }
}
fn sync_backup(path: &Path) -> Result<()> {
    // Windows FlushFileBuffers要求句柄具有写权限。
    fs::OpenOptions::new().write(true).open(path)?.sync_all()?;
    Ok(())
}

fn maintain_with(options: MaintenanceOptions<'_>, host: &impl Host) -> Result<PathBuf> {
    let cfg = Config::load(options.config)?;
    let ota = ota::load(options.config)?;
    let _maintenance = maintenance_lock(&ota.directory)?;
    ensure!(
        !ota::maintenance_requested(&ota.directory),
        "unfinished launcher maintenance requires recovery"
    );
    let state = ota::snapshot(&ota.directory)?;
    ensure!(
        !state.active(),
        "launcher maintenance requires no active OTA"
    );
    ordinary(options.candidate)?;
    ensure!(
        fs::metadata(options.candidate)?.len() <= ota::MAX_BINARY_SIZE,
        "oversized maintenance candidate"
    );
    ensure!(
        fs::metadata(options.manifest)?.len() <= 65536
            && fs::metadata(options.signature)?.len() <= 1024,
        "oversized release record"
    );
    let bytes = fs::read(options.candidate)?;
    let asset = ota::verify(
        &fs::read(options.manifest)?,
        fs::read_to_string(options.signature)?.trim(),
        &ota.public_keys,
        crate::platform().0,
        crate::platform().1,
    )?;
    ota::verify_bytes(&bytes, &asset)?;
    let service = host.snapshot(options.installed_binary, options.config)?;
    let running = service["running"]
        .as_bool()
        .context("service state unavailable")?;
    ensure!(
        !options.service_stopped || !running,
        "--service-stopped requires an already stopped service"
    );
    if running {
        let activity: ota::activity::Activity =
            read_json(&ota.directory.join("runtime-activity.json"))
                .context("legacy launcher: stop the service before maintenance")?;
        ensure!(
            chrono::Utc::now()
                .signed_duration_since(activity.at)
                .num_milliseconds()
                .abs()
                <= 5000,
            "runtime activity is stale; stop service before maintenance"
        );
        ensure!(
            idle(&activity.drain),
            "launcher maintenance requires idle workers, terminal and operations"
        );
    }
    ordinary(options.installed_binary)?;
    let backup_dir = options
        .installed_binary
        .parent()
        .context("launcher parent missing")?
        .join(format!("launcher-backup-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&backup_dir)?;
    host.private_backup(&backup_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700))?;
    }
    let original = fs::read(options.installed_binary)?;
    let backup = backup_dir.join(crate::ota::binary_name());
    fs::copy(options.installed_binary, &backup)?;
    #[cfg(windows)]
    fs::write(
        backup_dir.join("launcher-acl.sddl"),
        windows_acl(options.installed_binary)?,
    )?;
    sync_backup(&backup)?;
    let backup_config = backup_dir.join("emissary.json");
    fs::copy(options.config, &backup_config)?;
    sync_backup(&backup_config)?;
    fs::copy(
        ota.directory.join("state.json"),
        backup_dir.join("state.json"),
    )?;
    sync_backup(&backup_dir.join("state.json"))?;
    #[cfg(windows)]
    sync_backup(&backup_dir.join("launcher-acl.sddl"))?;
    let identity_hash = identity(&cfg)?;
    let old_capabilities: Option<ota::LauncherCapabilities> =
        read_json(&ota.directory.join("capabilities.json")).ok();
    let mut journal = Journal {
        schema_version: 1,
        phase: "prepared".into(),
        identity: identity_hash.clone(),
        original_sha256: hex::encode(Sha256::digest(&original)),
        candidate_sha256: asset.sha256,
        service: service.clone(),
        backup: backup.to_string_lossy().into(),
    };
    atomic_json(&backup_dir.join("maintenance.json"), &journal)?;
    #[cfg(unix)]
    fs::File::open(backup_dir.parent().context("backup parent missing")?)?.sync_all()?;
    let maintenance_nonce = uuid::Uuid::new_v4();
    let maintenance_fence = ota::activity::MaintenanceFence {
        nonce: maintenance_nonce,
        backup: backup_dir.clone(),
    };
    #[cfg(unix)]
    fence::publish(&ota.directory, &maintenance_fence)?;
    #[cfg(not(unix))]
    atomic_json(&ota.directory.join("maintenance.json"), &maintenance_fence)?;
    let fenced_at = chrono::Utc::now();
    let mut stopped = false;
    let mut replaced = false;
    let result = (|| -> Result<()> {
        if running {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let activity: ota::activity::Activity =
                    read_json(&ota.directory.join("runtime-activity.json"))?;
                if fence_acknowledged(&activity, maintenance_nonce, fenced_at) {
                    ensure!(
                        idle(&activity.drain),
                        "work arrived before maintenance fence"
                    );
                    break;
                }
                ensure!(
                    std::time::Instant::now() < deadline,
                    "runtime did not acknowledge maintenance fence"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            // 停止调用报错也不能推断原服务仍在运行。
            stopped = true;
            host.manage("stop", options.config)?;
        }
        stopped = true;
        let after = host.snapshot(options.installed_binary, options.config)?;
        ensure!(after["running"] == false, "service did not stop cleanly");
        ensure!(
            identity(&cfg)? == identity_hash,
            "node identity changed before maintenance"
        );
        ensure!(
            !ota::snapshot(&ota.directory)?.active(),
            "OTA started before maintenance"
        );
        ensure!(
            !has_files(&cfg.root.join("work-sessions"))?
                && !has_files(&cfg.root.join("terminal-sessions"))?,
            "worker process tree cleanup is incomplete"
        );
        ensure!(
            operations_terminal(&cfg.root.join("operations/journal.json"))?,
            "unfinished finite operation requires runtime recovery"
        );
        ensure!(
            preserved_payload(
                backup_dir.join("state.json").as_path(),
                &ota.directory.join("state.json")
            )?,
            "OTA image pointers changed during maintenance"
        );
        let runner_lock = lock_if_present(&cfg.root.join("runner.lock"))?;
        let launcher_lock = lock_if_present(&ota.directory.join("launcher.lock"))?;
        journal.phase = "stopped".into();
        atomic_json(&backup_dir.join("maintenance.json"), &journal)?;
        // rename成功后的目录同步也会失败；先武装备份恢复。
        replaced = true;
        host.replace(options.installed_binary, &bytes)?;
        journal.phase = "replaced".into();
        atomic_json(&backup_dir.join("maintenance.json"), &journal)?;
        drop(launcher_lock);
        drop(runner_lock);
        let started_at = chrono::Utc::now();
        host.manage("start", options.config)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let live: Result<ota::LauncherCapabilities> =
                read_json(&ota.directory.join("capabilities.json"));
            let health: Result<ota::Health> = read_json(&ota.directory.join("health.json"));
            if live.is_ok_and(|c| {
                c.schema_version == 1
                    && old_capabilities
                        .as_ref()
                        .is_none_or(|old| c.instance_id != old.instance_id)
                    && c.capabilities.iter().any(|v| v == "ota-range-v1")
            }) && health
                .is_ok_and(|h| h.time >= started_at && !h.session.is_empty() && !h.id.is_empty())
                && identity(&cfg)? == identity_hash
            {
                break;
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "new launcher capability/identity confirmation timed out"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let after = host.snapshot(options.installed_binary, options.config)?;
        ensure!(
            after["definition"] == service["definition"],
            "service account or definition changed"
        );
        journal.phase = "awaiting_acceptance".into();
        atomic_json(&backup_dir.join("maintenance.json"), &journal)?;
        Ok(())
    })();
    if let Err(error) = result {
        journal.phase = "recovery_required".into();
        atomic_json(&backup_dir.join("maintenance.json"), &journal)?;
        if stopped {
            host.manage("stop", options.config).context("candidate service did not stop; backup retained without replacing a running launcher")?;
            ensure!(
                host.snapshot(options.installed_binary, options.config)?["running"] == false,
                "service stop is uncertain; maintenance fence and backup retained"
            );
        }
        if replaced {
            host.replace(options.installed_binary, &original)
                .context("launcher restore failed; keep maintenance backup")?;
        }
        if stopped && running {
            host.manage("start", options.config)
                .context("original service restore failed; keep maintenance backup")?;
        }
        ensure!(
            hex::encode(Sha256::digest(fs::read(options.installed_binary)?))
                == journal.original_sha256,
            "original launcher restore is uncertain; maintenance fence and backup retained"
        );
        let restored = host.snapshot(options.installed_binary, options.config)?;
        ensure!(
            restored["definition"] == service["definition"] && restored["running"] == running,
            "original service restore is uncertain; maintenance fence and backup retained"
        );
        journal.phase = "rolled_back".into();
        atomic_json(&backup_dir.join("maintenance.json"), &journal)?;
        let _ = fs::remove_file(ota.directory.join("maintenance.json"));
        return Err(error.context(format!(
            "launcher maintenance rolled back; backup retained at {}",
            backup_dir.display()
        )));
    }
    fs::remove_file(ota.directory.join("maintenance.json"))?;
    Ok(backup_dir)
}

/// 中断恢复只接受原备份摘要和原身份；不改写配置、队列或current/previous。
pub fn rollback_launcher(config: &Path, installed_binary: &Path, backup_dir: &Path) -> Result<()> {
    let cfg = Config::load(config)?;
    let ota = ota::load(config)?;
    let _maintenance = maintenance_lock(&ota.directory)?;
    ensure!(
        !ota::snapshot(&ota.directory)?.active(),
        "launcher rollback requires no active OTA"
    );
    let journal: Journal = read_json(&backup_dir.join("maintenance.json"))?;
    ensure!(
        journal.schema_version == 1 && identity(&cfg)? == journal.identity,
        "maintenance backup identity mismatch"
    );
    let backup = backup_dir.join(ota::binary_name());
    ordinary(installed_binary)?;
    let current = fs::read(installed_binary)?;
    let current_hash = hex::encode(Sha256::digest(&current));
    ensure!(
        current_hash == journal.original_sha256 || current_hash == journal.candidate_sha256,
        "installed launcher changed after maintenance; preserve it for inspection"
    );
    ordinary(&backup)?;
    let original = fs::read(&backup)?;
    ensure!(
        hex::encode(Sha256::digest(&original)) == journal.original_sha256,
        "maintenance backup digest mismatch"
    );
    let service = native_snapshot(installed_binary, config)?;
    ensure!(
        service["definition"] == journal.service["definition"],
        "service definition changed; restore requires inspection"
    );
    ensure!(
        service["running"] == false,
        "stop the service before interrupted maintenance recovery"
    );
    let _runner = lock_if_present(&cfg.root.join("runner.lock"))?;
    let _launcher = lock_if_present(&ota.directory.join("launcher.lock"))?;
    replace(installed_binary, &original)?;
    #[cfg(windows)]
    {
        let acl = backup_dir.join("launcher-acl.sddl");
        ordinary(&acl)?;
        ensure!(fs::metadata(&acl)?.len() <= 16384, "oversized launcher ACL");
        let status = windows_powershell()
            .args(["-NoProfile", "-NonInteractive", "-Command", &format!(
                "$ErrorActionPreference='Stop'; $acl=Get-Acl -LiteralPath {0}; $acl.SetSecurityDescriptorSddlForm((Get-Content -Raw -LiteralPath {1}).Trim()); Set-Acl -LiteralPath {0} -AclObject $acl",
                ps_literal(installed_binary), ps_literal(&acl)
            )]).status()?;
        ensure!(status.success(), "launcher ACL restore failed; keep backup");
    }
    drop(_launcher);
    drop(_runner);
    if journal.service["running"] == true {
        super::manage("start", config, None)?;
    }
    let _ = fs::remove_file(ota.directory.join("maintenance.json"));
    Ok(())
}
fn idle(d: &ota::Drain) -> bool {
    d.active_workers == 0
        && d.pending_terminal_records == 0
        && d.terminals == 0
        && d.operations == 0
}
fn fence_acknowledged(
    activity: &ota::activity::Activity,
    nonce: uuid::Uuid,
    fenced_at: chrono::DateTime<chrono::Utc>,
) -> bool {
    activity.maintenance_nonce == Some(nonce)
        && activity.at >= fenced_at
        && chrono::Utc::now()
            .signed_duration_since(activity.at)
            .num_milliseconds()
            .abs()
            <= 5000
}
fn ordinary(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            m.file_attributes() & 0x400 == 0,
            "launcher reparse point rejected"
        );
    }
    ensure!(
        m.is_file() && !m.file_type().is_symlink(),
        "unsafe launcher file"
    );
    Ok(())
}
fn identity(cfg: &Config) -> Result<String> {
    let mut h = Sha256::new();
    h.update(cfg.emissary_id.as_bytes());
    for p in [&cfg.certificate_file, &cfg.key_file, &cfg.ca_file] {
        h.update(fs::read(p)?);
    }
    Ok(hex::encode(h.finalize()))
}
fn has_files(path: &Path) -> Result<bool> {
    if !path.try_exists()? {
        return Ok(false);
    }
    Ok(fs::read_dir(path)?.next().is_some())
}
fn operations_terminal(path: &Path) -> Result<bool> {
    if !path.try_exists()? {
        return Ok(true);
    }
    ensure!(
        fs::metadata(path)?.len() <= 8 << 20,
        "oversized operations journal"
    );
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    Ok(value["entries"]
        .as_array()
        .context("invalid operations journal")?
        .iter()
        .all(|entry| {
            entry["update"]["state"].as_str().is_some_and(|state| {
                ["succeeded", "failed", "canceled", "expired", "interrupted"].contains(&state)
            })
        }))
}
fn preserved_payload(old: &Path, current: &Path) -> Result<bool> {
    let old: ota::State = read_json(old)?;
    let current: ota::State = read_json(current)?;
    Ok(old.current.path == current.current.path
        && old.previous.path == current.previous.path
        && old.candidate.path == current.candidate.path
        && old.generation == current.generation)
}
fn lock_if_present(path: &Path) -> Result<Option<fs::File>> {
    if !path.try_exists()? {
        return Ok(None);
    }
    ordinary(path)?;
    let file = fs::OpenOptions::new().write(true).open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).context("service process has not exited")?;
    Ok(Some(file))
}
fn maintenance_lock(directory: &Path) -> Result<fs::File> {
    let path = directory.join("maintenance.lock");
    if !path.try_exists()? {
        crate::config::private_write(&path, b"", false, false)?;
    }
    ordinary(&path)?;
    let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).context("launcher maintenance is already active")?;
    Ok(file)
}
fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    ordinary(path)?;
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("launcher parent missing")?)?;
    #[cfg(unix)]
    {
        let mut copy = std::process::Command::new("cp");
        if cfg!(target_os = "macos") {
            copy.arg("-p");
        } else {
            copy.arg("--preserve=all");
        }
        ensure!(
            copy.arg(path).arg(file.path()).status()?.success(),
            "launcher permission copy failed"
        );
        file.as_file_mut().set_len(0)?;
    }
    #[cfg(windows)]
    {
        let source = ps_literal(path);
        let target = ps_literal(file.path());
        let status=windows_powershell().args(["-NoProfile","-NonInteractive","-Command",&format!("$ErrorActionPreference='Stop'; $acl=Get-Acl -LiteralPath {source}; Set-Acl -LiteralPath {target} -AclObject $acl")]).status()?;
        ensure!(status.success(), "launcher ACL copy failed");
    }
    file.write_all(bytes)?;
    file.as_file()
        .set_permissions(fs::metadata(path)?.permissions())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
#[cfg(windows)]
fn ps_literal(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}
#[cfg(windows)]
fn windows_powershell() -> std::process::Command {
    let mut command = std::process::Command::new("powershell.exe");
    // PowerShell 7的模块不能由Windows PowerShell 5加载。
    // 原生子进程不经过pwsh的兼容启动器，必须让5自行构造模块路径。
    command.env_remove("PSModulePath");
    command
}
#[cfg(windows)]
fn windows_acl(path: &Path) -> Result<Vec<u8>> {
    let out = windows_powershell()
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$ErrorActionPreference='Stop'; (Get-Acl -LiteralPath {}).Sddl",
                ps_literal(path)
            ),
        ])
        .output()?;
    ensure!(
        out.status.success() && out.stdout.len() <= 16384,
        "launcher ACL backup failed"
    );
    Ok(out.stdout)
}

#[cfg(unix)]
fn native_snapshot(binary: &Path, config: &Path) -> Result<serde_json::Value> {
    let uid = std::process::Command::new("id").arg("-u").output()?;
    ensure!(
        uid.status.success() && std::str::from_utf8(&uid.stdout)?.trim() == "0",
        "launcher maintenance requires administrator privilege"
    );
    let (definition, running) = if cfg!(target_os = "macos") {
        let definition =
            fs::read_to_string(format!("/Library/LaunchDaemons/{}.plist", super::NAME))?;
        ensure!(
            !definition.contains("<key>UserName</key><string>root</string>")
                && definition.contains("<key>UserName</key><string>"),
            "privileged or unknown service account rejected"
        );
        ensure!(
            definition.contains(&format!(
                "<string>{}</string>",
                super::xml(&binary.to_string_lossy())
            )) && definition.contains(&format!(
                "<string>{}</string>",
                super::xml(&config.to_string_lossy())
            )),
            "service paths differ from maintenance input"
        );
        (
            definition,
            std::process::Command::new("launchctl")
                .args(["print", &format!("system/{}", super::NAME)])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?
                .success(),
        )
    } else {
        let definition =
            fs::read_to_string(format!("/etc/systemd/system/{}.service", super::NAME))?;
        let user = definition
            .lines()
            .find_map(|s| s.strip_prefix("User="))
            .context("service user missing")?;
        ensure!(
            !user.is_empty() && user != "root",
            "privileged service account rejected"
        );
        let expected = super::systemd(binary, config, user)?;
        let command = expected
            .lines()
            .find(|s| s.starts_with("ExecStart="))
            .unwrap();
        ensure!(
            definition.lines().any(|s| s == command),
            "service paths differ from maintenance input"
        );
        (
            definition,
            std::process::Command::new("systemctl")
                .args(["is-active", "--quiet", super::NAME])
                .status()?
                .success(),
        )
    };
    Ok(serde_json::json!({"definition":definition,"running":running}))
}

#[cfg(windows)]
fn native_snapshot(binary: &Path, config: &Path) -> Result<serde_json::Value> {
    use windows_service::{
        service::ServiceAccess,
        service::ServiceState,
        service_manager::{ServiceManager, ServiceManagerAccess},
    };
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        super::NAME,
        ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS,
    )?;
    let value = service.query_config()?;
    let account = value
        .account_name
        .context("service account missing")?
        .to_string_lossy()
        .into_owned();
    ensure!(
        !["localsystem", "nt authority\\system"]
            .iter()
            .any(|v| account.eq_ignore_ascii_case(v)),
        "privileged service account rejected"
    );
    let arguments = windows_arguments(value.executable_path.as_os_str())?;
    ensure!(
        arguments.len() == 4
            && arguments[1] == "service-run"
            && arguments[2] == "--config"
            && fs::canonicalize(Path::new(&arguments[0]))? == fs::canonicalize(binary)?
            && fs::canonicalize(Path::new(&arguments[3]))? == fs::canonicalize(config)?,
        "service paths differ from maintenance input"
    );
    Ok(
        serde_json::json!({"definition":format!("{:?}|{}|{}",value.start_type,account,value.executable_path.to_string_lossy()),"running":service.query_status()?.current_state!=ServiceState::Stopped}),
    )
}
#[cfg(windows)]
fn windows_arguments(command: &std::ffi::OsStr) -> Result<Vec<std::ffi::OsString>> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let command: Vec<u16> = command.encode_wide().chain(Some(0)).collect();
    let mut count = 0;
    // Win32解析SCM原始命令；分配的数组包含全部字符串。
    unsafe {
        let values =
            windows_sys::Win32::UI::Shell::CommandLineToArgvW(command.as_ptr(), &mut count);
        ensure!(!values.is_null(), "invalid service command line");
        let result = std::slice::from_raw_parts(values, count as usize)
            .iter()
            .map(|pointer| {
                let mut length = 0;
                while *pointer.add(length) != 0 {
                    length += 1;
                }
                std::ffi::OsString::from_wide(std::slice::from_raw_parts(*pointer, length))
            })
            .collect();
        windows_sys::Win32::Foundation::LocalFree(values.cast());
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
