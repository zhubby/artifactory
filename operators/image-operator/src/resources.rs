//! [INPUT]: 依赖固定 ImageBuild、管理员池级代理和可用节点。
//! [OUTPUT]: 按角色预算创建源码 init、Runner、BuildKit 和 Publisher Pod。
//! [POS]: 声明 1.28 身份与进程隔离；信任卷只投影公开 CA。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::api::{BuilderPool, ContainerResources, ImageBuild};
use anyhow::{Result, ensure};
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::{Resource, ResourceExt};
use serde_json::json;
use std::collections::BTreeMap;

pub const POD_FINALIZER: &str = "images.artifactory.dev/pod-cleanup";

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

pub fn pod(
    build: &ImageBuild,
    pool: &BuilderPool,
    platform: Option<&str>,
    selector: BTreeMap<String, String>,
) -> Result<Pod> {
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
    let worker_budget = if publisher {
        &pool.spec.resources.publisher
    } else {
        &pool.spec.resources.runner
    };
    // 源码卷在 init 退出后仍占磁盘，计入普通容器阶段的存储请求。
    let retained_source = if publisher {
        0
    } else {
        storage_bytes(&pool.spec.resources.source.ephemeral_storage)?
    };
    let labels = json!({"app.kubernetes.io/name":"artifactory-image-worker","artifactory.dev/image-build":build.name_any(),"artifactory.dev/builder-pool":build.spec.pool,"artifactory.dev/project":build.spec.project_id,"artifactory.dev/platform":platform.unwrap_or("publish").strip_prefix("linux/").unwrap_or("publish")});
    let mut value = json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":name(build,platform)?,"namespace":build.namespace(),"ownerReferences":[owner],"finalizers":[POD_FINALIZER],"labels":labels},"spec":{"activeDeadlineSeconds":build.spec.timeout_seconds,"serviceAccountName":"artifactory-image-worker","automountServiceAccountToken":false,"restartPolicy":"Never","nodeSelector":selector,"securityContext":{"runAsUser":1000,"runAsGroup":1000,"fsGroup":1000,"runAsNonRoot":true,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"worker","image":pool.spec.worker_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":[binary],"env":[{"name":"ARTIFACTORY_IMAGE_TASK","value":"/run/task/task.json"},{"name":"ARTIFACTORY_IMAGE_PLATFORM","value":platform.unwrap_or("publish")},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},{"name":"BUILDKIT_HOST","value":"unix:///run/buildkit/buildkitd.sock"}],"resources":requirements(worker_budget,retained_source)?,"securityContext":{"readOnlyRootFilesystem":true,"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"task","mountPath":"/run/task","readOnly":true},{"name":"workspace","mountPath":"/workspace","readOnly":true},{"name":"work","mountPath":"/work"},{"name":"tmp","mountPath":"/tmp"},{"name":"socket","mountPath":"/run/buildkit"}]}],"volumes":[{"name":"task","secret":{"secretName":build.spec.task_secret,"defaultMode":288,"items":[{"key":task_key,"path":"task.json"}]}},{"name":"workspace","emptyDir":{"sizeLimit":pool.spec.resources.source.ephemeral_storage}},{"name":"work","emptyDir":{"sizeLimit":worker_budget.ephemeral_storage}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}},{"name":"socket","emptyDir":{}}]}});
    let pod = &mut value["spec"];
    pod["terminationGracePeriodSeconds"] = json!(45);
    if !publisher {
        pod["containers"].as_array_mut().unwrap().push(json!({"name":"buildkit","image":pool.spec.buildkit_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":["artifactory-image-buildkit"],"args":[],"env":[{"name":"ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS","value":build.spec.timeout_seconds.to_string()},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}}],"securityContext":{"runAsUser":1000,"runAsGroup":1000,"runAsNonRoot":true,"allowPrivilegeEscalation":true,"seccompProfile":{"type":"Unconfined"}},"resources":requirements(&pool.spec.resources.buildkit,0)?,"volumeMounts":[{"name":"socket","mountPath":"/run/buildkit"},{"name":"buildkit-state","mountPath":"/home/user/.local/share/buildkit"}]}));
        pod["containers"][0]["env"].as_array_mut().unwrap().push(json!({"name":"ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS","value":build.spec.timeout_seconds.to_string()}));
        pod["volumes"].as_array_mut().unwrap().push(
            json!({"name":"buildkit-state","emptyDir":{"sizeLimit":pool.spec.resources.buildkit.ephemeral_storage}}),
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
            "resources":requirements(&pool.spec.resources.source,0)?,
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
        value["metadata"]["annotations"] =
            json!({"container.apparmor.security.beta.kubernetes.io/buildkit":"unconfined"});
    }
    if let Some(proxy) = &pool.spec.proxy {
        let api_host = std::env::var("KUBERNETES_SERVICE_HOST").ok();
        let environment: Vec<_> = proxy
            .environment(&[&pool.spec.control_url], api_host.as_deref())?
            .into_iter()
            .map(|(name, value)| json!({"name":name,"value":value}))
            .collect();
        for group in ["containers", "initContainers"] {
            for container in value["spec"][group].as_array_mut().into_iter().flatten() {
                container["env"]
                    .as_array_mut()
                    .expect("environment array")
                    .extend(environment.clone());
                if container["name"] == "worker" {
                    container["env"]
                        .as_array_mut()
                        .expect("environment array")
                        .push(json!({"name":"ARTIFACTORY_POOL_PROXY","value":"1"}));
                }
            }
        }
    }
    Ok(serde_json::from_value(value)?)
}

fn requirements(budget: &ContainerResources, retained_storage: u64) -> Result<serde_json::Value> {
    let total = storage_bytes(&budget.ephemeral_storage)?
        .checked_add(retained_storage)
        .filter(|bytes| *bytes <= i64::MAX as u64)
        .ok_or_else(|| anyhow::anyhow!("ephemeral storage budget overflow"))?;
    let values =
        json!({"cpu":budget.cpu,"memory":budget.memory,"ephemeral-storage":total.to_string()});
    Ok(json!({"requests":values,"limits":values}))
}

fn storage_bytes(quantity: &str) -> Result<u64> {
    let split = quantity
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(quantity.len());
    let (number, suffix) = quantity.split_at(split);
    let factor = match suffix {
        "" => 1,
        "Ki" => 1 << 10,
        "Mi" => 1 << 20,
        "Gi" => 1 << 30,
        "Ti" => 1 << 40,
        "Pi" => 1 << 50,
        "Ei" => 1 << 60,
        "k" => 1000,
        "M" => 1000_u64.pow(2),
        "G" => 1000_u64.pow(3),
        "T" => 1000_u64.pow(4),
        "P" => 1000_u64.pow(5),
        "E" => 1000_u64.pow(6),
        _ => anyhow::bail!(
            "ephemeral storage requires a positive integer byte quantity with an optional SI or binary suffix"
        ),
    };
    let bytes = number
        .parse::<u64>()?
        .checked_mul(factor)
        .filter(|bytes| *bytes > 0 && *bytes <= i64::MAX as u64)
        .ok_or_else(|| anyhow::anyhow!("ephemeral storage budget must fit positive int64 bytes"))?;
    ensure!(
        !number.starts_with('0'),
        "ephemeral storage requires a positive integer byte quantity"
    );
    Ok(bytes)
}

pub fn owned(build: &ImageBuild, pod: &Pod) -> bool {
    let Some(uid) = build.uid() else { return false };
    pod.owner_references().iter().any(|r| {
        r.uid == uid
            && r.kind == "ImageBuild"
            && r.api_version == "images.artifactory.dev/v1alpha1"
            && r.name == build.name_any()
            && r.controller == Some(true)
    })
}
