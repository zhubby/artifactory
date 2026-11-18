//! [INPUT]: 依赖真实 kube 客户端与隔离 HTTP Kubernetes peer。
//! [OUTPUT]: 验证容器失败、UID 台账、直接删除和断联容量。
//! [POS]: Image Operator 跨 API 生命周期回归，不依赖运行中集群。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(feature = "operator")]
use artifactory_image_operator::{
    api::{BuilderPool, ImageBuild},
    controller::{Context, reconcile},
    leadership::Leadership,
    resources,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{Method, StatusCode, Uri},
};
use kube::{Api, Client};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

struct Stored {
    pool: Value,
    builds: Vec<Value>,
    jobs: Vec<Value>,
    pods: Vec<Value>,
    deletions: Vec<(Value, Value)>,
    fail_status: bool,
    fail_pods: bool,
    fail_metadata: bool,
    creates: usize,
}
struct Peer {
    stored: Arc<Mutex<Stored>>,
    client: Client,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.server.abort();
    }
}
fn build(name: &str, id: &str) -> Value {
    json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild",
        "metadata":{"name":name,"namespace":"test","uid":format!("{name}-uid"),"resourceVersion":"1","generation":1,"finalizers":["images.artifactory.dev/cleanup"]},
        "spec":{"executionId":id,"tenantId":"tenant","projectId":"project","pool":"pool",
            "sourceManifestId":"manifest","sourceDigest":"a".repeat(64),"taskSecret":"task","platforms":["linux/amd64"],
            "allowEmulation":false,"publishOnly":false,"timeoutSeconds":600,"cancelRequested":false}})
}
fn pool() -> BuilderPool {
    serde_json::from_value(json!({"metadata":{"name":"pool"},"spec":{"workerImage":"worker:1","buildkitImage":"buildkit:1",
        "sourcePreparerImage":"source:1","controlUrl":"http://control","maxActive":2,"maxPerProject":2}})).unwrap()
}
fn job(build: &Value, platform: Option<&str>, terminal: Option<&str>) -> Value {
    let b: ImageBuild = serde_json::from_value(build.clone()).unwrap();
    let mut j =
        serde_json::to_value(resources::job(&b, &pool(), platform, BTreeMap::new()).unwrap())
            .unwrap();
    j["metadata"]["uid"] = json!(format!("{}-uid", j["metadata"]["name"].as_str().unwrap()));
    j["metadata"]["resourceVersion"] = json!("1");
    if let Some(condition) = terminal {
        j["status"] = json!({"conditions":[{"type":condition,"status":"True"}]});
    }
    j
}
fn pod(job: &Value, states: &[(&str, Value)], init: Option<Value>) -> Value {
    let mut p = json!({"metadata":{"name":format!("{}-pod", job["metadata"]["name"].as_str().unwrap()),"uid":"pod-uid",
        "ownerReferences":[{"apiVersion":"batch/v1","kind":"Job","controller":true,"name":job["metadata"]["name"],"uid":job["metadata"]["uid"]}]},
        "spec":job["spec"]["template"]["spec"],
        "status":{"phase":"Running","containerStatuses":states.iter().map(|(name,state)|json!({"name":name,"state":state,"image":"image","imageID":"id","ready":false,"restartCount":0})).collect::<Vec<_>>()}});
    if let Some(state) = init {
        p["status"]["initContainerStatuses"] = json!([{"name":"source","state":state,"image":"image","imageID":"id","ready":false,"restartCount":0}]);
    }
    p
}
fn running() -> Value {
    json!({"running":{}})
}
fn ended(code: i32) -> Value {
    json!({"terminated":{"exitCode":code,"reason":if code == 137 { "OOMKilled" } else { "Completed" }}})
}
fn response(code: StatusCode, value: Value) -> (StatusCode, Json<Value>) {
    (code, Json(value))
}
fn error(code: StatusCode) -> (StatusCode, Json<Value>) {
    response(
        code,
        json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":if code == StatusCode::NOT_FOUND { "NotFound" } else { "Injected" },"message":"injected API failure","code":code.as_u16()}),
    )
}
async fn handle(
    State(stored): State<Arc<Mutex<Stored>>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let path = uri.path();
    let data: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut s = stored.lock().unwrap();
    if path.contains("/leases/") {
        return response(
            StatusCode::OK,
            json!({"apiVersion":"coordination.k8s.io/v1","kind":"Lease","metadata":{"name":"operator"},
            "spec":{"holderIdentity":"leader","renewTime":k8s_openapi::jiff::Timestamp::now().to_string(),"leaseDurationSeconds":15}}),
        );
    }
    if path.ends_with("/status") && method == Method::PATCH {
        if s.fail_status {
            return error(StatusCode::INTERNAL_SERVER_ERROR);
        }
        let name = path.split('/').rev().nth(1).unwrap();
        let b = s
            .builds
            .iter_mut()
            .find(|b| b["metadata"]["name"] == name)
            .unwrap();
        b["status"] = data["status"].clone();
        let version = b["metadata"]["resourceVersion"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            + 1;
        b["metadata"]["resourceVersion"] = json!(version.to_string());
        return response(StatusCode::OK, b.clone());
    }
    if path.contains("/imagebuilds/") && method == Method::PATCH {
        if s.fail_metadata {
            return error(StatusCode::INTERNAL_SERVER_ERROR);
        }
        let name = path.rsplit('/').next().unwrap();
        let index = s
            .builds
            .iter()
            .position(|b| b["metadata"]["name"] == name)
            .unwrap();
        let b = &mut s.builds[index];
        if data["metadata"]["resourceVersion"] != b["metadata"]["resourceVersion"]
            || data["metadata"]["uid"] != b["metadata"]["uid"]
        {
            return error(StatusCode::CONFLICT);
        }
        b["metadata"]["finalizers"] = data["metadata"]["finalizers"].clone();
        let version = b["metadata"]["resourceVersion"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            + 1;
        b["metadata"]["resourceVersion"] = json!(version.to_string());
        let result = b.clone();
        if b["metadata"]["deletionTimestamp"].is_string()
            && b["metadata"]["finalizers"].as_array().unwrap().is_empty()
        {
            s.builds.remove(index);
        }
        return response(StatusCode::OK, result);
    }
    if path.ends_with("/imagebuilds") {
        return response(
            StatusCode::OK,
            json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuildList","metadata":{},"items":s.builds}),
        );
    }
    if path.contains("/builderpools/") {
        return response(StatusCode::OK, s.pool.clone());
    }
    if path.ends_with("/nodes") {
        return response(
            StatusCode::OK,
            json!({"apiVersion":"v1","kind":"NodeList","metadata":{},"items":[
        {"metadata":{"name":"amd64","labels":{"kubernetes.io/os":"linux","kubernetes.io/arch":"amd64"}},"status":{"conditions":[{"type":"Ready","status":"True"}]}},
        {"metadata":{"name":"arm64","labels":{"kubernetes.io/os":"linux","kubernetes.io/arch":"arm64"}},"status":{"conditions":[{"type":"Ready","status":"True"}]}}]}),
        );
    }
    if path.ends_with("/pods") {
        if s.fail_pods {
            return error(StatusCode::FORBIDDEN);
        }
        return response(
            StatusCode::OK,
            json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":s.pods}),
        );
    }
    if path.ends_with("/jobs") {
        if method == Method::POST {
            let mut job = data;
            job["metadata"]["uid"] = json!(format!("created-{}", s.creates));
            job["metadata"]["resourceVersion"] = json!("1");
            s.creates += 1;
            s.jobs.push(job.clone());
            return response(StatusCode::CREATED, job);
        }
        return response(
            StatusCode::OK,
            json!({"apiVersion":"batch/v1","kind":"JobList","metadata":{},"items":s.jobs}),
        );
    }
    if path.contains("/jobs/") {
        let name = path.rsplit('/').next().unwrap();
        let Some(index) = s.jobs.iter().position(|j| j["metadata"]["name"] == name) else {
            return error(StatusCode::NOT_FOUND);
        };
        if method == Method::DELETE {
            let current = s.builds[0].clone();
            s.deletions.push((data, current));
            s.jobs.remove(index);
            return response(
                StatusCode::OK,
                json!({"apiVersion":"v1","kind":"Status","status":"Success"}),
            );
        }
        return response(StatusCode::OK, s.jobs[index].clone());
    }
    error(StatusCode::NOT_FOUND)
}
impl Peer {
    async fn new(build: Value, jobs: Vec<Value>, pods: Vec<Value>) -> Self {
        let stored = Arc::new(Mutex::new(Stored {
            pool: serde_json::to_value(pool()).unwrap(),
            builds: vec![build],
            jobs,
            pods,
            deletions: vec![],
            fail_status: false,
            fail_pods: false,
            fail_metadata: false,
            creates: 0,
        }));
        let app = Router::new().fallback(handle).with_state(stored.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::try_from(kube::Config::new(
            format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        ))
        .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            stored,
            client,
            server,
        }
    }
    async fn reconcile(&self) -> anyhow::Result<()> {
        self.reconcile_index(0).await
    }
    async fn reconcile_index(&self, index: usize) -> anyhow::Result<()> {
        let build: ImageBuild =
            serde_json::from_value(self.stored.lock().unwrap().builds[index].clone())?;
        let ctx = Arc::new(Context {
            client: self.client.clone(),
            namespace: "test".into(),
            admission: tokio::sync::Mutex::new(()),
            leader: Leadership {
                api: Api::namespaced(self.client.clone(), "test"),
                identity: Arc::new("leader".into()),
                name: "operator".into(),
            },
        });
        reconcile(Arc::new(build), ctx).await?;
        Ok(())
    }
    fn current(&self) -> Value {
        self.stored.lock().unwrap().builds[0].clone()
    }
}

#[tokio::test]
async fn any_failed_owned_container_fails_while_job_remains_nonterminal() {
    for (states, init) in [
        (
            vec![("worker", ended(137)), ("buildkit", running())],
            Some(ended(0)),
        ),
        (
            vec![("worker", running()), ("buildkit", ended(137))],
            Some(ended(0)),
        ),
        (vec![], Some(ended(1))),
    ] {
        let b = build("build", "00000000-0000-0000-0000-000000000001");
        let j = job(&b, Some("linux/amd64"), None);
        let p = pod(&j, &states, init);
        let peer = Peer::new(b, vec![j], vec![p]).await;
        peer.reconcile().await.unwrap();
        assert_eq!(peer.current()["status"]["phase"], "Failed");
        assert!(
            peer.stored.lock().unwrap().deletions.is_empty(),
            "terminal status precedes deletion"
        );
    }
}

#[tokio::test]
async fn completed_init_and_normal_buildkit_stop_do_not_fail_a_live_runner() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let j = job(&b, Some("linux/amd64"), None);
    let p = pod(
        &j,
        &[("worker", running()), ("buildkit", ended(0))],
        Some(ended(0)),
    );
    let peer = Peer::new(b, vec![j], vec![p]).await;
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Building");
    assert_eq!(
        peer.current()["status"]["jobOwnership"]["jobs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn cleanup_saves_uid_before_delete_and_survives_job_disappearance_and_restart() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["cancelRequested"] = json!(true);
    let j = job(&b, Some("linux/amd64"), None);
    let p = pod(
        &j,
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    let peer = Peer::new(b, vec![j.clone()], vec![p]).await;
    peer.stored.lock().unwrap().fail_status = true;
    assert!(peer.reconcile().await.is_err());
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
    peer.stored.lock().unwrap().fail_status = false;
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    let current = peer.current();
    assert_eq!(current["status"]["phase"], "Canceled");
    assert!(
        current["status"]["message"]
            .as_str()
            .unwrap()
            .starts_with("CleanupPending:")
    );
    {
        let s = peer.stored.lock().unwrap();
        assert_eq!(s.deletions.len(), 1);
        assert_eq!(
            s.deletions[0].0["preconditions"]["uid"],
            j["metadata"]["uid"]
        );
        assert_eq!(
            s.deletions[0].1["status"]["jobOwnership"]["jobs"][0]["uid"],
            j["metadata"]["uid"]
        );
    }
    peer.stored.lock().unwrap().fail_pods = true;
    assert!(peer.reconcile().await.is_err());
    peer.stored.lock().unwrap().fail_pods = false;
    let mut foreign = peer.stored.lock().unwrap().pods[0].clone();
    foreign["metadata"]["ownerReferences"][0]["uid"] = json!("foreign-job-uid");
    peer.stored.lock().unwrap().pods = vec![foreign];
    peer.reconcile().await.unwrap();
    assert!(peer.current()["status"]["message"].is_null());
    assert_eq!(
        peer.current()["status"]["jobOwnership"],
        current["status"]["jobOwnership"]
    );
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
}

#[tokio::test]
async fn a_ledger_bound_to_another_imagebuild_uid_cannot_delete_jobs() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["cancelRequested"] = json!(true);
    b["status"] = json!({"phase":"Canceled","observedGeneration":1,"jobOwnership":{"imageBuildUid":"foreign","jobs":[]}});
    let j = job(&b, Some("linux/amd64"), None);
    let peer = Peer::new(b, vec![j], vec![]).await;
    assert!(peer.reconcile().await.is_err());
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
}

#[tokio::test]
async fn two_completed_platform_pods_release_capacity_for_the_publisher() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["platforms"] = json!(["linux/amd64", "linux/arm64"]);
    let jobs: Vec<_> = ["linux/amd64", "linux/arm64"]
        .into_iter()
        .map(|p| job(&b, Some(p), Some("Complete")))
        .collect();
    let pods = jobs
        .iter()
        .map(|j| {
            pod(
                j,
                &[("worker", ended(0)), ("buildkit", ended(0))],
                Some(ended(0)),
            )
        })
        .collect();
    let peer = Peer::new(b, jobs, pods).await;
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
    assert_eq!(peer.current()["status"]["phase"], "Publishing");
    assert_eq!(
        peer.current()["status"]["jobOwnership"]["jobs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn orphaned_unknown_pods_keep_pool_and_project_capacity_reserved() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let peer = Peer::new(b, vec![], vec![]).await;
    for (index, id) in [
        "00000000-0000-0000-0000-000000000002",
        "00000000-0000-0000-0000-000000000003",
    ]
    .into_iter()
    .enumerate()
    {
        let mut old = build(&format!("old-{index}"), id);
        let j = job(&old, Some("linux/amd64"), None);
        old["status"] = json!({"phase":"Failed","observedGeneration":1,"message":"CleanupPending: waiting","jobOwnership":{"imageBuildUid":old["metadata"]["uid"],
            "jobs":[{"name":j["metadata"]["name"],"uid":j["metadata"]["uid"],"platform":"linux/amd64"}]}});
        let mut p = pod(
            &j,
            &[("worker", running()), ("buildkit", running())],
            Some(ended(0)),
        );
        p["status"]["phase"] = json!("Unknown");
        let mut s = peer.stored.lock().unwrap();
        s.builds.push(old);
        s.pods.push(p);
    }
    peer.reconcile().await.unwrap();
    assert_eq!(
        peer.stored.lock().unwrap().creates,
        0,
        "unconfirmed remote stop must retain both slots"
    );
}

#[tokio::test]
async fn project_limit_counts_a_failed_job_with_a_live_buildkit_container() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let old = build("old", "00000000-0000-0000-0000-000000000002");
    let j = job(&old, Some("linux/amd64"), Some("Failed"));
    let p = pod(
        &j,
        &[("worker", ended(137)), ("buildkit", running())],
        Some(ended(0)),
    );
    let peer = Peer::new(b, vec![j], vec![p]).await;
    {
        let mut state = peer.stored.lock().unwrap();
        state.pool["spec"]["maxPerProject"] = json!(1);
        state.builds.push(old);
    }
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert_eq!(peer.current()["status"]["phase"], "Building");
}

#[tokio::test]
async fn failure_cleans_all_platforms_and_waits_for_the_other_live_pod() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["platforms"] = json!(["linux/amd64", "linux/arm64"]);
    let first = job(&b, Some("linux/amd64"), None);
    let second = job(&b, Some("linux/arm64"), None);
    let failed = pod(
        &first,
        &[("worker", ended(1)), ("buildkit", running())],
        Some(ended(0)),
    );
    let live = pod(
        &second,
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    let peer = Peer::new(b, vec![first, second], vec![failed, live]).await;
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().deletions.len(), 2);
    peer.stored.lock().unwrap().pods.remove(0);
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Failed");
    assert!(
        peer.current()["status"]["message"]
            .as_str()
            .unwrap()
            .starts_with("CleanupPending:")
    );
    assert_eq!(
        peer.current()["status"]["jobOwnership"]["jobs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn publisher_completion_persists_success_and_all_uids_before_cleanup() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let platform = job(&b, Some("linux/amd64"), Some("Complete"));
    let publish = job(&b, None, Some("Complete"));
    let peer = Peer::new(b, vec![platform, publish], vec![]).await;
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Succeeded");
    assert_eq!(peer.current()["status"]["observedGeneration"], 1);
    let ledger = peer.current()["status"]["jobOwnership"].clone();
    assert_eq!(ledger["jobs"].as_array().unwrap().len(), 2);
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Succeeded");
    assert_eq!(peer.current()["status"]["jobOwnership"], ledger);
    assert!(peer.current()["status"]["message"].is_null());
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert_eq!(peer.stored.lock().unwrap().deletions.len(), 2);
}

#[tokio::test]
async fn replacing_a_recorded_job_uid_is_rejected_without_deletion() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    let mut j = job(&b, Some("linux/amd64"), None);
    b["status"] = json!({"phase":"Building","observedGeneration":1,"jobOwnership":{"imageBuildUid":b["metadata"]["uid"],
        "jobs":[{"name":j["metadata"]["name"],"uid":j["metadata"]["uid"],"platform":"linux/amd64"}]}});
    j["metadata"]["uid"] = json!("replacement-job-uid");
    let peer = Peer::new(b, vec![j], vec![]).await;
    assert!(peer.reconcile().await.is_err());
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
}

#[tokio::test]
async fn foreign_failed_pods_with_matching_names_do_not_fail_the_build() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let j = job(&b, Some("linux/amd64"), None);
    let mut p = pod(
        &j,
        &[("worker", ended(137)), ("buildkit", running())],
        Some(ended(0)),
    );
    p["metadata"]["ownerReferences"][0]["uid"] = json!("foreign-job-uid");
    let peer = Peer::new(b, vec![j], vec![p]).await;
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Building");
}

#[tokio::test]
async fn finalizer_must_be_persisted_before_any_worker_job_is_created() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["metadata"].as_object_mut().unwrap().remove("finalizers");
    let peer = Peer::new(b, vec![], vec![]).await;
    peer.stored.lock().unwrap().fail_metadata = true;
    assert!(peer.reconcile().await.is_err());
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.stored.lock().unwrap().fail_metadata = false;
    peer.reconcile().await.unwrap();
    assert_eq!(
        peer.current()["metadata"]["finalizers"],
        json!(["images.artifactory.dev/cleanup"])
    );
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn deleting_imagebuild_keeps_uid_ledger_and_capacity_until_disconnected_pod_disappears() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["metadata"]["deletionTimestamp"] = json!("2026-11-18T00:00:00Z");
    let j = job(&b, Some("linux/amd64"), None);
    let mut p = pod(
        &j,
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    p["status"]["phase"] = json!("Unknown");
    let peer = Peer::new(b, vec![j.clone()], vec![p]).await;
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    // 每次调用新建 Context，代表 Operator 重启后重新读取持久 CR。
    peer.reconcile().await.unwrap();
    let current = peer.current();
    assert_eq!(
        current["metadata"]["finalizers"],
        json!(["images.artifactory.dev/cleanup"])
    );
    assert_eq!(current["status"]["phase"], "Canceled");
    assert!(
        current["status"]["message"]
            .as_str()
            .unwrap()
            .starts_with("CleanupPending:")
    );
    assert_eq!(
        current["status"]["jobOwnership"]["jobs"][0]["uid"],
        j["metadata"]["uid"]
    );
    assert!(peer.stored.lock().unwrap().jobs.is_empty());
    {
        let mut s = peer.stored.lock().unwrap();
        s.pool["spec"]["maxActive"] = json!(1);
        s.pool["spec"]["maxPerProject"] = json!(1);
        s.builds
            .push(build("waiting", "00000000-0000-0000-0000-000000000002"));
    }
    peer.reconcile_index(1).await.unwrap();
    assert_eq!(
        peer.stored.lock().unwrap().creates,
        0,
        "deleted CR must retain disconnected Pod's slot"
    );
    peer.stored.lock().unwrap().fail_pods = true;
    assert!(peer.reconcile().await.is_err());
    assert_eq!(
        peer.current()["metadata"]["finalizers"],
        json!(["images.artifactory.dev/cleanup"])
    );
    {
        let mut s = peer.stored.lock().unwrap();
        s.fail_pods = false;
        s.pods.clear();
    }
    peer.reconcile().await.unwrap();
    peer.reconcile().await.unwrap();
    assert!(
        peer.stored
            .lock()
            .unwrap()
            .builds
            .iter()
            .all(|b| b["metadata"]["name"] != "build")
    );
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn cleanup_removes_only_its_own_finalizer_after_status_is_clear() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["metadata"]["deletionTimestamp"] = json!("2026-11-18T00:00:00Z");
    b["metadata"]["finalizers"] = json!(["images.artifactory.dev/cleanup", "example.com/other"]);
    let peer = Peer::new(b, vec![], vec![]).await;
    for _ in 0..3 {
        peer.reconcile().await.unwrap();
    }
    assert_eq!(
        peer.current()["metadata"]["finalizers"],
        json!(["example.com/other"])
    );
    assert!(peer.current()["status"]["message"].is_null());
}
