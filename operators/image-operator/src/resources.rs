//! [INPUT]: 依赖固定 ImageBuild、管理员 BuilderPool 和可用节点。
//! [OUTPUT]: 按 BuilderPool sidecar 模式创建隔离 BuildKit 与 Publisher Job。
//! [POS]: 声明工作负载身份、凭据投影和调度安全边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::api::{BuilderPool, ImageBuild};
use anyhow::{Result, ensure};
use k8s_openapi::api::{batch::v1::Job, core::v1::Node};
use kube::{Resource, ResourceExt};
use serde_json::json;
use std::collections::BTreeMap;

pub(crate) const LEGACY_WORKER: &str = r#"set -eu
trap 'touch /run/buildkit/worker.done' EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
attempt=0
until buildctl --addr unix:///run/buildkit/buildkitd.sock debug workers >/dev/null 2>&1; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 90 ]; then
        echo 'BuildKit startup timed out' >&2
        exit 1
    fi
    sleep 2
done
"$@"
"#;

pub(crate) const LEGACY_BUILDKIT: &str = r#"set -eu
rootlesskit buildkitd "$@" &
daemon=$!
trap 'kill "$daemon" 2>/dev/null || true; wait "$daemon" 2>/dev/null || true' EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
while kill -0 "$daemon" 2>/dev/null; do
    if [ -f /run/buildkit/worker.done ]; then
        exit 0
    fi
    sleep 1 &
    wait $!
done
wait "$daemon"
"#;

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
    let labels = json!({"app.kubernetes.io/name":"artifactory-image-worker","artifactory.dev/image-build":build.name_any(),"artifactory.dev/builder-pool":build.spec.pool,"artifactory.dev/project":build.spec.project_id});
    let mut value = json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"name":name(build,platform)?,"namespace":build.namespace(),"ownerReferences":[owner],"labels":labels},"spec":{"backoffLimit":0,"activeDeadlineSeconds":build.spec.timeout_seconds,"ttlSecondsAfterFinished":86400,"template":{"metadata":{"labels":labels},"spec":{"serviceAccountName":"artifactory-image-worker","automountServiceAccountToken":false,"restartPolicy":"Never","nodeSelector":selector,"securityContext":{"runAsUser":1000,"runAsGroup":1000,"fsGroup":1000,"runAsNonRoot":true,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"worker","image":pool.spec.worker_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":[binary],"env":[{"name":"ARTIFACTORY_IMAGE_TASK","value":"/run/task/task.json"},{"name":"ARTIFACTORY_IMAGE_PLATFORM","value":platform.unwrap_or("publish")},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},{"name":"BUILDKIT_HOST","value":"unix:///run/buildkit/buildkitd.sock"}],"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage}},"securityContext":{"readOnlyRootFilesystem":true,"allowPrivilegeEscalation":false,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"task","mountPath":"/run/task","readOnly":true},{"name":"workspace","mountPath":"/workspace"},{"name":"tmp","mountPath":"/tmp"},{"name":"socket","mountPath":"/run/buildkit"}]}],"volumes":[{"name":"task","secret":{"secretName":build.spec.task_secret,"defaultMode":288,"items":[{"key":task_key,"path":"task.json"}]}},{"name":"workspace","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}},{"name":"socket","emptyDir":{}}]}}}});
    let pod = &mut value["spec"]["template"]["spec"];
    if !publisher {
        pod["initContainers"] = json!([{"name":"buildkit","restartPolicy":"Always","image":pool.spec.buildkit_image,"imagePullPolicy":pool.spec.image_pull_policy,"args":["--addr","unix:///run/buildkit/buildkitd.sock","--oci-worker-no-process-sandbox"],"securityContext":{"runAsUser":1000,"runAsGroup":1000,"runAsNonRoot":true,"seccompProfile":{"type":"Unconfined"},"appArmorProfile":{"type":"Unconfined"}},"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage}},"volumeMounts":[{"name":"socket","mountPath":"/run/buildkit"},{"name":"buildkit-state","mountPath":"/home/user/.local/share/buildkit"}],"startupProbe":{"exec":{"command":["buildctl","--addr","unix:///run/buildkit/buildkitd.sock","debug","workers"]},"periodSeconds":2,"failureThreshold":90}}]);
        pod["volumes"].as_array_mut().unwrap().push(
            json!({"name":"buildkit-state","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}}),
        );
    }
    if !publisher && !pool.spec.buildkit_config_map.is_empty() {
        pod["volumes"].as_array_mut().unwrap().push(
            json!({"name":"buildkit-config","configMap":{"name":pool.spec.buildkit_config_map}}),
        );
        pod["initContainers"][0]["volumeMounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"buildkit-config","mountPath":"/etc/buildkit","readOnly":true}));
        pod["initContainers"][0]["args"]
            .as_array_mut()
            .unwrap()
            .extend([json!("--config"), json!("/etc/buildkit/buildkitd.toml")]);
    }
    if !pool.spec.ca_secret.is_empty() {
        pod["volumes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"ca","secret":{"secretName":pool.spec.ca_secret}}));
        if !publisher {
            pod["initContainers"][0]["volumeMounts"]
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
    if !publisher && !pool.spec.native_sidecar {
        // 旧集群以共享完成标记停止普通容器，保留 Runner 的退出码。
        let mut buildkit = pod
            .as_object_mut()
            .unwrap()
            .remove("initContainers")
            .unwrap()[0]
            .take();
        buildkit.as_object_mut().unwrap().remove("restartPolicy");
        buildkit["command"] = json!(["/bin/sh", "-c", LEGACY_BUILDKIT, "buildkit-sidecar"]);
        pod["containers"][0]["command"] =
            json!(["/bin/sh", "-c", LEGACY_WORKER, "image-worker", binary]);
        pod["containers"].as_array_mut().unwrap().push(buildkit);
        value["spec"]["template"]["metadata"]["annotations"] = json!({
            "container.apparmor.security.beta.kubernetes.io/buildkit": "unconfined"
        });
    }
    Ok(serde_json::from_value(value)?)
}

pub fn owned(build: &ImageBuild, job: &Job) -> bool {
    job.owner_references()
        .iter()
        .any(|r| r.uid == build.uid().unwrap_or_default() && r.kind == "ImageBuild")
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
