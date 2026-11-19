//! [INPUT]: 依赖序列化、规划身份与固定源码清单。
//! [OUTPUT]: 提供规划意图、执行故障、租约与规划事件契约。
//! [POS]: 控制平面、规划协调器与 Operator 的纯数据边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
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

/// Operator 观察到的执行终止；不传递容器日志或 Kubernetes message。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionFailure {
    pub execution: Execution,
    pub reason: ExecutionFailureReason,
}

/// 固定诊断类别使初始化凭据与源码内容无法进入错误文本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionFailureReason {
    SourcePreparation { exit_code: i32 },
    WorkerExit { exit_code: i32 },
    PodFailed,
}

/// Operator 报告资源消失；未创建 Pod 的删除不伪造执行身份。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionLost {
    pub run_id: Uuid,
    pub generation: i64,
    pub execution: Option<Execution>,
    pub reason: ExecutionLostReason,
}

/// Pod 消失或 Worker 中断使旧授权失效；删除运行声明使业务失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLostReason {
    PodMissing,
    WorkerInterrupted,
    ResourceDeleted,
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
pub struct PlanningEvent {
    pub event_id: Uuid,
    pub run_id: Uuid,
    pub generation: i64,
    pub kind: String,
    pub payload: Value,
}
