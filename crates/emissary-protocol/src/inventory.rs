// [INPUT]: 不可变Inventory v1、启动身份与原始观察时间。
// [OUTPUT]: 校验独立清单、固定交付摘要与旧批次迁移身份。
// [POS]: 清单持久记录和接收事务之间的共享身份边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    InventoryAck, InventoryObservation,
    telemetry::{Batch, Inventory},
    telemetry_negotiation::INVENTORY_SCHEMA_V1,
};
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_INVENTORY_BYTES: usize = 256 << 10;

impl InventoryObservation {
    pub fn new(
        startup_id: Uuid,
        revision: u64,
        observed_at: DateTime<Utc>,
        monotonic_ms: u64,
        inventory: &Inventory,
    ) -> Result<Self> {
        let mut observation = Self {
            observation_id: Uuid::new_v4().to_string(),
            source_startup_id: startup_id.to_string(),
            revision,
            observed_at_ms: observed_at.timestamp_millis(),
            monotonic_ms,
            inventory_schema: INVENTORY_SCHEMA_V1,
            inventory_json: serde_json::to_vec(inventory)?,
            delivery_digest: String::new(),
            source_batch_id: String::new(),
        };
        observation.delivery_digest = observation.computed_digest();
        observation.validate()?;
        Ok(observation)
    }

    /// 迁移保持采集时间；来源UUID构成独立legacy命名空间。
    pub fn from_legacy_batch(batch: &Batch) -> Result<Option<Self>> {
        let Some(inventory) = &batch.inventory else {
            return Ok(None);
        };
        batch.validate()?;
        let mut observation = Self::new(
            batch.session_id,
            0,
            batch.sampled_at,
            batch.monotonic_ms,
            inventory,
        )?;
        observation.source_batch_id = batch.id.to_string();
        let mut hash = Sha256::new();
        hash.update(b"artifactory.inventory.legacy-id.v1\0");
        hash.update(batch.session_id.as_bytes());
        hash.update(batch.id.as_bytes());
        let digest = hash.finalize();
        let mut bytes: [u8; 16] = digest[..16].try_into()?;
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        observation.observation_id = Uuid::from_bytes(bytes).to_string();
        observation.delivery_digest = observation.computed_digest();
        observation.validate()?;
        Ok(Some(observation))
    }

    pub fn validate(&self) -> Result<Inventory> {
        for id in [&self.observation_id, &self.source_startup_id] {
            let parsed = Uuid::parse_str(id)?;
            ensure!(
                !parsed.is_nil() && parsed.to_string() == *id,
                "invalid inventory UUID"
            );
        }
        if !self.source_batch_id.is_empty() {
            let parsed = Uuid::parse_str(&self.source_batch_id)?;
            ensure!(
                !parsed.is_nil() && parsed.to_string() == self.source_batch_id,
                "invalid source batch UUID"
            );
        }
        ensure!(
            self.inventory_schema == INVENTORY_SCHEMA_V1,
            "unsupported inventory schema"
        );
        ensure!(
            self.inventory_json.len() <= MAX_INVENTORY_BYTES,
            "oversized inventory"
        );
        ensure!(
            self.revision <= i64::MAX as u64 && self.monotonic_ms <= i64::MAX as u64,
            "invalid inventory counter"
        );
        ensure!(self.observed_at_ms >= 0, "invalid inventory time");
        let sampled_at = DateTime::from_timestamp_millis(self.observed_at_ms)
            .ok_or_else(|| anyhow::anyhow!("invalid inventory time"))?;
        ensure!(
            self.delivery_digest == self.computed_digest(),
            "inventory delivery digest mismatch"
        );
        let inventory: Inventory = serde_json::from_slice(&self.inventory_json)?;
        Batch {
            id: Uuid::parse_str(&self.observation_id)?,
            session_id: Uuid::parse_str(&self.source_startup_id)?,
            sampled_at,
            monotonic_ms: self.monotonic_ms,
            dropped_batches: 0,
            omitted_processes: 0,
            host: None,
            processes: vec![],
            inventory: Some(inventory.clone()),
        }
        .validate()?;
        Ok(inventory)
    }

    /// 字段以u64大端长度前缀分隔，整数以固定大端字节编码。
    pub fn computed_digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"artifactory.inventory.delivery.v1\0");
        for field in [
            self.observation_id.as_bytes(),
            self.source_startup_id.as_bytes(),
            self.revision.to_be_bytes().as_slice(),
            self.observed_at_ms.to_be_bytes().as_slice(),
            self.monotonic_ms.to_be_bytes().as_slice(),
            self.inventory_schema.to_be_bytes().as_slice(),
            self.inventory_json.as_slice(),
            self.source_batch_id.as_bytes(),
        ] {
            hash.update((field.len() as u64).to_be_bytes());
            hash.update(field);
        }
        format!("{:x}", hash.finalize())
    }

    pub fn matches_ack(&self, ack: &InventoryAck) -> bool {
        ack.observation_id == self.observation_id && ack.delivery_digest == self.delivery_digest
    }
}
