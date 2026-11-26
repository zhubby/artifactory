//! [INPUT]: 依赖私有原子文件、操作身份与结果摘要。
//! [OUTPUT]: 耐久保存操作记录；销毁时显式释放日志独占权。
//! [POS]: 操作执行前的落盘边界；满额时拒绝准入。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::config::{atomic_json, private_dir, private_write};
use anyhow::{Context, Result, ensure};
use artifactory_emissary_protocol::operations::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use uuid::Uuid;
const MAX_RECORDS: usize = 64;
const MAX_BYTES: u64 = 8 << 20;
const RETENTION_SECONDS: i64 = 7 * 24 * 3600;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Entry {
    pub envelope: OperationEnvelope,
    pub update: OperationUpdate,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub acknowledged: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    entries: Vec<Entry>,
    result_expired: u64,
}
pub(super) struct Journal {
    root: PathBuf,
    state: State,
    _lock: fs::File,
}
impl Drop for Journal {
    fn drop(&mut self) {
        // 并行fork可暂时继承描述符，关闭本句柄前先释放独占权。
        let _ = fs2::FileExt::unlock(&self._lock);
    }
}
impl Journal {
    pub fn open(root: PathBuf) -> Result<Self> {
        private_dir(&root)?;
        let lock_path = root.join("journal.lock");
        if !lock_path.try_exists()? {
            private_write(&lock_path, b"", false, false)?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)?;
        fs2::FileExt::try_lock_exclusive(&lock).context("operation journal is already open")?;
        let state = match fs::File::open(root.join("journal.json")) {
            Ok(file) => {
                ensure!(
                    file.metadata()?.len() <= MAX_BYTES,
                    "operation journal exceeds quota"
                );
                serde_json::from_reader(file)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(error) => return Err(error.into()),
        };
        let mut journal = Self {
            root,
            state,
            _lock: lock,
        };
        ensure!(
            journal.state.entries.len() <= MAX_RECORDS,
            "operation journal record limit exceeded"
        );
        let mut seen = std::collections::BTreeSet::new();
        for entry in &journal.state.entries {
            entry.envelope.validate()?;
            ensure!(
                seen.insert(entry.envelope.operation_id),
                "duplicate operation journal identity"
            );
            // ACK 后只保留摘要，成功状态不再带正文。
            if !entry.acknowledged {
                entry.update.validate()?;
            }
        }
        let mut next = journal.state.clone();
        for entry in &mut next.entries {
            if !entry.update.state.terminal() {
                set_terminal(
                    entry,
                    OperationState::Interrupted,
                    Some(OperationReason::StartupChanged),
                    OperationResult::default(),
                )?;
            }
        }
        expire(&mut next);
        journal.save(next)?;
        Ok(journal)
    }
    fn save(&mut self, next: State) -> Result<()> {
        ensure!(
            serde_json::to_vec(&next)?.len() as u64 <= MAX_BYTES,
            "operation journal exceeds quota"
        );
        atomic_json(&self.root.join("journal.json"), &next)?;
        self.state = next;
        Ok(())
    }
    pub fn get(&self, id: Uuid) -> Option<&Entry> {
        self.state
            .entries
            .iter()
            .find(|e| e.envelope.operation_id == id)
    }
    pub fn active(&self) -> bool {
        self.state
            .entries
            .iter()
            .any(|e| !e.update.state.terminal())
    }
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.state.entries.iter()
    }
    pub fn accept(
        &mut self,
        envelope: OperationEnvelope,
    ) -> std::result::Result<OperationUpdate, OperationReason> {
        if let Some(entry) = self.get(envelope.operation_id) {
            if entry.envelope.input_digest != envelope.input_digest
                || entry.envelope.node_id != envelope.node_id
                || entry.envelope.tenant_id != envelope.tenant_id
                || entry.envelope.startup_id != envelope.startup_id
                || entry.envelope.actor_id != envelope.actor_id
                || entry.envelope.issued_at_ms != envelope.issued_at_ms
                || entry.envelope.expires_at_ms != envelope.expires_at_ms
            {
                return Err(OperationReason::InputConflict);
            }
            let update = entry.update.clone();
            let mut next = self.state.clone();
            next.entries
                .iter_mut()
                .find(|e| e.envelope.operation_id == envelope.operation_id)
                .unwrap()
                .envelope
                .control_session = envelope.control_session;
            self.save(next)
                .map_err(|_| OperationReason::StorageUnavailable)?;
            return Ok(update);
        }
        if self.active() {
            return Err(OperationReason::Busy);
        }
        let mut next = self.state.clone();
        expire(&mut next);
        if next.entries.len() >= MAX_RECORDS {
            return Err(OperationReason::JournalFull);
        }
        let update = OperationUpdate {
            operation_id: envelope.operation_id,
            startup_id: envelope.startup_id,
            sequence: 1,
            state: OperationState::Accepted,
            reason: None,
            result: None,
            result_digest: None,
        };
        next.entries.push(Entry {
            envelope,
            update: update.clone(),
            created_at: Utc::now(),
            finished_at: None,
            acknowledged: false,
        });
        self.save(next)
            .map_err(|_| OperationReason::StorageUnavailable)?;
        Ok(update)
    }
    pub fn reauthorize(&mut self, resume: &OperationResume) -> Result<OperationUpdate> {
        let mut next = self.state.clone();
        let entry = next
            .entries
            .iter_mut()
            .find(|entry| entry.envelope.operation_id == resume.operation_id)
            .context("operation missing")?;
        ensure!(
            entry.envelope.tenant_id == resume.tenant_id
                && entry.envelope.node_id == resume.node_id
                && entry.envelope.startup_id == resume.original_startup_id
                && entry.envelope.input_digest == resume.input_digest,
            "operation resume identity mismatch"
        );
        entry.envelope.control_session = resume.control_session;
        let update = entry.update.clone();
        self.save(next)?;
        Ok(update)
    }
    pub fn running(&mut self, id: Uuid) -> Result<OperationUpdate> {
        let mut next = self.state.clone();
        let entry = next
            .entries
            .iter_mut()
            .find(|e| e.envelope.operation_id == id)
            .context("operation missing")?;
        ensure!(
            entry.update.state == OperationState::Accepted,
            "operation already started or terminal"
        );
        entry.update.state = OperationState::Running;
        entry.update.sequence += 1;
        let update = entry.update.clone();
        self.save(next)?;
        Ok(update)
    }
    pub fn finish(
        &mut self,
        id: Uuid,
        state: OperationState,
        reason: Option<OperationReason>,
        result: OperationResult,
    ) -> Result<OperationUpdate> {
        let mut next = self.state.clone();
        let entry = next
            .entries
            .iter_mut()
            .find(|e| e.envelope.operation_id == id)
            .context("operation missing")?;
        if entry.update.state.terminal() {
            return Ok(entry.update.clone());
        }
        set_terminal(entry, state, reason, result)?;
        let update = entry.update.clone();
        self.save(next)?;
        Ok(update)
    }
    pub fn ack(&mut self, ack: &OperationAck) -> Result<bool> {
        let Some(entry) = self.get(ack.operation_id) else {
            return Ok(false);
        };
        if !entry.update.state.terminal()
            || entry.update.result_digest.as_deref() != Some(ack.result_digest.as_str())
        {
            return Ok(false);
        }
        let mut next = self.state.clone();
        let entry = next
            .entries
            .iter_mut()
            .find(|e| e.envelope.operation_id == ack.operation_id)
            .unwrap();
        entry.acknowledged = true;
        entry.update.result = None;
        self.save(next)?;
        Ok(true)
    }
    pub fn maintenance(&mut self) -> Result<u64> {
        let mut next = self.state.clone();
        expire(&mut next);
        let count = next.result_expired;
        self.save(next)?;
        Ok(count)
    }
}
fn set_terminal(
    entry: &mut Entry,
    state: OperationState,
    reason: Option<OperationReason>,
    result: OperationResult,
) -> Result<()> {
    ensure!(state.terminal(), "operation finish requires terminal state");
    entry.update.sequence += 1;
    entry.update.state = state;
    entry.update.reason = reason;
    entry.update.result_digest = Some(result.digest()?);
    entry.update.result = Some(result);
    entry.update.validate()?;
    entry.finished_at = Some(Utc::now());
    Ok(())
}
fn expire(state: &mut State) {
    let cutoff = Utc::now() - chrono::Duration::seconds(RETENTION_SECONDS);
    for entry in &mut state.entries {
        if entry.finished_at.is_some_and(|finished| finished < cutoff)
            && !entry.acknowledged
            && entry.update.result.take().is_some()
        {
            state.result_expired = state.result_expired.saturating_add(1);
            entry.acknowledged = true;
        }
    }
    state
        .entries
        .retain(|e| !e.acknowledged || e.finished_at.is_none_or(|finished| finished >= cutoff));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_releases_ownership_before_inherited_descriptor_closes() {
        let root = tempfile::tempdir().unwrap();
        let journal = Journal::open(root.path().into()).unwrap();
        let inherited = journal._lock.try_clone().unwrap();
        assert!(Journal::open(root.path().into()).is_err());
        drop(journal);
        let reopened = Journal::open(root.path().into())
            .expect("completed journal still holds an inherited lock");
        assert!(Journal::open(root.path().into()).is_err());
        drop(inherited);
        drop(reopened);
    }
}
