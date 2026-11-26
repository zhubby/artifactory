//! [INPUT]: 私有原子文件、已冻结更新身份与当前启动器握手。
//! [OUTPUT]: 保存真实下载进度并读取当前启动器实际能力。
//! [POS]: OTA旁路观测，不授权激活或跨进程恢复。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Asset, Request, State};
use crate::config::{atomic_json, read_json};
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherCapabilities {
    pub schema_version: u32,
    pub launcher_version: String,
    pub instance_id: String,
    pub capabilities: Vec<String>,
}

/// 缺少实际父启动器握手时返回legacy能力，拒绝信任遗留文件。
pub fn launcher_snapshot(directory: &Path) -> Result<LauncherCapabilities> {
    let expected = std::env::var("ARTIFACTORY_OTA_LAUNCHER_INSTANCE").unwrap_or_default();
    if expected.is_empty() {
        return Ok(LauncherCapabilities::default());
    }
    let capabilities: LauncherCapabilities = read_json(&directory.join("capabilities.json"))?;
    ensure!(
        capabilities.schema_version == 1 && capabilities.instance_id == expected,
        "launcher handshake mismatch"
    );
    ensure!(
        capabilities.capabilities.len() <= 8
            && capabilities.capabilities.iter().all(|v| v.len() <= 64),
        "launcher capability bounds exceeded"
    );
    Ok(capabilities)
}

pub(super) fn announce(directory: &Path, version: &str) -> Result<LauncherCapabilities> {
    let capabilities = LauncherCapabilities {
        schema_version: 1,
        launcher_version: version.into(),
        instance_id: uuid::Uuid::new_v4().to_string(),
        capabilities: [
            "ota-progress-v1",
            "ota-space-v1",
            "ota-storage-gc-v1",
            "ota-range-v1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    };
    atomic_json(&directory.join("capabilities.json"), &capabilities)?;
    Ok(capabilities)
}

pub(super) fn transition(directory: &Path, state: &State) -> Result<()> {
    let Some(mut progress) = progress_snapshot(directory)? else {
        return Ok(());
    };
    if progress.update_id != state.update_id || progress.generation != state.generation {
        return Ok(());
    }
    let subphase = match state.phase.as_str() {
        "draining" => "drain",
        "activating" => "health",
        "succeeded" => "succeeded",
        "failed" => "failed",
        "rolled_back" => "rolled_back",
        "canceled" => "canceled",
        _ => return Ok(()),
    };
    if progress.subphase == subphase {
        return Ok(());
    }
    progress.sequence = progress
        .sequence
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("OTA progress sequence exhausted"))?;
    progress.subphase = subphase.into();
    progress.updated_at = Utc::now();
    progress.last_progress_at = progress.updated_at;
    if matches!(subphase, "failed" | "rolled_back" | "canceled") && progress.error_code.is_none() {
        progress.error_code = Some(subphase.into());
    }
    atomic_json(&directory.join("progress.json"), &progress)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub schema_version: u32,
    pub update_id: String,
    pub generation: i64,
    pub asset_sha256: String,
    pub sequence: u64,
    pub subphase: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub download_attempt: u32,
    pub last_progress_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error_code: Option<String>,
}

pub fn progress_snapshot(directory: &Path) -> Result<Option<Progress>> {
    let path = directory.join("progress.json");
    if !path.try_exists()? {
        return Ok(None);
    }
    let progress: Progress = read_json(&path)?;
    ensure!(
        serde_json::to_vec(&progress)?.len() <= 4096
            && progress.update_id.len() <= 100
            && progress.asset_sha256.len() == 64
            && progress.bytes_done <= progress.bytes_total,
        "invalid OTA progress"
    );
    Ok(Some(progress))
}

pub(super) struct Writer {
    path: PathBuf,
    pub value: Progress,
    saved: Instant,
    saved_bytes: u64,
}
impl Writer {
    pub fn new(directory: &Path, request: &Request, asset: &Asset) -> Result<Self> {
        ensure!(request.id.len() <= 100, "invalid update ID");
        let now = Utc::now();
        Ok(Self {
            path: directory.join("progress.json"),
            value: Progress {
                schema_version: 1,
                update_id: request.id.clone(),
                generation: request.generation,
                asset_sha256: asset.sha256.clone(),
                sequence: 0,
                subphase: "space_check".into(),
                bytes_done: 0,
                bytes_total: asset.size,
                download_attempt: 1,
                last_progress_at: now,
                updated_at: now,
                error_code: None,
            },
            saved: Instant::now(),
            saved_bytes: 0,
        })
    }
    pub fn phase(&mut self, phase: &str) -> Result<()> {
        self.value.subphase = phase.into();
        self.value.last_progress_at = Utc::now();
        self.save()
    }
    pub fn bytes(&mut self, bytes: u64) -> Result<()> {
        if self.value.bytes_done != bytes {
            self.value.last_progress_at = Utc::now();
        }
        self.value.bytes_done = bytes;
        if self.saved.elapsed() >= Duration::from_secs(5)
            || bytes.abs_diff(self.saved_bytes) >= 4 << 20
        {
            self.save()?;
        }
        Ok(())
    }
    pub fn fail(&mut self, code: &str) -> Result<()> {
        self.value.error_code = Some(code.into());
        self.save()
    }
    fn save(&mut self) -> Result<()> {
        self.value.sequence = self
            .value
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("OTA progress sequence exhausted"))?;
        self.value.updated_at = Utc::now();
        ensure!(
            serde_json::to_vec(&self.value)?.len() <= 4096,
            "OTA progress too large"
        );
        atomic_json(&self.path, &self.value)?;
        self.saved = Instant::now();
        self.saved_bytes = self.value.bytes_done;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_state_keeps_the_specific_download_error_code() {
        let root = tempfile::tempdir().unwrap();
        let request = Request {
            id: "update".into(),
            generation: 3,
            ..Default::default()
        };
        let asset = Asset::from_bytes("linux", "amd64", b"asset");
        let mut progress = Writer::new(root.path(), &request, &asset).unwrap();
        progress.fail("insufficient_space").unwrap();
        transition(
            root.path(),
            &State {
                update_id: request.id,
                generation: request.generation,
                phase: "failed".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let progress = progress_snapshot(root.path()).unwrap().unwrap();
        assert_eq!(progress.subphase, "failed");
        assert_eq!(progress.error_code.as_deref(), Some("insufficient_space"));
    }
}
