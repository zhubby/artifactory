//! [INPUT]: 依赖本机 HTTP、真实 kube 客户端与直接 Pod 契约。
//! [OUTPUT]: 提供 CR CAS、创建意图、Pod finalizer 和查池故障注入。
//! [POS]: 隔离 API 禁用客户端退避，由协调器测试控制重试。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
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
use std::sync::{Arc, Mutex};

pub struct Stored {
    pub pool: Value,
    pub builds: Vec<Value>,
    pub pods: Vec<Value>,
    pub deletions: Vec<(Value, Value)>,
    pub requests: Vec<(String, String)>,
    pub fail_status: bool,
    pub fail_pods: bool,
    pub fail_metadata: bool,
    pub pool_status: Option<u16>,
    pub creates: usize,
    pub create_intents: Vec<Value>,
}
pub struct Peer {
    pub stored: Arc<Mutex<Stored>>,
    client: Client,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.server.abort();
    }
}
pub fn build(name: &str, id: &str) -> Value {
    json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":name,"namespace":"test","uid":format!("{name}-uid"),"resourceVersion":"1","generation":1,"finalizers":["images.artifactory.dev/cleanup"]},"spec":{"executionId":id,"tenantId":"tenant","projectId":"project","pool":"pool","sourceManifestId":"manifest","sourceDigest":"a".repeat(64),"taskSecret":"task","platforms":["linux/amd64"],"allowEmulation":false,"publishOnly":false,"timeoutSeconds":600,"cancelRequested":false}})
}
pub fn pool() -> BuilderPool {
    serde_json::from_value(json!({"metadata":{"name":"pool"},"spec":{"workerImage":"worker:1","buildkitImage":"buildkit:1","sourcePreparerImage":"source:1","controlUrl":"http://control","maxActive":2,"maxPerProject":2}})).unwrap()
}
pub fn running() -> Value {
    json!({"running":{}})
}
pub fn ended(code: i32) -> Value {
    json!({"terminated":{"exitCode":code,"reason":if code == 137 { "OOMKilled" } else if code == 0 { "Completed" } else { "Error" }}})
}
pub fn pod(
    build: &Value,
    platform: Option<&str>,
    states: &[(&str, Value)],
    init: Option<Value>,
) -> Value {
    let b: ImageBuild = serde_json::from_value(build.clone()).unwrap();
    let mut pod =
        serde_json::to_value(resources::pod(&b, &pool(), platform, Default::default()).unwrap())
            .unwrap();
    pod["metadata"]["uid"] = json!(format!("{}-uid", pod["metadata"]["name"].as_str().unwrap()));
    pod["metadata"]["resourceVersion"] = json!("1");
    pod["spec"]["nodeName"] = json!("worker-node");
    pod["status"] = json!({"phase":"Running","containerStatuses":states.iter().map(|(name,state)|json!({"name":name,"state":state,"containerID":format!("containerd://{name}"),"image":"image","imageID":"id","ready":false,"restartCount":0})).collect::<Vec<_>>()});
    if let Some(state) = init {
        pod["status"]["initContainerStatuses"] = json!([{"name":"source","state":state,"containerID":"containerd://source","image":"image","imageID":"id","ready":false,"restartCount":0}]);
    }
    pod
}
pub fn complete(build: &Value, platform: Option<&str>) -> Value {
    let states = if platform.is_some() {
        vec![("worker", ended(0)), ("buildkit", ended(0))]
    } else {
        vec![("worker", ended(0))]
    };
    let mut p = pod(build, platform, &states, platform.map(|_| ended(0)));
    p["status"]["phase"] = json!("Succeeded");
    p
}
pub fn ledger(build: &mut Value, pods: &[Value], stopped: bool) {
    build["status"] = json!({"phase":"Building","observedGeneration":1,"podOwnership":{"imageBuildUid":build["metadata"]["uid"],"pods":pods.iter().map(|p| json!({"name":p["metadata"]["name"],"uid":p["metadata"]["uid"],"platform":if p["metadata"]["labels"]["artifactory.dev/platform"] == "publish" {"publish".to_owned()} else {format!("linux/{}",p["metadata"]["labels"]["artifactory.dev/platform"].as_str().unwrap())},"stopped":stopped})).collect::<Vec<_>>()}});
}
fn error(code: StatusCode) -> (StatusCode, Json<Value>) {
    (
        code,
        Json(
            json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":if code == StatusCode::NOT_FOUND { "NotFound" } else { "Injected" },"message":"injected API failure","code":code.as_u16()}),
        ),
    )
}
pub fn bump(value: &mut Value) {
    let version = value["metadata"]["resourceVersion"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap()
        + 1;
    value["metadata"]["resourceVersion"] = json!(version.to_string());
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
    s.requests.push((method.to_string(), path.into()));
    if path.contains("/leases/") {
        return (
            StatusCode::OK,
            Json(
                json!({"apiVersion":"coordination.k8s.io/v1","kind":"Lease","metadata":{"name":"operator"},"spec":{"holderIdentity":"leader","renewTime":k8s_openapi::jiff::Timestamp::now().to_string(),"leaseDurationSeconds":15}}),
            ),
        );
    }
    if path.ends_with("/imagebuilds") {
        return (
            StatusCode::OK,
            Json(
                json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuildList","metadata":{},"items":s.builds}),
            ),
        );
    }
    if path.contains("/imagebuilds/") && method == Method::GET {
        let name = path.rsplit('/').next().unwrap();
        return match s
            .builds
            .iter()
            .find(|build| build["metadata"]["name"] == name)
        {
            Some(build) => (StatusCode::OK, Json(build.clone())),
            None => error(StatusCode::NOT_FOUND),
        };
    }
    if path.contains("/imagebuilds/") && method == Method::PATCH {
        let status = path.ends_with("/status");
        if status && s.fail_status || !status && s.fail_metadata {
            return error(StatusCode::INTERNAL_SERVER_ERROR);
        }
        let name = if status {
            path.split('/').rev().nth(1).unwrap()
        } else {
            path.rsplit('/').next().unwrap()
        };
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
        if status {
            b["status"] = data["status"].clone();
        } else {
            b["metadata"]["finalizers"] = data["metadata"]["finalizers"].clone();
        }
        bump(b);
        let result = b.clone();
        if b["metadata"]["deletionTimestamp"].is_string()
            && b["metadata"]["finalizers"].as_array().unwrap().is_empty()
        {
            s.builds.remove(index);
        }
        return (StatusCode::OK, Json(result));
    }
    if path.contains("/builderpools/") {
        if let Some(code) = s.pool_status {
            return error(StatusCode::from_u16(code).unwrap());
        }
        return (StatusCode::OK, Json(s.pool.clone()));
    }
    if path.ends_with("/nodes") {
        return (
            StatusCode::OK,
            Json(
                json!({"apiVersion":"v1","kind":"NodeList","metadata":{},"items":[{"metadata":{"name":"amd64","labels":{"kubernetes.io/os":"linux","kubernetes.io/arch":"amd64"}},"status":{"conditions":[{"type":"Ready","status":"True"}]}},{"metadata":{"name":"arm64","labels":{"kubernetes.io/os":"linux","kubernetes.io/arch":"arm64"}},"status":{"conditions":[{"type":"Ready","status":"True"}]}}]}),
            ),
        );
    }
    if path.ends_with("/pods") {
        if s.fail_pods {
            return error(StatusCode::FORBIDDEN);
        }
        if method == Method::POST {
            let mut p = data;
            if s.pods
                .iter()
                .any(|pod| pod["metadata"]["name"] == p["metadata"]["name"])
            {
                return error(StatusCode::CONFLICT);
            }
            let owner = s
                .builds
                .iter()
                .find(|build| {
                    build["metadata"]["uid"] == p["metadata"]["ownerReferences"][0]["uid"]
                })
                .unwrap()
                .clone();
            s.create_intents
                .push(owner["status"]["podOwnership"]["creating"].clone());
            p["metadata"]["uid"] = json!(format!("created-{}", s.creates));
            p["metadata"]["resourceVersion"] = json!("1");
            s.creates += 1;
            s.pods.push(p.clone());
            return (StatusCode::CREATED, Json(p));
        }
        return (
            StatusCode::OK,
            Json(json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":s.pods})),
        );
    }
    if path.contains("/pods/") {
        let name = path.rsplit('/').next().unwrap();
        let Some(index) = s.pods.iter().position(|p| p["metadata"]["name"] == name) else {
            return error(StatusCode::NOT_FOUND);
        };
        if method == Method::DELETE {
            if data["preconditions"]["uid"] != s.pods[index]["metadata"]["uid"] {
                return error(StatusCode::CONFLICT);
            }
            let build = s
                .builds
                .iter()
                .find(|b| {
                    b["metadata"]["uid"] == s.pods[index]["metadata"]["ownerReferences"][0]["uid"]
                })
                .unwrap()
                .clone();
            s.deletions.push((data.clone(), build));
            s.pods[index]["metadata"]["deletionTimestamp"] = json!("2026-11-19T00:00:00Z");
            bump(&mut s.pods[index]);
        }
        if method == Method::PATCH {
            let p = &mut s.pods[index];
            if data["metadata"]["uid"] != p["metadata"]["uid"]
                || data["metadata"]["resourceVersion"] != p["metadata"]["resourceVersion"]
            {
                return error(StatusCode::CONFLICT);
            }
            p["metadata"]["finalizers"] = data["metadata"]["finalizers"].clone();
            bump(p);
        }
        let result = s.pods[index].clone();
        if result["metadata"]["deletionTimestamp"].is_string()
            && result["metadata"]["finalizers"]
                .as_array()
                .unwrap()
                .is_empty()
        {
            s.pods.remove(index);
        }
        return (StatusCode::OK, Json(result));
    }
    error(StatusCode::NOT_FOUND)
}
impl Peer {
    pub async fn new(build: Value, pods: Vec<Value>) -> Self {
        let stored = Arc::new(Mutex::new(Stored {
            pool: serde_json::to_value(pool()).unwrap(),
            builds: vec![build],
            pods,
            deletions: vec![],
            requests: vec![],
            fail_status: false,
            fail_pods: false,
            fail_metadata: false,
            pool_status: None,
            creates: 0,
            create_intents: vec![],
        }));
        let app = Router::new().fallback(handle).with_state(stored.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = kube::Config::new(
            format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        );
        config.default_retry = false;
        let client = Client::try_from(config).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            stored,
            client,
            server,
        }
    }
    pub fn current(&self) -> Value {
        self.stored.lock().unwrap().builds[0].clone()
    }
    pub async fn reconcile(&self) -> Result<(), String> {
        self.reconcile_index(0).await
    }
    pub async fn reconcile_index(&self, index: usize) -> Result<(), String> {
        let build = self.stored.lock().unwrap().builds[index].clone();
        self.reconcile_value(build).await
    }
    pub async fn reconcile_value(&self, build: Value) -> Result<(), String> {
        let build: ImageBuild = serde_json::from_value(build).unwrap();
        let ctx = Arc::new(Context {
            client: self.client.clone(),
            namespace: "test".into(),
            leader: Leadership {
                api: Api::namespaced(self.client.clone(), "test"),
                name: "operator".into(),
                identity: Arc::new("leader".into()),
            },
            admission: tokio::sync::Mutex::new(()),
        });
        reconcile(Arc::new(build), ctx)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub async fn repeat(&self, times: usize) {
        for _ in 0..times {
            self.reconcile().await.unwrap();
        }
    }
}
