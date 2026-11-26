// [INPUT]: 现有命令信封、终端输入与能力握手JSON。
// [OUTPUT]: 静态目录、显式通知边界与有限解码拒收。
// [POS]: 共享命令名称边界；执行生命周期仍由各执行器负责。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    Assignment, Command,
    operations::{OperationAck, OperationCancel, OperationEnvelope, OperationResume},
    terminal::OpenTerminal,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

pub const CAPABILITY: &str = "command-catalog-v1";
/// Control只向声明此能力的设备发送显式可忽略扩展通知。
pub const NOTIFICATION_CAPABILITY: &str = "command-notification-v1";
pub const MAX_COMMAND_JSON_BYTES: usize = 128 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryPolicy {
    AttemptLease,
    AttemptEventAck,
    UpdateGeneration,
    TerminalSession,
    StartupSnapshot,
    Connection,
    NodeOperation,
    NodeOperationResume,
    NodeOperationResultAck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaDescriptor {
    /// 标识原有protobuf或JSON契约；不新增wire字段。
    pub name: &'static str,
    pub version: u32,
}

/// 权限由原执行器复核；目录字段本身不授予权限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationPolicy {
    ProjectAttemptLease,
    TenantAdminUpdateAndSignedAsset,
    TenantAdminSessionAndLocalOptIn,
    TenantAdminSession,
    NodeControlSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadlinePolicy {
    /// 复用Attempt租约；start/renew额外校验请求锚点。
    AttemptLease,
    /// 固定启动器的单次下载和健康确认预算。
    UpdateDownloadAndHealth,
    /// 现有启动截止与独立终端租约。
    TerminalOpenAndLease,
    /// 共享operations接收窗口与执行预算。
    OperationReceiveAndExecution,
    CurrentConnection,
    HeartbeatAnchor,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeduplicationPolicy {
    AttemptAndLeaseToken,
    AttemptEventSequence,
    UpdateGenerationAndId,
    TerminalIdAndControlSession,
    OperationIdAndInputDigest,
    OperationIdAndResultDigest,
    StartupId,
    CurrentConnection,
    HeartbeatAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancellationPolicy {
    AttemptLeaseRevocation,
    UpdateGenerationAndId,
    TerminalCloseOrLeaseExpiry,
    OperationIdWithDurableTerminal,
    None,
}

/// 描述既有契约，不提供第二套通用命令执行器。
#[derive(Clone, Copy, Debug)]
pub struct CommandDescriptor {
    pub kind: &'static str,
    pub schema_version: u32,
    pub input_schema: SchemaDescriptor,
    pub result_schema: Option<SchemaDescriptor>,
    pub required_capability: Option<&'static str>,
    pub delivery: DeliveryPolicy,
    pub executes_work: bool,
    pub authorization: AuthorizationPolicy,
    pub deadline: DeadlinePolicy,
    pub deduplication: DeduplicationPolicy,
    pub cancellation: CancellationPolicy,
}

const CATALOG: &[CommandDescriptor] = &[
    descriptor(
        "node_operation_resume",
        Some(crate::operations::CAPABILITY),
        DeliveryPolicy::NodeOperationResume,
        false,
        "operation_resume",
        Some("operation_update"),
    ),
    descriptor(
        "node_operation",
        Some(crate::operations::CAPABILITY),
        DeliveryPolicy::NodeOperation,
        true,
        "operation_envelope",
        Some("operation_update"),
    ),
    descriptor(
        "node_operation_cancel",
        Some(crate::operations::CAPABILITY),
        DeliveryPolicy::NodeOperation,
        false,
        "operation_cancel",
        Some("operation_update"),
    ),
    descriptor(
        "node_operation_result_ack",
        Some(crate::operations::CAPABILITY),
        DeliveryPolicy::NodeOperationResultAck,
        false,
        "operation_ack",
        None,
    ),
    descriptor(
        "assign",
        None,
        DeliveryPolicy::AttemptLease,
        true,
        "assignment",
        Some("attempt_events"),
    ),
    descriptor(
        "start",
        None,
        DeliveryPolicy::AttemptLease,
        true,
        "attempt_start",
        Some("attempt_events"),
    ),
    descriptor(
        "renew",
        None,
        DeliveryPolicy::AttemptLease,
        false,
        "attempt_renew",
        None,
    ),
    descriptor(
        "cancel",
        None,
        DeliveryPolicy::AttemptLease,
        false,
        "attempt_cancel",
        Some("attempt_events"),
    ),
    descriptor(
        "event_ack",
        None,
        DeliveryPolicy::AttemptEventAck,
        false,
        "attempt_event_ack",
        None,
    ),
    descriptor(
        "metadata_ack",
        None,
        DeliveryPolicy::StartupSnapshot,
        false,
        "metadata_ack",
        None,
    ),
    descriptor(
        "emissary_update",
        Some("emissary-ota-v1"),
        DeliveryPolicy::UpdateGeneration,
        true,
        "update_request",
        Some("ota_state"),
    ),
    descriptor(
        "emissary_health",
        Some("emissary-ota-v1"),
        DeliveryPolicy::UpdateGeneration,
        false,
        "ota_health_ack",
        None,
    ),
    descriptor(
        "terminal_open",
        Some("terminal-v1"),
        DeliveryPolicy::TerminalSession,
        true,
        "terminal_open",
        Some("terminal_lifecycle"),
    ),
    descriptor(
        "terminal_close",
        Some("terminal-v1"),
        DeliveryPolicy::TerminalSession,
        false,
        "terminal_close",
        Some("terminal_lifecycle"),
    ),
    descriptor(
        "capabilities_ack",
        Some(CAPABILITY),
        DeliveryPolicy::Connection,
        false,
        "capabilities_ack",
        None,
    ),
];

const fn descriptor(
    kind: &'static str,
    required_capability: Option<&'static str>,
    delivery: DeliveryPolicy,
    executes_work: bool,
    input_schema: &'static str,
    result_schema: Option<&'static str>,
) -> CommandDescriptor {
    let (authorization, deadline, deduplication, cancellation) = match delivery {
        DeliveryPolicy::AttemptLease => (
            AuthorizationPolicy::ProjectAttemptLease,
            DeadlinePolicy::AttemptLease,
            DeduplicationPolicy::AttemptAndLeaseToken,
            CancellationPolicy::AttemptLeaseRevocation,
        ),
        DeliveryPolicy::AttemptEventAck => (
            AuthorizationPolicy::ProjectAttemptLease,
            DeadlinePolicy::None,
            DeduplicationPolicy::AttemptEventSequence,
            CancellationPolicy::None,
        ),
        DeliveryPolicy::UpdateGeneration if executes_work => (
            AuthorizationPolicy::TenantAdminUpdateAndSignedAsset,
            DeadlinePolicy::UpdateDownloadAndHealth,
            DeduplicationPolicy::UpdateGenerationAndId,
            CancellationPolicy::UpdateGenerationAndId,
        ),
        DeliveryPolicy::UpdateGeneration => (
            AuthorizationPolicy::NodeControlSession,
            DeadlinePolicy::HeartbeatAnchor,
            DeduplicationPolicy::HeartbeatAnchor,
            CancellationPolicy::None,
        ),
        DeliveryPolicy::TerminalSession => (
            AuthorizationPolicy::TenantAdminSessionAndLocalOptIn,
            if executes_work {
                DeadlinePolicy::TerminalOpenAndLease
            } else {
                DeadlinePolicy::CurrentConnection
            },
            DeduplicationPolicy::TerminalIdAndControlSession,
            CancellationPolicy::TerminalCloseOrLeaseExpiry,
        ),
        DeliveryPolicy::NodeOperation => (
            AuthorizationPolicy::TenantAdminSession,
            if executes_work {
                DeadlinePolicy::OperationReceiveAndExecution
            } else {
                DeadlinePolicy::CurrentConnection
            },
            DeduplicationPolicy::OperationIdAndInputDigest,
            CancellationPolicy::OperationIdWithDurableTerminal,
        ),
        DeliveryPolicy::NodeOperationResume => (
            AuthorizationPolicy::TenantAdminSession,
            DeadlinePolicy::CurrentConnection,
            DeduplicationPolicy::OperationIdAndInputDigest,
            CancellationPolicy::None,
        ),
        DeliveryPolicy::NodeOperationResultAck => (
            AuthorizationPolicy::TenantAdminSession,
            DeadlinePolicy::CurrentConnection,
            DeduplicationPolicy::OperationIdAndResultDigest,
            CancellationPolicy::None,
        ),
        DeliveryPolicy::StartupSnapshot => (
            AuthorizationPolicy::NodeControlSession,
            DeadlinePolicy::None,
            DeduplicationPolicy::StartupId,
            CancellationPolicy::None,
        ),
        DeliveryPolicy::Connection => (
            AuthorizationPolicy::NodeControlSession,
            DeadlinePolicy::CurrentConnection,
            DeduplicationPolicy::CurrentConnection,
            CancellationPolicy::None,
        ),
    };
    CommandDescriptor {
        kind,
        schema_version: 1,
        input_schema: SchemaDescriptor {
            name: input_schema,
            version: 1,
        },
        result_schema: match result_schema {
            Some(name) => Some(SchemaDescriptor { name, version: 1 }),
            None => None,
        },
        required_capability,
        delivery,
        executes_work,
        authorization,
        deadline,
        deduplication,
        cancellation,
    }
}

pub fn catalog() -> &'static [CommandDescriptor] {
    CATALOG
}

pub enum DecodedCommand<'a> {
    IgnoredNotification,
    Build {
        kind: &'a str,
        assignment: Option<&'a Assignment>,
    },
    Notification(&'a str),
    Update(UpdateRequest),
    TerminalOpen(OpenTerminal),
    TerminalClose(TerminalClose),
    CapabilitiesAck(CapabilitiesAck),
    NodeOperation(OperationEnvelope),
    NodeOperationCancel(OperationCancel),
    NodeOperationAck(OperationAck),
    NodeOperationResume(OperationResume),
}

impl std::fmt::Debug for DecodedCommand<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IgnoredNotification => f.write_str("IgnoredNotification"),
            Self::Build { kind, .. } => f
                .debug_struct("Build")
                .field("kind", kind)
                .finish_non_exhaustive(),
            Self::Notification(kind) => f.debug_tuple("Notification").field(kind).finish(),
            Self::Update(_) => f.debug_struct("Update").finish_non_exhaustive(),
            Self::TerminalOpen(_) => f.debug_struct("TerminalOpen").finish_non_exhaustive(),
            Self::TerminalClose(_) => f.debug_struct("TerminalClose").finish_non_exhaustive(),
            Self::NodeOperation(_) => f.debug_struct("NodeOperation").finish_non_exhaustive(),
            Self::NodeOperationCancel(_) => f
                .debug_struct("NodeOperationCancel")
                .finish_non_exhaustive(),
            Self::NodeOperationResume(_) => f
                .debug_struct("NodeOperationResume")
                .finish_non_exhaustive(),
            Self::NodeOperationAck(_) => f.debug_struct("NodeOperationAck").finish_non_exhaustive(),
            Self::CapabilitiesAck(ack) => f.debug_tuple("CapabilitiesAck").field(ack).finish(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesAck {
    pub schema_version: u32,
    pub server_time_ms: i64,
    pub responses: Vec<String>,
}

impl CapabilitiesAck {
    pub fn accepts_rejections(&self) -> bool {
        self.schema_version == 1 && self.responses.iter().any(|r| r == "command_rejected")
    }
}

/// 与现有OTA Request保持字段语义一致，不包含执行与恢复逻辑。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    pub generation: i64,
    pub id: String,
    pub action: String,
    pub manifest: String,
    pub signature: String,
    pub url: String,
}

impl std::fmt::Debug for UpdateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateRequest")
            .field("generation", &self.generation)
            .field("action", &self.action)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalClose {
    pub terminal_id: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    UnsupportedKind,
    SchemaMismatch,
    InvalidPayload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRejected {
    pub command_id: String,
    pub kind: String,
    pub code: RejectionCode,
}

pub fn decode(command: &Command) -> Result<DecodedCommand<'_>, CommandRejected> {
    let reject = |code| CommandRejected {
        command_id: bounded(&command.command_id, 128),
        kind: bounded(&command.kind, 128),
        code,
    };
    let Some(entry) = CATALOG.iter().find(|entry| entry.kind == command.kind) else {
        if !command.ignorable_notification {
            return Err(reject(RejectionCode::UnsupportedKind));
        }
        // 显式通知不携带执行与授权；已知命令不进入此分支。
        if command.kind.is_empty()
            || command.kind.len() > 128
            || !command
                .kind
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || command.payload_json.len() > MAX_COMMAND_JSON_BYTES
            || command.assignment.is_some()
            || !command.attempt_id.is_empty()
            || !command.lease_token.is_empty()
            || command.lease_seconds != 0
            || !command.signing_session_id.is_empty()
            || !command.request_id.is_empty()
            || command.ack_sequence != 0
            || !command.message.is_empty()
        {
            return Err(reject(RejectionCode::InvalidPayload));
        }
        return Ok(DecodedCommand::IgnoredNotification);
    };
    if command.payload_json.len() > MAX_COMMAND_JSON_BYTES {
        return Err(reject(RejectionCode::InvalidPayload));
    }
    match entry.kind {
        "node_operation_resume" => {
            let resume: OperationResume = json(command).map_err(reject)?;
            resume
                .validate()
                .map_err(|_| reject(RejectionCode::InvalidPayload))?;
            Ok(DecodedCommand::NodeOperationResume(resume))
        }
        "node_operation" => {
            let envelope: OperationEnvelope = json(command).map_err(reject)?;
            if envelope.schema != entry.schema_version {
                return Err(reject(RejectionCode::SchemaMismatch));
            }
            envelope
                .validate()
                .map_err(|_| reject(RejectionCode::InvalidPayload))?;
            Ok(DecodedCommand::NodeOperation(envelope))
        }
        "node_operation_cancel" => {
            let cancel: OperationCancel = json(command).map_err(reject)?;
            if cancel.operation_id.is_nil()
                || cancel.startup_id.is_nil()
                || cancel.control_session.is_nil()
            {
                return Err(reject(RejectionCode::InvalidPayload));
            }
            Ok(DecodedCommand::NodeOperationCancel(cancel))
        }
        "node_operation_result_ack" => {
            let ack: OperationAck = json(command).map_err(reject)?;
            if ack.operation_id.is_nil()
                || ack.result_digest.len() != 64
                || !ack
                    .result_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(reject(RejectionCode::InvalidPayload));
            }
            Ok(DecodedCommand::NodeOperationAck(ack))
        }
        "assign" => {
            let Some(assignment) = command.assignment.as_ref() else {
                return Err(reject(RejectionCode::InvalidPayload));
            };
            Ok(DecodedCommand::Build {
                kind: &command.kind,
                assignment: Some(assignment),
            })
        }
        "start" | "renew" | "cancel" => Ok(DecodedCommand::Build {
            kind: &command.kind,
            assignment: None,
        }),
        "metadata_ack" | "event_ack" | "emissary_health" => {
            Ok(DecodedCommand::Notification(&command.kind))
        }
        "emissary_update" => {
            let request: UpdateRequest = json(command).map_err(reject)?;
            if request.generation <= 0
                || request.id.is_empty()
                || request.id.len() > 100
                || !matches!(request.action.as_str(), "apply" | "cancel")
                || (request.action == "apply"
                    && (request.manifest.is_empty()
                        || request.signature.is_empty()
                        || request.url.is_empty()))
            {
                return Err(reject(RejectionCode::InvalidPayload));
            }
            Ok(DecodedCommand::Update(request))
        }
        "terminal_open" => {
            let open: OpenTerminal = json(command).map_err(reject)?;
            open.validate_structure()
                .map_err(|_| reject(RejectionCode::InvalidPayload))?;
            Ok(DecodedCommand::TerminalOpen(open))
        }
        "terminal_close" => {
            let close: TerminalClose = json(command).map_err(reject)?;
            if close.terminal_id.is_nil() {
                return Err(reject(RejectionCode::InvalidPayload));
            }
            Ok(DecodedCommand::TerminalClose(close))
        }
        "capabilities_ack" => {
            let ack: CapabilitiesAck = json(command).map_err(reject)?;
            if ack.schema_version != 1 {
                return Err(reject(RejectionCode::SchemaMismatch));
            }
            if ack.server_time_ms < 0
                || ack.responses.len() > 8
                || ack
                    .responses
                    .iter()
                    .any(|r| r.len() > 128 || r.is_empty() || r.chars().any(char::is_control))
            {
                return Err(reject(RejectionCode::InvalidPayload));
            }
            Ok(DecodedCommand::CapabilitiesAck(ack))
        }
        _ => Err(reject(RejectionCode::UnsupportedKind)),
    }
}

fn json<T: DeserializeOwned>(command: &Command) -> Result<T, RejectionCode> {
    serde_json::from_str(&command.payload_json).map_err(|_| RejectionCode::InvalidPayload)
}

fn bounded(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].chars().filter(|c| !c.is_control()).collect()
}
