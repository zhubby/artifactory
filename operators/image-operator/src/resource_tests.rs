//! [INPUT]: 依赖声明式资源生成器和模拟节点能力。
//! [OUTPUT]: 验证 1.28 双容器、权限隔离、资源预算与归属 CRD。
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
fn kubernetes_128_workers_use_two_regular_containers_and_a_single_source_init() {
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
    assert_eq!(pod["initContainers"].as_array().unwrap().len(), 1);
    assert_eq!(pod["containers"].as_array().unwrap().len(), 2);
    assert_eq!(pod["terminationGracePeriodSeconds"], 45);
    assert_ne!(pod["shareProcessNamespace"], true);
    assert_eq!(pod["containers"][1]["name"], "buildkit");
    assert_eq!(
        pod["containers"][1]["command"],
        json!(["artifactory-image-buildkit"])
    );
    assert_eq!(
        pod["containers"][1]["args"],
        json!(["--config", "/etc/buildkit/buildkitd.toml"])
    );
    assert!(pod["containers"][1].get("restartPolicy").is_none());
    assert!(
        pod["containers"][1]["securityContext"]
            .get("appArmorProfile")
            .is_none()
    );
    assert_eq!(
        value["spec"]["template"]["metadata"]["annotations"]["container.apparmor.security.beta.kubernetes.io/buildkit"],
        "unconfined"
    );
    assert_eq!(
        pod["containers"][1]["securityContext"]["seccompProfile"]["type"],
        "Unconfined"
    );
    assert_eq!(
        pod["containers"][0]["securityContext"]["allowPrivilegeEscalation"],
        false
    );
    assert_eq!(
        pod["containers"][1]["securityContext"]["allowPrivilegeEscalation"],
        true
    );
    for worker in pod["containers"].as_array().unwrap() {
        assert_eq!(worker["resources"]["requests"]["cpu"], "1");
        assert_eq!(worker["resources"]["requests"]["memory"], "2Gi");
        assert_eq!(worker["resources"]["requests"]["ephemeral-storage"], "20Gi");
        assert!(worker["env"].as_array().unwrap().iter().any(|e| e["name"] == "ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS" && e["value"] == "600"));
        assert!(
            worker["env"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["name"] == "POD_UID")
        );
    }
    let mounts = pod["containers"][1]["volumeMounts"].as_array().unwrap();
    assert!(mounts.iter().all(|m| {
        ![
            "task",
            "workspace",
            "work",
            "source-auth",
            "source-identity",
        ]
        .iter()
        .any(|name| m["name"] == *name)
    }));
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
fn crd_uses_typed_source_manifest_identity() {
    let crds = crate::api::crds();
    let value = serde_json::to_value(&crds[1]).unwrap();
    let properties = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
        ["properties"];
    assert!(
        properties.get("snapshotId").is_none(),
        "removed snapshot identity must not create an untyped CRD property"
    );
    for name in ["executionId", "tenantId", "projectId", "sourceManifestId"] {
        assert_eq!(properties[name]["type"], "string", "identity {name}");
        assert_eq!(properties[name]["format"], "uuid", "identity {name}");
    }
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
    assert_eq!(pod["initContainers"].as_array().unwrap().len(), 1);
    assert_eq!(pod["containers"][1]["name"], "buildkit");
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

#[test]
fn image_status_owns_a_bounded_job_uid_ledger() {
    let crds = crate::api::crds();
    let crd = serde_json::to_value(&crds[1]).unwrap();
    let ledger = &crd["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["status"]["properties"]
        ["jobOwnership"];
    assert_eq!(ledger["properties"]["imageBuildUid"]["type"], "string");
    assert_eq!(ledger["properties"]["jobs"]["maxItems"], 3);
    for field in ["name", "uid", "platform"] {
        assert!(
            ledger["properties"]["jobs"]["items"]["properties"]
                .get(field)
                .is_some(),
            "missing {field}"
        );
    }
}

#[test]
fn publisher_blob_staging_has_a_writable_disk_volume() {
    let job = resources::job(&build(), &pool(), None, Default::default()).unwrap();
    let pod = job.spec.unwrap().template.spec.unwrap();
    let worker = &pod.containers[0];
    let staging = worker
        .volume_mounts
        .as_ref()
        .unwrap()
        .iter()
        .find(|mount| mount.mount_path == crate::publisher::TEMPORARY_DIRECTORY)
        .expect("publisher blob staging directory has a volume mount");
    assert!(
        !staging.read_only.unwrap_or(false),
        "publisher must create blob verification files on a writable volume"
    );
    let volume = pod
        .volumes
        .as_ref()
        .unwrap()
        .iter()
        .find(|volume| volume.name == staging.name)
        .unwrap();
    let disk = volume
        .empty_dir
        .as_ref()
        .expect("publication staging is task-local storage");
    assert!(
        disk.medium.as_deref().is_none_or(str::is_empty),
        "large image blobs must use disk rather than the bounded memory volume"
    );
    assert_eq!(
        worker
            .security_context
            .as_ref()
            .unwrap()
            .read_only_root_filesystem,
        Some(true)
    );
    let source = worker
        .volume_mounts
        .as_ref()
        .unwrap()
        .iter()
        .find(|mount| mount.name == "workspace")
        .unwrap();
    assert_eq!(source.read_only, Some(true));
}
