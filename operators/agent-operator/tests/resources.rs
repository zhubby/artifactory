//! [INPUT]: Agent CRD、规划 Job 与临时源码任务契约。
//! [OUTPUT]: 验证执行身份、凭据隔离与不可变契约。
//! [POS]: Agent Operator 的离线资源回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_agent_operator::{api::*, controller::job};
use kube::ResourceExt;
#[test]
fn jobs_have_single_attempt_and_bound_pod_identity_without_provider_secrets() {
    let mut run = AgentRun::new(
        "run-a",
        AgentRunSpec {
            run_id: "00000000-0000-0000-0000-000000000001".into(),
            generation: 2,
            pool: "default".into(),
            source_manifest_id: Some("manifest".into()),
            source_digest: Some("a".repeat(64)),
        },
    );
    run.metadata.uid = Some("cr-uid".into());
    run.metadata.namespace = Some("test".into());
    let pool = AgentPool::new(
        "default",
        AgentPoolSpec {
            max_active: 8,
            max_per_project: 2,
            image: "pi@sha256:fixed".into(),
            image_pull_policy: "IfNotPresent".into(),
            runtime_url: "https://runtime".into(),
            control_url: "https://control".into(),
            source_preparer_image: "source-preparer@sha256:fixed".into(),
            ephemeral_storage: "10Gi".into(),
            cpu: "250m".into(),
            memory: "512Mi".into(),
            tls_secret_name: String::new(),
        },
    );
    let first = job(&run, &pool).unwrap();
    let second = job(&run, &pool).unwrap();
    assert_eq!(first, second);
    assert!(first.name_any().ends_with("-g2"));
    let value = serde_json::to_value(first).unwrap();
    assert_eq!(value["spec"]["backoffLimit"], 0);
    assert_eq!(
        value["spec"]["template"]["spec"]["automountServiceAccountToken"],
        false
    );
    assert_eq!(value["metadata"]["ownerReferences"][0]["uid"], "cr-uid");
    assert!(!value.to_string().contains("api_key"));
    assert!(value.to_string().contains("artifactory-runtime"));
}
#[test]
fn immutable_crd_context_is_enforced_by_the_api_server_schema() {
    let definitions = crds();
    assert_eq!(definitions.len(), 3);
    for definition in definitions {
        let value = serde_json::to_value(definition).unwrap();
        let kind = value["spec"]["names"]["kind"].as_str().unwrap();
        if kind == "AgentRun" {
            assert_eq!(
                value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]["x-kubernetes-validations"]
                    [0]["rule"],
                "self == oldSelf"
            );
        } else if kind == "SourceTask" {
            let rules = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
                ["x-kubernetes-validations"];
            assert_eq!(rules.as_array().unwrap().len(), 2);
            assert_eq!(
                rules[1]["rule"],
                "!oldSelf.cancelRequested || self.cancelRequested"
            );
        }
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
