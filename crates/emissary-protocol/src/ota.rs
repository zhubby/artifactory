//! [INPUT]: 固定启动器握手、真实下载进度与运行时排空计数。
//! [OUTPUT]: 定义有界OTA观察信封，不赋予更新或激活权限。
//! [POS]: Control与Emissary共享的可选观察契约，旧节点缺省未知。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CAPABILITY: &str = "emissary-ota-observability-v2";
pub const MAX_BYTES: usize = 4096;
pub const LAUNCHER_CAPABILITIES: &[&str] = &[
    "ota-progress-v1",
    "ota-space-v1",
    "ota-storage-gc-v1",
    "ota-range-v1",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub update_id: Uuid,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drain {
    pub active_workers: u32,
    pub pending_log_records: u64,
    pub pending_log_bytes: u64,
    pub pending_terminal_records: u32,
    pub terminals: u32,
    pub operations: u32,
    pub waiting_ids: Vec<Uuid>,
    pub remaining_objects: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observability {
    pub schema_version: u32,
    pub sequence: u64,
    pub launcher_version: String,
    pub launcher_capabilities: Vec<String>,
    pub progress: Option<Progress>,
    pub drain: Option<Drain>,
    pub last_progress_at: DateTime<Utc>,
    pub stalled: bool,
}

impl Observability {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.sequence > 0,
            "invalid OTA observation version"
        );
        ensure!(
            self.launcher_version.len() <= 100
                && !self.launcher_version.chars().any(char::is_control),
            "invalid launcher version"
        );
        ensure!(
            self.launcher_capabilities.len() <= 4
                && self
                    .launcher_capabilities
                    .iter()
                    .all(|v| LAUNCHER_CAPABILITIES.contains(&v.as_str())),
            "invalid launcher capability"
        );
        if let Some(p) = &self.progress {
            ensure!(
                !p.update_id.is_nil()
                    && p.generation > 0
                    && p.sequence > 0
                    && valid_digest(&p.asset_sha256)
                    && p.bytes_total > 0
                    && p.bytes_total <= 128 << 20
                    && p.bytes_done <= p.bytes_total
                    && (1..=2).contains(&p.download_attempt),
                "invalid OTA progress identity or size"
            );
            ensure!(
                [
                    "space_check",
                    "download",
                    "refresh_url",
                    "verify",
                    "verified",
                    "drain",
                    "health",
                    "succeeded",
                    "failed",
                    "rolled_back",
                    "canceled"
                ]
                .contains(&p.subphase.as_str()),
                "invalid OTA progress phase"
            );
            ensure!(
                p.error_code.as_deref().is_none_or(|v| [
                    "canceled",
                    "insufficient_space",
                    "space_check_unavailable",
                    "download_failed",
                    "failed",
                    "rolled_back"
                ]
                .contains(&v)),
                "invalid OTA progress error"
            );
        }
        if let Some(d) = &self.drain {
            ensure!(
                d.waiting_ids.len() <= 8 && d.waiting_ids.iter().all(|v| !v.is_nil()),
                "invalid OTA waiting objects"
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_BYTES,
            "oversized OTA observation"
        );
        Ok(())
    }
}

pub fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
