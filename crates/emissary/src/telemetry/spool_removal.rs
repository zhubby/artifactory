//! [INPUT]: 依赖单写者Spool、固定记录路径和原字节摘要。
//! [OUTPUT]: 原子提交缺口与待删除身份，重试恢复不重复计数。
//! [POS]: 普通采样淘汰事务；拒收索引仅为有界派生副本。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use std::path::{Component, Path};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Rejected {
    id: Option<uuid::Uuid>,
    sha256: String,
    reason: String,
    at: chrono::DateTime<chrono::Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PendingRemoval {
    path: PathBuf,
    id: Option<uuid::Uuid>,
    sha256: String,
    reason: String,
    rejected: bool,
}
impl Spool {
    pub(super) fn import_rejections(&mut self) -> Result<()> {
        ensure!(
            self.stats.rejections.len() <= 64
                && serde_json::to_vec(&self.stats.rejections)?.len() <= 64 << 10,
            "oversized rejection index"
        );
        if let Some(pending) = &self.stats.pending_removal {
            validate_relative(&pending.path)?;
        }

        if !self.stats.rejections.is_empty() {
            return Ok(());
        }
        match fs::read(self.root.join("rejected.json")) {
            Ok(bytes) => {
                ensure!(bytes.len() <= 64 << 10, "oversized rejection index");
                let records: Vec<Rejected> = serde_json::from_slice(&bytes)?;
                ensure!(records.len() <= 64, "oversized rejection count");
                self.stats.rejections = records;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
    fn identity(
        &self,
        path: &Path,
        bytes: &[u8],
        reason: &str,
        rejected: bool,
    ) -> Result<PendingRemoval> {
        let relative = path.strip_prefix(&self.root)?;
        validate_relative(relative)?;
        ensure!(
            matches!(
                reason,
                "expired_sample"
                    | "expired"
                    | "malformed"
                    | "bounds_exceeded"
                    | "id_content_conflict"
                    | "identity_retired"
                    | "quota_eviction"
            ),
            "invalid removal reason"
        );
        ensure!(
            !self.opaque.contains(path) && !self.inventory_blocked.contains(path),
            "protected telemetry source"
        );
        ensure!(
            fs::symlink_metadata(path)?.is_file(),
            "telemetry source is not a regular file"
        );
        let original = fs::read(path)?;
        ensure!(original == bytes, "telemetry removal content changed");
        Ok(PendingRemoval {
            path: relative.into(),
            id: serde_json::from_slice::<Batch>(bytes)
                .ok()
                .map(|batch| batch.id),
            sha256: artifactory_emissary_protocol::telemetry_negotiation::sha256(bytes),
            reason: reason.into(),
            rejected,
        })
    }
    fn save_stats(&mut self, next: Stats) -> Result<()> {
        let result = atomic_json(&self.root.join("stats.json"), &next);
        // rename成功后目录sync仍能失败；重新读取原子文件避免下次覆盖已提交计数。
        self.stats = read_json(&self.root.join("stats.json"))?;
        result
    }
    pub(super) fn begin_removal(
        &mut self,
        path: &Path,
        bytes: &[u8],
        reason: &str,
        rejected: bool,
    ) -> Result<()> {
        let disposition = self.identity(path, bytes, reason, rejected)?;
        if let Some(pending) = &self.stats.pending_removal {
            ensure!(
                pending.path == disposition.path
                    && pending.sha256 == disposition.sha256
                    && pending.id == disposition.id,
                "telemetry removal already pending"
            );
            return Ok(());
        }
        let mut next = self.stats.clone();
        let duplicate = rejected
            && next
                .rejections
                .iter()
                .any(|entry| entry.id == disposition.id && entry.sha256 == disposition.sha256);
        if !duplicate {
            next.dropped = next.dropped.saturating_add(1).min(i64::MAX as u64);
            let counter = next
                .reasons
                .entry(
                    match reason {
                        "expired_sample" | "expired" => "expired",
                        "malformed" => "malformed",
                        "bounds_exceeded" => "bounds_exceeded",
                        "id_content_conflict" => "id_content_conflict",
                        "identity_retired" => "identity_retired",
                        "quota_eviction" => "quota_eviction",
                        _ => "other_rejection",
                    }
                    .into(),
                )
                .or_default();
            *counter = counter.saturating_add(1);
            if rejected {
                let now = chrono::Utc::now();
                next.rejections.push(Rejected {
                    id: disposition.id,
                    sha256: disposition.sha256.clone(),
                    reason: reason.into(),
                    at: now,
                });
                if next.rejections.len() > 64 {
                    next.rejections.remove(0);
                }
                ensure!(
                    serde_json::to_vec(&next.rejections)?.len() <= 64 << 10,
                    "oversized rejection index"
                );
                next.last_rejection_reason = Some(reason.into());
                next.last_rejection_at = Some(now);
            }
        }
        next.pending_removal = Some(disposition);
        self.save_stats(next)
    }
    pub(super) fn finish_removal(&mut self) -> Result<()> {
        let Some(pending) = self.stats.pending_removal.clone() else {
            return Ok(());
        };
        validate_relative(&pending.path)?;
        let path = self.root.join(&pending.path);
        ensure!(
            !self.opaque.contains(&path) && !self.inventory_blocked.contains(&path),
            "protected telemetry source"
        );
        if pending.rejected {
            atomic_json(&self.root.join("rejected.json"), &self.stats.rejections)?;
        }
        // 前次原子写可在目录sync阶段失败；先确保持久化处置，再删除正文。
        #[cfg(unix)]
        fs::File::open(&self.root)?.sync_all()?;
        match fs::read(&path) {
            Ok(bytes) => {
                let identity = self.identity(&path, &bytes, &pending.reason, pending.rejected)?;
                ensure!(
                    identity.sha256 == pending.sha256 && identity.id == pending.id,
                    "pending removal identity changed"
                );
                #[cfg(test)]
                if std::mem::take(&mut self.fail_removal) {
                    anyhow::bail!("injected unlink failure");
                }
                self.ack(&path)?;
                ensure!(!path.try_exists()?, "pending removal was not indexed");
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.sync.flush(&self.root)?;
        #[cfg(unix)]
        fs::File::open(path.parent().context("removal parent missing")?)?.sync_all()?;
        let mut next = self.stats.clone();
        next.pending_removal = None;
        self.save_stats(next)
    }
    pub fn reject(&mut self, path: &Path, bytes: &[u8], reason: &str) -> Result<()> {
        if let Some(pending) = &self.stats.pending_removal {
            let same = self.root.join(&pending.path) == path
                && pending.sha256
                    == artifactory_emissary_protocol::telemetry_negotiation::sha256(bytes);
            self.finish_removal()?;
            if same {
                return Ok(());
            }
        }
        if !path.try_exists()? {
            return Ok(());
        }
        self.begin_removal(path, bytes, reason, true)?;
        self.finish_removal()
    }
}

fn validate_relative(relative: &Path) -> Result<()> {
    ensure!(
        relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
            && (relative.components().count() == 1
                || (relative.components().count() == 2 && relative.starts_with("fresh")))
            && relative.extension().is_some_and(|value| value == "batch")
            && relative
                .file_name()
                .is_some_and(|name| name != "pending.batch"),
        "invalid removal path"
    );
    Ok(())
}
