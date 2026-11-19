//! [INPUT]: 依赖不可变 CR、AgentPool 与预留执行台账。
//! [OUTPUT]: 创建直接归属 CR 的规划和源码普通 Pod。
//! [POS]: Kubernetes 1.28 资源边界；凭据只进入 init 或源码容器。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{AgentPool, AgentRun, PodExecution, SourcePurpose, SourceTask},
    pods,
};
use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::{Resource, ResourceExt};
use serde_json::json;

pub fn planning_pod(run: &AgentRun, pool: &AgentPool, execution: &PodExecution) -> Result<Pod> {
    let mut owner = run
        .controller_owner_ref(&())
        .ok_or_else(|| anyhow::anyhow!("run UID required"))?;
    owner.block_owner_deletion = Some(false);
    let name = &execution.name;
    let mut value = json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":name,"namespace":run.namespace(),"ownerReferences":[owner],"finalizers":[pods::POD_FINALIZER],"labels":{"app.kubernetes.io/name":"artifactory-pi","artifactory.dev/run":run.spec.run_id,"artifactory.dev/agent-pool":run.spec.pool,"artifactory.dev/project":run.spec.project_id,"artifactory.dev/generation":run.spec.generation.to_string(),"artifactory.dev/attempt":execution.attempt.to_string()}},"spec":{"serviceAccountName":"artifactory-pi","automountServiceAccountToken":false,"restartPolicy":"Never","securityContext":{"runAsNonRoot":true,"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"pi","image":pool.spec.image,"imagePullPolicy":pool.spec.image_pull_policy,"command":["node","dist/pod-main.js"],"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"env":[{"name":"ARTIFACTORY_RUN_ID","value":run.spec.run_id},{"name":"ARTIFACTORY_RUN_GENERATION","value":run.spec.generation.to_string()},{"name":"ARTIFACTORY_RUNTIME_URL","value":pool.spec.runtime_url},{"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},{"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}},{"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/artifactory/token"}],"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory},"limits":{"cpu":"2","memory":"2Gi"}},"volumeMounts":[{"name":"identity","mountPath":"/var/run/artifactory","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]}],"volumes":[{"name":"identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-runtime","expirationSeconds":600,"path":"token"}}]}},{"name":"tmp","emptyDir":{}}]}});
    if run.spec.source_manifest_id.is_some() {
        let pod = &mut value["spec"];
        let mut init = json!({
            "name":"source", "image":pool.spec.source_preparer_image,
            "imagePullPolicy":pool.spec.image_pull_policy,
            "command":["artifactory-source-preparer", "checkout"],
            "env":[
                {"name":"ARTIFACTORY_SOURCE_PREPARE_URL","value":format!("{}/v1/prepare",pool.spec.runtime_url.trim_end_matches('/'))},
                {"name":"ARTIFACTORY_SOURCE_IDENTITY","value":json!({"run_id":run.spec.run_id,"generation":run.spec.generation}).to_string()},
                {"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"},
                {"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"},
                {"name":"ARTIFACTORY_SOURCE_AUTH_ROOT","value":"/run/source-auth"},
                {"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/source-identity/token"},
                {"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},
                {"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},
                {"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}}
            ],
            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
            "resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":"2","memory":"2Gi","ephemeral-storage":pool.spec.ephemeral_storage}},
            "volumeMounts":[{"name":"source","mountPath":"/workspace"},{"name":"source-auth","mountPath":"/run/source-auth"},{"name":"source-identity","mountPath":"/var/run/source-identity","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]
        });
        if !pool.spec.tls_secret_name.is_empty() {
            init["env"]
                .as_array_mut()
                .expect("environment array")
                .push(json!({"name":"ARTIFACTORY_SERVICE_CA","value":"/run/service-ca/ca.crt"}));
            init["volumeMounts"]
                .as_array_mut()
                .expect("mount array")
                .push(json!({"name":"ca","mountPath":"/run/service-ca","readOnly":true}));
        }
        pod["initContainers"] = json!([init]);
        pod["volumes"].as_array_mut().expect("volume array").extend([
            json!({"name":"source","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}}),
            json!({"name":"source-auth","emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}}),
            json!({"name":"source-identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-runtime","expirationSeconds":600,"path":"token"}}]}})
        ]);
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .expect("mount array")
            .push(json!({"name":"source","mountPath":"/workspace","readOnly":true}));
        pod["containers"][0]["env"].as_array_mut().expect("environment array").extend([
            json!({"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"}),
            json!({"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"})
        ]);
    }
    if !pool.spec.tls_secret_name.is_empty() {
        let pod = &mut value["spec"];
        pod["containers"][0]["env"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"NODE_EXTRA_CA_CERTS","value":"/run/service-ca/ca.crt"}));
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"ca","mountPath":"/run/service-ca","readOnly":true}));
        pod["volumes"].as_array_mut().unwrap().push(json!({"name":"ca","secret":{"secretName":pool.spec.tls_secret_name,"items":[{"key":"ca.crt","path":"ca.crt"}]}}));
    }
    value["spec"]["activeDeadlineSeconds"] = json!(pods::remaining_seconds(execution));
    value["spec"]["terminationGracePeriodSeconds"] = json!(30);
    Ok(serde_json::from_value(value)?)
}
pub fn source_pod(task: &SourceTask, pool: &AgentPool, execution: &PodExecution) -> Result<Pod> {
    let name = &execution.name;
    let mut owner = task
        .controller_owner_ref(&())
        .ok_or_else(|| anyhow::anyhow!("SourceTask UID required"))?;
    owner.block_owner_deletion = Some(false);
    let mode = match task.spec.purpose {
        SourcePurpose::Resolve => "resolve",
        SourcePurpose::Refs => "refs",
    };
    let labels = json!({"app.kubernetes.io/name":"artifactory-source-task", "artifactory.dev/source-task":task.name_any(), "artifactory.dev/source-task-pool":task.spec.pool, "artifactory.dev/project":task.spec.project_id,"artifactory.dev/attempt":execution.attempt.to_string()});
    let mut value = json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":name,"namespace":task.namespace(),"ownerReferences":[owner],"finalizers":[pods::POD_FINALIZER],"labels":labels},"spec":{"serviceAccountName":"artifactory-source-preparer","automountServiceAccountToken":false,"restartPolicy":"Never","securityContext":{"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001,"runAsNonRoot":true,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"source","image":pool.spec.source_preparer_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":["artifactory-source-preparer",mode],"env":[
        {"name":"ARTIFACTORY_SOURCE_PREPARE_URL","value":format!("{}/internal/source/prepare",pool.spec.control_url.trim_end_matches('/'))},
        {"name":"ARTIFACTORY_SOURCE_IDENTITY","value":json!({"kind":"source-task","task_id":task.spec.task_id}).to_string()},
        {"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"},
        {"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"},
        {"name":"ARTIFACTORY_SOURCE_AUTH_ROOT","value":"/run/source-auth"},
        {"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/source-identity/token"},
        {"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},
        {"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},
        {"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}}
    ],"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":"2","memory":"2Gi","ephemeral-storage":pool.spec.ephemeral_storage}},"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"workspace","mountPath":"/workspace"},{"name":"auth","mountPath":"/run/source-auth"},{"name":"identity","mountPath":"/var/run/source-identity","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]}],"volumes":[{"name":"workspace","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}},{"name":"auth","emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}},{"name":"identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-source-prepare","expirationSeconds":600,"path":"token"}}]}}]}});
    if !pool.spec.tls_secret_name.is_empty() {
        let pod = &mut value["spec"];
        pod["containers"][0]["env"]
            .as_array_mut()
            .expect("environment array")
            .push(json!({"name":"ARTIFACTORY_SERVICE_CA","value":"/run/service-ca/ca.crt"}));
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .expect("mount array")
            .push(json!({"name":"ca","mountPath":"/run/service-ca","readOnly":true}));
        pod["volumes"].as_array_mut().expect("volume array").push(json!({"name":"ca","secret":{"secretName":pool.spec.tls_secret_name,"items":[{"key":"ca.crt","path":"ca.crt"}]}}));
    }
    value["spec"]["activeDeadlineSeconds"] = json!(pods::remaining_seconds(execution));
    value["spec"]["terminationGracePeriodSeconds"] = json!(30);
    Ok(serde_json::from_value(value)?)
}
