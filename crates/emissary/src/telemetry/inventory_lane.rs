//! [INPUT]: 独立清单队列、启动身份和私有检查点。
//! [OUTPUT]: 隔离清单存储故障与永久冲突，保留原记录和未知计数。
//! [POS]: 普通采样与清单持久化之间的故障边界；不猜测损坏身份。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::inventory_delivery::InventoryQueue;
use anyhow::{Result, anyhow};
use artifactory_emissary_protocol::{InventoryAck, InventoryObservation, telemetry::Inventory};
use std::path::PathBuf;
use uuid::Uuid;

pub(super) struct InventoryLane(Option<InventoryQueue>);
impl InventoryLane {
    pub fn open(root: PathBuf) -> Self {
        Self(InventoryQueue::open(root).ok())
    }
    fn queue(&mut self) -> Result<&mut InventoryQueue> {
        self.0
            .as_mut()
            .ok_or_else(|| anyhow!("inventory storage unavailable"))
    }
    pub fn available(&self) -> bool {
        self.0.is_some()
    }
    pub fn set_current(&mut self, startup: Uuid) {
        if let Some(queue) = &mut self.0 {
            queue.set_current(startup);
        }
    }
    pub fn seed_startup(
        &mut self,
        startup: Uuid,
        observed: chrono::DateTime<chrono::Utc>,
        inventory: &Inventory,
    ) -> Result<()> {
        let result = self.queue()?.seed_startup(startup, observed, inventory);
        if result.is_err() {
            self.0 = None;
        }
        result
    }
    pub fn push(&mut self, observation: InventoryObservation) -> Result<()> {
        self.queue()?.push(observation)
    }
    pub fn refresh(
        &mut self,
        startup: Uuid,
        inventory: &Inventory,
        monotonic: u64,
    ) -> Result<artifactory_emissary_protocol::operations::InventoryRefresh> {
        self.queue()?.refresh(startup, inventory, monotonic)
    }
    pub fn first_compatible(&mut self, max: u64) -> Result<Option<InventoryObservation>> {
        self.queue()?.first_compatible(max)
    }
    #[cfg(test)]
    pub fn first(&mut self) -> Result<Option<InventoryObservation>> {
        self.first_compatible(u64::MAX)
    }
    pub fn pin(&mut self, id: &str) {
        if let Some(queue) = &mut self.0 {
            queue.pin(id);
        }
    }
    pub fn unpin(&mut self) {
        if let Some(queue) = &mut self.0 {
            queue.unpin();
        }
    }
    pub fn ack(&mut self, ack: &InventoryAck) -> Result<bool> {
        self.queue()?.ack(ack)
    }
    pub fn conflict(&mut self, observation: &InventoryObservation) -> Result<()> {
        self.queue()?.conflict(observation)
    }
    pub fn conflicts(&self) -> Option<usize> {
        self.0.as_ref().map(InventoryQueue::conflicts)
    }
    pub fn conflict_diagnostics(&self) -> serde_json::Value {
        self.0.as_ref().map_or_else(
            || {
                serde_json::json!({"inventory_conflict_records":null,
            "inventory_conflicts_total":null,"inventory_last_conflict_at":null})
            },
            InventoryQueue::conflict_diagnostics,
        )
    }
    pub fn waiting(&self, max: u64) -> Option<usize> {
        self.0.as_ref().map(|queue| queue.waiting(max))
    }
    pub fn bytes(&self) -> Option<u64> {
        self.0.as_ref().map(InventoryQueue::bytes)
    }
    pub fn len(&self) -> Option<usize> {
        self.0.as_ref().map(InventoryQueue::len)
    }
    pub fn oldest_age(&self) -> Option<i64> {
        self.0.as_ref().and_then(InventoryQueue::oldest_age)
    }
    pub fn superseded(&self) -> Option<u64> {
        self.0.as_ref().map(InventoryQueue::superseded)
    }
}
