//! [INPUT]: 依赖服务认证、运行身份与固定源码清单。
//! [OUTPUT]: 提供 Runtime 意图、模型配置和租约契约。
//! [POS]: 控制平面、Operator 与 Kubernetes Worker 的共享协议。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod wire {
    tonic::include_proto!("artifactory.runtime.v1");
}
pub mod crypto;
pub mod error;
pub mod metrics;
pub mod service;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfiguration {
    pub id: Uuid,
    pub api: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default = "default_calls")]
    pub max_calls: u32,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}
pub fn default_calls() -> u32 {
    20
}
pub fn default_timeout() -> u32 {
    600
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIntent {
    pub tenant_id: Uuid,
    pub run_id: Uuid,
    pub build_id: Uuid,
    pub project_id: Uuid,
    pub version: i64,
    pub source_manifest_id: Option<Uuid>,
    pub source_digest: Option<String>,
    pub context: Value,
    pub model: ModelConfiguration,
    #[serde(default = "default_pool")]
    pub pool: String,
}
fn default_pool() -> String {
    "default".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub run_id: Uuid,
    pub generation: i64,
    pub uid: String,
    pub name: String,
    pub namespace: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub run_id: Uuid,
    pub generation: i64,
    pub uid: String,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub event_id: Uuid,
    pub run_id: Uuid,
    pub generation: i64,
    pub kind: String,
    pub payload: Value,
}
