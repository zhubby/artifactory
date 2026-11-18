//! [INPUT]: 依赖kube CRD、固定执行身份与序列化。
//! [OUTPUT]: 提供规划运行与临时源码任务 CRD。
//! [POS]: Agent Operator 的资源契约。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use kube::{CustomResource, CustomResourceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: String,
    // Explicit null clears obsolete fields when status uses JSON Merge Patch.
    pub message: Option<String>,
    pub pod_uid: Option<String>,
    pub observed_generation: i64,
}
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "agents.artifactory.dev",
    version = "v1alpha1",
    kind = "AgentPool",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase")]
pub struct AgentPoolSpec {
    #[schemars(range(min = 0, max = 1000))]
    pub max_active: u32,
    #[schemars(range(min = 1, max = 1000))]
    pub max_per_project: u32,
    pub image: String,
    #[serde(default = "pull_policy")]
    pub image_pull_policy: String,
    pub runtime_url: String,
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
    group = "agents.artifactory.dev",
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
    group = "agents.artifactory.dev",
    version = "v1alpha1",
    kind = "AgentRun",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunSpec {
    pub run_id: String,
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
    let mut crds = vec![AgentPool::crd(), SourceTask::crd(), AgentRun::crd()];
    for crd in &mut crds {
        let rules = match crd.spec.names.kind.as_str() {
            "AgentRun" => {
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
