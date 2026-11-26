// [INPUT]: 依赖节点启动身份、有界元数据和 UTC 时间。
// [OUTPUT]: 导出分队列及清单冲突有界诊断、心跳解码与选机契约。
// [POS]: 节点协议的分配边界；隔离旧启动采样与当前选机。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

/// Maximum encoded startup snapshot, including capabilities and startup identity.
pub const MAX_METADATA_BYTES: usize = 64 * 1024;
/// 网络诊断的完整 JSON 预算，不进入严格遥测契约。
pub const MAX_NETWORK_INTERFACES_BYTES: usize = 8 * 1024;
/// 链路诊断只传播有限键；旧节点缺失时保持未知。
pub const MAX_DELIVERY_DIAGNOSTICS_BYTES: usize = 4 * 1024;
pub fn delivery_diagnostics(value: &Value) -> Value {
    const KEYS: &[&str] = &[
        "state",
        "spool_bytes",
        "spool_records",
        "oldest_age_seconds",
        "inventory_bytes",
        "inventory_records",
        "inventory_superseded",
        "inventory_conflict_records",
        "inventory_conflicts_total",
        "inventory_last_conflict_at",
        "inventory_oldest_age_seconds",
        "fresh_records",
        "fresh_oldest_age_seconds",
        "quota_eviction_batches",
        "expired_batches",
        "malformed_batches",
        "disk_errors",
        "schema_wait_records",
        "inventory_schema_wait_records",
        "dropped_batches",
        "fresh_bytes",
        "coalesced_batches",
        "corrupt_records",
        "last_successful_ack_at",
        "last_inventory_ack_at",
        "upload_latency_ms",
        "backoff_seconds",
        "inventory_state",
        "collector_duration_ms",
        "collector_error",
        "negotiation_state",
        "last_rejection_reason",
        "last_rejection_at",
        "bounds_exceeded_batches",
        "id_content_conflict_batches",
        "identity_retired_batches",
        "fresh_state",
        "fresh_last_ack_at",
        "fresh_last_progress_at",
        "fresh_upload_latency_ms",
        "fresh_backoff_seconds",
        "history_state",
        "history_last_ack_at",
        "history_last_progress_at",
        "history_upload_latency_ms",
        "history_backoff_seconds",
        "inventory_last_ack_at",
        "inventory_last_progress_at",
        "inventory_upload_latency_ms",
        "inventory_backoff_seconds",
        "disk_state",
        "disk_observed_at",
        "disk_file_bytes",
        "disk_record_bytes",
        "disk_allocated_bytes",
        "disk_block_overhead_bytes",
        "disk_temporary_bytes",
        "disk_index_bytes",
        "disk_corrupt_bytes",
        "disk_unknown_bytes",
        "disk_files",
        "omitted_host_devices",
        "truncated_host_fields",
    ];
    let mut fields = serde_json::Map::new();
    for key in KEYS {
        if let Some(value) = value.get(key)
            && (value.is_number()
                || value.is_null()
                || value
                    .as_str()
                    .is_some_and(|s| s.len() <= 128 && !s.chars().any(char::is_control)))
        {
            fields.insert((*key).into(), value.clone());
        }
    }
    let result = Value::Object(fields);
    if serde_json::to_vec(&result).is_ok_and(|bytes| bytes.len() <= MAX_DELIVERY_DIAGNOSTICS_BYTES)
    {
        result
    } else {
        serde_json::json!({"state":"diagnostics_bounds_exceeded"})
    }
}
/// 完整 hello/heartbeat：64 KiB 启动、8 KiB 网络、24 KiB 运行信封。
/// 运行信封包含最多 32 项租约、OTA、指标及全部 JSON 字段开销。
/// 原有 72 KiB 信封加完整网络字段仍低于此上限。
pub const MAX_HEARTBEAT_BYTES: usize =
    MAX_METADATA_BYTES + MAX_NETWORK_INTERFACES_BYTES + 24 * 1024;
/// Maximum encoded node tool response, including pagination and timestamps.
pub const MAX_QUERY_BYTES: usize = 128 * 1024;

/// 在 JSON 解码前限制完整心跳，并保留旧节点能力数组校验。
///
/// # Errors
/// 拒绝超额信封、无效 JSON 或超过 256 项的能力数组。
pub fn decode_heartbeat(payload_json: &str) -> Result<Value> {
    ensure!(
        payload_json.len() <= MAX_HEARTBEAT_BYTES,
        "oversized heartbeat"
    );
    let payload: Value =
        serde_json::from_str(payload_json).map_err(|_| anyhow::anyhow!("invalid heartbeat"))?;
    if let Some(value) = payload
        .pointer("/ota/observability")
        .filter(|v| !v.is_null())
    {
        let observation: crate::ota::Observability = serde_json::from_value(value.clone())?;
        observation.validate()?;
    }
    if let Some(value) = payload.pointer("/diagnostics/metrics/telemetry") {
        ensure!(
            serde_json::to_vec(value)?.len() <= MAX_DELIVERY_DIAGNOSTICS_BYTES,
            "oversized telemetry diagnostics"
        );
    }
    if let Some(capabilities) = payload.get("capabilities") {
        ensure!(
            capabilities
                .as_array()
                .is_some_and(|values| values.len() <= 256),
            "invalid capabilities"
        );
    }
    Ok(payload)
}

/// 统一注册与改名输入；长度按 UTF-8 字节计算。
///
/// # Errors
/// 名称去除首尾空白后为空、超过 100 字节或包含控制字符时拒绝。
pub fn normalize_node_name(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(
        !name.is_empty() && name.len() <= 100 && !name.chars().any(char::is_control),
        "node name must contain 1..100 UTF-8 bytes without control characters"
    );
    Ok(name.to_owned())
}

/// An authorized node and the current runtime identity whose resources may be used.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceIdentity {
    pub node_id: Uuid,
    pub startup_id: Uuid,
}

/// One immutable snapshot per runtime process; reconnects reuse exactly these values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataSnapshot {
    pub startup_id: Uuid,
    pub metadata: BTreeMap<String, Value>,
    pub capabilities: Vec<String>,
}
impl MetadataSnapshot {
    /// Construct a snapshot after validating reserved namespaces and the encoded budget.
    ///
    /// # Errors
    /// Rejects invalid keys, startup IDs, capabilities or oversized snapshots.
    pub fn new(
        startup_id: Uuid,
        metadata: BTreeMap<String, Value>,
        capabilities: Vec<String>,
    ) -> Result<Self> {
        let snapshot = Self {
            startup_id,
            metadata,
            capabilities,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    /// Validate data received over the authenticated node channel.
    ///
    /// # Errors
    /// Rejects unrecognized namespaces or values exceeding the snapshot budget.
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.startup_id.is_nil(), "invalid startup ID");
        ensure!(
            self.capabilities.len() <= 256
                && self
                    .capabilities
                    .iter()
                    .all(|v| !v.is_empty() && v.len() <= 128 && !v.contains('\0')),
            "invalid capabilities"
        );
        ensure!(self.metadata.len() <= 1024, "too many metadata keys");
        for key in self.metadata.keys() {
            ensure!(
                key.len() <= 256
                    && key.split('.').all(|part| !part.is_empty()
                        && part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)))
                    && ["system.", "hardware.", "tools.", "emissary.", "custom."]
                        .iter()
                        .any(|prefix| key.starts_with(prefix)),
                "invalid metadata namespace"
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_METADATA_BYTES,
            "metadata exceeds 64 KiB"
        );
        Ok(())
    }
    /// Hash canonical metadata and capabilities; process identity is checked separately.
    ///
    /// # Errors
    /// Returns JSON serialization errors.
    pub fn digest(&self) -> Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(&self.metadata, &self.capabilities))?)
        ))
    }
}

/// Resource thresholds checked against a fresh snapshot, without exclusive reservations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRequirements {
    pub min_available_memory_bytes: Option<u64>,
    pub min_build_disk_available_bytes: Option<u64>,
    pub max_cpu_percent: Option<f64>,
}
impl ResourceRequirements {
    /// Validate representable capacities and normalized CPU utilization.
    ///
    /// # Errors
    /// Rejects out of range capacities and nonfinite percentages.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.min_available_memory_bytes,
                self.min_build_disk_available_bytes
            ]
            .into_iter()
            .all(|v| v.is_none_or(|v| v <= i64::MAX as u64)),
            "invalid resource capacity"
        );
        ensure!(
            self.max_cpu_percent
                .is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v)),
            "invalid CPU threshold"
        );
        Ok(())
    }
}
/// Latest host resources carry both event and server clocks and the runtime identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub startup_id: Uuid,
    pub sampled_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub available_memory_bytes: Option<u64>,
    pub build_disk_available_bytes: Option<u64>,
    pub cpu_percent: Option<f64>,
}
impl Resources {
    /// Accept current-startup samples only while both clocks satisfy the freshness window.
    #[must_use]
    pub fn fresh(&self, startup: Uuid, now: DateTime<Utc>) -> bool {
        self.startup_id == startup
            && [self.sampled_at, self.received_at]
                .into_iter()
                .all(|t| t >= now - Duration::seconds(90) && t <= now + Duration::seconds(30))
    }
    /// Unknown required metrics fail admission; optional unknown metrics remain unknown.
    #[must_use]
    pub fn satisfies(
        &self,
        startup: Uuid,
        required: &ResourceRequirements,
        now: DateTime<Utc>,
    ) -> bool {
        self.fresh(startup, now)
            && required
                .min_available_memory_bytes
                .is_none_or(|min| self.available_memory_bytes.is_some_and(|v| v >= min))
            && required
                .min_build_disk_available_bytes
                .is_none_or(|min| self.build_disk_available_bytes.is_some_and(|v| v >= min))
            && required.max_cpu_percent.is_none_or(|max| {
                self.cpu_percent
                    .is_some_and(|v| v.is_finite() && (0.0..=max).contains(&v))
            })
    }
}
/// A model-ranked device, fenced to the exact metadata used for the decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub node_id: Uuid,
    pub metadata_digest: String,
    pub reason: String,
}
/// Ordered candidates and target-wide resource requirements supplied by the Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub resources: ResourceRequirements,
}
impl Placement {
    /// Validate bounds and candidate identity before persisting an execution plan.
    ///
    /// # Errors
    /// Rejects duplicates, missing reasons, invalid digests and oversized candidate lists.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.candidates.is_empty() && self.candidates.len() <= 8,
            "placement needs 1 to 8 candidates"
        );
        let mut ids = HashSet::new();
        for candidate in &self.candidates {
            ensure!(
                !candidate.node_id.is_nil() && ids.insert(candidate.node_id),
                "duplicate or invalid candidate"
            );
            ensure!(
                candidate.metadata_digest.len() == 64
                    && candidate
                        .metadata_digest
                        .bytes()
                        .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)),
                "invalid metadata digest"
            );
            ensure!(
                !candidate.reason.trim().is_empty() && candidate.reason.len() <= 2000,
                "invalid placement reason"
            );
        }
        self.resources.validate()
    }
}
