// [INPUT]: 生成的遥测RPC、原字节摘要与固定格式预算。
// [OUTPUT]: 协商格式交集、校验回执并编码有限拒收原因。
// [POS]: 设备与独立遥测接收端共用的兼容边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    TelemetryAck, TelemetryNegotiateReply, TelemetryNegotiateRequest, TelemetryRejectionDetails,
    inventory::MAX_INVENTORY_BYTES, telemetry::MAX_BATCH_BYTES,
};
use prost::Message;
use sha2::{Digest, Sha256};
use tonic::{Code, Status};

pub const BATCH_SCHEMA_V1: u32 = 1;
pub const INVENTORY_SCHEMA_V1: u32 = 1;
pub const MAX_NEGOTIATION_BYTES: usize = 4096;
pub const DIGEST_ACK_FEATURE: &str = "digest_ack_v1";

/// 只声明本实现实际能解码或发送的格式。
pub fn offer() -> TelemetryNegotiateRequest {
    TelemetryNegotiateRequest {
        batch_schemas: vec![BATCH_SCHEMA_V1],
        inventory_schemas: vec![INVENTORY_SCHEMA_V1],
        encodings: vec!["identity".into()],
        features: vec![DIGEST_ACK_FEATURE.into()],
    }
}

pub fn negotiate(
    request: &TelemetryNegotiateRequest,
    server_time_ms: i64,
) -> Result<TelemetryNegotiateReply, Status> {
    validate_lists(
        &request.batch_schemas,
        &request.inventory_schemas,
        &request.encodings,
        &request.features,
        request.encoded_len(),
    )?;
    let identity = request.encodings.iter().any(|s| s == "identity");
    let batch = identity && request.batch_schemas.contains(&BATCH_SCHEMA_V1);
    let inventory = identity && request.inventory_schemas.contains(&INVENTORY_SCHEMA_V1);
    Ok(TelemetryNegotiateReply {
        batch_schemas: if batch { vec![BATCH_SCHEMA_V1] } else { vec![] },
        inventory_schemas: if inventory {
            vec![INVENTORY_SCHEMA_V1]
        } else {
            vec![]
        },
        encodings: if identity {
            vec!["identity".into()]
        } else {
            vec![]
        },
        features: if batch && request.features.iter().any(|s| s == DIGEST_ACK_FEATURE) {
            vec![DIGEST_ACK_FEATURE.into()]
        } else {
            vec![]
        },
        max_batch_bytes: MAX_BATCH_BYTES as u32,
        max_inventory_bytes: MAX_INVENTORY_BYTES as u32,
        server_time_ms,
    })
}

fn validate_lists(
    batch: &[u32],
    inventory: &[u32],
    encodings: &[String],
    features: &[String],
    encoded_len: usize,
) -> Result<(), Status> {
    if encoded_len > MAX_NEGOTIATION_BYTES
        || batch.len() > 8
        || inventory.len() > 8
        || encodings.len() > 8
        || features.len() > 8
        || batch.iter().chain(inventory).any(|s| *s == 0)
        || encodings.iter().chain(features).any(|s| {
            s.is_empty()
                || s.len() > 128
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        })
    {
        return Err(RejectionReason::BoundsExceeded.status());
    }
    Ok(())
}

/// 每次连接重新取得；RPC失败不能凭空建立新能力。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiated {
    pub batch_schema: Option<u32>,
    pub inventory_schema: Option<u32>,
    pub digest_ack: bool,
    pub max_batch_bytes: u32,
    pub max_inventory_bytes: u32,
}

impl Negotiated {
    /// 仅用于明确的旧端Unimplemented，或连接初始安全格式。
    pub fn legacy() -> Self {
        Self {
            batch_schema: Some(BATCH_SCHEMA_V1),
            inventory_schema: None,
            digest_ack: false,
            max_batch_bytes: MAX_BATCH_BYTES as u32,
            max_inventory_bytes: MAX_INVENTORY_BYTES as u32,
        }
    }

    pub fn from_reply(reply: &TelemetryNegotiateReply) -> Result<Self, Status> {
        validate_lists(
            &reply.batch_schemas,
            &reply.inventory_schemas,
            &reply.encodings,
            &reply.features,
            reply.encoded_len(),
        )?;
        if reply.batch_schemas.iter().any(|s| *s != BATCH_SCHEMA_V1)
            || reply
                .inventory_schemas
                .iter()
                .any(|s| *s != INVENTORY_SCHEMA_V1)
            || reply.encodings.iter().any(|s| s != "identity")
            || reply.max_batch_bytes == 0
            || reply.max_inventory_bytes == 0
        {
            return Err(RejectionReason::SchemaUnsupported.status());
        }
        let identity = reply.encodings.iter().any(|s| s == "identity");
        let batch_schema = identity
            .then(|| reply.batch_schemas.iter().max().copied())
            .flatten();
        let inventory_schema = identity
            .then(|| reply.inventory_schemas.iter().max().copied())
            .flatten();
        Ok(Self {
            batch_schema,
            inventory_schema,
            digest_ack: batch_schema.is_some()
                && reply.features.iter().any(|s| s == DIGEST_ACK_FEATURE),
            max_batch_bytes: reply.max_batch_bytes.min(MAX_BATCH_BYTES as u32),
            max_inventory_bytes: reply.max_inventory_bytes.min(MAX_INVENTORY_BYTES as u32),
        })
    }

    pub fn ack_requirement(self) -> AckRequirement {
        if self.digest_ack {
            AckRequirement::Digest {
                schema_version: self.batch_schema.unwrap_or(BATCH_SCHEMA_V1),
            }
        } else {
            AckRequirement::LegacyId
        }
    }
}

/// 发出请求时固定确认要求，不能因回执缺字段而降级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckRequirement {
    LegacyId,
    Digest { schema_version: u32 },
}

impl AckRequirement {
    pub fn matches_ack(self, ack: &TelemetryAck, batch_id: &str, bytes: &[u8]) -> bool {
        if ack.batch_id != batch_id {
            return false;
        }
        match self {
            Self::LegacyId => true,
            Self::Digest { schema_version } => {
                ack.schema_version == schema_version && ack.sha256 == sha256(bytes)
            }
        }
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// 固定原因避免把错误文本、路径或正文带入指标标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    SchemaUnsupported,
    EncodingUnsupported,
    Overloaded,
    DatabaseUnavailable,
    AuthenticationFailed,
    AuthorizationDenied,
    StartupUnregistered,
    ClockFuture,
    ExpiredSample,
    Malformed,
    BoundsExceeded,
    IdContentConflict,
    IdentityRetired,
}

impl RejectionReason {
    pub const ALL: &'static [Self] = &[
        Self::SchemaUnsupported,
        Self::EncodingUnsupported,
        Self::Overloaded,
        Self::DatabaseUnavailable,
        Self::AuthenticationFailed,
        Self::AuthorizationDenied,
        Self::StartupUnregistered,
        Self::ClockFuture,
        Self::ExpiredSample,
        Self::Malformed,
        Self::BoundsExceeded,
        Self::IdContentConflict,
        Self::IdentityRetired,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SchemaUnsupported => "schema_unsupported",
            Self::EncodingUnsupported => "encoding_unsupported",
            Self::Overloaded => "overloaded",
            Self::DatabaseUnavailable => "database_unavailable",
            Self::AuthenticationFailed => "authentication_failed",
            Self::AuthorizationDenied => "authorization_denied",
            Self::StartupUnregistered => "startup_unregistered",
            Self::ClockFuture => "clock_future",
            Self::ExpiredSample => "expired_sample",
            Self::Malformed => "malformed",
            Self::BoundsExceeded => "bounds_exceeded",
            Self::IdContentConflict => "id_content_conflict",
            Self::IdentityRetired => "identity_retired",
        }
    }

    pub fn code(self) -> Code {
        match self {
            Self::SchemaUnsupported
            | Self::EncodingUnsupported
            | Self::Malformed
            | Self::BoundsExceeded => Code::InvalidArgument,
            Self::Overloaded => Code::ResourceExhausted,
            Self::DatabaseUnavailable | Self::StartupUnregistered => Code::Unavailable,
            Self::AuthenticationFailed => Code::Unauthenticated,
            Self::AuthorizationDenied | Self::IdentityRetired => Code::PermissionDenied,
            Self::ClockFuture | Self::ExpiredSample => Code::OutOfRange,
            Self::IdContentConflict => Code::AlreadyExists,
        }
    }

    pub fn status(self) -> Status {
        Status::with_details(
            self.code(),
            self.as_str(),
            TelemetryRejectionDetails {
                reason: self.as_str().into(),
            }
            .encode_to_vec()
            .into(),
        )
    }

    pub fn from_status(status: &Status) -> Option<Self> {
        if status.details().len() > 256 {
            return None;
        }
        let details = TelemetryRejectionDetails::decode(status.details()).ok()?;
        Self::ALL
            .iter()
            .copied()
            .find(|r| r.as_str() == details.reason && r.code() == status.code())
    }
}
