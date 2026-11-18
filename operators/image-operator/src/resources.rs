//! [INPUT]: 依赖固定 ImageBuild、管理员 BuilderPool 和可用节点。
//! [OUTPUT]: 创建普通源码 init、独立 Runner 与 BuildKit 容器和 Publisher Job。
//! [POS]: 声明 1.28 身份与进程隔离；信任卷只投影公开 CA。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::api::{BuilderPool, ImageBuild};
use anyhow::{Result, ensure};
use k8s_openapi::api::{batch::v1::Job, core::v1::Node};
use kube::{Resource, ResourceExt};
use serde_json::json;
use std::collections::BTreeMap;

pub fn name(build: &ImageBuild, platform: Option<&str>) -> Result<String> {
    let id = uuid::Uuid::parse_str(&build.spec.execution_id)?;
    let suffix = platform.map_or("publish", |p| p.strip_prefix("linux/").unwrap_or("invalid"));
    ensure!(
        ["publish", "amd64", "arm64"].contains(&suffix),
        "unsupported platform"
    );
    Ok(format!("image-{}-{suffix}", id.simple()))
}

pub fn placement(
    build: &ImageBuild,
    pool: &BuilderPool,
    platform: &str,
    nodes: &[Node],
) -> Result<Option<BTreeMap<String, String>>> {
    ensure!(
        artifactory_image_protocol::SUPPORTED_PLATFORMS.contains(&platform),
        "invalid platform"
    );
    let arch = platform.strip_prefix("linux/").unwrap();
    let available = |selector: &BTreeMap<String, String>| {
        nodes.iter().any(|node| {
            node.spec
                .as_ref()
                .is_none_or(|s| !s.unschedulable.unwrap_or(false))
                && node
                    .status
                    .as_ref()
                    .and_then(|s| s.conditions.as_ref())
                    .is_some_and(|c| c.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
                && selector
                    .iter()
                    .all(|(k, v)| node.labels().get(k) == Some(v))
        })
    };
    let mut native = pool.spec.node_selector.clone();
    native.insert("kubernetes.io/os".into(), "linux".into());
    native.insert("kubernetes.io/arch".into(), arch.into());
    if pool
        .spec
        .node_selector
        .get("kubernetes.io/arch")
        .is_none_or(|v| v == arch)
        && available(&native)
    {
        return Ok(Some(native));
    }
    if build.spec.allow_emulation
        && pool.spec.emulation_platforms.iter().any(|p| p == platform)
        && !pool.spec.emulation_node_selector.is_empty()
    {
        let mut simulated = pool.spec.node_selector.clone();
        simulated.extend(pool.spec.emulation_node_selector.clone());
        simulated.insert("kubernetes.io/os".into(), "linux".into());
        if available(&simulated) {
            return Ok(Some(simulated));
        }
    }
    Ok(None)
}

pub fn job(
    build: &ImageBuild,
    pool: &BuilderPool,
    platform: Option<&str>,
    selector: BTreeMap<String, String>,
) -> Result<Job> {
    ensure!(
        pool.spec.max_active > 0 && pool.spec.max_per_project > 0,
        "builder limits must be positive"
    );
    let mut owner = build
        .controller_owner_ref(&())
        .ok_or_else(|| anyhow::anyhow!("ImageBuild UID required"))?;
    // 子资源依赖 GC 回收，不要求控制器拥有父资源 finalizers 权限。
    owner.block_owner_deletion = Some(false);
    let publisher = platform.is_none();
    let task_key = if publisher {
        "publish.json"
    } else {
        "build.json"
    };
    let binary = if publisher {
        "artifactory-image-publisher"
    } else {
        "artifactory-image-runner"
    };
    let labels = json!({"app.kubernetes.io/name":"artifactory-image-worker","artifactory.dev/image-build":build.name_any(),"artifactory.dev/builder-pool":build.spec.pool,"artifactory.dev/project":build.spec.project_id,"artifactory.dev/platform":platform.unwrap_or("publish").strip_prefix("linux/").unwrap_or("publish")});
    let mut value = json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"name":name(build,platform)?,"namespace":build.namespace(),"ownerReferences":[owner],"labels":labels},"spec":{"backoffLimit":0,"activeDeadlineSeconds":build.spec.timeout_seconds,"template":{"metadata":{"labels":labels},"spec":{"serviceAccountName":"artifactory-image-worker","automountServiceAccountToken":false,"restartPolicy":"Never","nodeSelector":selector,"securityContext":{"runAsUser":1000,"runAsGroup":1000,"fsGroup":1000,"runAsNonRoot":true,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"worker","image":pool.spec.worker_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":[binary],"env":[{"name":"ARTIFACTORY_IMAGE_TASK","value":"/run/task/task.json"},{"name":"ARTIFACTORY_IMAGE_PLATFORM","value":platform.unwrap_or("publish")},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},{"name":"BUILDKIT_HOST","value":"unix:///run/buildkit/buildkitd.sock"}],"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage}},"securityContext":{"readOnlyRootFilesystem":true,"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"task","mountPath":"/run/task","readOnly":true},{"name":"workspace","mountPath":"/workspace","readOnly":true},{"name":"work","mountPath":"/work"},{"name":"tmp","mountPath":"/tmp"},{"name":"socket","mountPath":"/run/buildkit"}]}],"volumes":[{"name":"task","secret":{"secretName":build.spec.task_secret,"defaultMode":288,"items":[{"key":task_key,"path":"task.json"}]}},{"name":"workspace","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}},{"name":"work","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}},{"name":"socket","emptyDir":{}}]}}}});
    let pod = &mut value["spec"]["template"]["spec"];
    pod["terminationGracePeriodSeconds"] = json!(45);
    if !publisher {
        pod["containers"].as_array_mut().unwrap().push(json!({"name":"buildkit","image":pool.spec.buildkit_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":["artifactory-image-buildkit"],"args":[],"env":[{"name":"ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS","value":build.spec.timeout_seconds.to_string()},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}}],"securityContext":{"runAsUser":1000,"runAsGroup":1000,"runAsNonRoot":true,"allowPrivilegeEscalation":true,"seccompProfile":{"type":"Unconfined"}},"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage}},"volumeMounts":[{"name":"socket","mountPath":"/run/buildkit"},{"name":"buildkit-state","mountPath":"/home/user/.local/share/buildkit"}]}));
        pod["containers"][0]["env"].as_array_mut().unwrap().push(json!({"name":"ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS","value":build.spec.timeout_seconds.to_string()}));
        pod["volumes"].as_array_mut().unwrap().push(
            json!({"name":"buildkit-state","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}}),
        );
    }
    if !publisher && !pool.spec.buildkit_config_map.is_empty() {
        pod["volumes"].as_array_mut().unwrap().push(
            json!({"name":"buildkit-config","configMap":{"name":pool.spec.buildkit_config_map}}),
        );
        pod["containers"][1]["volumeMounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"buildkit-config","mountPath":"/etc/buildkit","readOnly":true}));
        pod["containers"][1]["args"]
            .as_array_mut()
            .unwrap()
            .extend([json!("--config"), json!("/etc/buildkit/buildkitd.toml")]);
    }
    if !pool.spec.ca_secret.is_empty() {
        pod["volumes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"ca","secret":{"secretName":pool.spec.ca_secret,"items":[{"key":"ca.crt","path":"ca.crt"}]}}));
        if !publisher {
            pod["containers"][1]["volumeMounts"]
                .as_array_mut()
                .unwrap()
                .push(json!({"name":"ca","mountPath":"/run/ca","readOnly":true}));
        }
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"ca","mountPath":"/run/ca","readOnly":true}));
        pod["containers"][0]["env"].as_array_mut().unwrap().extend([
            json!({"name":"ARTIFACTORY_SERVICE_CA","value":"/run/ca/ca.crt"}),
            json!({"name":"GIT_SSL_CAINFO","value":"/run/ca/ca.crt"}),
        ]);
    }
    if !publisher {
        let mut source = json!({
            "name":"source", "image":pool.spec.source_preparer_image, "imagePullPolicy":pool.spec.image_pull_policy,
            "command":["artifactory-source-preparer","checkout"],
            "env":[
                {"name":"ARTIFACTORY_SOURCE_PREPARE_URL","value":format!("{}/internal/source/prepare",pool.spec.control_url.trim_end_matches('/'))},
                {"name":"ARTIFACTORY_SOURCE_IDENTITY","value":json!({"kind":"image-build","execution_id":build.spec.execution_id,"platform":platform}).to_string()},
                {"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"},
                {"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"},
                {"name":"ARTIFACTORY_SOURCE_AUTH_ROOT","value":"/run/source-auth"},
                {"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/source-identity/token"},
                {"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},
                {"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},
                {"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}}
            ],
            "resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage}},
            "securityContext":{"readOnlyRootFilesystem":true,"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}},
            "volumeMounts":[{"name":"workspace","mountPath":"/workspace"},{"name":"source-auth","mountPath":"/run/source-auth"},{"name":"source-identity","mountPath":"/var/run/source-identity","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]
        });
        if !pool.spec.ca_secret.is_empty() {
            source["env"]
                .as_array_mut()
                .expect("environment array")
                .push(json!({"name":"ARTIFACTORY_SERVICE_CA","value":"/run/ca/ca.crt"}));
            source["volumeMounts"]
                .as_array_mut()
                .expect("mount array")
                .push(json!({"name":"ca","mountPath":"/run/ca","readOnly":true}));
        }
        pod["initContainers"] = json!([source]);
        pod["volumes"].as_array_mut().expect("volume array").extend([
            json!({"name":"source-auth","emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}}),
            json!({"name":"source-identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-source-prepare","expirationSeconds":600,"path":"token"}}]}})
        ]);
    }
    if !publisher {
        value["spec"]["template"]["metadata"]["annotations"] =
            json!({"container.apparmor.security.beta.kubernetes.io/buildkit":"unconfined"});
    }
    Ok(serde_json::from_value(value)?)
}

pub fn owned(build: &ImageBuild, job: &Job) -> bool {
    let Some(uid) = build.uid() else { return false };
    job.owner_references().iter().any(|r| {
        r.uid == uid
            && r.kind == "ImageBuild"
            && r.api_version == "images.artifactory.dev/v1alpha1"
            && r.name == build.name_any()
            && r.controller == Some(true)
    })
}
pub fn phase(job: &Job) -> Option<&str> {
    job.status
        .as_ref()?
        .conditions
        .as_ref()?
        .iter()
        .find(|c| c.status == "True" && ["Complete", "Failed"].contains(&c.type_.as_str()))
        .map(|c| {
            if c.type_ == "Complete" {
                "Succeeded"
            } else {
                "Failed"
            }
        })
}
