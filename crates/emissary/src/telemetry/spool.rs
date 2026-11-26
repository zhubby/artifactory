//! [INPUT]: 依赖节点遥测批次、磁盘额度和私有原子记录。
//! [OUTPUT]: 持久化有界采样与幂等缺口，等量主机合并不淘汰历史。
//! [POS]: 节点遥测断线缓冲边界，独占锁阻止重复发送器。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::inventory_lane::InventoryLane;
#[path = "spool_removal.rs"]
mod removal;
use crate::config::{atomic_json, private_dir, read_json};
use anyhow::{Context, Result, ensure};
use artifactory_emissary_protocol::telemetry::{Batch, MAX_BATCH_BYTES};
use removal::{PendingRemoval, Rejected};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::PathBuf,
};

const MAX_RECORDS: usize = 8192;
#[cfg(test)]
#[path = "spool_capacity_tests.rs"]
mod capacity_tests;
#[derive(Clone, Default, Serialize, Deserialize)]
struct Stats {
    dropped: u64,
    #[serde(default)]
    coalesced: u64,
    #[serde(default)]
    reasons: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    last_rejection_reason: Option<String>,
    #[serde(default)]
    last_rejection_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    omitted_host_devices: u64,
    #[serde(default)]
    truncated_host_fields: u64,
    #[serde(default)]
    pending_removal: Option<PendingRemoval>,
    #[serde(default)]
    rejections: Vec<Rejected>,
}
pub(super) struct Spool {
    root: PathBuf,
    quota: u64,
    bytes: u64,
    next: u64,
    records: VecDeque<(PathBuf, u64)>,
    stats: Stats,
    pub inventories: InventoryLane,
    pinned: Option<PathBuf>,
    opaque: HashSet<PathBuf>,
    inventory_blocked: HashSet<PathBuf>,
    fresh: super::scheduler::Fresh,
    history_turn: bool,
    lock: fs::File,
    sync: super::durability::DirectorySync,
    storage: serde_json::Value,
    storage_checked: Option<std::time::Instant>,
    #[cfg(test)]
    fail_removal: bool,
}
impl Spool {
    pub fn open(root: PathBuf, quota: u64) -> Result<Self> {
        private_dir(&root)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).context("telemetry spool already in use")?;
        let stats = match read_json(&root.join("stats.json")) {
            Ok(s) => s,
            Err(_) if !root.join("stats.json").exists() => Stats::default(),
            Err(e) => return Err(e),
        };
        let mut records = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".batch") && entry.file_type()?.is_file() {
                records.push((entry.path(), entry.metadata()?.len()));
            }
        }
        records.sort_by(|a, b| a.0.cmp(&b.0));
        let next = records
            .last()
            .and_then(|r| r.0.file_name())
            .and_then(|n| n.to_str())
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(0, |n| n.saturating_add(1));
        let mut inventories = InventoryLane::open(root.join("inventories"));
        let mut opaque = HashSet::new();
        let mut inventory_blocked = HashSet::new();
        // 升级先保护旧清单，再执行普通队列额度清扫。
        for (path, length) in &records {
            if *length > MAX_BATCH_BYTES as u64 {
                opaque.insert(path.clone());
                continue;
            }
            let bytes = fs::read(path)?;
            match serde_json::from_slice::<Batch>(&bytes) {
                Ok(batch) => {
                    match artifactory_emissary_protocol::InventoryObservation::from_legacy_batch(
                        &batch,
                    ) {
                        Ok(Some(observation)) => {
                            if inventories.push(observation).is_err() {
                                inventory_blocked.insert(path.clone());
                            }
                        }
                        Ok(None) => {}
                        Err(_) => {
                            opaque.insert(path.clone());
                        }
                    }
                }
                Err(_) => {
                    opaque.insert(path.clone());
                }
            }
        }
        let mut spool = Self {
            root: root.clone(),
            quota,
            bytes: records.iter().map(|r| r.1).sum(),
            next,
            records: records.into(),
            stats,
            inventories,
            pinned: None,
            opaque,
            inventory_blocked,
            fresh: super::scheduler::Fresh::open(root.join("fresh"))?,
            history_turn: false,
            lock,
            sync: Default::default(),
            storage: serde_json::json!({}),
            storage_checked: None,
            #[cfg(test)]
            fail_removal: false,
        };
        spool.import_rejections()?;
        let _ = spool.finish_removal();
        while spool.bytes > quota
            || spool.records.len() + spool.fresh.records()? as usize > MAX_RECORDS
        {
            if spool.discard().is_err() {
                break;
            }
        }
        spool.refresh_storage();
        Ok(spool)
    }
    pub fn push(&mut self, batch: &mut Batch) -> Result<()> {
        self.sync.flush(&self.root)?;
        if batch.host.is_some() && batch.processes.is_empty() && batch.inventory.is_none() {
            let bounds = super::sample_bounds::bounded_host(batch)?;
            if bounds.omitted_devices > 0 || bounds.truncated_fields > 0 {
                self.stats.omitted_host_devices = self
                    .stats
                    .omitted_host_devices
                    .saturating_add(bounds.omitted_devices);
                self.stats.truncated_host_fields = self
                    .stats
                    .truncated_host_fields
                    .saturating_add(bounds.truncated_fields);
                atomic_json(&self.root.join("stats.json"), &self.stats)?;
            }
        }
        batch.validate()?;
        if let Some(observation) =
            artifactory_emissary_protocol::InventoryObservation::from_legacy_batch(batch)?
        {
            self.inventories.set_current(batch.session_id);
            self.inventories.push(observation)?;
        }
        if batch.host.is_some() && batch.processes.is_empty() && batch.inventory.is_none() {
            let additional = self.fresh.additional_record()?;
            while self.records.len() + self.fresh.records()? as usize + additional > MAX_RECORDS {
                self.discard()?;
            }
            batch.dropped_batches = self.stats.dropped;
            let coalesced = self.fresh.push(batch)?;
            if coalesced {
                self.stats.coalesced = self.stats.coalesced.saturating_add(1);
                atomic_json(&self.root.join("stats.json"), &self.stats)?;
            }
            return Ok(());
        }
        // Re-encode after each eviction so the durable record exposes the resulting gap counter.
        loop {
            batch.dropped_batches = self.stats.dropped;
            let data = serde_json::to_vec(batch)?;
            ensure!(
                data.len() <= MAX_BATCH_BYTES && data.len() as u64 <= self.quota,
                "telemetry batch exceeds quota"
            );
            if self.bytes + data.len() as u64 > self.quota
                || self.records.len() + self.fresh.records()? as usize >= MAX_RECORDS
            {
                self.discard()?;
                continue;
            }
            let path = self
                .root
                .join(format!("{:020}-{}.batch", self.next, batch.id));
            self.next = self
                .next
                .checked_add(1)
                .context("telemetry spool sequence exhausted")?;
            super::durability::write_record(&path, &data)?;
            self.bytes += data.len() as u64;
            self.records.push_back((path, data.len() as u64));
            return self.sync.changed(&self.root);
        }
    }
    #[cfg(test)]
    pub fn first(&mut self) -> Result<Option<(PathBuf, Vec<u8>)>> {
        self.first_compatible(u64::MAX)
    }
    fn first_compatible(&mut self, max: u64) -> Result<Option<(PathBuf, Vec<u8>)>> {
        self.finish_removal()?;
        self.sync.flush(&self.root)?;
        let Some((path, length)) = self.records.iter().find(|(path, length)| {
            !self.opaque.contains(path) && !self.inventory_blocked.contains(path) && *length <= max
        }) else {
            return Ok(None);
        };
        ensure!(
            *length <= MAX_BATCH_BYTES as u64,
            "invalid telemetry spool entry"
        );
        Ok(Some((path.clone(), fs::read(path)?)))
    }
    #[cfg(test)]
    pub fn next(&mut self) -> Result<Option<(PathBuf, Vec<u8>)>> {
        self.next_compatible(u64::MAX)
    }
    pub fn next_compatible(&mut self, max: u64) -> Result<Option<(PathBuf, Vec<u8>)>> {
        self.finish_removal()?;
        self.sync.flush(&self.root)?;
        if self.history_turn && !self.records.is_empty() {
            self.history_turn = false;
            if let Some(record) = self.first_compatible(max)? {
                return Ok(Some(record));
            }
        }
        if let Some(record) = self.fresh.first()?
            && record.1.len() as u64 <= max
        {
            self.history_turn = true;
            return Ok(Some(record));
        }
        self.first_compatible(max)
    }
    pub fn pin(&mut self, path: &std::path::Path) {
        self.pinned = Some(path.into());
    }
    pub fn unpin(&mut self) {
        self.pinned = None;
    }
    pub fn ack(&mut self, path: &std::path::Path) -> Result<()> {
        ensure!(
            !self.inventory_blocked.contains(path) && !self.opaque.contains(path),
            "protected telemetry source"
        );
        self.sync.flush(&self.root)?;
        if self.fresh.ack(path)? {
            return Ok(());
        }
        if let Some(index) = self.records.iter().position(|r| r.0 == path) {
            self.remove(index)?;
        }
        Ok(())
    }
    pub fn discard(&mut self) -> Result<()> {
        if self.stats.pending_removal.is_some() {
            return self.finish_removal();
        }
        let index = self
            .records
            .iter()
            .position(|r| {
                self.pinned.as_ref() != Some(&r.0)
                    && !self.opaque.contains(&r.0)
                    && !self.inventory_blocked.contains(&r.0)
            })
            .context("all telemetry records are in flight")?;
        let path = self.records[index].0.clone();
        let bytes = fs::read(&path)?;
        self.begin_removal(&path, &bytes, "quota_eviction", false)?;
        self.finish_removal()
    }
    pub fn note_host_bounds(&mut self, omitted: u64, truncated: u64) -> Result<()> {
        self.stats.omitted_host_devices = self.stats.omitted_host_devices.saturating_add(omitted);
        self.stats.truncated_host_fields =
            self.stats.truncated_host_fields.saturating_add(truncated);
        atomic_json(&self.root.join("stats.json"), &self.stats)
    }
    pub fn note_reason(&mut self, reason: &str) -> Result<()> {
        let count = self.stats.reasons.entry(reason.into()).or_default();
        *count = count.saturating_add(1);
        atomic_json(&self.root.join("stats.json"), &self.stats)
    }
    pub fn note_gap(&mut self) -> Result<()> {
        self.stats.dropped = self.stats.dropped.saturating_add(1).min(i64::MAX as u64);
        atomic_json(&self.root.join("stats.json"), &self.stats)?;
        Ok(())
    }
    fn remove(&mut self, index: usize) -> Result<()> {
        if let Some((path, length)) = self.records.get(index) {
            fs::remove_file(path)?;
            self.bytes = self.bytes.saturating_sub(*length);
            self.records.remove(index);
            self.sync.changed(&self.root)?;
        }
        Ok(())
    }
    pub fn schema_waiting(&self, max: u64) -> Option<usize> {
        Some(
            self.records.iter().filter(|(_, size)| *size > max).count()
                + self.fresh.waiting(max).ok()?,
        )
    }
    pub fn dropped(&self) -> u64 {
        self.stats.dropped
    }
    pub fn refresh_storage(&mut self) {
        if self
            .storage_checked
            .is_some_and(|checked| checked.elapsed() < std::time::Duration::from_secs(10))
        {
            return;
        }
        self.storage = super::storage_observation::snapshot(&self.root, &self.opaque);
        self.storage_checked = Some(std::time::Instant::now());
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        let oldest = self
            .records
            .front()
            .and_then(|(path, _)| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Batch>(&bytes).ok())
            .map(|batch| (chrono::Utc::now() - batch.sampled_at).num_seconds().max(0));
        let mut diagnostics = serde_json::json!({"spool_bytes": self.bytes, "spool_records": self.records.len(), "oldest_age_seconds":oldest, "inventory_bytes":self.inventories.bytes(), "inventory_records":self.inventories.len(), "inventory_superseded":self.inventories.superseded(),"inventory_oldest_age_seconds":self.inventories.oldest_age(), "dropped_batches":self.stats.dropped,"fresh_bytes":self.fresh.bytes().ok(),"fresh_records":self.fresh.records().ok(),"fresh_oldest_age_seconds":self.fresh.oldest_age().ok().flatten(),"coalesced_batches":self.stats.coalesced,"corrupt_records":self.opaque.len() as u64+self.fresh.corrupt(),"quota_eviction_batches":self.stats.reasons.get("quota_eviction").copied().unwrap_or(0),"expired_batches":self.stats.reasons.get("expired").copied().unwrap_or(0),"malformed_batches":self.stats.reasons.get("malformed").copied().unwrap_or(0),"bounds_exceeded_batches":self.stats.reasons.get("bounds_exceeded").copied().unwrap_or(0),"id_content_conflict_batches":self.stats.reasons.get("id_content_conflict").copied().unwrap_or(0),"identity_retired_batches":self.stats.reasons.get("identity_retired").copied().unwrap_or(0),"disk_errors":self.stats.reasons.get("disk_error").copied().unwrap_or(0)});
        if !self.inventories.available() {
            diagnostics["inventory_state"] = serde_json::json!("storage_unavailable");
        }
        if self.fresh.bytes().is_err() || self.fresh.records().is_err() {
            diagnostics["fresh_state"] = serde_json::json!("storage_unavailable");
        }
        diagnostics["last_rejection_reason"] = serde_json::json!(self.stats.last_rejection_reason);
        diagnostics["last_rejection_at"] = serde_json::json!(self.stats.last_rejection_at);
        diagnostics.as_object_mut().unwrap().extend(
            self.inventories
                .conflict_diagnostics()
                .as_object()
                .unwrap()
                .clone(),
        );
        diagnostics["omitted_host_devices"] = serde_json::json!(self.stats.omitted_host_devices);
        diagnostics["truncated_host_fields"] = serde_json::json!(self.stats.truncated_host_fields);
        if let Some(storage) = self.storage.as_object() {
            diagnostics.as_object_mut().unwrap().extend(storage.clone());
        }
        diagnostics
    }
}
impl Drop for Spool {
    fn drop(&mut self) {
        // Closing our descriptor alone can retain flock during a concurrent
        // fork/exec. Release ownership before any inherited descriptor closes.
        let _ = fs2::FileExt::unlock(&self.lock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;
    fn batch() -> Batch {
        Batch {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            sampled_at: Utc::now(),
            monotonic_ms: 1,
            dropped_batches: 0,
            omitted_processes: 0,
            host: None,
            processes: vec![],
            inventory: None,
        }
    }
    #[test]
    fn unacknowledged_inventory_survives_sample_quota_eviction_and_restart() {
        let directory = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(directory.path().into(), 4096).unwrap();
        let mut original: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        let startup = original.session_id;
        spool.push(&mut original).unwrap();
        for _ in 0..20 {
            spool.push(&mut batch()).unwrap();
        }
        assert!(spool.dropped() > 0);
        drop(spool);
        let _reopened = Spool::open(directory.path().into(), 4096).unwrap();
        let protected = directory.path().join("inventories");
        assert!(protected.is_dir(), "未确认清单必须独立于可淘汰采样持久化");
        let records: Vec<_> = fs::read_dir(protected).unwrap().collect();
        assert!(
            !records.is_empty(),
            "startup {startup} 的清单在重启后仍须等待业务确认"
        );
    }
    #[test]
    fn fresh_host_and_history_each_receive_a_turn_without_changing_inflight_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(directory.path().into(), 4096).unwrap();
        let mut history = batch();
        spool.push(&mut history).unwrap();
        let mut fresh: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        fresh.inventory = None;
        fresh.processes.clear();
        spool.push(&mut fresh).unwrap();
        let (flight, original) = spool.next().unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Batch>(&original).unwrap().id,
            fresh.id
        );
        fresh.id = Uuid::new_v4();
        spool.push(&mut fresh).unwrap();
        assert_eq!(fs::read(&flight).unwrap(), original);
        let (_, next) = spool.next().unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Batch>(&next).unwrap().id,
            history.id
        );
        spool.ack(&flight).unwrap();
        let (_, next) = spool.next().unwrap().unwrap();
        assert_eq!(serde_json::from_slice::<Batch>(&next).unwrap().id, fresh.id);
    }
    #[test]
    fn corrupt_fresh_record_does_not_disable_history_or_pending_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("fresh");
        fs::create_dir(&fresh).unwrap();
        let name = format!("{}.batch", Uuid::new_v4());
        fs::write(fresh.join(&name), b"{broken").unwrap();
        let mut original: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        spool.push(&mut original).unwrap();
        assert!(spool.first().unwrap().is_some());
        assert!(spool.inventories.first().unwrap().is_some());
        assert_eq!(
            fs::read(fresh.join("opaque").join(name)).unwrap(),
            b"{broken"
        );
        assert_eq!(spool.diagnostics()["corrupt_records"], 1);
    }
    #[test]
    fn corrupt_legacy_records_are_preserved_and_do_not_block_valid_history() {
        let dir = tempfile::tempdir().unwrap();
        let corrupt = dir.path().join("00000000000000000000-corrupt.batch");
        fs::write(&corrupt, b"{incomplete").unwrap();
        let mut spool = Spool::open(dir.path().into(), 800).unwrap();
        let mut sample = batch();
        spool.push(&mut sample).unwrap();
        let (path, bytes) = spool.first().unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Batch>(&bytes).unwrap().id,
            sample.id
        );
        spool.ack(&path).unwrap();
        assert_eq!(fs::read(corrupt).unwrap(), b"{incomplete");
        assert_eq!(spool.diagnostics()["corrupt_records"], 1);
    }
    #[test]
    fn damaged_inventory_lane_preserves_source_and_allows_unrelated_samples() {
        for name in ["stats.json", "startup.json", "broken.inventory"] {
            let dir = tempfile::tempdir().unwrap();
            let inventories = dir.path().join("inventories");
            fs::create_dir(&inventories).unwrap();
            let broken = inventories.join(name);
            fs::write(&broken, b"{incomplete").unwrap();
            let mut mixed: Batch = serde_json::from_slice(include_bytes!(
                "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
            ))
            .unwrap();
            let original = serde_json::to_vec(&mixed).unwrap();
            let source = dir.path().join("00000000000000000000-source.batch");
            fs::write(&source, &original).unwrap();
            let mut spool = Spool::open(dir.path().into(), 4096)
                .expect("inventory corruption must not disable ordinary telemetry");
            assert!(
                spool
                    .inventories
                    .seed_startup(
                        Uuid::new_v4(),
                        Utc::now(),
                        mixed.inventory.as_ref().unwrap()
                    )
                    .is_err()
            );
            mixed.inventory = None;
            mixed.processes.clear();
            mixed.id = Uuid::new_v4();
            spool.push(&mut mixed).unwrap();
            let (fresh, _) = spool.next().unwrap().unwrap();
            spool.ack(&fresh).unwrap();
            let mut history = batch();
            spool.push(&mut history).unwrap();
            let (path, bytes) = spool.next().unwrap().unwrap();
            assert_eq!(
                serde_json::from_slice::<Batch>(&bytes).unwrap().id,
                history.id
            );
            spool.ack(&path).unwrap();
            for _ in 0..30 {
                spool.push(&mut batch()).unwrap();
            }
            assert_eq!(
                fs::read(&source).unwrap(),
                original,
                "unextracted source cannot be evicted or acknowledged"
            );
            assert!(spool.ack(&source).is_err());
            assert_eq!(fs::read(&broken).unwrap(), b"{incomplete");
            drop(spool);
            let reopened = Spool::open(dir.path().into(), 4096).unwrap();
            assert_eq!(
                reopened.diagnostics()["inventory_state"],
                "storage_unavailable"
            );
            assert!(
                reopened.diagnostics()["inventory_records"].is_null(),
                "unreadable queue count stays unknown"
            );
            drop(reopened);
            fs::remove_file(&broken).unwrap();
            let mut repaired = Spool::open(dir.path().into(), 4096).unwrap();
            let (_, replay) = repaired.first().unwrap().unwrap();
            assert_eq!(replay, original);
            let original: Batch = serde_json::from_slice(&original).unwrap();
            assert_eq!(
                repaired.inventories.first().unwrap(),
                artifactory_emissary_protocol::InventoryObservation::from_legacy_batch(&original)
                    .unwrap()
            );
        }
    }
    #[test]
    fn missing_pending_startup_and_bad_revision_do_not_invent_observation_identity() {
        let dir = tempfile::tempdir().unwrap();
        let startup = Uuid::new_v4();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        spool
            .inventories
            .seed_startup(startup, Utc::now(), &Default::default())
            .unwrap();
        let observation = spool.inventories.first().unwrap().unwrap();
        drop(spool);
        fs::remove_file(
            dir.path()
                .join("inventories")
                .join(format!("{}.inventory", observation.observation_id)),
        )
        .unwrap();
        let checkpoint = fs::read(dir.path().join("inventories/startup.json")).unwrap();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        assert!(
            spool
                .inventories
                .seed_startup(startup, Utc::now(), &Default::default())
                .is_err()
        );
        assert!(!spool.inventories.available());
        spool.push(&mut batch()).unwrap();
        assert!(spool.next().unwrap().is_some());
        assert_eq!(
            fs::read(dir.path().join("inventories/startup.json")).unwrap(),
            checkpoint
        );
        drop(spool);
        let other = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(other.path().into(), 4096).unwrap();
        spool
            .inventories
            .seed_startup(startup, Utc::now(), &Default::default())
            .unwrap();
        fs::write(
            other.path().join("inventories/revision.json"),
            b"{incomplete",
        )
        .unwrap();
        assert!(
            spool
                .inventories
                .refresh(startup, &Default::default(), 5)
                .is_err()
        );
        assert!(
            spool.inventories.available(),
            "bad refresh checkpoint does not prevent existing observations"
        );
        spool.push(&mut batch()).unwrap();
        assert!(spool.next().unwrap().is_some());
        assert_eq!(
            fs::read(other.path().join("inventories/revision.json")).unwrap(),
            b"{incomplete"
        );
    }
    #[test]
    fn rejection_diagnostics_survive_restart_without_claiming_an_ack() {
        let dir = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        spool.push(&mut batch()).unwrap();
        let (path, bytes) = spool.first().unwrap().unwrap();
        spool.reject(&path, &bytes, "bounds_exceeded").unwrap();
        drop(spool);
        let diagnostics = Spool::open(dir.path().into(), 4096).unwrap().diagnostics();
        assert_eq!(diagnostics["bounds_exceeded_batches"], 1);
        assert_eq!(diagnostics["last_rejection_reason"], "bounds_exceeded");
        assert!(diagnostics["last_rejection_at"].is_string());
        assert!(diagnostics["last_successful_ack_at"].is_null());
    }
    #[test]
    fn new_host_is_bounded_and_omission_counters_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        let mut host: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        host.inventory = None;
        host.processes.clear();
        let identity = host.id;
        let disks = &mut host.host.as_mut().unwrap().disks;
        let mut disk = disks[0].clone();
        disk.device = "\u{1}".repeat(512);
        disk.mount = "\u{1}".repeat(1024);
        *disks = vec![disk; 256];
        spool.push(&mut host).unwrap();
        let (_, bytes) = spool.next().unwrap().unwrap();
        assert!(bytes.len() <= 512 << 10);
        let sent: Batch = serde_json::from_slice(&bytes).unwrap();
        sent.validate().unwrap();
        assert_eq!(sent.id, identity);
        assert!(
            spool.diagnostics()["omitted_host_devices"]
                .as_u64()
                .unwrap()
                > 0
        );
        drop(spool);
        let diagnostics = Spool::open(dir.path().into(), 4096).unwrap().diagnostics();
        assert!(diagnostics["omitted_host_devices"].as_u64().unwrap() > 0);
        assert_eq!(diagnostics["disk_state"], "ready");
        assert!(diagnostics["disk_file_bytes"].as_u64().unwrap() >= bytes.len() as u64);
    }
    #[test]
    fn namespace_sync_failure_does_not_strand_history_or_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        spool.push(&mut batch()).unwrap();
        spool.push(&mut batch()).unwrap();
        let (path, _) = spool.first().unwrap().unwrap();
        spool.sync.fail_next = true;
        assert!(spool.ack(&path).is_err());
        assert_ne!(spool.first().unwrap().unwrap().0, path);
    }
    #[test]
    fn size_waiting_original_does_not_block_compatible_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut spool = Spool::open(dir.path().into(), 4096).unwrap();
        let mut large: Batch = serde_json::from_slice(include_bytes!(
            "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        spool.push(&mut large).unwrap();
        let (original_path, original) = spool.first().unwrap().unwrap();
        let mut small = batch();
        spool.push(&mut small).unwrap();
        let (_, bytes) = spool.next_compatible(500).unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Batch>(&bytes).unwrap().id,
            small.id
        );
        assert_eq!(fs::read(original_path).unwrap(), original);
        assert_eq!(spool.schema_waiting(500), Some(1));
    }
    #[test]
    fn old_offline_batch_reopens_with_original_bytes_and_digest() {
        use sha2::{Digest, Sha256};
        const ORIGINAL: &[u8] =
            include_bytes!("../../../emissary-protocol/tests/fixtures/telemetry-batch.json");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("00000000000000000000-original.batch");
        fs::write(&path, ORIGINAL).unwrap();
        for _ in 0..2 {
            let mut spool = Spool::open(directory.path().into(), 1 << 20).unwrap();
            let (_, bytes) = spool.first().unwrap().unwrap();
            assert_eq!(bytes, ORIGINAL);
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                "132f630ebbcaa6195a712c3e17cd9b178ef8f5ee3ad96a4b9bb518bd30f1d6f0"
            );
            let batch: Batch = serde_json::from_slice(&bytes).unwrap();
            batch.validate().unwrap();
            assert_eq!(serde_json::to_vec(&batch).unwrap(), ORIGINAL);
        }
        let mut spool = Spool::open(directory.path().into(), 1 << 20).unwrap();
        spool.ack(&path).unwrap();
        assert!(spool.first().unwrap().is_none());
    }
    #[cfg(unix)]
    #[test]
    fn releases_lock_while_a_forked_descriptor_is_still_open() {
        let dir = tempfile::tempdir().unwrap();
        let spool = Spool::open(dir.path().into(), 800).unwrap();
        // A forked child holds the same open file description until exec closes
        // its CLOEXEC descriptors. A duplicate reproduces that interval.
        let inherited = spool.lock.try_clone().unwrap();
        assert!(Spool::open(dir.path().into(), 800).is_err());
        drop(spool);
        let reopened = Spool::open(dir.path().into(), 800).unwrap();
        assert!(Spool::open(dir.path().into(), 800).is_err());
        drop(inherited);
        drop(reopened);
    }
    #[test]
    fn quota_eviction_survives_restart_and_late_ack() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path().into(), 800).unwrap();
        s.push(&mut batch()).unwrap();
        let (first, _) = s.first().unwrap().unwrap();
        for _ in 0..8 {
            s.push(&mut batch()).unwrap();
        }
        assert!(s.bytes <= 800);
        assert!(s.dropped() > 0);
        s.ack(&first).unwrap();
        let dropped = s.dropped();
        let bytes = s.first().unwrap().unwrap().1;
        drop(s);
        let mut s = Spool::open(dir.path().into(), 800).unwrap();
        assert_eq!(s.dropped(), dropped);
        assert_eq!(s.first().unwrap().unwrap().1, bytes);
        let (path, _) = s.first().unwrap().unwrap();
        s.ack(&path).unwrap();
    }
}

#[cfg(test)]
#[path = "spool_removal_tests.rs"]
mod removal_tests;
