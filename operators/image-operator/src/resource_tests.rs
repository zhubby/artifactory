//! [INPUT]: 依赖声明式资源生成器和模拟节点能力。
//! [OUTPUT]: 验证架构选择、sidecar 生命周期、任务材料隔离与归属。
//! [POS]: 不连接集群的工作负载契约回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{BuilderPool, ImageBuild},
    resources,
};
use serde_json::{Value, json};
fn build() -> ImageBuild {
    serde_json::from_value(json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":"test","namespace":"test","uid":"owner-uid"},"spec":{"executionId":"10000000-0000-4000-8000-000000000001","tenantId":"10000000-0000-4000-8000-000000000002","projectId":"10000000-0000-4000-8000-000000000003","pool":"default","sourceManifestId":"10000000-0000-4000-8000-000000000004","sourceDigest":"a".repeat(64),"taskSecret":"task","platforms":["linux/amd64","linux/arm64"],"allowEmulation":false,"timeoutSeconds":600}})).unwrap()
}
fn pool() -> BuilderPool {
    serde_json::from_value(json!({"metadata":{"name":"default"},"spec":{"workerImage":"worker:1","buildkitImage":"buildkit:1","sourcePreparerImage":"preparer:1","controlUrl":"https://control","maxActive":4,"maxPerProject":2,"emulationNodeSelector":{"build.example.com/binfmt":"ready"},"emulationPlatforms":["linux/arm64"],"buildkitConfigMap":"buildkit-config","caSecret":"ca"}})).unwrap()
}
fn node(arch: &str) -> k8s_openapi::api::core::v1::Node {
    serde_json::from_value(json!({"metadata":{"labels":{"kubernetes.io/os":"linux","kubernetes.io/arch":arch,"build.example.com/binfmt":"ready"}},"status":{"conditions":[{"type":"Ready","status":"True"}]}})).unwrap()
}
#[test]
fn native_architecture_precedes_explicit_emulation() {
    let mut build = build();
    let pool = pool();
    let nodes = vec![node("amd64")];
    assert!(
        resources::placement(&build, &pool, "linux/arm64", &nodes)
            .unwrap()
            .is_none()
    );
    build.spec.allow_emulation = true;
    assert!(
        resources::placement(&build, &pool, "linux/arm64", &nodes)
            .unwrap()
            .unwrap()
            .contains_key("build.example.com/binfmt")
    );
    let native = resources::placement(
        &build,
        &pool,
        "linux/arm64",
        &[node("amd64"), node("arm64")],
    )
    .unwrap()
    .unwrap();
    assert_eq!(native["kubernetes.io/arch"], "arm64");
    assert!(!native.contains_key("build.example.com/binfmt"));
}
#[test]
fn workers_mount_only_their_task_and_sidecar_stops_with_job() {
    let build = build();
    let pool = pool();
    let build_job = resources::job(&build, &pool, Some("linux/amd64"), Default::default()).unwrap();
    assert!(resources::owned(&build, &build_job));
    assert_eq!(
        build_job.metadata.owner_references.as_ref().unwrap()[0].block_owner_deletion,
        Some(false)
    );
    let mut impostor = build.clone();
    impostor.metadata.uid = Some("other".into());
    assert!(!resources::owned(&impostor, &build_job));
    let value = serde_json::to_value(build_job).unwrap();
    let pod = &value["spec"]["template"]["spec"];
    assert_eq!(pod["automountServiceAccountToken"], false);
    assert_eq!(pod["volumes"][0]["secret"]["items"][0]["key"], "build.json");
    assert_eq!(pod["initContainers"][1]["restartPolicy"], "Always");
    assert_eq!(
        pod["containers"][0]["securityContext"]["readOnlyRootFilesystem"],
        true
    );
    let publisher: Value =
        serde_json::to_value(resources::job(&build, &pool, None, Default::default()).unwrap())
            .unwrap();
    let pod = &publisher["spec"]["template"]["spec"];
    assert!(pod["initContainers"].is_null());
    assert_eq!(
        pod["volumes"][0]["secret"]["items"][0]["key"],
        "publish.json"
    );
}
#[test]
fn crd_fences_publication_mode_and_cancellation() {
    let crds = crate::api::crds();
    let value = serde_json::to_value(&crds[1]).unwrap();
    let platforms = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
        ["properties"]["platforms"];
    assert!(platforms.get("uniqueItems").is_none());
    assert_eq!(platforms["x-kubernetes-list-type"], "set");
    let rules=value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]["x-kubernetes-validations"].as_array().unwrap();
    assert!(
        rules
            .iter()
            .any(|r| r["rule"] == "self.publishOnly == oldSelf.publishOnly")
    );
    assert!(
        rules
            .iter()
            .any(|r| r["rule"] == "!oldSelf.cancelRequested || self.cancelRequested")
    );
}

#[test]
fn source_init_has_exclusive_auth_and_no_persistent_cache() {
    let value = serde_json::to_value(
        resources::job(&build(), &pool(), Some("linux/arm64"), Default::default()).unwrap(),
    )
    .unwrap();
    let pod = &value["spec"]["template"]["spec"];
    assert_eq!(pod["initContainers"][0]["name"], "source");
    assert_eq!(pod["initContainers"][1]["name"], "buildkit");
    assert!(
        value["spec"].get("ttlSecondsAfterFinished").is_none(),
        "platform history survives a slower architecture"
    );
    let source_mounts = pod["initContainers"][0]["volumeMounts"].as_array().unwrap();
    assert!(
        source_mounts.iter().all(|v| v["name"] != "task"),
        "Git initializer never reads push credentials"
    );
    let runner_mounts = pod["containers"][0]["volumeMounts"].as_array().unwrap();
    assert!(
        runner_mounts
            .iter()
            .all(|v| v["name"] != "source-auth" && v["name"] != "source-identity")
    );
    assert!(
        runner_mounts
            .iter()
            .any(|v| v["name"] == "workspace" && v["readOnly"] == true)
    );
    assert!(
        pod["volumes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.get("persistentVolumeClaim").is_none())
    );
}
