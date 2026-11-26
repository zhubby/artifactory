// [INPUT]: 依赖签名发行、受限下载存储、能力握手与进程监督。
// [OUTPUT]: 耐久接受请求并监督激活；保存故障保全进程，候选失败先恢复原程序。
// [POS]: 原生服务启动器与受信运行时之间的更新边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    config::{atomic_json, private_dir, read_json},
    process, source,
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use process_wrap::tokio::ChildWrapper;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{process::Command, task::JoinHandle};
use tokio_util::sync::CancellationToken;

pub(crate) mod activity;
mod download;
mod progress;
mod report;
mod storage;
mod url_refresh;
pub use activity::{
    maintenance_nonce, maintenance_requested, record_activity, record_fenced_activity,
};
pub use download::download;
pub use progress::{LauncherCapabilities, Progress, launcher_snapshot, progress_snapshot};
pub use report::{CAPABILITY, Drain, Observability, Reporter, forward_url_refresh};
pub use storage::{GcPressure, GcReport, garbage_collect, required_space};
pub use url_refresh::{
    UrlRefreshReply, UrlRefreshRequest, complete_url_refresh, url_refresh_request,
};

pub const MAX_BINARY_SIZE: u64 = 128 << 20;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub os: String,
    pub arch: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}
impl Asset {
    pub fn from_bytes(os: &str, arch: &str, bytes: &[u8]) -> Self {
        Self {
            os: os.into(),
            arch: arch.into(),
            name: asset_name(os, arch),
            size: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(bytes)),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub commit: String,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub assets: Vec<Asset>,
}
pub fn asset_name(os: &str, arch: &str) -> String {
    format!(
        "artifactory-emissary-{os}-{arch}{}",
        if os == "windows" { ".exe" } else { "" }
    )
}
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "artifactory-emissary.exe"
    } else {
        "artifactory-emissary"
    }
}
pub fn safe_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 100
        && version.as_bytes()[0].is_ascii_alphanumeric()
        && version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
pub fn verify(raw: &[u8], signature: &str, keys: &[String], os: &str, arch: &str) -> Result<Asset> {
    ensure!(raw.len() <= 65536, "oversized manifest");
    let signature = Signature::from_slice(
        &STANDARD
            .decode(signature)
            .context("invalid signature encoding")?,
    )?;
    let valid = keys.iter().any(|key| {
        STANDARD
            .decode(key)
            .ok()
            .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
            .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok())
            .is_some_and(|key| key.verify(raw, &signature).is_ok())
    });
    ensure!(valid, "manifest signature rejected");
    let manifest: Manifest = serde_json::from_slice(raw)?;
    ensure!(
        safe_version(&manifest.version)
            && manifest.commit.len() == 40
            && manifest.protocol_min == 1
            && manifest.protocol_max >= 1
            && !manifest.assets.is_empty()
            && manifest.assets.len() <= 6,
        "incompatible release manifest"
    );
    let mut seen = std::collections::HashSet::new();
    let mut result = None;
    for asset in manifest.assets {
        let platform = format!("{}/{}", asset.os, asset.arch);
        ensure!(
            matches!(
                platform.as_str(),
                "linux/amd64"
                    | "linux/arm64"
                    | "darwin/amd64"
                    | "darwin/arm64"
                    | "windows/amd64"
                    | "windows/arm64"
            ) && seen.insert(platform)
                && asset.name == asset_name(&asset.os, &asset.arch)
                && asset.size > 0
                && asset.size <= MAX_BINARY_SIZE
                && asset.sha256.len() == 64
                && asset
                    .sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "invalid release asset"
        );
        if asset.os == os && asset.arch == arch {
            result = Some(asset);
        }
    }
    result.with_context(|| format!("release has no compatible {os}/{arch} binary"))
}
pub fn verify_bytes(bytes: &[u8], asset: &Asset) -> Result<()> {
    ensure!(
        bytes.len() as u64 == asset.size && hex::encode(Sha256::digest(bytes)) == asset.sha256,
        "binary size or SHA-256 mismatch"
    );
    Ok(())
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct OtaConfig {
    pub directory: PathBuf,
    pub public_keys: Vec<String>,
    #[serde(default)]
    pub allow_http: bool,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Image {
    pub version: String,
    pub path: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub activation_pending: bool,
    pub generation: i64,
    pub update_id: String,
    pub target: String,
    pub phase: String,
    pub error: String,
    pub current: Image,
    pub previous: Image,
    pub candidate: Image,
    pub launcher_version: String,
    pub started_at: DateTime<Utc>,
}
impl State {
    pub fn recover(&mut self) -> bool {
        match self.phase.as_str() {
            "activating" => {
                self.phase = "rolled_back".into();
                self.error =
                    "launcher interrupted during activation; restored previous runtime".into();
            }
            "downloading" | "draining" => {
                self.phase = "failed".into();
                self.error =
                    "launcher interrupted before activation; current runtime preserved".into();
            }
            _ => return false,
        }
        self.candidate = Image::default();
        self.activation_pending = false;
        true
    }
    pub fn active(&self) -> bool {
        matches!(
            self.phase.as_str(),
            "downloading" | "draining" | "activating"
        )
    }
    pub fn request(&mut self, request: &Request) -> bool {
        if request.id.is_empty() || request.id.len() > 100 {
            return false;
        }
        if request.action == "cancel" {
            if request.generation > self.generation && !self.active() {
                self.generation = request.generation;
                self.update_id = request.id.clone();
            } else if request.id != self.update_id
                || request.generation != self.generation
                || !matches!(self.phase.as_str(), "downloading" | "draining")
            {
                return false;
            }
            self.phase = "canceled".into();
            self.error.clear();
            self.candidate = Image::default();
            return true;
        }
        if request.action == "apply" && request.generation > self.generation && !self.active() {
            self.generation = request.generation;
            self.update_id = request.id.clone();
            self.error.clear();
            self.phase = "downloading".into();
            self.started_at = Utc::now();
            self.candidate = Image::default();
            self.target = serde_json::from_str::<Manifest>(&request.manifest)
                .map(|m| m.version)
                .unwrap_or_default();
            return true;
        }
        false
    }
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub generation: i64,
    #[serde(rename = "id")]
    pub id: String,
    pub action: String,
    pub manifest: String,
    pub signature: String,
    pub url: String,
}
#[derive(Serialize, Deserialize)]
pub struct Health {
    pub session: String,
    pub id: String,
    pub time: DateTime<Utc>,
}
#[derive(Default)]
pub struct HealthWindow {
    session: String,
    last: String,
    count: u32,
    at: Option<Instant>,
}
impl HealthWindow {
    pub fn ack(&mut self, session: &str, id: &str, now: Instant) -> bool {
        if self.session != session
            || self
                .at
                .is_some_and(|at| now.saturating_duration_since(at) > Duration::from_secs(25))
            || id.is_empty()
        {
            *self = Self {
                session: session.into(),
                ..Default::default()
            };
        }
        if !id.is_empty() && id != self.last {
            self.last = id.into();
            self.count += 1;
            self.at = Some(now);
        }
        self.count >= 3
    }
}
pub fn load(path: &Path) -> Result<OtaConfig> {
    let cfg: OtaConfig = read_json(
        &path
            .parent()
            .context("configuration parent missing")?
            .join("ota.json"),
    )?;
    ensure!(
        cfg.directory.is_absolute() && !cfg.public_keys.is_empty(),
        "invalid OTA configuration"
    );
    Ok(cfg)
}
pub fn snapshot(directory: &Path) -> Result<State> {
    read_json(&directory.join("state.json"))
}
pub fn submit(directory: &Path, request: &Request) -> Result<()> {
    let _request = storage::request_lock(directory)?;
    atomic_json(&directory.join("request.json"), request)
}
pub fn confirm(directory: &Path, session: &str, id: &str) -> Result<()> {
    atomic_json(
        &directory.join("health.json"),
        &Health {
            session: session.into(),
            id: id.into(),
            time: Utc::now(),
        },
    )
}
pub fn initialize(
    config_path: &Path,
    directory: &Path,
    binary: &Path,
    version: &str,
    keys: Vec<String>,
    allow_http: bool,
) -> Result<()> {
    ensure!(
        directory.is_absolute() && safe_version(version) && !keys.is_empty(),
        "invalid bootstrap options"
    );
    for key in &keys {
        ensure!(STANDARD.decode(key)?.len() == 32, "invalid OTA public key");
    }
    let ota_path = config_path
        .parent()
        .context("configuration parent missing")?
        .join("ota.json");
    ensure!(!ota_path.try_exists()?, "OTA already configured");
    private_dir(directory)?;
    let body = fs::read(binary)?;
    let target = directory
        .join("versions")
        .join(hex::encode(Sha256::digest(&body)))
        .join(binary_name());
    private_dir(target.parent().context("image parent missing")?)?;
    crate::config::atomic_bytes(&target, &body, true)?;
    atomic_json(
        &directory.join("state.json"),
        &State {
            current: Image {
                version: version.into(),
                path: target.to_string_lossy().into(),
            },
            phase: "idle".into(),
            launcher_version: version.into(),
            ..Default::default()
        },
    )?;
    atomic_json(
        &ota_path,
        &OtaConfig {
            directory: directory.into(),
            public_keys: keys,
            allow_http,
        },
    )
}

struct Runtime {
    child: Box<dyn ChildWrapper>,
}
impl Runtime {
    fn start(
        config_path: &Path,
        cfg: &OtaConfig,
        path: &str,
        version: &str,
        instance: &str,
    ) -> Result<Self> {
        let mut command = Command::new(path);
        command
            .args(["run", "--config"])
            .arg(config_path)
            .env("ARTIFACTORY_OTA_DIRECTORY", &cfg.directory)
            .env("ARTIFACTORY_OTA_LAUNCHER_VERSION", version)
            .env("ARTIFACTORY_OTA_LAUNCHER_INSTANCE", instance)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        Ok(Self {
            child: process::spawn(command)?,
        })
    }
    async fn stop(&mut self) {
        self.child.stdin().take();
        if tokio::time::timeout(Duration::from_secs(8), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
    }
}
pub async fn supervise(config_path: &Path, version: &str, cancel: CancellationToken) -> Result<()> {
    let cfg = load(config_path)?;
    let _lock = source::lock(
        &cfg.directory.join("launcher.lock"),
        &cancel,
        Duration::from_millis(1),
    )
    .await?;
    let capabilities = progress::announce(&cfg.directory, version)?;
    let mut state = snapshot(&cfg.directory)?;
    state.recover();
    storage::release_download(&cfg.directory)?;
    state.launcher_version = version.into();
    let path = cfg.directory.join("state.json");
    atomic_json(&path, &state)?;
    let _ = progress::transition(&cfg.directory, &state);
    let mut current = Runtime::start(
        config_path,
        &cfg,
        &state.current.path,
        version,
        &capabilities.instance_id,
    )?;
    let mut download_task: Option<JoinHandle<(String, Result<Image>)>> = None;
    let mut pending_state = false;
    let mut retry_state_at = Instant::now();
    let mut retry_activation_at = Instant::now();
    let mut retry_health_commit_at = Instant::now();
    let mut pending_request: Option<Request> = None;
    let mut retry_request_at = Instant::now();
    let mut download_cancel = cancel.child_token();
    let mut health = HealthWindow::default();
    let mut health_confirmed = false;
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let result = async {
        loop {
            tokio::select! {
                _=cancel.cancelled()=>return Ok(()),
                _=tick.tick()=>{},
            }
            if let Some(status) = current.child.try_wait()? {
                if state.phase == "activating" {
                    state.phase = "rolled_back".into();
                    state.error = format!("candidate exited before health confirmation ({status})");
                    state.candidate = Image::default();
                    current = Runtime::start(
                        config_path,
                        &cfg,
                        &state.current.path,
                        version,
                        &capabilities.instance_id,
                    )?;
                    pending_state = true;
                    retry_state_at = Instant::now();
                } else {
                    bail!("Emissary runtime exited; service restart required");
                }
            }
            if download_task
                .as_ref()
                .is_some_and(|task| task.is_finished())
            {
                let task = download_task.take().context("missing download task")?;
                let (id, result) = task.await?;
                if state.update_id == id && state.phase == "downloading" {
                    match result {
                        Ok(image) => {
                            state.candidate = image;
                            state.phase = "draining".into();
                        }
                        Err(e) => {
                            state.phase = "failed".into();
                            state.error = e.to_string();
                        }
                    }
                    pending_state = true;
                    retry_state_at = Instant::now();
                }
            }
            if pending_state {
                // 下载或恢复结果未耐久保存前保持可用进程，不接收后续更新。
                if Instant::now() < retry_state_at {
                    continue;
                }
                if atomic_json(&path, &state).is_err()
                    || storage::release_download(&cfg.directory).is_err()
                {
                    retry_state_at = Instant::now() + Duration::from_secs(1);
                    continue;
                }
                let _ = progress::transition(&cfg.directory, &state);
                pending_state = false;
            }
            let mailbox = cfg.directory.join("request.json");
            if pending_request.is_none() && Instant::now() >= retry_request_at {
                if let Ok(_request) = storage::request_lock(&cfg.directory) {
                    if let Ok(request) = read_json::<Request>(&mailbox) {
                        let mut accepted = state.clone();
                        if accept_request(&cfg.directory, &mut accepted, &request) {
                            // 状态耐久保存前保留邮箱和原内存状态，磁盘恢复后重试。
                            if atomic_json(&path, &accepted).is_err() {
                                retry_request_at = Instant::now() + Duration::from_secs(1);
                                continue;
                            }
                            state = accepted;
                            pending_request = Some(request);
                        } else if fs::remove_file(&mailbox).is_err() {
                            retry_request_at = Instant::now() + Duration::from_secs(1);
                        }
                    }
                } else {
                    // 邮箱交接故障不阻止已有候选确认或健康超时恢复。
                    retry_request_at = Instant::now() + Duration::from_secs(1);
                }
            }
            if let Some(request) = pending_request.take() {
                if Instant::now() < retry_request_at {
                    pending_request = Some(request);
                    continue;
                }
                download_cancel.cancel();
                if let Some(task) = download_task.take() {
                    task.abort();
                    let _ = task.await;
                }
                let Ok(_request) = storage::request_lock(&cfg.directory) else {
                    pending_request = Some(request);
                    retry_request_at = Instant::now() + Duration::from_secs(1);
                    continue;
                };
                // 清理故障不能停止当前进程，也不能让取消后的下载重启。
                if storage::release_download(&cfg.directory).is_err()
                    || (read_json::<Request>(&mailbox).is_ok_and(|value| value == request)
                        && fs::remove_file(&mailbox).is_err())
                {
                    pending_request = Some(request);
                    retry_request_at = Instant::now() + Duration::from_secs(1);
                    continue;
                }
                // 下载任务退出后再写终态，禁止旧字节进度覆盖取消。
                let _ = progress::transition(&cfg.directory, &state);
                if state.phase == "downloading" {
                    download_cancel = cancel.child_token();
                    let token = download_cancel.clone();
                    let cfg = cfg.clone();
                    download_task = Some(tokio::spawn(async move {
                        let result = download(&cfg, &request, &token).await;
                        (request.id, result)
                    }));
                }
            }
            if state.phase == "draining" && !activity::maintenance_requested(&cfg.directory) {
                if Instant::now() < retry_activation_at {
                    continue;
                }
                let ready = read_json::<serde_json::Value>(&cfg.directory.join("drained.json"));
                if ready
                    .ok()
                    .and_then(|v| v.get("id").and_then(|v| v.as_str()).map(str::to_owned))
                    .as_deref()
                    == Some(&state.update_id)
                {
                    let Ok(_storage) = storage::lock(&cfg.directory) else {
                        retry_activation_at = Instant::now() + Duration::from_secs(1);
                        continue;
                    };
                    let mut activating = state.clone();
                    activating.phase = "activating".into();
                    activating.started_at = Utc::now();
                    // 激活意图耐久保存前，保留当前进程及内存中的draining状态。
                    if atomic_json(&path, &activating).is_err() {
                        retry_activation_at = Instant::now() + Duration::from_secs(1);
                        continue;
                    }
                    state = activating;
                    current.stop().await;
                    health = HealthWindow::default();
                    health_confirmed = false;
                    retry_health_commit_at = Instant::now();
                    let _ = fs::remove_file(cfg.directory.join("health.json"));
                    let _ = progress::transition(&cfg.directory, &state);
                    match Runtime::start(
                        config_path,
                        &cfg,
                        &state.candidate.path,
                        version,
                        &capabilities.instance_id,
                    ) {
                        Ok(runtime) => current = runtime,
                        Err(_) => {
                            state.phase = "rolled_back".into();
                            state.error = "candidate failed to start".into();
                            state.candidate = Image::default();
                            current = Runtime::start(
                                config_path,
                                &cfg,
                                &state.current.path,
                                version,
                                &capabilities.instance_id,
                            )?;
                            pending_state = true;
                            retry_state_at = Instant::now();
                        }
                    }
                }
            }
            if state.phase == "activating" {
                if let Some(ack) = read_json::<Health>(&cfg.directory.join("health.json"))
                    .ok()
                    .filter(|ack| {
                        Utc::now()
                            .signed_duration_since(ack.time)
                            .num_seconds()
                            .abs()
                            < 25
                    })
                {
                    // 同会话的确认跨存储重试保持；新会话必须重新取得三次确认。
                    if health.session != ack.session || ack.id.is_empty() {
                        health_confirmed = false;
                    }
                    if !health_confirmed {
                        health_confirmed = health.ack(&ack.session, &ack.id, Instant::now());
                    }
                }
                if health_confirmed {
                    if Instant::now() < retry_health_commit_at {
                        continue;
                    }
                    let Ok(_storage) = storage::lock(&cfg.directory) else {
                        retry_health_commit_at = Instant::now() + Duration::from_secs(1);
                        continue;
                    };
                    let mut committed = state.clone();
                    committed.previous = committed.current.clone();
                    committed.current = std::mem::take(&mut committed.candidate);
                    committed.phase = "succeeded".into();
                    // 已健康的进程继续运行；成功状态耐久保存前不移动内存指向。
                    if atomic_json(&path, &committed).is_err() {
                        retry_health_commit_at = Instant::now() + Duration::from_secs(1);
                        continue;
                    }
                    state = committed;
                    let _ = progress::transition(&cfg.directory, &state);
                } else if Utc::now()
                    .signed_duration_since(state.started_at)
                    .num_seconds()
                    >= 120
                {
                    current.stop().await;
                    state.phase = "rolled_back".into();
                    state.error =
                        "candidate health confirmation timed out after 120 seconds".into();
                    state.candidate = Image::default();
                    current = Runtime::start(
                        config_path,
                        &cfg,
                        &state.current.path,
                        version,
                        &capabilities.instance_id,
                    )?;
                    pending_state = true;
                    retry_state_at = Instant::now();
                }
            }
        }
    }
    .await;
    download_cancel.cancel();
    if let Some(task) = download_task {
        task.abort();
        let _ = task.await;
    }
    let _ = storage::release_download(&cfg.directory);
    current.stop().await;
    result
}
fn accept_request(directory: &Path, state: &mut State, request: &Request) -> bool {
    (!activity::maintenance_requested(directory) || request.action == "cancel")
        && state.request(request)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maintenance_fence_rejects_queued_apply_without_advancing_generation() {
        let root = tempfile::tempdir().unwrap();
        atomic_json(
            &root.path().join("maintenance.json"),
            &serde_json::json!({"nonce":uuid::Uuid::new_v4(),"backup":root.path()}),
        )
        .unwrap();
        let mut state = State {
            phase: "idle".into(),
            ..Default::default()
        };
        let mut request = Request {
            id: "queued".into(),
            generation: 3,
            action: "apply".into(),
            ..Default::default()
        };
        assert!(!accept_request(root.path(), &mut state, &request));
        assert_eq!(state.generation, 0);
        request.action = "cancel".into();
        assert!(accept_request(root.path(), &mut state, &request));
        assert_eq!(state.generation, 3);
    }
    #[test]
    fn cancel_overtakes_apply_and_persists_generation_fence() {
        let mut state = State {
            phase: "idle".into(),
            ..Default::default()
        };
        let mut request = Request {
            generation: 9,
            id: "update".into(),
            action: "cancel".into(),
            ..Default::default()
        };
        assert!(state.request(&request));
        request.action = "apply".into();
        assert!(!state.request(&request));
        request.generation = 10;
        assert!(state.request(&request));
        request.action = "cancel".into();
        assert!(state.request(&request));
        assert_eq!(state.generation, 10);
        assert_eq!(state.phase, "canceled");
    }
}
