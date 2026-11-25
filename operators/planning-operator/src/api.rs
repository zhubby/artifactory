//! [INPUT]: 依赖 kube CRD、不可变项目身份与序列化。
//! [OUTPUT]: 提供规划运行、池级代理、源码任务与执行台账。
//! [POS]: Planning Operator 的资源契约。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_service_transport::proxy::ProxyConfig;
use kube::{CustomResource, CustomResourceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: String,
    // JSON Merge Patch 使用显式 null 清除执行台账。
    pub message: Option<String>,
    pub execution: Option<PodExecution>,
    pub observed_generation: i64,
}
/// 先预留名称，再记录创建意图和 UID；停机证据先于 finalizer 移除落盘。
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodExecution {
    pub name: String,
    pub uid: Option<String>,
    pub node_name: Option<String>,
    pub create_started: bool,
    #[schemars(range(min = 1, max = 3))]
    pub attempt: u32,
    pub deadline_ms: i64,
    pub stopped: bool,
}
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "planning.artifactory.dev",
    version = "v1alpha1",
    kind = "PlanningPool",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase")]
pub struct PlanningPoolSpec {
    #[schemars(range(min = 0, max = 1000))]
    pub max_active: u32,
    #[schemars(range(min = 1, max = 1000))]
    pub max_per_project: u32,
    pub image: String,
    #[serde(default = "pull_policy")]
    pub image_pull_policy: String,
    pub coordinator_url: String,
    pub control_url: String,
    pub source_preparer_image: String,
    #[serde(default = "disk")]
    pub ephemeral_storage: String,
    #[serde(default = "cpu")]
    pub cpu: String,
    #[serde(default = "memory")]
    pub memory: String,
    #[serde(default)]
    pub tls_secret_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
}
fn cpu() -> String {
    "250m".into()
}
fn pull_policy() -> String {
    "IfNotPresent".into()
}
fn memory() -> String {
    "512Mi".into()
}
fn disk() -> String {
    "10Gi".into()
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourcePurpose {
    Resolve,
    Refs,
}
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "planning.artifactory.dev",
    version = "v1alpha1",
    kind = "SourceTask",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceTaskSpec {
    pub task_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub purpose: SourcePurpose,
    pub pool: String,
    #[serde(default = "source_timeout")]
    #[schemars(range(min = 1, max = 900))]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub cancel_requested: bool,
}
fn source_timeout() -> u32 {
    900
}
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "planning.artifactory.dev",
    version = "v1alpha1",
    kind = "PlanningRun",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase")]
pub struct PlanningRunSpec {
    #[serde(default)]
    pub source_required: bool,
    pub run_id: String,
    pub project_id: String,
    #[schemars(range(min = 1))]
    pub generation: i64,
    pub pool: String,
    #[serde(default)]
    pub source_manifest_id: Option<String>,
    #[serde(default)]
    pub source_digest: Option<String>,
}
pub fn crds()
-> Vec<k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition>
{
    let mut crds = vec![PlanningPool::crd(), SourceTask::crd(), PlanningRun::crd()];
    for crd in &mut crds {
        let rules = match crd.spec.names.kind.as_str() {
            "PlanningRun" => {
                serde_json::json!([{"rule":"self == oldSelf","message":"execution context is immutable"}])
            }
            "SourceTask" => serde_json::json!([
                {"rule":"self.taskId == oldSelf.taskId && self.tenantId == oldSelf.tenantId && self.projectId == oldSelf.projectId && self.purpose == oldSelf.purpose && self.pool == oldSelf.pool && self.timeoutSeconds == oldSelf.timeoutSeconds","message":"source task context is immutable"},
                {"rule":"!oldSelf.cancelRequested || self.cancelRequested","message":"cancellation is irreversible"}
            ]),
            _ => continue,
        };
        let mut value = serde_json::to_value(&*crd).expect("CRD serializes");
        value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]["x-kubernetes-validations"] =
            rules;
        *crd = serde_json::from_value(value).expect("validated CRD schema");
    }
    crds
}
