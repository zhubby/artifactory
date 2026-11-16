//! [INPUT]: 依赖固定签名绑定、mTLS 领取、跨平台文件锁和原生工具。
//! [OUTPUT]: 提供签名材料、临时 Keychain、显式解锁和清理闭锁。
//! [POS]: 同一构建节点上的任务签名生命周期；不向源码目录写入密钥。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod redact;
mod verify;
use crate::{
    client,
    config::{Config, private_dir, private_write},
    process, protocol as pb,
    worker::Input,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use fs2::FileExt;
use pb::signing::{Binding, Material, Output};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
pub use verify::verify_outputs;
use zeroize::Zeroizing;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub bindings: Vec<Binding>,
    pub session_id: String,
    pub outputs: Vec<Output>,
    #[serde(skip)]
    pub sessions: Option<tokio::sync::watch::Receiver<String>>,
}
impl Assignment {
    pub fn session_id(&self) -> String {
        self.sessions
            .as_ref()
            .map(|r| r.borrow().clone())
            .unwrap_or_else(|| self.session_id.clone())
    }
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Ok(Self::default());
        }
        let value: Self = serde_json::from_str(value)?;
        ensure!(
            value.bindings.len() == 1 && !value.outputs.is_empty(),
            "invalid managed signing assignment"
        );
        Ok(value)
    }
}
pub struct Materials {
    pub environment: BTreeMap<OsString, OsString>,
    pub secrets: redact::Secrets,
    directory: PathBuf,
    root: PathBuf,
    lock: Option<File>,
    keychain: bool,
}
fn signing_root(cfg: &Config) -> Result<PathBuf> {
    let root = cfg.root.join("signing");
    private_dir(&root)?;
    private_dir(&root.join("locks"))?;
    private_dir(&root.join("materials"))?;
    Ok(root)
}
fn lock(root: &Path, id: &str) -> Result<File> {
    let path = root.join("locks").join(id);
    if !path.exists() {
        match private_write(&path, b"", false, true) {
            Ok(()) => {}
            Err(e) if path.exists() => {
                drop(e);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(fs::OpenOptions::new().read(true).write(true).open(path)?)
}
pub fn ready(cfg: &Config) -> Result<()> {
    ensure!(
        !cfg.root.join("signing/blocked").exists(),
        "signing material cleanup failed; restart node after resolving private directory access"
    );
    Ok(())
}
pub async fn sweep(cfg: &Config) -> Result<()> {
    let root = signing_root(cfg)?;
    let mut busy = false;
    for entry in fs::read_dir(root.join("materials"))? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        ensure!(
            crate::source::valid_id(&id) && entry.file_type()?.is_dir(),
            "unsafe signing recovery directory"
        );
        let file = lock(&root, &id)?;
        if let Err(error) = file.try_lock_exclusive() {
            if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                busy = true;
                continue;
            }
            return Err(error.into());
        }
        let mut material = Materials {
            environment: BTreeMap::new(),
            secrets: Default::default(),
            directory: entry.path(),
            root: root.clone(),
            lock: Some(file),
            keychain: entry.path().join("identity.keychain-db").exists(),
        };
        material.cleanup(cfg).await?;
    }
    let blocked = root.join("blocked");
    if blocked.exists() {
        ensure!(!busy, "signing cleanup is blocked by an active worker");
        fs::remove_file(blocked)?;
    }
    Ok(())
}
impl Materials {
    pub async fn acquire(
        input: &Input,
        assignment: &Assignment,
        index: usize,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        ready(&input.config)?;
        let binding = &assignment.bindings[0];
        let mut control = client::control(&input.config).await?;
        let mut request = pb::SigningMaterialRequest {
            attempt_id: input.attempt_id.clone(),
            lease_token: input.lease_token.clone(),
            session_id: assignment.session_id(),
            profile: binding.profile.clone(),
            version_id: binding.version_id.clone(),
            step_index: index as u32,
        };
        // step_started is durable only after the event ACK; never retry a denial.
        let mut attempts = 0;
        let reply = loop {
            request.session_id = assignment.session_id();
            let result = tokio::select! {_=cancel.cancelled()=>anyhow::bail!("signing canceled"),result=tokio::time::timeout(Duration::from_secs(10),control.acquire_signing_material(request.clone()))=>result.context("signing acquisition timed out")?};
            match result {
                Ok(reply) => break reply.into_inner(),
                Err(e)
                    if e.code() == tonic::Code::FailedPrecondition
                        && e.message() == "signing step not active"
                        && attempts < 100 =>
                {
                    attempts += 1;
                    tokio::select! {_=cancel.cancelled()=>anyhow::bail!("signing canceled"),_=tokio::time::sleep(Duration::from_millis(100))=>{}}
                }
                Err(_) => anyhow::bail!("signing material acquisition rejected"),
            }
        };
        let raw = Zeroizing::new(reply.material_json);
        let material: Material =
            serde_json::from_str(&raw).context("invalid signing material response")?;
        ensure!(
            material.platform() == binding.platform,
            "signing platform mismatch"
        );
        let root = signing_root(&input.config)?;
        let file = lock(&root, &input.attempt_id)?;
        file.try_lock_exclusive()
            .context("signing attempt already active")?;
        let directory = root.join("materials").join(&input.attempt_id);
        fs::create_dir(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let mut value = Self {
            environment: BTreeMap::new(),
            secrets: Default::default(),
            directory,
            root,
            lock: Some(file),
            keychain: false,
        };
        value.secrets.add(&raw);
        value.secrets.add(material.file_base64());
        let bytes = Zeroizing::new(STANDARD.decode(material.file_base64())?);
        let prepared = async {
            match material {
                Material::AndroidKeystore {
                    store_password,
                    key_password,
                    alias,
                    ..
                } => {
                    let path = value.directory.join("identity.keystore");
                    private_write(&path, &bytes, false, true)?;
                    value.set("ARTIFACTORY_ANDROID_KEYSTORE", path.as_os_str());
                    value.set("ARTIFACTORY_ANDROID_STORE_PASSWORD", &store_password);
                    value.set("ARTIFACTORY_ANDROID_KEY_PASSWORD", &key_password);
                    value.set("ARTIFACTORY_ANDROID_KEY_ALIAS", &alias);
                    value.secrets.add(&store_password);
                    value.secrets.add(&key_password);
                }
                Material::MacosPkcs12 {
                    password, notary, ..
                } => {
                    ensure!(
                        cfg!(target_os = "macos"),
                        "macOS signing requires a macOS node"
                    );
                    let p12 = value.directory.join("identity.p12");
                    private_write(&p12, &bytes, false, true)?;
                    let keychain = value.directory.join("identity.keychain-db");
                    let keypass = Zeroizing::new(uuid::Uuid::new_v4().to_string());
                    value.secrets.add(&password);
                    value.secrets.add(&keypass);
                    value.keychain = true;
                    let kc = keychain.to_str().context("keychain path is not UTF-8")?;
                    for args in [
                        vec!["create-keychain", "-p", &keypass, kc],
                        vec!["set-keychain-settings", "-lut", "21600", kc],
                        vec!["unlock-keychain", "-p", &keypass, kc],
                        vec![
                            "import",
                            p12.to_str().context("PKCS12 path is not UTF-8")?,
                            "-k",
                            kc,
                            "-P",
                            &password,
                            "-T",
                            "/usr/bin/codesign",
                        ],
                        vec![
                            "set-key-partition-list",
                            "-S",
                            "apple-tool:,apple:,codesign:",
                            "-s",
                            "-k",
                            &keypass,
                            kc,
                        ],
                    ] {
                        native(&input.config, "security", &args, cancel).await?;
                    }
                    fs::remove_file(p12)?;
                    value.set("ARTIFACTORY_APPLE_KEYCHAIN", keychain.as_os_str());
                    value.set("ARTIFACTORY_APPLE_IDENTITY", &binding.metadata.identity);
                    if let Some(notary) = notary {
                        let key = value.directory.join("notary.p8");
                        private_write(&key, notary.private_key.as_bytes(), false, true)?;
                        value.secrets.add(&notary.private_key);
                        value.set("ARTIFACTORY_APPLE_NOTARY_KEY", key.as_os_str());
                        value.set("ARTIFACTORY_APPLE_NOTARY_KEY_ID", &notary.key_id);
                        value.set("ARTIFACTORY_APPLE_NOTARY_ISSUER", &notary.issuer_id);
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = prepared {
            value.cleanup(&input.config).await?;
            return Err(error);
        }
        Ok(value)
    }
    fn set(&mut self, name: &str, value: impl AsRef<std::ffi::OsStr>) {
        self.environment.insert(name.into(), value.as_ref().into());
    }
    fn release_lock(&mut self) -> Result<()> {
        if let Some(file) = self.lock.take() {
            fs2::FileExt::unlock(&file).context("release signing material lock")?;
        }
        Ok(())
    }
    pub async fn cleanup(&mut self, cfg: &Config) -> Result<()> {
        let result = async {
            if self.keychain && self.directory.join("identity.keychain-db").exists() {
                native(
                    cfg,
                    "security",
                    &[
                        "delete-keychain",
                        self.directory
                            .join("identity.keychain-db")
                            .to_str()
                            .context("invalid keychain path")?,
                    ],
                    &CancellationToken::new(),
                )
                .await?;
            }
            if self.directory.exists() {
                fs::remove_dir_all(&self.directory)?;
            }
            self.environment.clear();
            self.release_lock()?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if result.is_err() {
            let _ = private_write(&self.root.join("blocked"), b"cleanup failed", false, false);
        }
        result
    }
}
impl Drop for Materials {
    fn drop(&mut self) {
        if self.directory.exists() {
            // Crash recovery deletes the Keychain through security before reuse.
            if self.keychain || fs::remove_dir_all(&self.directory).is_err() {
                let _ = private_write(
                    &self.root.join("blocked"),
                    b"cleanup required",
                    false,
                    false,
                );
            }
        }
        if self.release_lock().is_err() {
            let _ = private_write(&self.root.join("blocked"), b"unlock failed", false, false);
        }
    }
}
pub(super) async fn native(
    cfg: &Config,
    tool: &str,
    args: &[&str],
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    let mut command = Command::new(cfg.tool(tool));
    command.args(args).env("LC_ALL", "C");
    let (status, output) = process::output(command, cancel, Duration::from_secs(120)).await?;
    ensure!(status.success(), "native signing tool failed: {tool}");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_releases_locks_while_an_inherited_descriptor_is_open() {
        for explicit_cleanup in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let cfg = Config {
                root: directory.path().into(),
                ..Default::default()
            };
            let root = signing_root(&cfg).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            let path = root.join("materials").join(&id);
            private_dir(&path).unwrap();
            let file = lock(&root, &id).unwrap();
            file.try_lock_exclusive().unwrap();
            let inherited = file.try_clone().unwrap();
            let mut material = Materials {
                directory: path,
                root: root.clone(),
                environment: BTreeMap::new(),
                secrets: Default::default(),
                lock: Some(file),
                keychain: false,
            };
            if explicit_cleanup {
                material.cleanup(&cfg).await.unwrap();
            }
            drop(material);
            let next = lock(&root, &id).unwrap();
            next.try_lock_exclusive()
                .expect("completed signing still holds its inherited lock");
            drop(inherited);
        }
    }
    #[tokio::test]
    async fn recovery_preserves_active_material_and_removes_unlocked_orphans() {
        let directory = tempfile::tempdir().unwrap();
        let cfg = Config {
            root: directory.path().into(),
            ..Default::default()
        };
        let root = signing_root(&cfg).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let material = root.join("materials").join(&id);
        private_dir(&material).unwrap();
        private_write(&material.join("private.key"), b"secret", false, true).unwrap();
        let file = lock(&root, &id).unwrap();
        file.try_lock_exclusive().unwrap();
        sweep(&cfg).await.unwrap();
        assert!(material.join("private.key").exists());
        private_write(&root.join("blocked"), b"failure", false, true).unwrap();
        assert!(ready(&cfg).is_err());
        assert!(sweep(&cfg).await.is_err());
        assert!(root.join("blocked").exists());
        assert!(material.join("private.key").exists());
        fs2::FileExt::unlock(&file).unwrap();
        drop(file);
        sweep(&cfg).await.unwrap();
        assert!(!material.exists());
        assert!(ready(&cfg).is_ok());
    }
    #[test]
    fn abnormal_worker_exit_removes_android_material() {
        let directory = tempfile::tempdir().unwrap();
        let cfg = Config {
            root: directory.path().into(),
            ..Default::default()
        };
        let root = signing_root(&cfg).unwrap();
        let path = root
            .join("materials")
            .join(uuid::Uuid::new_v4().to_string());
        private_dir(&path).unwrap();
        private_write(&path.join("identity.keystore"), b"secret", false, true).unwrap();
        drop(Materials {
            directory: path.clone(),
            root,
            environment: BTreeMap::new(),
            secrets: Default::default(),
            lock: None,
            keychain: false,
        });
        assert!(!path.exists());
    }
}
