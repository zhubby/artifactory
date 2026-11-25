//! [INPUT]: Planning CRD、直接规划 Pod 与源码任务契约。
//! [OUTPUT]: 验证执行身份、池代理、凭据隔离与空值安全不可变契约。
//! [POS]: Planning Operator 的离线资源回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_planning_operator::{api::*, pods, resources};
use kube::ResourceExt;
#[test]
fn pods_have_single_attempt_and_bound_identity_without_provider_secrets() {
    let mut run = PlanningRun::new(
        "run-a",
        PlanningRunSpec {
            source_required: true,
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            project_id: "00000000-0000-0000-0000-000000000002".into(),
            generation: 2,
            pool: "default".into(),
            source_manifest_id: None,
            source_digest: None,
        },
    );
    run.metadata.uid = Some("cr-uid".into());
    run.metadata.namespace = Some("test".into());
    let pool = PlanningPool::new(
        "default",
        PlanningPoolSpec {
            max_active: 8,
            max_per_project: 2,
            image: "pi@sha256:fixed".into(),
            image_pull_policy: "IfNotPresent".into(),
            coordinator_url: "https://coordinator".into(),
            control_url: "https://control".into(),
            source_preparer_image: "source-preparer@sha256:fixed".into(),
            ephemeral_storage: "10Gi".into(),
            cpu: "250m".into(),
            memory: "512Mi".into(),
            tls_secret_name: "services-tls".into(),
            proxy: None,
        },
    );
    let execution = pods::reserve(
        &run.spec.run_id,
        "cr-uid",
        "planning-worker",
        1,
        chrono::Utc::now().timestamp_millis() + 600_000,
    )
    .unwrap();
    let first = resources::planning_pod(&run, &pool, &execution).unwrap();
    let second = resources::planning_pod(&run, &pool, &execution).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.name_any(), execution.name);
    let value = serde_json::to_value(first).unwrap();
    assert_eq!(value["kind"], "Pod");
    assert_eq!(value["spec"]["restartPolicy"], "Never");
    assert_eq!(value["metadata"]["finalizers"][0], pods::POD_FINALIZER);
    assert_eq!(value["spec"]["automountServiceAccountToken"], false);
    assert_eq!(value["metadata"]["ownerReferences"][0]["uid"], "cr-uid");
    assert_eq!(
        value["metadata"]["labels"]["artifactory.dev/project"],
        run.spec.project_id
    );
    let mounts = value["spec"]["containers"][0]["volumeMounts"]
        .as_array()
        .unwrap();
    assert!(
        mounts
            .iter()
            .all(|mount| mount["name"] != "source-auth" && mount["name"] != "source-identity")
    );
    assert!(
        mounts
            .iter()
            .any(|mount| mount["name"] == "source" && mount["readOnly"] == true)
    );
    let ca = value["spec"]["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|volume| volume["name"] == "ca")
        .unwrap();
    assert_eq!(
        ca["secret"]["items"],
        serde_json::json!([{"key":"ca.crt","path":"ca.crt"}])
    );
    assert!(
        value["spec"]["initContainers"][0]["volumeMounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mount| mount["name"] == "source-auth")
    );
    assert_eq!(
        value["spec"]["initContainers"][0]["command"],
        serde_json::json!(["artifactory-source-preparer", "prepare"])
    );
    assert!(!value.to_string().contains("api_key"));
    assert!(value.to_string().contains("artifactory-planning"));
}
#[test]
fn immutable_crd_context_is_enforced_by_the_api_server_schema() {
    let definitions = crds();
    assert_eq!(definitions.len(), 3);
    for definition in definitions {
        let value = serde_json::to_value(definition).unwrap();
        let kind = value["spec"]["names"]["kind"].as_str().unwrap();
        if kind == "PlanningRun" {
            let spec =
                &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"];
            assert!(
                spec["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("projectId"))
            );
            let rule = spec["x-kubernetes-validations"][0]["rule"]
                .as_str()
                .unwrap();
            for field in ["runId", "projectId", "generation", "pool", "sourceRequired"] {
                assert!(rule.contains(&format!("self.{field} == oldSelf.{field}")));
            }
            for field in ["sourceManifestId", "sourceDigest"] {
                assert!(rule.contains(&format!("has(self.{field}) == has(oldSelf.{field})")));
                assert!(rule.contains(&format!(
                    "(!has(self.{field}) || self.{field} == oldSelf.{field})"
                )));
            }
            assert!(!rule.contains("self == oldSelf"));
        } else if kind == "SourceTask" {
            let rules = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
                ["x-kubernetes-validations"];
            assert_eq!(rules.as_array().unwrap().len(), 2);
            assert_eq!(
                rules[1]["rule"],
                "!oldSelf.cancelRequested || self.cancelRequested"
            );
        }
        let execution = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["status"]
            ["properties"]["execution"];
        assert!(
            execution["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("createStarted"))
        );
    }
}

#[test]
fn source_task_contract_accepts_only_current_fields() {
    let mut value = serde_json::json!({
        "taskId": "00000000-0000-0000-0000-000000000001",
        "tenantId": "00000000-0000-0000-0000-000000000002",
        "projectId": "00000000-0000-0000-0000-000000000003",
        "purpose": "resolve", "pool": "default"
    });
    let spec: SourceTaskSpec = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(spec.timeout_seconds, 900);
    assert!(!spec.cancel_requested);
    value["snapshotId"] = serde_json::json!("obsolete");
    assert!(serde_json::from_value::<SourceTaskSpec>(value).is_err());
}

#[test]
fn configured_pool_proxies_source_and_planning_without_arbitrary_environment() {
    let mut pool: PlanningPool = serde_json::from_value(serde_json::json!({
        "metadata":{"name":"default"},"spec":{
            "maxActive":4,"maxPerProject":2,"image":"worker:1",
            "coordinatorUrl":"https://coordinator/internal/planning","controlUrl":"https://control:8445",
            "sourcePreparerImage":"source:1","proxy":{
                "httpProxy":"http://egress:18443","httpsProxy":"http://egress:18443",
                "noProxy":"localhost,127.0.0.1,.svc,control,coordinator,registry"
            }
        }
    }))
    .unwrap();
    let run: PlanningRun = serde_json::from_value(serde_json::json!({
        "metadata":{"name":"run","namespace":"test","uid":"run-uid"},"spec":{
            "runId":"00000000-0000-0000-0000-000000000001",
            "projectId":"project","generation":1,"pool":"default",
            "sourceManifestId":"manifest","sourceDigest":"a".repeat(64)
        }
    }))
    .unwrap();
    let task: SourceTask = serde_json::from_value(serde_json::json!({
        "metadata":{"name":"source","namespace":"test","uid":"task-uid"},"spec":{
            "taskId":"00000000-0000-0000-0000-000000000002",
            "tenantId":"tenant","projectId":"project","purpose":"resolve","pool":"default"
        }
    }))
    .unwrap();
    let execution = pods::reserve(
        &run.spec.run_id,
        "run-uid",
        "planning-worker",
        1,
        chrono::Utc::now().timestamp_millis() + 600_000,
    )
    .unwrap();
    for pod in [
        resources::planning_pod(&run, &pool, &execution).unwrap(),
        resources::source_pod(&task, &pool, &execution).unwrap(),
    ] {
        let spec = pod.spec.unwrap();
        for container in spec
            .containers
            .iter()
            .chain(spec.init_containers.iter().flatten())
        {
            let environment = container.env.as_ref().unwrap();
            if container.name == "planning-worker" {
                assert!(!environment.iter().any(|entry| entry.name == "HTTP_PROXY"));
                continue;
            }
            for name in ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy"] {
                assert_eq!(
                    environment
                        .iter()
                        .find(|entry| entry.name == name)
                        .unwrap()
                        .value
                        .as_deref(),
                    Some("http://egress:18443"),
                    "container {} requires {name}",
                    container.name
                );
            }
            assert_eq!(
                environment
                    .iter()
                    .find(|entry| entry.name == "NO_PROXY")
                    .unwrap()
                    .value,
                environment
                    .iter()
                    .find(|entry| entry.name == "no_proxy")
                    .unwrap()
                    .value
            );
        }
    }
    pool.spec.proxy.as_mut().unwrap().no_proxy = ".svc,control".into();
    assert!(
        resources::planning_pod(&run, &pool, &execution).is_err(),
        "coordinator must bypass the proxy"
    );
    pool.spec.proxy = None;
    let pod = resources::planning_pod(&run, &pool, &execution).unwrap();
    assert!(!serde_json::to_string(&pod).unwrap().contains("HTTP_PROXY"));
    assert!(
        serde_json::from_value::<artifactory_service_transport::proxy::ProxyConfig>(
            serde_json::json!({
                "httpProxy":"http://egress:18443","noProxy":".svc","env":{"UNTRUSTED":"value"}
            })
        )
        .is_err()
    );
}
