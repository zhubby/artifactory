//! [INPUT]: 依赖父模块控制器与隔离 Kubernetes peer。
//! [OUTPUT]: 验证运行归属、状态更新与终态清理。
//! [POS]: Agent 控制器的行为回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
async fn status_transitions_clear_stale_fields_and_steady_reconcile_does_not_write() {
    use axum::{
        Json, Router,
        extract::State,
        routing::{get, patch},
    };
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    let mut run = AgentRun::new(
        "run-test",
        AgentRunSpec {
            run_id: "run".into(),
            generation: 1,
            pool: "pool".into(),
            source_manifest_id: Some("manifest".into()),
            source_digest: Some("a".repeat(64)),
        },
    );
    run.metadata.namespace = Some("test".into());
    run.metadata.resource_version = Some("1".into());
    run.status = Some(Status {
        phase: "Provisioning".into(),
        message: Some("ContainerCreating".into()),
        pod_uid: Some("pod-a".into()),
        observed_generation: 1,
    });
    let stored = Arc::new(Mutex::new(serde_json::to_value(&run).unwrap()));
    let writes = Arc::new(AtomicUsize::new(0));
    let state = (stored.clone(), writes.clone());
    let app = Router::new()
        .route(
            "/apis/coordination.k8s.io/v1/namespaces/test/leases/operator",
            get(|| async {
                Json(json!({"apiVersion":"coordination.k8s.io/v1","kind":"Lease",
                    "metadata":{"name":"operator"},"spec":{"holderIdentity":"leader",
                    "renewTime":chrono::Utc::now().to_rfc3339(),"leaseDurationSeconds":15}}))
            }),
        )
        .route(
            "/apis/agents.artifactory.dev/v1alpha1/namespaces/test/agentruns/run-test/status",
            patch(
                |State((stored, writes)): State<(Arc<Mutex<Value>>, Arc<AtomicUsize>)>,
                 Json(patch): Json<Value>| async move {
                    let mut value = stored.lock().unwrap();
                    // RFC 7396: absent properties are retained; explicit null deletes them.
                    for (key, field) in patch["status"].as_object().unwrap() {
                        if field.is_null() {
                            value["status"].as_object_mut().unwrap().remove(key);
                        } else {
                            value["status"][key] = field.clone();
                        }
                    }
                    writes.fetch_add(1, Ordering::SeqCst);
                    Json(value.clone())
                },
            ),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::try_from(kube::Config::new(
        format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap(),
    ))
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let ctx = Context {
        client: client.clone(),
        namespace: "test".into(),
        runtime: ServiceClient::new("https://runtime.invalid".into(), "x".repeat(64)).unwrap(),
        source_admission: tokio::sync::Mutex::new(()),
        leader: Leadership {
            api: Api::namespaced(client, "test"),
            identity: Arc::new("leader".into()),
            name: "operator".into(),
        },
    };
    let running = Status {
        phase: "Running".into(),
        message: None,
        pod_uid: Some("pod-a".into()),
        observed_generation: 1,
    };
    update(&ctx, &run, running.clone()).await.unwrap();
    let current: AgentRun = serde_json::from_value(stored.lock().unwrap().clone()).unwrap();
    assert_eq!(
        current.status.as_ref().unwrap().message,
        None,
        "a ready Pod must clear its previous waiting reason"
    );
    update(&ctx, &current, running).await.unwrap();
    assert_eq!(
        writes.load(Ordering::SeqCst),
        1,
        "steady reconcile must not patch status"
    );
    let terminal = Status {
        phase: "succeeded".into(),
        message: None,
        pod_uid: None,
        observed_generation: 1,
    };
    update(&ctx, &current, terminal.clone()).await.unwrap();
    let current: AgentRun = serde_json::from_value(stored.lock().unwrap().clone()).unwrap();
    assert_eq!(
        current.status.as_ref().unwrap().pod_uid,
        None,
        "terminal status must clear the old Pod identity"
    );
    update(&ctx, &current, terminal).await.unwrap();
    assert_eq!(
        writes.load(Ordering::SeqCst),
        2,
        "only the two state transitions may write"
    );
    server.abort();
}

#[test]
fn job_cleanup_cascades_pods_and_fences_replaced_resource_uids() {
    let mut run = AgentRun::new(
        "run-test",
        AgentRunSpec {
            run_id: "run".into(),
            generation: 1,
            pool: "pool".into(),
            source_manifest_id: Some("manifest".into()),
            source_digest: Some("a".repeat(64)),
        },
    );
    run.metadata.uid = Some("original-run".into());
    let mut job: Job = serde_json::from_value(json!({
        "metadata":{"uid":"original-job","ownerReferences":[{
            "apiVersion":"agents.artifactory.dev/v1alpha1", "kind":"AgentRun",
            "name":"run-test", "uid":"original-run", "controller":true
        }]}
    }))
    .unwrap();
    let deletion = serde_json::to_value(job_delete_params(&run, &job).unwrap()).unwrap();
    assert_eq!(deletion["propagationPolicy"], "Background");
    assert_eq!(deletion["preconditions"]["uid"], "original-job");
    run.metadata.uid = Some("replacement-run".into());
    assert!(job_delete_params(&run, &job).is_err());
    run.metadata.uid = Some("original-run".into());
    job.metadata.uid = None;
    assert!(job_delete_params(&run, &job).is_err());
}

#[tokio::test]
async fn cleanup_keeps_identity_until_pods_disappear_and_retries_api_failures() {
    use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
    use std::sync::Mutex;

    struct Stored {
        run: Value,
        job: Option<Value>,
        pods: Vec<Value>,
        fail_list: bool,
        deletions: Vec<Value>,
        writes: usize,
    }
    let mut run = AgentRun::new(
        "run-test",
        AgentRunSpec {
            run_id: "run".into(),
            generation: 1,
            pool: "pool".into(),
            source_manifest_id: Some("manifest".into()),
            source_digest: Some("a".repeat(64)),
        },
    );
    run.metadata.uid = Some("run-a".into());
    run.metadata.resource_version = Some("1".into());
    run.status = Some(Status {
        phase: "Running".into(),
        message: None,
        pod_uid: Some("pod-a".into()),
        observed_generation: 1,
    });
    let stored = Arc::new(Mutex::new(Stored {
        run: serde_json::to_value(&run).unwrap(),
        job: Some(json!({"apiVersion":"batch/v1","kind":"Job", "metadata":{
                "name":"pi-run-g1","uid":"job-a","ownerReferences":[{
                    "apiVersion":"agents.artifactory.dev/v1alpha1","kind":"AgentRun",
                    "name":"run-test","uid":"run-a","controller":true}]}})),
        pods: vec![json!({"metadata":{"name":"pi-run-g1-test","uid":"pod-a",
                "deletionTimestamp":"2026-11-12T00:00:00Z", "ownerReferences":[{
                    "apiVersion":"batch/v1","kind":"Job","name":"pi-run-g1",
                    "uid":"job-a","controller":true}]},"status":{"phase":"Running"}})],
        fail_list: false,
        deletions: Vec::new(),
        writes: 0,
    }));
    let app = Router::new()
        .route(
            "/apis/coordination.k8s.io/v1/namespaces/test/leases/operator",
            get(|| async {
                Json(json!({"apiVersion":"coordination.k8s.io/v1","kind":"Lease",
                    "metadata":{"name":"operator"},"spec":{"holderIdentity":"leader",
                    "renewTime":chrono::Utc::now().to_rfc3339(),"leaseDurationSeconds":15}}))
            }),
        )
        .route(
            "/apis/batch/v1/namespaces/test/jobs/pi-run-g1",
            get(|State(stored): State<Arc<Mutex<Stored>>>| async move {
                let value = stored.lock().unwrap().job.clone();
                match value {
                    Some(job) => (StatusCode::OK, Json(job)),
                    None => (
                        StatusCode::NOT_FOUND,
                        Json(json!({"kind":"Status",
                            "apiVersion":"v1","status":"Failure","reason":"NotFound",
                            "message":"job gone","code":404})),
                    ),
                }
            })
            .delete(
                |State(stored): State<Arc<Mutex<Stored>>>, Json(params): Json<Value>| async move {
                    let mut value = stored.lock().unwrap();
                    value.job = None;
                    value.deletions.push(params);
                    Json(json!({"apiVersion":"v1","kind":"Status","status":"Success"}))
                },
            ),
        )
        .route(
            "/api/v1/namespaces/test/pods",
            get(|State(stored): State<Arc<Mutex<Stored>>>| async move {
                let value = stored.lock().unwrap();
                if value.fail_list {
                    (
                        StatusCode::FORBIDDEN,
                        Json(json!({"kind":"Status",
                            "apiVersion":"v1","status":"Failure","code":403,
                            "reason":"Forbidden","message":"injected loss of API access"})),
                    )
                } else {
                    (
                        StatusCode::OK,
                        Json(json!({"apiVersion":"v1","kind":"PodList",
                            "metadata":{},"items":value.pods})),
                    )
                }
            }),
        )
        .route(
            "/apis/agents.artifactory.dev/v1alpha1/namespaces/test/agentruns/run-test/status",
            axum::routing::patch(
                |State(stored): State<Arc<Mutex<Stored>>>, Json(patch): Json<Value>| async move {
                    let mut value = stored.lock().unwrap();
                    for (key, field) in patch["status"].as_object().unwrap() {
                        if field.is_null() {
                            value.run["status"].as_object_mut().unwrap().remove(key);
                        } else {
                            value.run["status"][key] = field.clone();
                        }
                    }
                    value.writes += 1;
                    Json(value.run.clone())
                },
            ),
        )
        .with_state(stored.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::try_from(kube::Config::new(
        format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap(),
    ))
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let ctx = Context {
        client: client.clone(),
        namespace: "test".into(),
        runtime: ServiceClient::new("https://runtime.invalid".into(), "x".repeat(64)).unwrap(),
        source_admission: tokio::sync::Mutex::new(()),
        leader: Leadership {
            api: Api::namespaced(client, "test"),
            identity: Arc::new("leader".into()),
            name: "operator".into(),
        },
    };
    let action = cleanup_run(&ctx, &run, "canceled").await.unwrap();
    let current: AgentRun = serde_json::from_value(stored.lock().unwrap().run.clone()).unwrap();
    let status = current.status.as_ref().unwrap();
    assert_eq!(status.phase, "canceled");
    assert_eq!(
        status.pod_uid.as_deref(),
        Some("pod-a"),
        "Job deletion ACK must not discard an existing Pod identity"
    );
    assert!(
        status
            .message
            .as_deref()
            .unwrap()
            .starts_with("CleanupPending:")
    );
    assert_eq!(action, Action::requeue(Duration::from_secs(5)));
    assert_eq!(
        stored.lock().unwrap().deletions[0]["preconditions"]["uid"],
        "job-a"
    );
    cleanup_run(&ctx, &current, "canceled").await.unwrap();
    assert_eq!(
        stored.lock().unwrap().writes,
        1,
        "a deleted Job with a lingering Pod must remain pending without status churn"
    );
    stored.lock().unwrap().fail_list = true;
    assert!(
        cleanup_run(&ctx, &current, "canceled").await.is_err(),
        "an API failure must retry, never declare cleanup complete"
    );
    assert_eq!(stored.lock().unwrap().writes, 1);
    {
        let mut value = stored.lock().unwrap();
        value.fail_list = false;
        value.pods.clear();
    }
    assert_eq!(
        cleanup_run(&ctx, &current, "canceled").await.unwrap(),
        Action::requeue(Duration::from_secs(60))
    );
    let current: AgentRun = serde_json::from_value(stored.lock().unwrap().run.clone()).unwrap();
    assert!(current.status.as_ref().unwrap().pod_uid.is_none());
    assert!(current.status.as_ref().unwrap().message.is_none());
    cleanup_run(&ctx, &current, "canceled").await.unwrap();
    assert_eq!(stored.lock().unwrap().writes, 2);
    server.abort();
}

#[test]
fn history_retention_does_not_remove_pending_cleanup() {
    use k8s_openapi::{apimachinery::pkg::apis::meta::v1::Time, jiff::Timestamp};
    let now = chrono::Utc::now().timestamp();
    let mut run = AgentRun::new(
        "run-test",
        AgentRunSpec {
            run_id: "run".into(),
            generation: 1,
            pool: "pool".into(),
            source_manifest_id: Some("manifest".into()),
            source_digest: Some("a".repeat(64)),
        },
    );
    run.metadata.creation_timestamp = Some(Time(
        Timestamp::from_second(now - HISTORY_SECONDS - 1).unwrap(),
    ));
    run.status = Some(Status {
        phase: "canceled".into(),
        message: Some("CleanupPending: waiting for Pod deletion".into()),
        pod_uid: Some("pod-a".into()),
        observed_generation: 1,
    });
    assert!(
        !run_reclaimable(&run, now),
        "history TTL must retain unfinished cleanup"
    );
    run.status.as_mut().unwrap().pod_uid = None;
    assert!(
        !run_reclaimable(&run, now),
        "a pending Job still protects the run"
    );
    run.status.as_mut().unwrap().message = None;
    assert!(run_reclaimable(&run, now));
    run.status.as_mut().unwrap().phase = "Running".into();
    assert!(!run_reclaimable(&run, now));
}

#[test]
fn planning_job_keeps_source_identity_out_of_main_container() {
    let mut run = AgentRun::new(
        "run-test",
        AgentRunSpec {
            run_id: "run".into(),
            generation: 1,
            pool: "pool".into(),
            source_manifest_id: Some("manifest".into()),
            source_digest: Some("a".repeat(64)),
        },
    );
    run.metadata.uid = Some("run-uid".into());
    let pool:AgentPool=serde_json::from_value(json!({"metadata":{"name":"pool"},"spec":{"maxActive":2,"maxPerProject":1,"image":"worker:1","sourcePreparerImage":"preparer:1","runtimeUrl":"https://runtime","controlUrl":"https://control"}})).unwrap();
    let value = serde_json::to_value(job(&run, &pool).unwrap()).unwrap();
    let pod = &value["spec"]["template"]["spec"];
    assert_eq!(
        pod["initContainers"][0]["command"],
        json!(["artifactory-source-preparer", "checkout"])
    );
    assert!(
        pod["containers"][0]["volumeMounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["name"] != "source-auth" && m["name"] != "source-identity")
    );
    assert!(
        pod["containers"][0]["volumeMounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "source" && m["readOnly"] == true)
    );
    run.spec.source_manifest_id = None;
    let value = serde_json::to_value(job(&run, &pool).unwrap()).unwrap();
    assert!(
        value["spec"]["template"]["spec"]
            .get("initContainers")
            .is_none(),
        "placement never checks out code"
    );
}

#[test]
fn source_tasks_only_project_bound_tokens_and_temporary_volumes() {
    let mut task:SourceTask=serde_json::from_value(json!({"metadata":{"name":"source-task","namespace":"test","uid":"task-uid"},"spec":{"taskId":"00000000-0000-0000-0000-000000000001","tenantId":"tenant","projectId":"project","pool":"pool","purpose":"resolve"}})).unwrap();
    let pool:AgentPool=serde_json::from_value(json!({"metadata":{"name":"pool"},"spec":{"maxActive":2,"maxPerProject":1,"image":"worker:1","sourcePreparerImage":"preparer:1","runtimeUrl":"https://runtime","controlUrl":"https://control"}})).unwrap();
    let value = serde_json::to_value(crate::source_tasks::job(&task, &pool).unwrap()).unwrap();
    let pod = &value["spec"]["template"]["spec"];
    assert_eq!(value["spec"]["activeDeadlineSeconds"], 900);
    assert_eq!(pod["serviceAccountName"], "artifactory-source-preparer");
    assert!(
        pod["volumes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.get("persistentVolumeClaim").is_none())
    );
    assert_eq!(pod["containers"][0]["command"][1], "resolve");
    task.spec.purpose = SourcePurpose::Refs;
    task.spec.timeout_seconds = 30;
    let value = serde_json::to_value(crate::source_tasks::job(&task, &pool).unwrap()).unwrap();
    assert_eq!(value["spec"]["activeDeadlineSeconds"], 30);
    assert_eq!(
        value["spec"]["template"]["spec"]["containers"][0]["command"][1],
        "refs"
    );
}
