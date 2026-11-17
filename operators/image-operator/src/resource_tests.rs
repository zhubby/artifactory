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
    serde_json::from_value(json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":"test","namespace":"test","uid":"owner-uid"},"spec":{"executionId":"10000000-0000-4000-8000-000000000001","tenantId":"10000000-0000-4000-8000-000000000002","projectId":"10000000-0000-4000-8000-000000000003","pool":"default","snapshotId":"10000000-0000-4000-8000-000000000004","snapshotDigest":"a".repeat(64),"taskSecret":"task","platforms":["linux/amd64","linux/arm64"],"allowEmulation":false,"timeoutSeconds":600}})).unwrap()
}
fn pool() -> BuilderPool {
    serde_json::from_value(json!({"metadata":{"name":"default"},"spec":{"workerImage":"worker:1","buildkitImage":"buildkit:1","maxActive":4,"maxPerProject":2,"emulationNodeSelector":{"build.example.com/binfmt":"ready"},"emulationPlatforms":["linux/arm64"],"buildkitConfigMap":"buildkit-config","caSecret":"ca"}})).unwrap()
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
    assert!(pool.spec.native_sidecar);
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
    assert_eq!(pod["initContainers"][0]["restartPolicy"], "Always");
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
    let pool = serde_json::to_value(&crds[0]).unwrap();
    assert_eq!(
        pool["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]["properties"]
            ["nativeSidecar"]["default"],
        true
    );
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
fn legacy_sidecar_runs_with_worker_and_preserves_publication_isolation() {
    let build = build();
    let mut value = serde_json::to_value(pool()).unwrap();
    value["spec"]["nativeSidecar"] = json!(false);
    let pool: BuilderPool = serde_json::from_value(value).unwrap();
    let value = serde_json::to_value(
        resources::job(&build, &pool, Some("linux/amd64"), Default::default()).unwrap(),
    )
    .unwrap();
    let pod = &value["spec"]["template"]["spec"];
    assert!(pod["initContainers"].is_null());
    assert_eq!(pod["containers"].as_array().unwrap().len(), 2);
    assert_eq!(pod["containers"][1]["name"], "buildkit");
    assert!(pod["containers"][1]["restartPolicy"].is_null());
    assert_eq!(pod["containers"][0]["command"][0], "/bin/sh");
    assert_eq!(pod["containers"][1]["command"][0], "/bin/sh");
    assert_eq!(
        pod["containers"][1]["volumeMounts"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        value["spec"]["template"]["metadata"]["annotations"]["container.apparmor.security.beta.kubernetes.io/buildkit"],
        "unconfined"
    );
    let publisher =
        serde_json::to_value(resources::job(&build, &pool, None, Default::default()).unwrap())
            .unwrap();
    let pod = &publisher["spec"]["template"]["spec"];
    assert!(pod["initContainers"].is_null());
    assert_eq!(pod["containers"].as_array().unwrap().len(), 1);
    assert_eq!(
        pod["containers"][0]["command"][0],
        "artifactory-image-publisher"
    );
}

#[cfg(unix)]
#[test]
fn legacy_worker_marks_completion_and_keeps_runner_failure() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let dir = tempfile::tempdir().unwrap();
    let buildctl = dir.path().join("buildctl");
    fs::write(&buildctl, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&buildctl, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = dir.path().join("worker.done");
    let script =
        resources::LEGACY_WORKER.replace("/run/buildkit/worker.done", marker.to_str().unwrap());
    let result = Command::new("/bin/sh")
        .args(["-c", &script, "worker", "/bin/sh", "-c", "exit 37"])
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .status()
        .unwrap();
    assert_eq!(result.code(), Some(37));
    assert!(marker.exists());
}

#[cfg(unix)]
#[test]
fn legacy_buildkit_exits_after_completion_marker() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    let dir = tempfile::tempdir().unwrap();
    let daemon = dir.path().join("rootlesskit");
    let ready = dir.path().join("ready");
    let stopped = dir.path().join("stopped");
    fs::write(
        &daemon,
        format!(
            "#!/bin/sh\ntrap 'touch {}; exit 0' TERM\ntouch {}\nwhile :; do sleep 0.1; done\n",
            stopped.display(),
            ready.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&daemon, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = dir.path().join("worker.done");
    let script =
        resources::LEGACY_BUILDKIT.replace("/run/buildkit/worker.done", marker.to_str().unwrap());
    let mut child = Command::new("/bin/sh")
        .args(["-c", &script, "buildkit"])
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    fs::write(&marker, "").unwrap();
    let result = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(ready.exists());
    assert!(result.is_some_and(|status| status.success()));
    assert!(stopped.exists());
}
