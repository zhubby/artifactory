//! [INPUT]: 依赖源码凭据鉴权与实际 Kubernetes 对象契约。
//! [OUTPUT]: 验证绑定令牌、直接 CR 归属、重试身份及错误状态。
//! [POS]: 源码 prepare、pin、commit 共用授权的行为回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

async fn failure(error: anyhow::Error) -> Response {
    let error = SourceError::authorization(error);
    let router = Router::new().route(
        "/internal/source/prepare",
        post(move || async move { Err::<Json<Value>, _>(error) }),
    );
    router
        .oneshot(
            Request::post("/internal/source/prepare")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn bootstrap_http_preserves_transient_cluster_failures() {
    for code in [404, 409, 429, 500, 502, 503, 504] {
        let error = kube::Error::Api(Box::new(kube::core::Status {
            message: "private cluster diagnostic".into(),
            reason: "temporary".into(),
            code,
            ..Default::default()
        }));
        let response = failure(anyhow::Error::new(error).context("reading current Pod")).await;
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "Kubernetes {code} must allow the init container to retry"
        );
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private cluster diagnostic"));
    }
}

#[tokio::test]
async fn bootstrap_http_rejects_identity_failures_without_retry() {
    for code in [401, 403] {
        let error = kube::Error::Api(Box::new(kube::core::Status {
            message: "private token diagnostic".into(),
            reason: "denied".into(),
            code,
            ..Default::default()
        }));
        assert_eq!(failure(error.into()).await.status(), StatusCode::FORBIDDEN);
    }
    for reason in [
        "Pod token identity mismatch",
        "source execution stopped",
        "source scope changed",
    ] {
        assert_eq!(
            failure(anyhow::anyhow!(reason)).await.status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn bootstrap_waits_for_durable_pod_registration() {
    assert_eq!(
        failure(IdentityPending.into()).await.status(),
        StatusCode::CONFLICT
    );
}

fn pod(name: &str, owner: &Value, identity: Value, account: &str) -> Pod {
    serde_json::from_value(json!({
        "apiVersion":"v1","kind":"Pod",
        "metadata":{"name":name,"namespace":"test","uid":"pod-current","ownerReferences":[owner]},
        "spec":{"serviceAccountName":account,"containers":[{"name":"source","image":"preparer", "env":[{
            "name":"ARTIFACTORY_SOURCE_IDENTITY","value":identity.to_string()
        }]}]},"status":{"phase":"Running"}
    })).unwrap()
}

fn source_fixture() -> (Pod, DynamicObject, Value) {
    let id = Uuid::from_u128(1);
    let cr_name = format!("source-{}", id.simple());
    let name = format!(
        "{cr_name}-a1-{}",
        &crate::crypto::hash(b"source-current")[..8]
    );
    let owner = json!({"apiVersion":"agents.artifactory.dev/v1alpha1","kind":"SourceTask","name":cr_name,"uid":"source-current","controller":true});
    let mut pod = pod(
        &name,
        &owner,
        json!({"kind":"source-task","task_id":id.to_string()}),
        "artifactory-source-preparer",
    );
    pod.metadata.labels = Some(std::collections::BTreeMap::from([(
        "artifactory.dev/attempt".into(),
        "1".into(),
    )]));
    let task = json!({"id":id,"tenant_id":Uuid::from_u128(2),"project_id":Uuid::from_u128(3),"purpose":"resolve","source_task_uid":"source-current"});
    let resource=serde_json::from_value(json!({
        "apiVersion":"agents.artifactory.dev/v1alpha1","kind":"SourceTask",
        "metadata":{"name":cr_name,"namespace":"test","uid":"source-current","generation":1},
        "spec":{"taskId":id,"tenantId":task["tenant_id"],"projectId":task["project_id"],"purpose":"resolve","pool":std::env::var("ARTIFACTORY_AGENT_POOL").unwrap_or_else(|_|"default".into()),"timeoutSeconds":900,"cancelRequested":false},
        "status":{"phase":"Running","observedGeneration":1,"execution":{"name":name,"uid":"pod-current","attempt":1,"deadlineMs":chrono::Utc::now().timestamp_millis()+60000}}
    })).unwrap();
    (pod, resource, task)
}

#[test]
fn bound_token_and_current_pod_are_both_required() {
    let (pod, _, _) = source_fixture();
    let name = pod.name_any();
    let reviewed = json!({"status":{"authenticated":true,"audiences":["artifactory-source-prepare"],"user":{
        "username":"system:serviceaccount:test:artifactory-source-preparer","extra":{
            "authentication.kubernetes.io/pod-uid":["pod-current"],"authentication.kubernetes.io/pod-name":[name]
        }
    }}});
    validate_review(
        &reviewed,
        "test",
        "artifactory-source-preparer",
        &name,
        "pod-current",
    )
    .unwrap();
    validate_pod(
        &pod,
        "test",
        "artifactory-source-preparer",
        &name,
        "pod-current",
    )
    .unwrap();
    for (path, value) in [
        ("/status/authenticated", json!(false)),
        ("/status/audiences", json!(["different-audience"])),
        (
            "/status/user/username",
            json!("system:serviceaccount:foreign:artifactory-source-preparer"),
        ),
        (
            "/status/user/extra/authentication.kubernetes.io~1pod-uid",
            json!(["pod-replaced"]),
        ),
        (
            "/status/user/extra/authentication.kubernetes.io~1pod-name",
            json!(["different-pod"]),
        ),
    ] {
        let mut changed = reviewed.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert!(
            validate_review(
                &changed,
                "test",
                "artifactory-source-preparer",
                &name,
                "pod-current"
            )
            .is_err(),
            "accepted {path}"
        );
    }
    for (path, value) in [
        ("/metadata/uid", json!("pod-replaced")),
        ("/metadata/namespace", json!("foreign")),
        ("/metadata/name", json!("different-pod")),
        ("/spec/serviceAccountName", json!("other")),
        ("/status/phase", json!("Failed")),
    ] {
        let mut changed = serde_json::to_value(&pod).unwrap();
        *changed.pointer_mut(path).unwrap() = value;
        let changed = serde_json::from_value(changed).unwrap();
        assert!(
            validate_pod(
                &changed,
                "test",
                "artifactory-source-preparer",
                &name,
                "pod-current"
            )
            .is_err(),
            "accepted {path}"
        );
    }
}

#[test]
fn source_authorization_requires_direct_owner_pinned_cr_and_current_attempt_uid() {
    let (pod, resource, task) = source_fixture();
    let owner = direct_owner(
        &pod,
        "agents.artifactory.dev/v1alpha1",
        "SourceTask",
        &resource.name_any(),
    )
    .unwrap();
    validate_resource(&resource, owner, "test", task["source_task_uid"].as_str()).unwrap();
    validate_source_contract(&pod, &resource, &task).unwrap();
    for (path, value) in [
        ("/metadata/ownerReferences/0/apiVersion", json!("batch/v1")),
        ("/metadata/ownerReferences/0/kind", json!("Job")),
        ("/metadata/ownerReferences/0/name", json!("other-task")),
        ("/metadata/ownerReferences/0/controller", json!(false)),
    ] {
        let mut changed = serde_json::to_value(&pod).unwrap();
        *changed.pointer_mut(path).unwrap() = value;
        let changed = serde_json::from_value(changed).unwrap();
        assert!(
            direct_owner(
                &changed,
                "agents.artifactory.dev/v1alpha1",
                "SourceTask",
                &resource.name_any()
            )
            .is_err(),
            "accepted {path}"
        );
    }
    assert!(validate_resource(&resource, owner, "test", Some("replaced-cr")).is_err());
    let mut replacement = resource.clone();
    replacement.metadata.uid = Some("replaced-cr".into());
    let mut replacement_owner = owner.clone();
    replacement_owner.uid = "replaced-cr".into();
    assert!(
        validate_resource(
            &replacement,
            &replacement_owner,
            "test",
            Some("source-current")
        )
        .is_err(),
        "same-name replacement must not acquire an existing task"
    );
    for (path, value) in [
        ("/spec/tenantId", json!(Uuid::from_u128(4))),
        ("/spec/projectId", json!(Uuid::from_u128(4))),
        ("/spec/taskId", json!(Uuid::from_u128(4))),
        ("/spec/purpose", json!("refs")),
        ("/spec/pool", json!("foreign-pool")),
        ("/spec/timeoutSeconds", json!(901)),
        ("/status/execution/uid", json!("old-pod")),
        ("/status/execution/name", json!("old-attempt")),
        ("/status/execution/attempt", json!(2)),
        ("/status/execution/deadlineMs", json!(1)),
        ("/status/observedGeneration", json!(2)),
    ] {
        let mut changed = serde_json::to_value(&resource).unwrap();
        *changed.pointer_mut(path).unwrap() = value;
        let changed = serde_json::from_value(changed).unwrap();
        assert!(
            validate_source_contract(&pod, &changed, &task).is_err(),
            "accepted {path}"
        );
    }
    let mut unregistered = resource.clone();
    unregistered.data["status"]["execution"]["uid"] = Value::Null;
    assert!(
        validate_source_contract(&pod, &unregistered, &task)
            .unwrap_err()
            .is::<IdentityPending>()
    );
    let mut changed = pod.clone();
    changed.metadata.uid = Some("same-name-replacement".into());
    assert!(validate_source_contract(&changed, &resource, &task).is_err());
    let mut stopped = resource.clone();
    stopped.data["status"]["execution"]["stopped"] = json!(true);
    assert!(validate_source_contract(&pod, &stopped, &task).is_err());
}

#[test]
fn image_source_authorization_checks_full_spec_and_recorded_platform_pod() {
    let id = Uuid::from_u128(10);
    let name = format!("image-{}", id.simple());
    let pod_name = format!("{name}-amd64");
    let owner = json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","name":name,"uid":"image-current","controller":true});
    let mut pod = pod(
        &pod_name,
        &owner,
        json!({"kind":"image-build","execution_id":id,"platform":"linux/amd64"}),
        "artifactory-image-worker",
    );
    pod.metadata.labels = Some(std::collections::BTreeMap::from([(
        "artifactory.dev/platform".into(),
        "amd64".into(),
    )]));
    let expected = json!({"executionId":id,"tenantId":Uuid::from_u128(2),"projectId":Uuid::from_u128(3),"pool":"default","sourceManifestId":Uuid::from_u128(4),"sourceDigest":"a".repeat(64),"taskSecret":format!("{name}-task"),"platforms":["linux/amd64"],"allowEmulation":false,"publishOnly":false,"timeoutSeconds":300,"cancelRequested":false});
    let resource:DynamicObject=serde_json::from_value(json!({
        "apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild",
        "metadata":{"name":name,"namespace":"test","uid":"image-current","generation":1},"spec":expected,
        "status":{"observedGeneration":1,"podOwnership":{"imageBuildUid":"image-current","pods":[{"name":pod_name,"uid":"pod-current","platform":"linux/amd64"}]}}
    })).unwrap();
    let owner = direct_owner(&pod, "images.artifactory.dev/v1alpha1", "ImageBuild", &name).unwrap();
    validate_resource(&resource, owner, "test", Some("image-current")).unwrap();
    validate_image_contract(&pod, &resource, &expected, "linux/amd64").unwrap();
    assert!(validate_resource(&resource, owner, "foreign", Some("image-current")).is_err());
    assert!(validate_image_contract(&pod, &resource, &expected, "linux/arm64").is_err());
    for key in expected.as_object().unwrap().keys() {
        let mut changed = resource.clone();
        changed.data["spec"][key] = json!("changed");
        assert!(
            validate_image_contract(&pod, &changed, &expected, "linux/amd64").is_err(),
            "accepted spec {key}"
        );
    }
    for (path, value) in [
        ("/status/podOwnership/imageBuildUid", json!("replaced-cr")),
        ("/status/podOwnership/pods/0/name", json!("different-pod")),
        ("/status/podOwnership/pods/0/uid", json!("old-pod")),
        ("/status/observedGeneration", json!(2)),
    ] {
        let mut changed = serde_json::to_value(&resource).unwrap();
        *changed.pointer_mut(path).unwrap() = value;
        assert!(
            validate_image_contract(
                &pod,
                &serde_json::from_value(changed).unwrap(),
                &expected,
                "linux/amd64"
            )
            .is_err(),
            "accepted {path}"
        );
    }
    let mut unregistered = resource.clone();
    unregistered.data["status"]["podOwnership"] = Value::Null;
    assert!(
        validate_image_contract(&pod, &unregistered, &expected, "linux/amd64")
            .unwrap_err()
            .is::<IdentityPending>()
    );
    let mut stopped = resource.clone();
    stopped.data["status"]["podOwnership"]["pods"][0]["stopped"] = json!(true);
    assert!(validate_image_contract(&pod, &stopped, &expected, "linux/amd64").is_err());
}
