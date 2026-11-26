//! [INPUT]: 依赖认证节点身份、固定工具目录与白名单探测结果。
//! [OUTPUT]: 定义有限操作输入、截止信封、生命周期及结果摘要。
//! [POS]: Control 与设备共享的只读操作契约；不接受任意命令。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const CAPABILITY: &str = "node-operations-v1";
pub const MAX_INPUT_BYTES: usize = 8 << 10;
pub const MAX_RESULT_BYTES: usize = 64 << 10;
pub const RECEIVE_WINDOW_MS: i64 = 30_000;
pub const EXECUTION_SECONDS: u64 = 30;
pub const PROBE_IDS: &[&str] = &[
    "git",
    "git-lfs",
    "bash",
    "sh",
    "powershell",
    "pwsh",
    "cc",
    "cargo",
    "go",
    "node",
    "npm",
    "xcodebuild",
    "java",
    "gradle",
    "cmake",
    "ninja",
    "dotnet",
    "msbuild",
    "signtool",
    "codesign",
    "security",
    "apksigner",
    "jarsigner",
    "keytool",
    "xcrun",
    "ditto",
    "hdiutil",
    "docker",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    CollectDiagnostics,
    RefreshInventory,
}
impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CollectDiagnostics => "collect_diagnostics",
            Self::RefreshInventory => "refresh_inventory",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationCreate {
    pub request_id: Uuid,
    pub kind: OperationKind,
    pub schema: u32,
    pub input: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsInput {
    #[serde(default)]
    pub probes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshInput {
    pub mode: RefreshMode,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Full,
}
#[derive(Clone, Debug)]
pub enum OperationInput {
    Diagnostics(DiagnosticsInput),
    Refresh(RefreshInput),
}
impl OperationCreate {
    pub fn validated(&self) -> Result<OperationInput> {
        ensure!(!self.request_id.is_nil(), "invalid request_id");
        validate_input(self.kind, self.schema, &self.input)
    }
}
pub fn validate_input(kind: OperationKind, schema: u32, input: &Value) -> Result<OperationInput> {
    ensure!(schema == 1, "unsupported operation schema");
    ensure!(
        serde_json::to_vec(input)?.len() <= MAX_INPUT_BYTES,
        "operation input exceeds budget"
    );
    Ok(match kind {
        OperationKind::CollectDiagnostics => {
            let mut input: DiagnosticsInput = serde_json::from_value(input.clone())?;
            ensure!(input.probes.len() <= PROBE_IDS.len(), "too many probes");
            let unique: BTreeSet<_> = input.probes.iter().collect();
            ensure!(unique.len() == input.probes.len(), "duplicate probe");
            ensure!(
                input
                    .probes
                    .iter()
                    .all(|id| PROBE_IDS.contains(&id.as_str())),
                "unknown probe"
            );
            input.probes.sort();
            OperationInput::Diagnostics(input)
        }
        OperationKind::RefreshInventory => {
            OperationInput::Refresh(serde_json::from_value(input.clone())?)
        }
    })
}
impl OperationInput {
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::Diagnostics(_) => OperationKind::CollectDiagnostics,
            Self::Refresh(_) => OperationKind::RefreshInventory,
        }
    }
    pub fn value(&self) -> Value {
        match self {
            Self::Diagnostics(input) => serde_json::to_value(input).expect("typed input"),
            Self::Refresh(input) => serde_json::to_value(input).expect("typed input"),
        }
    }
    pub fn digest(&self) -> String {
        digest(&serde_json::to_vec(&(self.kind(), 1, self.value())).expect("typed input"))
    }
    pub fn probes(&self) -> Vec<String> {
        match self {
            Self::Diagnostics(input) if !input.probes.is_empty() => input.probes.clone(),
            _ => PROBE_IDS.iter().map(|id| (*id).to_owned()).collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationEnvelope {
    pub operation_id: Uuid,
    pub tenant_id: Uuid,
    pub node_id: Uuid,
    pub startup_id: Uuid,
    pub control_session: Uuid,
    pub actor_id: Uuid,
    pub kind: OperationKind,
    pub schema: u32,
    pub input: Value,
    pub input_digest: String,
    pub issued_at_ms: i64,
    pub expires_at_ms: i64,
}
impl OperationEnvelope {
    pub fn validate(&self) -> Result<OperationInput> {
        ensure!(
            [
                self.operation_id,
                self.tenant_id,
                self.node_id,
                self.startup_id,
                self.control_session,
                self.actor_id
            ]
            .iter()
            .all(|id| !id.is_nil()),
            "operation identity missing"
        );
        ensure!(
            self.issued_at_ms > 0
                && self.expires_at_ms.checked_sub(self.issued_at_ms) == Some(RECEIVE_WINDOW_MS),
            "invalid operation receive window"
        );
        let input = validate_input(self.kind, self.schema, &self.input)?;
        ensure!(
            input.digest() == self.input_digest,
            "operation input digest mismatch"
        );
        Ok(input)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Requested,
    Accepted,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Expired,
    Interrupted,
    Rejected,
}
impl OperationState {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Requested | Self::Accepted | Self::Running)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Accepted => "accepted",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Expired => "expired",
            Self::Interrupted => "interrupted",
            Self::Rejected => "rejected",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Available,
    Unsupported,
    PermissionDenied,
    Timeout,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeErrorCode {
    ExitFailure,
    PermissionDenied,
    DeadlineExceeded,
    SpawnFailed,
    Canceled,
    CleanupFailed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeReport {
    pub status: ProbeStatus,
    pub observed_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub probe_version: u32,
    pub version: Option<String>,
    pub error_code: Option<ProbeErrorCode>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    BudgetExhausted,
    Canceled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryDelivery {
    Pending,
    Confirmed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryRefresh {
    pub observation_id: Uuid,
    pub revision: u64,
    pub delivery: InventoryDelivery,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationResult {
    #[serde(default)]
    pub probes: BTreeMap<String, ProbeReport>,
    #[serde(default)]
    pub skipped: BTreeMap<String, SkippedReason>,
    pub inventory: Option<InventoryRefresh>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub omitted: u32,
}
impl OperationResult {
    pub fn successful(&self) -> bool {
        self.skipped.is_empty()
            && !self.truncated
            && self.omitted == 0
            && self
                .probes
                .values()
                .all(|p| matches!(p.status, ProbeStatus::Available | ProbeStatus::Unsupported))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_RESULT_BYTES,
            "operation result exceeds budget"
        );
        ensure!(
            self.probes.len() + self.skipped.len() <= PROBE_IDS.len(),
            "too many probe results"
        );
        ensure!(
            self.probes
                .keys()
                .chain(self.skipped.keys())
                .all(|id| PROBE_IDS.contains(&id.as_str())),
            "unknown result probe"
        );
        ensure!(
            self.probes.keys().all(|id| !self.skipped.contains_key(id)),
            "duplicate result probe"
        );
        for report in self.probes.values() {
            ensure!(
                report.probe_version == 1 && report.duration_ms <= 30_000,
                "invalid probe report"
            );
            ensure!(
                report.version.as_ref().is_none_or(
                    |v| v.len() <= 128 && v.chars().all(|c| c.is_ascii_digit() || c == '.')
                ),
                "invalid normalized version"
            );
        }
        if let Some(refresh) = &self.inventory {
            ensure!(
                !refresh.observation_id.is_nil() && refresh.revision > 0,
                "invalid inventory observation"
            );
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(digest(&serde_json::to_vec(self)?))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationReason {
    Busy,
    InvalidInput,
    IdentityMismatch,
    ClockUnsynchronized,
    ReceiveExpired,
    StorageUnavailable,
    JournalFull,
    InputConflict,
    ExecutionBudgetExhausted,
    Canceled,
    StartupChanged,
    ResultExpired,
    InventoryUnavailable,
    CleanupFailed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationUpdate {
    pub operation_id: Uuid,
    pub startup_id: Uuid,
    pub sequence: u64,
    pub state: OperationState,
    pub reason: Option<OperationReason>,
    pub result: Option<OperationResult>,
    pub result_digest: Option<String>,
}
impl OperationUpdate {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.operation_id.is_nil()
                && !self.startup_id.is_nil()
                && self.sequence > 0
                && self.state != OperationState::Requested,
            "invalid operation update"
        );
        if let Some(result) = &self.result {
            ensure!(
                self.state.terminal()
                    && self.result_digest.as_deref() == Some(result.digest()?.as_str()),
                "invalid result digest"
            );
        } else {
            ensure!(
                self.result_digest.is_none() || self.state.terminal(),
                "unexpected result digest"
            );
        }
        if self.state == OperationState::Succeeded {
            if let Some(result) = &self.result {
                ensure!(
                    result.successful(),
                    "success requires complete probe result"
                );
            } else {
                ensure!(
                    self.result_digest
                        .as_ref()
                        .is_some_and(|digest| digest.len() == 64
                            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())),
                    "terminal replay requires digest"
                );
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationAck {
    pub operation_id: Uuid,
    pub result_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationCancel {
    pub operation_id: Uuid,
    pub startup_id: Uuid,
    pub control_session: Uuid,
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationResume {
    pub operation_id: Uuid,
    pub tenant_id: Uuid,
    pub node_id: Uuid,
    pub original_startup_id: Uuid,
    pub current_startup_id: Uuid,
    pub control_session: Uuid,
    pub input_digest: String,
}
impl OperationResume {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.operation_id,
                self.tenant_id,
                self.node_id,
                self.original_startup_id,
                self.current_startup_id,
                self.control_session
            ]
            .iter()
            .all(|id| !id.is_nil())
                && self.input_digest.len() == 64
                && self
                    .input_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "invalid operation resume"
        );
        Ok(())
    }
}
