//! [INPUT]: 平台安装器、隔离宿主、真实或冻结队列和完整文件读取。
//! [OUTPUT]: 验证维护及runtime OTA，提供同级原生故障夹具的文件和权限读回。
//! [POS]: native_service集成用例的辅助模块；不注册独立Cargo测试目标。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Machine, USER, support};
use artifactory_emissary::{config::Config, ota, platform};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
#[path = "installer.rs"]
mod installer;
#[path = "post_maintenance.rs"]
mod post_maintenance;
#[path = "queues.rs"]
pub(super) mod queues;
#[path = "readiness.rs"]
mod readiness;

pub(super) struct Prepared {
    candidate: PathBuf,
    candidate_version: String,
    manifest: PathBuf,
    signature: PathBuf,
    bad: PathBuf,
    bad_manifest: PathBuf,
    bad_signature: PathBuf,
    ota: PathBuf,
    identity: Vec<(PathBuf, Vec<u8>)>,
    queues: Vec<(PathBuf, Vec<u8>)>,
    original: Vec<u8>,
    pointers: Value,
    evidence: Option<PathBuf>,
}
impl Prepared {
    pub async fn prepare(machine: &Machine, cfg: &Config) -> Self {
        assert!(
            cfg.telemetry_endpoint.is_empty(),
            "retained queues require no receiver"
        );
        let queues = queues::prepare(cfg).await.unwrap();
        Self::prepare_with_queues(machine, cfg, queues)
    }
    pub fn prepare_with_queues(
        machine: &Machine,
        cfg: &Config,
        queues: Vec<(PathBuf, Vec<u8>)>,
    ) -> Self {
        let categories: std::collections::BTreeSet<_> = queues
            .iter()
            .map(|(path, bytes)| {
                queues::validate(path, bytes).expect("frozen queue no longer validates")
            })
            .collect();
        for required in [
            "ordinary_batch",
            "history_batch",
            "startup_inventory",
            "refresh_inventory",
            "terminal_operation",
        ] {
            assert!(
                categories.contains(required),
                "frozen queues lack required class {required}"
            );
        }
        let key = SigningKey::from_bytes(&[29; 32]);
        let candidate = machine.binary.with_file_name(format!(
            "maintenance-tool{}",
            if cfg!(windows) { ".exe" } else { "" }
        ));
        let input = std::env::var_os("ARTIFACTORY_NATIVE_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| machine.binary.clone());
        fs::copy(&input, &candidate).unwrap();
        // Linux测试程序的DWARF超过发行预算；显式发行资产保持原字节。
        if cfg!(target_os = "linux") && std::env::var_os("ARTIFACTORY_NATIVE_BINARY").is_none() {
            assert!(
                machine
                    .command(
                        false,
                        "strip",
                        &[OsStr::new("--strip-debug"), candidate.as_os_str()]
                    )
                    .status
                    .success()
            );
        }
        let bytes = fs::read(&candidate).unwrap();
        assert!(bytes.len() as u64 <= ota::MAX_BINARY_SIZE);
        let original = fs::read(&machine.binary).unwrap();
        if std::env::var_os("ARTIFACTORY_NATIVE_BINARY").is_some() {
            assert!(
                bytes != original,
                "explicit release candidate must differ from installed test binary"
            );
        }
        let output = machine.command(false, &candidate, &[OsStr::new("version")]);
        assert!(
            output.status.success(),
            "maintenance candidate cannot execute on this platform"
        );
        let version = String::from_utf8(output.stdout).unwrap();
        assert!(
            version.starts_with(&format!(
                "artifactory-emissary {} {}/{} protocol=1 commit=",
                artifactory_emissary::VERSION,
                platform().0,
                platform().1
            )),
            "maintenance candidate version or platform differs: {version}"
        );
        let commit = post_maintenance::candidate_commit(&version)
            .expect("native candidate requires its actual full build commit");
        let (manifest, signature) = signed(&machine.directory, "good", &key, &bytes, &commit);
        let bad = machine.directory.join("invalid-native-image");
        fs::write(&bad, b"signed but invalid native executable\n").unwrap();
        let (bad_manifest, bad_signature) = signed(
            &machine.directory,
            "bad",
            &key,
            &fs::read(&bad).unwrap(),
            &commit,
        );
        let ota = cfg.root.join("ota");
        ota::initialize(
            &machine.config,
            &ota,
            &machine.binary,
            artifactory_emissary::VERSION,
            vec![STANDARD.encode(key.verifying_key().to_bytes())],
            true,
        )
        .unwrap();
        let identity = [
            &machine.config,
            &cfg.ca_file,
            &cfg.certificate_file,
            &cfg.key_file,
        ]
        .into_iter()
        .map(|path| (path.clone(), fs::read(path).unwrap()))
        .collect();
        let prepared = Self {
            candidate,
            candidate_version: version,
            manifest,
            signature,
            bad,
            bad_manifest,
            bad_signature,
            original,
            pointers: image_pointers(&serde_json::to_value(ota::snapshot(&ota).unwrap()).unwrap()),
            ota,
            identity,
            queues,
            evidence: std::env::var_os("ARTIFACTORY_NATIVE_EVIDENCE_DIR").map(PathBuf::from),
        };
        prepared.evidence(&machine.binary, "prepared", true, Value::Null);
        prepared
    }
    pub async fn exercise(
        &self,
        machine: &Machine,
        fixture: &mut support::Fixture,
        mut connection: support::Connection,
    ) -> support::Connection {
        machine.running().await;
        readiness::wait(&mut connection).await;
        let host = self.contract(machine);
        let capabilities = self
            .healthy(machine, &mut connection, &fixture.cfg.emissary_id, None)
            .await;
        self.evidence(&machine.binary, "maintain", false, Value::Null);
        let invocation = installer::stage(
            cfg!(windows),
            &machine.directory,
            installer::Files {
                installed: &machine.binary,
                candidate: &self.candidate,
                manifest: &self.manifest,
                signature: &self.signature,
                config: &machine.config,
            },
        );
        if cfg!(windows) {
            assert_eq!(machine.powershell("([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)").trim(), "True", "maintenance installer requires an administrator process");
        } else {
            let identity = machine.command(false, "id", &[OsStr::new("-u")]);
            assert!(identity.status.success());
            assert_ne!(
                String::from_utf8(identity.stdout).unwrap().trim(),
                "0",
                "Unix maintenance installer requires an ordinary test identity"
            );
        }
        let args: Vec<_> = invocation
            .args
            .iter()
            .map(|value| value.as_os_str())
            .collect();
        let output = machine.command(invocation.admin, invocation.program, &args);
        assert!(
            output.status.success(),
            "launcher maintenance failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        connection.closed().await;
        connection = fixture.connection().await;
        let next = self
            .healthy(
                machine,
                &mut connection,
                &fixture.cfg.emissary_id,
                Some(&capabilities),
            )
            .await;
        let backup = self.backup(machine, &self.candidate);
        assert_eq!(
            self.journal(machine, &backup)["phase"],
            "awaiting_acceptance"
        );
        assert!(
            machine.read_bytes(&machine.binary) == machine.read_bytes(&self.candidate),
            "installed launcher differs from the signed candidate"
        );
        self.preserved(machine, &host, "maintain", &self.pointers);
        let runtime = post_maintenance::exercise(self, machine, fixture, connection, &next).await;
        connection = runtime.connection;
        let next = runtime.instance;
        let pointers = runtime.pointers;

        self.evidence(&machine.binary, "explicit_rollback", false, Value::Null);
        machine.act("stop");
        connection.closed().await;
        let output = machine.command(
            true,
            &self.candidate,
            &[
                OsStr::new("launcher-rollback"),
                OsStr::new("--config"),
                machine.config.as_os_str(),
                OsStr::new("--installed-binary"),
                machine.binary.as_os_str(),
                OsStr::new("--backup"),
                backup.as_os_str(),
            ],
        );
        assert!(
            output.status.success(),
            "launcher rollback failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        connection = fixture.connection().await;
        let restored = self
            .healthy(
                machine,
                &mut connection,
                &fixture.cfg.emissary_id,
                Some(&next),
            )
            .await;
        assert!(
            machine.read_bytes(&machine.binary) == self.original,
            "explicit rollback did not restore launcher bytes"
        );
        self.preserved(machine, &host, "explicit_rollback", &pointers);

        // 良好工具执行坏候选；坏候选不能在维护入口之前抢先失败。
        self.evidence(&machine.binary, "automatic_rollback", false, Value::Null);
        let output = self.maintain(machine, &self.bad, &self.bad_manifest, &self.bad_signature);
        assert!(
            !output.status.success(),
            "invalid native image was accepted"
        );
        connection.closed().await;
        connection = fixture.connection().await;
        self.healthy(
            machine,
            &mut connection,
            &fixture.cfg.emissary_id,
            Some(&restored),
        )
        .await;
        assert!(
            machine.read_bytes(&machine.binary) == self.original,
            "automatic rollback did not restore launcher bytes"
        );
        assert_eq!(
            self.journal(machine, &self.backup(machine, &self.bad))["phase"],
            "rolled_back"
        );
        self.preserved(machine, &host, "automatic_rollback", &pointers);
        connection
    }
    fn maintain(
        &self,
        machine: &Machine,
        candidate: &Path,
        manifest: &Path,
        signature: &Path,
    ) -> std::process::Output {
        machine.command(
            true,
            &self.candidate,
            &[
                OsStr::new("launcher-maintain"),
                OsStr::new("--config"),
                machine.config.as_os_str(),
                OsStr::new("--installed-binary"),
                machine.binary.as_os_str(),
                OsStr::new("--candidate"),
                candidate.as_os_str(),
                OsStr::new("--manifest"),
                manifest.as_os_str(),
                OsStr::new("--signature"),
                signature.as_os_str(),
            ],
        )
    }
    async fn healthy(
        &self,
        machine: &Machine,
        connection: &mut support::Connection,
        id: &str,
        previous: Option<&str>,
    ) -> String {
        assert!(
            connection.hello.emissary_id == id,
            "node identity changed after launcher transition"
        );
        machine.running().await;
        let mut anchors = std::collections::HashSet::from([connection.hello.command_id.clone()]);
        tokio::time::timeout(Duration::from_secs(35), async {
            loop {
                while let Ok(event) = connection.events.try_recv() {
                    if matches!(event.kind.as_str(), "hello" | "heartbeat") {
                        anchors.insert(event.command_id);
                    }
                }
                let capabilities = machine.optional_json::<ota::LauncherCapabilities>(
                    &self.ota.join("capabilities.json"),
                );
                let health = machine.optional_json::<ota::Health>(&self.ota.join("health.json"));
                if let (Some(capabilities), Some(health)) = (capabilities, health)
                    && capabilities.schema_version == 1
                    && !capabilities.instance_id.is_empty()
                    && previous.is_none_or(|old| old != capabilities.instance_id)
                    && !health.session.is_empty()
                    && anchors.contains(&health.id)
                {
                    assert!(
                        capabilities
                            .capabilities
                            .iter()
                            .any(|v| v == "ota-range-v1")
                    );
                    return capabilities.instance_id;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("original identity never became healthy after launcher transition")
    }
    fn preserved(&self, machine: &Machine, host: &Value, phase: &str, pointers: &Value) {
        let identity_unchanged = self
            .identity
            .iter()
            .all(|(path, bytes)| machine.read_bytes(path) == *bytes);
        let (queue_unchanged, queue_records) = self.queue_status(machine);
        let state: Value = machine.read_json(&self.ota.join("state.json"));
        let contract = self.contract(machine);
        let mut checks = json!({"identity_unchanged":identity_unchanged,"queue_unchanged":queue_unchanged,
            "ota_pointers_unchanged":image_pointers(&state)==*pointers,
            "account_unchanged":contract["service"]==host["service"],
            "acl_unchanged":contract==*host,
            "maintenance_fence_removed":self.absent(machine, &self.ota.join("maintenance.json"))});
        let passed = checks.as_object().unwrap().values().all(|v| v == true);
        checks["queue_records"] = queue_records;
        self.evidence(&machine.binary, phase, passed, checks);
        assert!(
            passed,
            "maintenance changed identity, queue, image pointers, service account or permissions"
        );
    }
    fn queue_status(&self, machine: &Machine) -> (bool, Value) {
        let mut unchanged = true;
        let mut actual = Vec::new();
        for (path, expected) in &self.queues {
            assert!(
                machine.ordinary_file(path),
                "retained queue is not an ordinary file"
            );
            let bytes = machine.read_bytes(path);
            unchanged &= bytes == *expected;
            queues::validate(path, &bytes).expect("retained queue no longer validates");
            actual.push((path.clone(), bytes));
        }
        (unchanged, queues::evidence(&actual).unwrap())
    }
    fn absent(&self, machine: &Machine, path: &Path) -> bool {
        if cfg!(unix) {
            !machine
                .command(true, "test", &[OsStr::new("-e"), path.as_os_str()])
                .status
                .success()
        } else {
            !path.exists()
        }
    }
    fn backup(&self, machine: &Machine, candidate: &Path) -> PathBuf {
        let digest = hex::encode(Sha256::digest(machine.read_bytes(candidate)));
        let mut matches = fs::read_dir(machine.binary.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("launcher-backup-")
            })
            .filter(|path| self.journal(machine, path)["candidate_sha256"] == digest);
        let found = matches.next().expect("maintenance backup missing");
        assert!(
            matches.next().is_none(),
            "maintenance backup digest is not unique"
        );
        found
    }
    fn journal(&self, machine: &Machine, backup: &Path) -> Value {
        machine.read_json(&backup.join("maintenance.json"))
    }
    fn contract(&self, machine: &Machine) -> Value {
        let permissions: Vec<_> = self
            .identity
            .iter()
            .chain(&self.queues)
            .map(|(path, _)| machine.permissions(path))
            .collect();
        json!({"service":machine.service_contract(),"retained_permissions":permissions})
    }
    fn evidence(&self, installed_binary: &Path, phase: &str, passed: bool, checks: Value) {
        let Some(directory) = &self.evidence else {
            return;
        };
        let candidate = fs::read(&self.candidate).unwrap();
        let installed = fs::read(installed_binary).unwrap();
        let record = json!({"platform":format!("{}/{}",platform().0,platform().1),"phase":phase,"passed":passed,
            "original_sha256":hex::encode(Sha256::digest(&self.original)),
            "candidate_sha256":hex::encode(Sha256::digest(&candidate)),
            "restored_sha256":hex::encode(Sha256::digest(&installed)),
            "candidate_original_different":candidate!=self.original,"checks":checks});
        artifactory_emissary::config::atomic_json(
            &directory.join(format!("native-maintenance-{phase}.json")),
            &record,
        )
        .unwrap();
    }
}

impl Machine {
    #[cfg(unix)]
    fn ordinary_file(&self, path: &Path) -> bool {
        self.command(true, "test", &[OsStr::new("-f"), path.as_os_str()])
            .status
            .success()
            && self
                .command(
                    true,
                    "test",
                    &[OsStr::new("!"), OsStr::new("-L"), path.as_os_str()],
                )
                .status
                .success()
    }
    #[cfg(windows)]
    fn ordinary_file(&self, path: &Path) -> bool {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.is_file() && metadata.file_attributes() & REPARSE_POINT == 0
        })
    }
    pub(super) fn read_bytes(&self, path: &Path) -> Vec<u8> {
        if cfg!(unix) {
            let output = self
                .try_command_limited(
                    true,
                    "cat",
                    &[path.as_os_str()],
                    ota::MAX_BINARY_SIZE as usize + (1 << 20),
                )
                .expect("private acceptance file capture failed");
            assert!(
                output.status.success(),
                "cannot read private acceptance file {}",
                path.display()
            );
            output.stdout
        } else {
            fs::read(path).unwrap()
        }
    }
    pub(super) fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> T {
        serde_json::from_slice(&self.read_bytes(path)).unwrap()
    }
    fn optional_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Option<T> {
        if cfg!(unix) {
            let output = self.command(true, "test", &[OsStr::new("-e"), path.as_os_str()]);
            if output.status.code() == Some(1) {
                return None;
            }
            assert!(
                output.status.success(),
                "private acceptance file lookup failed"
            );
            Some(self.read_json(path))
        } else {
            match fs::read(path) {
                Ok(bytes) => Some(serde_json::from_slice(&bytes).unwrap()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("private acceptance read failed: {:?}", error.kind()),
            }
        }
    }
    pub(super) fn service_contract(&self) -> Value {
        if cfg!(windows) {
            let path = self.binary.to_string_lossy().replace('\'', "''");
            serde_json::from_str(&self.powershell(&format!("$s=Get-CimInstance Win32_Service -Filter \"Name='artifactory-emissary'\"; @{{path=$s.PathName;user=$s.StartName;start=$s.StartMode;sid=(Get-LocalUser -Name '{USER}').SID.Value;acl=(Get-Acl -LiteralPath '{path}').Sddl}}|ConvertTo-Json -Compress"))).unwrap()
        } else {
            let definition = if cfg!(target_os = "macos") {
                PathBuf::from("/Library/LaunchDaemons/artifactory-emissary.plist")
            } else {
                PathBuf::from("/etc/systemd/system/artifactory-emissary.service")
            };
            let mode = if cfg!(target_os = "macos") {
                "-f"
            } else {
                "-c"
            };
            let format = if cfg!(target_os = "macos") {
                "%u:%g:%Lp"
            } else {
                "%u:%g:%a"
            };
            json!({"definition":self.read_bytes(&definition),"account":self.require("id", &["-u", USER]),
                "enabled":self.state().1,"binary_permissions":self.require("stat", &[mode, format, self.binary.to_str().unwrap()])})
        }
    }
    pub(super) fn permissions(&self, path: &Path) -> String {
        if cfg!(windows) {
            self.powershell(&format!(
                "(Get-Acl -LiteralPath '{}').Sddl",
                path.to_string_lossy().replace('\'', "''")
            ))
        } else {
            let mode = if cfg!(target_os = "macos") {
                "-f"
            } else {
                "-c"
            };
            let format = if cfg!(target_os = "macos") {
                "%u:%g:%Lp"
            } else {
                "%u:%g:%a"
            };
            self.require("stat", &[mode, format, path.to_str().unwrap()])
        }
    }
}
fn image_pointers(state: &Value) -> Value {
    json!({"current":state["current"],"previous":state["previous"],"candidate":state["candidate"],"generation":state["generation"]})
}
fn signed(
    directory: &Path,
    name: &str,
    key: &SigningKey,
    bytes: &[u8],
    commit: &str,
) -> (PathBuf, PathBuf) {
    let raw = serde_json::to_vec(&ota::Manifest {
        version: artifactory_emissary::VERSION.into(),
        commit: commit.into(),
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![ota::Asset::from_bytes(platform().0, platform().1, bytes)],
    })
    .unwrap();
    let manifest = directory.join(format!("{name}-manifest.json"));
    let signature = directory.join(format!("{name}-manifest.sig"));
    fs::write(&manifest, &raw).unwrap();
    fs::write(&signature, STANDARD.encode(key.sign(&raw).to_bytes())).unwrap();
    (manifest, signature)
}

#[test]
fn maintenance_fixtures_bind_signed_native_assets() {
    let root = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[29; 32]);
    let (manifest, signature) = signed(
        root.path(),
        "candidate",
        &key,
        b"native image",
        "0123456789abcdef0123456789abcdef01234567",
    );
    let asset = ota::verify(
        &fs::read(manifest).unwrap(),
        fs::read_to_string(signature).unwrap().trim(),
        &[STANDARD.encode(key.verifying_key().to_bytes())],
        platform().0,
        platform().1,
    )
    .unwrap();
    ota::verify_bytes(b"native image", &asset).unwrap();
    assert!(ota::verify_bytes(b"changed image", &asset).is_err());
}

#[test]
fn stage_evidence_survives_interruption_without_identity_or_payload() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("private-installed-location");
    let candidate = root.path().join("private-candidate-location");
    fs::write(&installed, b"private original payload").unwrap();
    fs::write(&candidate, b"private candidate payload").unwrap();
    let prepared = Prepared {
        candidate,
        candidate_version: String::new(),
        original: b"private original payload".to_vec(),
        manifest: PathBuf::new(),
        signature: PathBuf::new(),
        bad: PathBuf::new(),
        bad_manifest: PathBuf::new(),
        bad_signature: PathBuf::new(),
        ota: PathBuf::new(),
        identity: vec![],
        queues: vec![],
        pointers: Value::Null,
        evidence: Some(root.path().join("evidence")),
    };
    prepared.evidence(&installed, "maintain", false, Value::Null);
    let path = root
        .path()
        .join("evidence/native-maintenance-maintain.json");
    let started: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(started["passed"], false);
    prepared.evidence(
        &installed,
        "maintain",
        true,
        json!({"identity_unchanged":true}),
    );
    let bytes = fs::read(path).unwrap();
    let passed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(passed["passed"], true);
    assert_eq!(passed["candidate_original_different"], true);
    assert_eq!(passed["checks"]["identity_unchanged"], true);
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("private"));
    assert_eq!(passed["original_sha256"].as_str().unwrap().len(), 64);
}
