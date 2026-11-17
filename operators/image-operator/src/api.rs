//! [INPUT]: 依赖 kube CRD、Serde 和固定执行身份。
//! [OUTPUT]: 提供含 sidecar 模式的 BuilderPool、ImageBuild 与状态定义。
//! [POS]: Image Operator 的 Kubernetes 公开契约。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use kube::{CustomResource, CustomResourceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: String,
    pub message: Option<String>,
    pub observed_generation: i64,
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "images.artifactory.dev",
    version = "v1alpha1",
    kind = "BuilderPool",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuilderPoolSpec {
    pub worker_image: String,
    pub buildkit_image: String,
    #[serde(default = "native_sidecar")]
    pub native_sidecar: bool,
    #[serde(default = "pull_policy")]
    pub image_pull_policy: String,
    #[schemars(range(min = 1, max = 1000))]
    pub max_active: u32,
    #[schemars(range(min = 1, max = 1000))]
    pub max_per_project: u32,
    #[serde(default)]
    pub node_selector: BTreeMap<String, String>,
    #[serde(default)]
    pub emulation_node_selector: BTreeMap<String, String>,
    #[serde(default)]
    pub emulation_platforms: Vec<String>,
    #[serde(default = "cpu")]
    pub cpu: String,
    #[serde(default = "memory")]
    pub memory: String,
    #[serde(default = "disk")]
    pub ephemeral_storage: String,
    #[serde(default)]
    pub ca_secret: String,
    #[serde(default)]
    pub buildkit_config_map: String,
}
fn native_sidecar() -> bool {
    true
}
fn pull_policy() -> String {
    "IfNotPresent".into()
}
fn cpu() -> String {
    "1".into()
}
fn memory() -> String {
    "2Gi".into()
}
fn disk() -> String {
    "20Gi".into()
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "images.artifactory.dev",
    version = "v1alpha1",
    kind = "ImageBuild",
    namespaced,
    status = "Status"
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageBuildSpec {
    pub execution_id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub pool: String,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    pub task_secret: String,
    pub platforms: Vec<String>,
    pub allow_emulation: bool,
    #[serde(default)]
    pub publish_only: bool,
    #[schemars(range(min = 1, max = 87000))]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub cancel_requested: bool,
}

pub fn crds()
-> Vec<k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition>
{
    let pool = BuilderPool::crd();
    let mut image = serde_json::to_value(ImageBuild::crd()).unwrap();
    let spec = &mut image["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"];
    for name in ["executionId", "tenantId", "projectId", "snapshotId"] {
        spec["properties"][name]["format"] = serde_json::json!("uuid");
    }
    spec["properties"]["platforms"]["minItems"] = serde_json::json!(1);
    spec["properties"]["platforms"]["maxItems"] = serde_json::json!(2);
    spec["properties"]["platforms"]["uniqueItems"] = serde_json::json!(true);
    spec["properties"]["platforms"]["items"]["enum"] =
        serde_json::json!(["linux/amd64", "linux/arm64"]);
    let mut validations: Vec<_> = ["executionId", "tenantId", "projectId", "pool", "snapshotId", "snapshotDigest", "taskSecret", "platforms", "allowEmulation", "publishOnly", "timeoutSeconds"].iter().map(|f| serde_json::json!({"rule":format!("self.{f} == oldSelf.{f}"),"message":"execution context is immutable"})).collect();
    validations.push(serde_json::json!({"rule":"!oldSelf.cancelRequested || self.cancelRequested","message":"cancellation is irreversible"}));
    spec["x-kubernetes-validations"] = serde_json::Value::Array(validations);
    vec![pool, serde_json::from_value(image).unwrap()]
}
