//! [INPUT]: 签名候选、隔离文件系统与可注入服务故障。
//! [OUTPUT]: 验证身份保留、替换恢复与维护围栏。
//! [POS]: 固定启动器维护事务的无宿主变更验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
struct FakeHost {
    directory: PathBuf,
    running: std::cell::Cell<bool>,
    fail_start: bool,
    fail_after_replace: bool,
    fail_restore: bool,
}
impl Host for FakeHost {
    fn replace(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if self.fail_restore && bytes == b"original launcher" {
            anyhow::bail!("simulated restore failure");
        }
        replace(path, bytes)?;
        ensure!(
            !self.fail_after_replace || bytes != b"signed next launcher",
            "simulated post-rename sync failure"
        );
        Ok(())
    }
    fn snapshot(&self, _: &Path, _: &Path) -> Result<serde_json::Value> {
        Ok(
            serde_json::json!({"running":self.running.get(),"definition":"unchanged dedicated account/startup mode"}),
        )
    }
    fn manage(&self, action: &str, _: &Path) -> Result<()> {
        if action == "start" {
            ensure!(!self.fail_start, "simulated service start failure");
            self.running.set(true);
            atomic_json(
                &self.directory.join("capabilities.json"),
                &ota::LauncherCapabilities {
                    schema_version: 1,
                    launcher_version: "0.2.3".into(),
                    instance_id: uuid::Uuid::new_v4().to_string(),
                    capabilities: vec!["ota-range-v1".into()],
                },
            )?;
            atomic_json(
                &self.directory.join("health.json"),
                &ota::Health {
                    session: uuid::Uuid::new_v4().to_string(),
                    id: uuid::Uuid::new_v4().to_string(),
                    time: chrono::Utc::now(),
                },
            )?;
        } else {
            self.running.set(false);
        }
        Ok(())
    }
}

#[test]
fn maintenance_preserves_identity_state_spool_and_original_backup() {
    maintenance_transaction(false, false, false);
}
#[test]
fn failed_new_service_start_restores_original_launcher() {
    maintenance_transaction(true, false, false);
}
#[test]
fn post_rename_durability_failure_restores_original_launcher() {
    maintenance_transaction(false, true, false);
}
#[test]
fn failed_restore_retains_fence_and_recovery_journal() {
    maintenance_transaction(false, true, true);
}
#[test]
fn maintenance_and_recovery_share_one_exclusive_lock() {
    let root = tempfile::tempdir().unwrap();
    let first = maintenance_lock(root.path()).unwrap();
    assert!(maintenance_lock(root.path()).is_err());
    drop(first);
    assert!(maintenance_lock(root.path()).is_ok());
}
#[test]
fn maintenance_requires_exact_fresh_admission_acknowledgement() {
    let nonce = uuid::Uuid::new_v4();
    let at = chrono::Utc::now();
    let mut activity = ota::activity::Activity {
        at,
        drain: ota::Drain::default(),
        maintenance_nonce: None,
    };
    assert!(!fence_acknowledged(&activity, nonce, at));
    activity.maintenance_nonce = Some(uuid::Uuid::new_v4());
    assert!(!fence_acknowledged(&activity, nonce, at));
    activity.maintenance_nonce = Some(nonce);
    assert!(fence_acknowledged(&activity, nonce, at));
    activity.at = at - chrono::Duration::seconds(6);
    assert!(!fence_acknowledged(&activity, nonce, at));
}
fn maintenance_transaction(fail_start: bool, fail_after_replace: bool, fail_restore: bool) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer, SigningKey};
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let config = root.join("emissary.json");
    let installed = root.join("launcher");
    let candidate = root.join("candidate");
    let manifest = root.join("manifest");
    let signature = root.join("signature");
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    let cfg = Config {
        endpoint: "example.invalid:443".into(),
        root: data.clone(),
        emissary_id: uuid::Uuid::new_v4().to_string(),
        ca_file: root.join("ca"),
        certificate_file: root.join("cert"),
        key_file: root.join("key"),
        ..Default::default()
    };
    for path in [&cfg.ca_file, &cfg.certificate_file, &cfg.key_file] {
        fs::write(path, b"preserved identity").unwrap();
    }
    cfg.save(&config).unwrap();
    fs::write(&installed, b"original launcher").unwrap();
    fs::write(&candidate, b"signed next launcher").unwrap();
    let key = SigningKey::from_bytes(&[5; 32]);
    let body = fs::read(&candidate).unwrap();
    let asset = ota::Asset::from_bytes(crate::platform().0, crate::platform().1, &body);
    let raw = serde_json::to_vec(&ota::Manifest {
        version: "0.2.3".into(),
        commit: "a".repeat(40),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![asset],
    })
    .unwrap();
    fs::write(&manifest, &raw).unwrap();
    fs::write(&signature, STANDARD.encode(key.sign(&raw).to_bytes())).unwrap();
    let directory = data.join("ota");
    ota::initialize(
        &config,
        &directory,
        &installed,
        "0.2.2",
        vec![STANDARD.encode(key.verifying_key().to_bytes())],
        false,
    )
    .unwrap();
    fs::create_dir(data.join("spool")).unwrap();
    fs::write(data.join("spool/unconfirmed"), b"retained logs").unwrap();
    let original_state = fs::read(directory.join("state.json")).unwrap();
    let original_config = fs::read(&config).unwrap();
    let host = FakeHost {
        directory: directory.clone(),
        running: std::cell::Cell::new(false),
        fail_start,
        fail_after_replace,
        fail_restore,
    };
    let result = maintain_with(
        MaintenanceOptions {
            config: &config,
            installed_binary: &installed,
            candidate: &candidate,
            manifest: &manifest,
            signature: &signature,
            service_stopped: true,
        },
        &host,
    );
    if fail_restore {
        assert!(result.is_err());
        assert_eq!(fs::read(&installed).unwrap(), b"signed next launcher");
        let fence: ota::activity::MaintenanceFence =
            read_json(&directory.join("maintenance.json")).unwrap();
        let journal: Journal = read_json(&fence.backup.join("maintenance.json")).unwrap();
        assert_eq!(journal.phase, "recovery_required");
    } else if fail_start || fail_after_replace {
        assert!(result.is_err());
        assert_eq!(fs::read(&installed).unwrap(), b"original launcher");
    } else {
        let backup = result.unwrap();
        assert_eq!(
            fs::read(backup.join(ota::binary_name())).unwrap(),
            b"original launcher"
        );
        assert_eq!(fs::read(&installed).unwrap(), b"signed next launcher");
    }
    assert_eq!(fs::read(&config).unwrap(), original_config);
    assert_eq!(
        fs::read(directory.join("state.json")).unwrap(),
        original_state
    );
    assert_eq!(
        fs::read(data.join("spool/unconfirmed")).unwrap(),
        b"retained logs"
    );
    assert_eq!(ota::maintenance_requested(&directory), fail_restore);
}

#[test]
fn replacement_is_complete_and_preserves_permissions() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("launcher");
    fs::write(&target, b"original").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o751)).unwrap();
    }
    replace(&target, b"new signed launcher").unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new signed launcher");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o751
        );
    }
}

#[cfg(unix)]
#[test]
fn replacement_rejects_symlink_and_preserves_external_file() {
    let root = tempfile::tempdir().unwrap();
    let external = root.path().join("external");
    let target = root.path().join("launcher");
    fs::write(&external, b"keep").unwrap();
    std::os::unix::fs::symlink(&external, &target).unwrap();
    assert!(replace(&target, b"replacement").is_err());
    assert_eq!(fs::read(&external).unwrap(), b"keep");
}

#[test]
fn pending_logs_do_not_authorize_discard_or_block_identity_preserving_maintenance() {
    let mut drain = ota::Drain {
        pending_log_records: 3,
        pending_log_bytes: 100,
        ..Default::default()
    };
    assert!(idle(&drain));
    drain.pending_terminal_records = 1;
    assert!(!idle(&drain));
    drain.pending_terminal_records = 0;
    drain.operations = 1;
    assert!(!idle(&drain));
}
#[cfg(windows)]
#[test]
fn windows_service_arguments_keep_exact_paths_and_quoted_spaces() {
    let args = windows_arguments(std::ffi::OsStr::new(r#""C:\Program Files\Artifactory\emissary.exe" service-run --config "C:\Program Data\emissary.json""#)).unwrap();
    assert_eq!(args.len(), 4);
    assert_eq!(args[0], r"C:\Program Files\Artifactory\emissary.exe");
    assert_ne!(args[0], r"C:\Program Files\Artifactory\emissary.exe-other");
    assert_eq!(args[3], r"C:\Program Data\emissary.json");
}
