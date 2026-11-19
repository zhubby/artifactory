//! [INPUT]: 依赖 kube CRD、共享出口代理和固定执行身份。
//! [OUTPUT]: 提供池级代理、角色预算、不可变执行 CRD 与 Pod UID 台账。
//! [POS]: Image Operator 的 Kubernetes 公开契约。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_service_transport::proxy::ProxyConfig;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_ownership: Option<PodOwnership>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodOwnership {
    pub image_build_uid: String,
    pub creating: Option<String>,
    #[schemars(length(max = 3))]
    pub pods: Vec<OwnedPod>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedPod {
    pub name: String,
    pub uid: String,
    pub platform: String,
    pub stopped: bool,
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
    pub source_preparer_image: String,
    pub control_url: String,
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
    #[serde(default)]
    pub resources: WorkloadResources,
    #[serde(default)]
    pub ca_secret: String,
    #[serde(default)]
    pub buildkit_config_map: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
}
fn pull_policy() -> String {
    "IfNotPresent".into()
}

/// 每种任务角色独立申请资源，避免将编译预算重复分配给辅助容器。
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadResources {
    #[serde(default = "source_resources")]
    pub source: ContainerResources,
    #[serde(default = "runner_resources")]
    pub runner: ContainerResources,
    #[serde(default = "buildkit_resources")]
    pub buildkit: ContainerResources,
    #[serde(default = "publisher_resources")]
    pub publisher: ContainerResources,
}

impl Default for WorkloadResources {
    fn default() -> Self {
        Self {
            source: source_resources(),
            runner: runner_resources(),
            buildkit: buildkit_resources(),
            publisher: publisher_resources(),
        }
    }
}

/// 容器 CPU/内存与工作卷预算；Runner 另承担保留源码的存储请求。
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContainerResources {
    pub cpu: String,
    pub memory: String,
    #[schemars(regex(pattern = r"^[1-9][0-9]*(Ki|Mi|Gi|Ti|Pi|Ei|k|M|G|T|P|E)?$"))]
    pub ephemeral_storage: String,
}

fn budget(cpu: &str, memory: &str, disk: &str) -> ContainerResources {
    ContainerResources {
        cpu: cpu.into(),
        memory: memory.into(),
        ephemeral_storage: disk.into(),
    }
}
fn source_resources() -> ContainerResources {
    budget("250m", "512Mi", "10Gi")
}
fn runner_resources() -> ContainerResources {
    budget("250m", "256Mi", "1Gi")
}
fn buildkit_resources() -> ContainerResources {
    budget("1", "2Gi", "20Gi")
}
fn publisher_resources() -> ContainerResources {
    budget("500m", "512Mi", "20Gi")
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
    pub source_manifest_id: String,
    pub source_digest: String,
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
    for name in ["executionId", "tenantId", "projectId", "sourceManifestId"] {
        spec["properties"][name]["format"] = serde_json::json!("uuid");
    }
    spec["properties"]["platforms"]["minItems"] = serde_json::json!(1);
    spec["properties"]["platforms"]["maxItems"] = serde_json::json!(2);
    spec["properties"]["platforms"]["x-kubernetes-list-type"] = serde_json::json!("set");
    spec["properties"]["platforms"]["items"]["enum"] =
        serde_json::json!(["linux/amd64", "linux/arm64"]);
    let mut validations: Vec<_> = ["executionId", "tenantId", "projectId", "pool", "sourceManifestId", "sourceDigest", "taskSecret", "platforms", "allowEmulation", "publishOnly", "timeoutSeconds"].iter().map(|f| serde_json::json!({"rule":format!("self.{f} == oldSelf.{f}"),"message":"execution context is immutable"})).collect();
    validations.push(serde_json::json!({"rule":"!oldSelf.cancelRequested || self.cancelRequested","message":"cancellation is irreversible"}));
    spec["x-kubernetes-validations"] = serde_json::Value::Array(validations);
    vec![pool, serde_json::from_value(image).unwrap()]
}
