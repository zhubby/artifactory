//! [INPUT]: 依赖严格主机批次、私有原子文件和单写者队列锁。
//! [OUTPUT]: 提供待发槽、固定身份槽和新增计数需求，探测失败保留未知。
//! [POS]: 新鲜主机数据的1MiB边界；历史发送份额由Spool轮换。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use artifactory_emissary_protocol::telemetry::Batch;
use std::{
    fs,
    path::{Path, PathBuf},
};
const MAX_HOST_BYTES: usize = 512 << 10;

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}
fn plain_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink() && !reparse(&metadata),
        "unsafe fresh directory"
    );
    Ok(())
}
fn slot_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && !reparse(&metadata),
        "unsafe fresh slot"
    );
    Ok(Some(metadata))
}
pub(super) struct Fresh {
    root: PathBuf,
    inflight: Option<PathBuf>,
    sync: super::durability::DirectorySync,
    corrupt: u64,
}
impl Fresh {
    pub fn open(root: PathBuf) -> Result<Self> {
        crate::config::private_dir(&root)?;
        plain_directory(&root)?;
        let mut inflight = None;
        let mut corrupt = 0;
        let mut recovered_namespace = false;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.path().extension().is_none_or(|s| s != "batch") {
                continue;
            }
            let valid = (|| -> Result<Batch> {
                ensure!(
                    entry.file_type()?.is_file()
                        && entry.metadata()?.len() <= MAX_HOST_BYTES as u64,
                    "invalid fresh host record"
                );
                let batch: Batch = serde_json::from_slice(&fs::read(entry.path())?)?;
                batch.validate()?;
                ensure!(
                    entry.file_name() == "pending.batch"
                        || entry
                            .path()
                            .file_stem()
                            .is_some_and(|s| s == batch.id.to_string().as_str()),
                    "fresh identity mismatch"
                );
                ensure!(
                    entry.file_name() == "pending.batch" || inflight.is_none(),
                    "multiple fresh inflight records"
                );
                Ok(batch)
            })();
            if valid.is_err() {
                // 只移动原字节；损坏槽不得阻塞其他类别，也不自动删除。
                let opaque = root.join("opaque");
                crate::config::private_dir(&opaque)?;
                // 单写者下使用独立保留名，重复损坏pending不得覆盖旧来源。
                let mut target = opaque.join(entry.file_name());
                while target.try_exists()? {
                    target = opaque.join(format!(
                        "{}-{}",
                        uuid::Uuid::new_v4(),
                        entry.file_name().to_string_lossy()
                    ));
                }
                fs::rename(entry.path(), target)?;
                recovered_namespace = true;
                continue;
            }
            if entry.file_name() != "pending.batch" {
                inflight = Some(entry.path());
            }
        }
        if recovered_namespace {
            #[cfg(unix)]
            {
                fs::File::open(&root)?.sync_all()?;
                fs::File::open(root.join("opaque"))?.sync_all()?;
            }
        }
        if let Ok(entries) = fs::read_dir(root.join("opaque")) {
            corrupt = entries.filter_map(std::result::Result::ok).count() as u64;
        }
        Ok(Self {
            root,
            inflight,
            sync: Default::default(),
            corrupt,
        })
    }
    pub fn push(&mut self, batch: &Batch) -> Result<bool> {
        plain_directory(&self.root)?;
        self.sync.flush(&self.root)?;
        let bytes = serde_json::to_vec(batch)?;
        ensure!(bytes.len() <= MAX_HOST_BYTES, "fresh host exceeds budget");
        let path = self.root.join("pending.batch");
        let replaced = slot_metadata(&path)?.is_some();
        super::durability::write_record(&path, &bytes)?;
        self.sync.changed(&self.root)?;
        Ok(replaced)
    }
    pub fn additional_record(&self) -> Result<usize> {
        plain_directory(&self.root)?;
        Ok(usize::from(
            slot_metadata(&self.root.join("pending.batch"))?.is_none(),
        ))
    }
    pub fn first(&mut self) -> Result<Option<(PathBuf, Vec<u8>)>> {
        plain_directory(&self.root)?;
        self.sync.flush(&self.root)?;
        let pending = self.root.join("pending.batch");
        if self.inflight.is_none() {
            if slot_metadata(&pending)?.is_none() {
                return Ok(None);
            }
            let batch: Batch = serde_json::from_slice(&fs::read(&pending)?)?;
            let inflight = self.root.join(format!("{}.batch", batch.id));
            fs::rename(pending, &inflight)?;
            self.inflight = Some(inflight);
            self.sync.changed(&self.root)?;
        }
        let inflight = self.inflight.as_ref().expect("inflight established");
        ensure!(
            slot_metadata(inflight)?.is_some(),
            "missing fresh inflight slot"
        );
        Ok(Some((inflight.clone(), fs::read(inflight)?)))
    }
    pub fn ack(&mut self, path: &Path) -> Result<bool> {
        plain_directory(&self.root)?;
        self.sync.flush(&self.root)?;
        if self.inflight.as_deref() != Some(path) {
            return Ok(false);
        }
        ensure!(
            slot_metadata(path)?.is_some(),
            "missing fresh inflight slot"
        );
        fs::remove_file(path)?;
        self.inflight = None;
        self.sync.changed(&self.root)?;
        Ok(true)
    }
    pub fn bytes(&self) -> Result<u64> {
        Ok(self
            .slots()?
            .iter()
            .map(|(_, metadata)| metadata.len())
            .sum())
    }
    pub fn corrupt(&self) -> u64 {
        self.corrupt
    }
    pub fn waiting(&self, max: u64) -> Result<usize> {
        Ok(self
            .slots()?
            .iter()
            .filter(|(_, metadata)| metadata.len() > max)
            .count())
    }
    pub fn records(&self) -> Result<u64> {
        Ok(self.slots()?.len() as u64)
    }
    pub fn oldest_age(&self) -> Result<Option<i64>> {
        let mut oldest = None;
        for (path, metadata) in self.slots()? {
            ensure!(
                metadata.len() <= MAX_HOST_BYTES as u64,
                "invalid fresh host record"
            );
            let batch: Batch = serde_json::from_slice(&fs::read(path)?)?;
            batch.validate()?;
            let age = (chrono::Utc::now() - batch.sampled_at).num_seconds().max(0);
            oldest = Some(oldest.map_or(age, |previous: i64| previous.max(age)));
        }
        Ok(oldest)
    }
    fn slots(&self) -> Result<Vec<(PathBuf, fs::Metadata)>> {
        plain_directory(&self.root)?;
        let mut slots = Vec::with_capacity(2);
        for path in std::iter::once(self.root.join("pending.batch")).chain(self.inflight.clone()) {
            if let Some(metadata) = slot_metadata(&path)? {
                slots.push((path, metadata));
            }
        }
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch() -> Batch {
        let mut batch: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        batch.inventory = None;
        batch.processes.clear();
        batch
    }
    #[test]
    fn absent_slot_is_known_empty_but_io_failure_is_unknown() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("fresh");
        let mut fresh = Fresh::open(root.clone()).unwrap();
        assert_eq!(fresh.bytes().unwrap(), 0);
        assert_eq!(fresh.records().unwrap(), 0);
        assert_eq!(fresh.waiting(1).unwrap(), 0);
        assert_eq!(fresh.oldest_age().unwrap(), None);
        fs::rename(&root, directory.path().join("saved")).unwrap();
        fs::write(&root, b"not a directory").unwrap();
        assert!(
            fresh.bytes().is_err(),
            "I/O error must not become known zero bytes"
        );
        assert!(
            fresh.records().is_err(),
            "I/O error must close record admission"
        );
        assert!(
            fresh.waiting(1).is_err(),
            "schema wait count is unknown on I/O error"
        );
        assert!(
            fresh.oldest_age().is_err(),
            "age requires a successful slot observation"
        );
        assert!(
            fresh.first().is_err(),
            "unavailable fresh selector must fail closed"
        );
    }
    #[test]
    fn slot_directory_is_unknown_and_valid_slot_metrics_follow_ack() {
        let directory = tempfile::tempdir().unwrap();
        let mut fresh = Fresh::open(directory.path().into()).unwrap();
        let mut batch = batch();
        batch.sampled_at = chrono::Utc::now() - chrono::Duration::seconds(60);
        fresh.push(&batch).unwrap();
        let encoded = serde_json::to_vec(&batch).unwrap().len() as u64;
        assert_eq!(fresh.bytes().unwrap(), encoded);
        assert_eq!(fresh.records().unwrap(), 1);
        assert_eq!(fresh.waiting(encoded - 1).unwrap(), 1);
        assert_eq!(fresh.waiting(encoded).unwrap(), 0);
        assert!(fresh.oldest_age().unwrap().is_some_and(|age| age >= 60));
        let (path, _) = fresh.first().unwrap().unwrap();
        fresh.ack(&path).unwrap();
        assert_eq!(fresh.bytes().unwrap(), 0);
        assert_eq!(fresh.records().unwrap(), 0);
        assert_eq!(fresh.oldest_age().unwrap(), None);
        fs::create_dir(directory.path().join("pending.batch")).unwrap();
        assert!(fresh.bytes().is_err());
        assert!(fresh.records().is_err());
        assert!(fresh.waiting(1).is_err());
        assert!(fresh.oldest_age().is_err());
        assert!(fresh.first().is_err());
    }
    #[test]
    fn invalid_payload_keeps_age_unknown_instead_of_claiming_empty_queue() {
        let directory = tempfile::tempdir().unwrap();
        let fresh = Fresh::open(directory.path().into()).unwrap();
        fs::write(directory.path().join("pending.batch"), b"{damaged").unwrap();
        assert_eq!(fresh.records().unwrap(), 1);
        assert_eq!(fresh.bytes().unwrap(), 8);
        assert!(fresh.oldest_age().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn unreadable_slot_preserves_unknown_and_root_link_does_not_change_target_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("fresh");
        let mut fresh = Fresh::open(root.clone()).unwrap();
        fresh.push(&batch()).unwrap();
        let permissions = fs::metadata(&root).unwrap().permissions();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o0)).unwrap();
        if fs::read_dir(&root).is_err() {
            assert!(fresh.bytes().is_err());
            assert!(fresh.records().is_err());
            assert!(fresh.waiting(1).is_err());
            assert!(fresh.oldest_age().is_err());
            assert!(fresh.first().is_err());
        }
        fs::set_permissions(&root, permissions).unwrap();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        let link = directory.path().join("linked-root");
        symlink(&outside, &link).unwrap();
        assert!(Fresh::open(link).is_err());
        assert_eq!(
            fs::metadata(&outside).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    #[cfg(unix)]
    #[test]
    fn slot_links_are_rejected_without_observing_external_payloads() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("outside.batch");
        fs::write(&outside, b"private external payload").unwrap();
        let root = directory.path().join("fresh");
        let mut fresh = Fresh::open(root.clone()).unwrap();
        symlink(&outside, root.join("pending.batch")).unwrap();
        assert!(fresh.bytes().is_err());
        assert!(fresh.records().is_err());
        assert!(fresh.waiting(1).is_err());
        assert!(fresh.oldest_age().is_err());
        assert!(fresh.first().is_err());
        assert!(fresh.push(&batch()).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"private external payload");
    }
    #[test]
    fn repeated_corrupt_pending_slot_preserves_each_original() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("pending.batch"), b"{first").unwrap();
        drop(Fresh::open(directory.path().into()).unwrap());
        fs::write(directory.path().join("pending.batch"), b"{second").unwrap();
        drop(Fresh::open(directory.path().into()).unwrap());
        let contents = fs::read_dir(directory.path().join("opaque"))
            .unwrap()
            .map(|entry| fs::read(entry.unwrap().path()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            contents.len(),
            2,
            "quarantine never overwrites a previous damaged source"
        );
        assert!(contents.iter().any(|value| value == b"{first"));
        assert!(contents.iter().any(|value| value == b"{second"));
    }
    #[test]
    fn sync_failure_after_rename_or_unlink_preserves_live_index() {
        let directory = tempfile::tempdir().unwrap();
        let mut fresh = Fresh::open(directory.path().into()).unwrap();
        let mut batch: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        batch.inventory = None;
        batch.processes.clear();
        fresh.push(&batch).unwrap();
        fresh.sync.fail_next = true;
        assert!(fresh.first().is_err());
        let (path, original) = fresh.first().unwrap().unwrap();
        fresh.sync.fail_next = true;
        assert!(fresh.ack(&path).is_err());
        assert!(fresh.first().unwrap().is_none());
        batch.id = uuid::Uuid::new_v4();
        fresh.push(&batch).unwrap();
        assert_ne!(fresh.first().unwrap().unwrap().1, original);
        drop(fresh);
        assert!(
            Fresh::open(directory.path().into())
                .unwrap()
                .first()
                .unwrap()
                .is_some()
        );
    }
}
