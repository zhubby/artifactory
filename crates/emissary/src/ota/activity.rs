//! [INPUT]: 运行时真实排空计数与私有OTA根目录。
//! [OUTPUT]: 写入维护准入快照，读取维护围栏。
//! [POS]: 本机管理员迁移的观察边界；缺失与过期不代表空闲。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Drain;
use crate::config::{atomic_json, read_json};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Activity {
    pub at: chrono::DateTime<chrono::Utc>,
    pub drain: Drain,
    #[serde(default)]
    pub maintenance_nonce: Option<uuid::Uuid>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaintenanceFence {
    pub nonce: uuid::Uuid,
    pub backup: std::path::PathBuf,
}
pub fn record_activity(directory: &Path, drain: &Drain) -> Result<()> {
    record_fenced_activity(directory, drain, None)
}
/// 只能传入已关闭任务、终端和操作准入的确切围栏。
pub fn record_fenced_activity(
    directory: &Path,
    drain: &Drain,
    acknowledged_maintenance: Option<uuid::Uuid>,
) -> Result<()> {
    if let Some(nonce) = acknowledged_maintenance {
        ensure!(
            maintenance_nonce(directory)? == Some(nonce),
            "maintenance fence changed before acknowledgement"
        );
    }
    atomic_json(
        &directory.join("runtime-activity.json"),
        &Activity {
            at: chrono::Utc::now(),
            drain: drain.clone(),
            maintenance_nonce: acknowledged_maintenance,
        },
    )
}
pub fn maintenance_nonce(directory: &Path) -> Result<Option<uuid::Uuid>> {
    let path = directory.join("maintenance.json");
    if !path.try_exists()? {
        return Ok(None);
    }
    let fence: MaintenanceFence = read_json(&path)?;
    ensure!(!fence.nonce.is_nil(), "invalid maintenance fence");
    Ok(Some(fence.nonce))
}
pub fn maintenance_requested(directory: &Path) -> bool {
    directory
        .join("maintenance.json")
        .symlink_metadata()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_activity_does_not_ack_and_stale_nonce_cannot_ack_new_fence() {
        let root = tempfile::tempdir().unwrap();
        let nonce = uuid::Uuid::new_v4();
        atomic_json(
            &root.path().join("maintenance.json"),
            &MaintenanceFence {
                nonce,
                backup: root.path().join("backup"),
            },
        )
        .unwrap();
        record_activity(root.path(), &Drain::default()).unwrap();
        let activity: Activity = read_json(&root.path().join("runtime-activity.json")).unwrap();
        assert!(activity.maintenance_nonce.is_none());
        assert!(
            record_fenced_activity(root.path(), &Drain::default(), Some(uuid::Uuid::new_v4()))
                .is_err()
        );
        record_fenced_activity(root.path(), &Drain::default(), Some(nonce)).unwrap();
        let activity: Activity = read_json(&root.path().join("runtime-activity.json")).unwrap();
        assert_eq!(activity.maintenance_nonce, Some(nonce));
    }
}
