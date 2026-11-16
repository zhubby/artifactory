//! [INPUT]: 依赖 Serde 提供跨服务 JSON 契约。
//! [OUTPUT]: 提供托管凭据、固定绑定、签名输出及验签证据类型。
//! [POS]: 控制平面和 Emissary 共用的签名契约；秘密仅用于领取响应。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Material {
    MacosPkcs12 {
        file_base64: String,
        password: String,
        #[serde(default)]
        notary: Option<Notary>,
    },
    AndroidKeystore {
        file_base64: String,
        store_password: String,
        key_password: String,
        alias: String,
        purpose: AndroidPurpose,
    },
}

impl Material {
    pub fn platform(&self) -> &'static str {
        match self {
            Self::MacosPkcs12 { .. } => "macos",
            Self::AndroidKeystore { .. } => "android",
        }
    }
    pub fn file_base64(&self) -> &str {
        match self {
            Self::MacosPkcs12 { file_base64, .. } | Self::AndroidKeystore { file_base64, .. } => {
                file_base64
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notary {
    pub key_id: String,
    pub issuer_id: String,
    pub private_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidPurpose {
    Application,
    Upload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub subject: String,
    pub fingerprint_sha256: String,
    pub not_before: i64,
    pub not_after: i64,
    pub identity: String,
    pub team_id: Option<String>,
    pub alias: Option<String>,
    pub purpose: Option<AndroidPurpose>,
    pub notarization_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub profile: String,
    pub platform: String,
    pub credential_id: String,
    pub version_id: String,
    pub metadata: Metadata,
    pub notarize: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub path: String,
    /// ZIP/DMG 内待验签的 .app 相对路径。Android 不设置此字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub path: String,
    pub sha256: String,
    pub profile: String,
    pub version_id: String,
    pub fingerprint_sha256: String,
    pub notarized: bool,
}
