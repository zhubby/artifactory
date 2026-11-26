//! [INPUT]: 依赖不可变清单观察、私有原子文件和单写者spool锁。
//! [OUTPUT]: 保留待确认清单，持久隔离永久冲突，匹配业务回执后删除。
//! [POS]: 采样可淘汰队列之外的清单交付边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[cfg(test)]
use crate::config::atomic_bytes;
use crate::config::{atomic_json, private_dir};
use anyhow::{Context, Result, ensure};
use artifactory_emissary_protocol::{InventoryAck, InventoryObservation};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::PathBuf};
use uuid::Uuid;

const MAX_RECORDS: usize = 16;
pub(super) const QUOTA: u64 = 4 << 20;
const MAX_SUPERSEDED: usize = 64;
const MAX_RECORD_BYTES: u64 = (256 << 10) + 4096;

#[derive(Clone, Default, Serialize, Deserialize)]
struct Stats {
    superseded: u64,
    #[serde(default)]
    recent: Vec<Superseded>,
    #[serde(default)]
    conflicts: Vec<Conflict>,
    #[serde(default)]
    conflicts_total: u64,
    #[serde(default)]
    last_conflict_at: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Superseded {
    observation_id: String,
    reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Conflict {
    observation_id: String,
    delivery_digest: String,
}
#[derive(Serialize, Deserialize)]
struct Startup {
    startup_id: Uuid,
    observation_id: String,
    acknowledged: bool,
}

struct Record {
    path: PathBuf,
    bytes: u64,
    observation: InventoryObservation,
}

pub(super) struct InventoryQueue {
    root: PathBuf,
    records: Vec<Record>,
    current: Option<Uuid>,
    stats: Stats,
    pinned: Option<String>,
    sync: super::durability::DirectorySync,
}

impl InventoryQueue {
    pub fn open(root: PathBuf) -> Result<Self> {
        private_dir(&root)?;
        let stats: Stats = match fs::File::open(root.join("stats.json")) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take((64 << 10) + 1).read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 64 << 10, "oversized inventory statistics");
                serde_json::from_slice(&bytes)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stats::default(),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            stats.conflicts.len() <= 64 && stats.conflicts_total >= stats.conflicts.len() as u64,
            "invalid inventory conflict index"
        );
        let mut keys = std::collections::HashSet::new();
        for conflict in &stats.conflicts {
            ensure!(
                Uuid::parse_str(&conflict.observation_id)?.to_string() == conflict.observation_id
                    && conflict.delivery_digest.len() == 64
                    && conflict
                        .delivery_digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    && keys.insert((&conflict.observation_id, &conflict.delivery_digest)),
                "invalid inventory conflict identity"
            );
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.path().extension().is_none_or(|s| s != "inventory") {
                continue;
            }
            ensure!(
                entry.file_type()?.is_file() && entry.metadata()?.len() <= MAX_RECORD_BYTES,
                "invalid inventory record"
            );
            let observation = InventoryObservation::decode(fs::read(entry.path())?.as_slice())?;
            observation.validate()?;
            ensure!(
                entry
                    .path()
                    .file_stem()
                    .is_some_and(|s| s == observation.observation_id.as_str()),
                "inventory filename mismatch"
            );
            records.push(Record {
                path: entry.path(),
                bytes: entry.metadata()?.len(),
                observation,
            });
        }
        records.sort_by_key(|r| {
            (
                r.observation.observed_at_ms,
                r.observation.revision,
                r.observation.observation_id.clone(),
            )
        });
        ensure!(
            records.len() <= MAX_RECORDS + 1,
            "inventory queue record limit exceeded"
        );
        ensure!(
            stats.recent.len() <= MAX_SUPERSEDED,
            "oversized superseded index"
        );
        let current = match fs::read(root.join("startup.json")) {
            Ok(bytes) => {
                ensure!(bytes.len() <= 4096, "oversized startup checkpoint");
                Some(serde_json::from_slice::<Startup>(&bytes)?.startup_id)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            root,
            records,
            current,
            stats,
            pinned: None,
            sync: Default::default(),
        })
    }

    pub fn set_current(&mut self, startup: Uuid) {
        self.current = Some(startup);
    }

    pub fn seed_startup(
        &mut self,
        startup: Uuid,
        observed_at: chrono::DateTime<chrono::Utc>,
        inventory: &artifactory_emissary_protocol::telemetry::Inventory,
    ) -> Result<()> {
        self.set_current(startup);
        // 恢复先完成上次写入后的额度清扫，再读取已确认检查点。
        self.trim()?;
        let checkpoint = self.root.join("startup.json");
        if let Ok(bytes) = fs::read(&checkpoint) {
            ensure!(bytes.len() <= 4096, "oversized startup checkpoint");
            let saved: Startup = serde_json::from_slice(&bytes)?;
            if saved.startup_id == startup {
                ensure!(
                    saved.acknowledged
                        || self
                            .records
                            .iter()
                            .any(|r| r.observation.observation_id == saved.observation_id),
                    "pending startup inventory missing"
                );
                return Ok(());
            }
        }
        let existing = self
            .records
            .iter()
            .find(|r| {
                r.observation.source_startup_id == startup.to_string()
                    && r.observation.revision == 0
                    && r.observation.source_batch_id.is_empty()
            })
            .map(|r| r.observation.clone());
        let observation = match existing {
            Some(observation) => observation,
            None => InventoryObservation::new(startup, 0, observed_at, 0, inventory)?,
        };
        self.push(observation.clone())?;
        atomic_json(
            &checkpoint,
            &Startup {
                startup_id: startup,
                observation_id: observation.observation_id,
                acknowledged: false,
            },
        )
    }

    pub fn refresh(
        &mut self,
        startup: Uuid,
        inventory: &artifactory_emissary_protocol::telemetry::Inventory,
        monotonic_ms: u64,
    ) -> Result<artifactory_emissary_protocol::operations::InventoryRefresh> {
        #[derive(Serialize, Deserialize)]
        struct Revision {
            startup_id: Uuid,
            last: u64,
        }
        let path = self.root.join("revision.json");
        let last = match fs::read(&path) {
            Ok(bytes) => {
                ensure!(bytes.len() <= 4096, "oversized revision checkpoint");
                let saved: Revision = serde_json::from_slice(&bytes)?;
                if saved.startup_id == startup {
                    saved.last
                } else {
                    0
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e.into()),
        };
        let revision = last
            .checked_add(1)
            .context("inventory revision exhausted")?;
        atomic_json(
            &path,
            &Revision {
                startup_id: startup,
                last: revision,
            },
        )?;
        let observation = InventoryObservation::new(
            startup,
            revision,
            chrono::Utc::now(),
            monotonic_ms,
            inventory,
        )?;
        let id = Uuid::parse_str(&observation.observation_id)?;
        self.push(observation)?;
        Ok(
            artifactory_emissary_protocol::operations::InventoryRefresh {
                observation_id: id,
                revision,
                delivery: artifactory_emissary_protocol::operations::InventoryDelivery::Pending,
            },
        )
    }

    pub fn push(&mut self, observation: InventoryObservation) -> Result<()> {
        self.sync.flush(&self.root)?;
        self.trim()?;
        observation.validate()?;
        if let Some(record) = self
            .records
            .iter()
            .find(|r| r.observation.observation_id == observation.observation_id)
        {
            ensure!(
                record.observation.delivery_digest == observation.delivery_digest,
                "inventory ID content conflict"
            );
            return Ok(());
        }
        let bytes = observation.encode_to_vec();
        ensure!(
            bytes.len() as u64 <= MAX_RECORD_BYTES,
            "oversized inventory record"
        );
        // 新记录先耐久写入；额度清扫不能先移除唯一当前清单。
        let path = self
            .root
            .join(format!("{}.inventory", observation.observation_id));
        super::durability::write_record(&path, &bytes)?;
        self.records.push(Record {
            path,
            bytes: bytes.len() as u64,
            observation,
        });
        self.records.sort_by_key(|r| {
            (
                r.observation.observed_at_ms,
                r.observation.revision,
                r.observation.observation_id.clone(),
            )
        });
        self.sync.changed(&self.root)?;
        self.trim()
    }

    fn trim(&mut self) -> Result<()> {
        while self.records.len() > MAX_RECORDS || self.bytes() > QUOTA {
            let latest = self
                .records
                .iter()
                .filter(|r| self.is_current(r) && r.observation.source_batch_id.is_empty())
                .map(|r| r.observation.revision)
                .max();
            let index = self
                .records
                .iter()
                .position(|r| {
                    self.pinned.as_ref() != Some(&r.observation.observation_id)
                        && (!self.is_current(r)
                            || (!r.observation.source_batch_id.is_empty())
                            || (r.observation.revision != 0
                                && Some(r.observation.revision) != latest))
                })
                .context("protected inventories exceed quota")?;
            let id = &self.records[index].observation.observation_id;
            // 先记录再删除；崩溃重试同一删除不重复增加计数。
            if !self
                .stats
                .recent
                .iter()
                .any(|entry| &entry.observation_id == id)
            {
                let mut next = self.stats.clone();
                next.superseded = next.superseded.saturating_add(1);
                next.recent.push(Superseded {
                    observation_id: id.clone(),
                    reason: "inventory_quota".into(),
                });
                if next.recent.len() > MAX_SUPERSEDED {
                    next.recent.remove(0);
                }
                self.save_stats(next)?;
            }
            self.remove(index)?;
        }
        Ok(())
    }

    fn is_current(&self, record: &Record) -> bool {
        self.current
            .is_some_and(|id| record.observation.source_startup_id == id.to_string())
    }

    pub fn pin(&mut self, id: &str) {
        self.pinned = Some(id.into());
    }
    pub fn unpin(&mut self) {
        self.pinned = None;
    }
    #[cfg(test)]
    pub fn first(&mut self) -> Result<Option<InventoryObservation>> {
        self.first_compatible(u64::MAX)
    }
    pub fn first_compatible(&mut self, max: u64) -> Result<Option<InventoryObservation>> {
        self.sync.flush(&self.root)?;
        Ok(self
            .records
            .iter()
            .filter(|r| {
                r.observation.inventory_json.len() as u64 <= max
                    && !self.is_conflicted(&r.observation)
            })
            .find(|r| self.is_current(r))
            .or_else(|| {
                self.records.iter().find(|r| {
                    r.observation.inventory_json.len() as u64 <= max
                        && !self.is_conflicted(&r.observation)
                })
            })
            .map(|r| r.observation.clone()))
    }
    pub fn waiting(&self, max: u64) -> usize {
        self.records
            .iter()
            .filter(|r| {
                r.observation.inventory_json.len() as u64 > max
                    && !self.is_conflicted(&r.observation)
            })
            .count()
    }
    pub fn oldest_age(&self) -> Option<i64> {
        self.records
            .iter()
            .map(|r| {
                (chrono::Utc::now().timestamp_millis() - r.observation.observed_at_ms).max(0) / 1000
            })
            .max()
    }

    pub fn ack(&mut self, ack: &InventoryAck) -> Result<bool> {
        self.sync.flush(&self.root)?;
        if let Some(index) = self
            .records
            .iter()
            .position(|r| r.observation.matches_ack(ack))
        {
            let checkpoint = self.root.join("startup.json");
            if let Ok(bytes) = fs::read(&checkpoint) {
                ensure!(bytes.len() <= 4096, "oversized startup checkpoint");
                let mut saved: Startup = serde_json::from_slice(&bytes)?;
                if saved.observation_id == ack.observation_id {
                    saved.acknowledged = true;
                    atomic_json(&checkpoint, &saved)?;
                }
            }
            self.remove(index)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn remove(&mut self, index: usize) -> Result<()> {
        fs::remove_file(&self.records[index].path)?;
        self.records.remove(index);
        self.sync.changed(&self.root)
    }

    pub fn bytes(&self) -> u64 {
        self.records.iter().map(|r| r.bytes).sum()
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn superseded(&self) -> u64 {
        self.stats.superseded
    }
    fn is_conflicted(&self, observation: &InventoryObservation) -> bool {
        self.stats.conflicts.iter().any(|entry| {
            entry.observation_id == observation.observation_id
                && entry.delivery_digest == observation.delivery_digest
        })
    }
    fn save_stats(&mut self, next: Stats) -> Result<()> {
        let bytes = serde_json::to_vec(&next)?;
        ensure!(bytes.len() <= 64 << 10, "oversized inventory statistics");
        super::durability::write_record(&self.root.join("stats.json"), &bytes)?;
        // rename已提交，内存先采用新索引；目录同步失败保留债务。
        self.stats = next;
        self.sync.changed(&self.root)
    }
    pub fn conflict(&mut self, observation: &InventoryObservation) -> Result<()> {
        self.sync.flush(&self.root)?;
        ensure!(
            self.records
                .iter()
                .any(|record| record.observation == *observation),
            "inventory conflict source differs"
        );
        if self.is_conflicted(observation) {
            return Ok(());
        }
        let mut next = self.stats.clone();
        while next.conflicts.len() >= 64 {
            let inactive = next
                .conflicts
                .iter()
                .position(|entry| {
                    !self.records.iter().any(|record| {
                        record.observation.observation_id == entry.observation_id
                            && record.observation.delivery_digest == entry.delivery_digest
                    })
                })
                .context("active inventory conflict index full")?;
            next.conflicts.remove(inactive);
        }
        next.conflicts.push(Conflict {
            observation_id: observation.observation_id.clone(),
            delivery_digest: observation.delivery_digest.clone(),
        });
        next.conflicts_total = next.conflicts_total.saturating_add(1);
        next.last_conflict_at = Some(chrono::Utc::now());
        self.save_stats(next)
    }
    pub fn conflicts(&self) -> usize {
        self.records
            .iter()
            .filter(|record| self.is_conflicted(&record.observation))
            .count()
    }
    pub fn conflict_diagnostics(&self) -> serde_json::Value {
        serde_json::json!({"inventory_conflict_records":self.conflicts(),
            "inventory_conflicts_total":self.stats.conflicts_total,
            "inventory_last_conflict_at":self.stats.last_conflict_at})
    }
}

#[cfg(test)]
#[path = "inventory_conflict_tests.rs"]
mod conflict_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use artifactory_emissary_protocol::telemetry::Inventory;

    fn observation(startup: Uuid, revision: u64) -> InventoryObservation {
        InventoryObservation::new(
            startup,
            revision,
            chrono::Utc::now(),
            0,
            &Inventory {
                os: "linux".into(),
                arch: "arm64".into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn mismatched_ack_preserves_record_and_duplicate_id_cannot_replace_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let mut queue = InventoryQueue::open(directory.path().into()).unwrap();
        let record = observation(Uuid::new_v4(), 0);
        queue.push(record.clone()).unwrap();
        let ack = InventoryAck {
            observation_id: record.observation_id.clone(),
            delivery_digest: "wrong".into(),
            ..Default::default()
        };
        assert!(!queue.ack(&ack).unwrap());
        let mut conflict = record.clone();
        conflict.revision = 1;
        conflict.delivery_digest = conflict.computed_digest();
        assert!(queue.push(conflict).is_err());
        drop(queue);
        let mut queue = InventoryQueue::open(directory.path().into()).unwrap();
        assert_eq!(queue.first().unwrap(), Some(record.clone()));
        assert!(
            queue
                .ack(&InventoryAck {
                    observation_id: record.observation_id,
                    delivery_digest: record.delivery_digest,
                    ..Default::default()
                })
                .unwrap()
        );
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn restart_finishes_interrupted_trim_even_with_acknowledged_startup() {
        let directory = tempfile::tempdir().unwrap();
        let startup = Uuid::new_v4();
        let mut queue = InventoryQueue::open(directory.path().into()).unwrap();
        queue
            .seed_startup(startup, chrono::Utc::now(), &Inventory::default())
            .unwrap();
        let seed = queue.first().unwrap().unwrap();
        queue
            .ack(&InventoryAck {
                observation_id: seed.observation_id,
                delivery_digest: seed.delivery_digest,
                ..Default::default()
            })
            .unwrap();
        for _ in 0..MAX_RECORDS {
            queue.push(observation(Uuid::new_v4(), 0)).unwrap();
        }
        // 模拟新记录fsync后、trim前断电。
        let extra = observation(Uuid::new_v4(), 0);
        atomic_bytes(
            &directory
                .path()
                .join(format!("{}.inventory", extra.observation_id)),
            &extra.encode_to_vec(),
            false,
        )
        .unwrap();
        drop(queue);
        let mut recovered = InventoryQueue::open(directory.path().into()).unwrap();
        recovered
            .seed_startup(startup, chrono::Utc::now(), &Inventory::default())
            .unwrap();
        assert_eq!(recovered.len(), MAX_RECORDS);
        assert_eq!(recovered.superseded(), 1);
        assert_eq!(recovered.stats.recent.len(), 1);
        assert_eq!(recovered.stats.recent[0].reason, "inventory_quota");
        assert!(recovered.first().unwrap().is_some());
    }

    #[test]
    fn size_waiting_inventory_is_not_sent_and_unlink_sync_failure_recovers() {
        let directory = tempfile::tempdir().unwrap();
        let mut queue = InventoryQueue::open(directory.path().into()).unwrap();
        let observation = observation(Uuid::new_v4(), 0);
        queue.push(observation.clone()).unwrap();
        assert!(queue.first_compatible(1).unwrap().is_none());
        assert_eq!(queue.waiting(1), 1);
        queue.sync.fail_next = true;
        assert!(
            queue
                .ack(&InventoryAck {
                    observation_id: observation.observation_id,
                    delivery_digest: observation.delivery_digest,
                    ..Default::default()
                })
                .is_err()
        );
        assert!(queue.first().unwrap().is_none());
        queue
            .push(super::tests::observation(Uuid::new_v4(), 0))
            .unwrap();
        assert!(queue.first().unwrap().is_some());
    }
    #[test]
    fn historical_restarts_do_not_evict_current_startup_or_latest_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let mut queue = InventoryQueue::open(directory.path().into()).unwrap();
        let startup = Uuid::new_v4();
        queue.set_current(startup);
        let first = observation(startup, 0);
        queue.push(first.clone()).unwrap();
        let latest = observation(startup, 7);
        queue.push(latest.clone()).unwrap();
        for _ in 0..30 {
            queue.push(observation(Uuid::new_v4(), 0)).unwrap();
        }
        assert_eq!(queue.len(), MAX_RECORDS);
        assert!(queue.bytes() <= QUOTA);
        assert!(queue.superseded() > 0);
        assert!(queue.records.iter().any(|r| r.observation == first));
        assert!(queue.records.iter().any(|r| r.observation == latest));
    }
}
