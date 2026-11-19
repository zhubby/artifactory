//! [INPUT]: 依赖本机 HTTP、真实 kube 客户端与直接 Pod 契约。
//! [OUTPUT]: 模拟 CR CAS、Pod finalizer 与 Runtime 授权状态。
//! [POS]: 生命周期测试的隔离 API，未声明 batch 路由。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_agent_operator::{
    controller::{self, Context},
    leadership::Leadership,
    source_tasks,
};
use artifactory_runtime_protocol::service::Client as ServiceClient;
use axum::{
    Router,
    body::Body,
    extract::{Query, Request, State},
    http::StatusCode,
    response::Response,
    routing::any,
};
use kube::{Api, Client};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub const RUN: &str = "00000000-0000-4000-8000-000000000001";
pub const TASK: &str = "00000000-0000-4000-8000-000000000002";
pub fn run() -> Value {
    json!({"apiVersion":"agents.artifactory.dev/v1alpha1","kind":"AgentRun","metadata":{"name":"run","namespace":"test","uid":"run-owner","resourceVersion":"1","generation":1},"spec":{"runId":RUN,"projectId":RUN,"generation":1,"pool":"default","sourceManifestId":"00000000-0000-4000-8000-000000000003","sourceDigest":"a".repeat(64)}})
}
pub fn task() -> Value {
    json!({"apiVersion":"agents.artifactory.dev/v1alpha1","kind":"SourceTask","metadata":{"name":"task","namespace":"test","uid":"task-owner","resourceVersion":"1","generation":1},"spec":{"taskId":TASK,"tenantId":RUN,"projectId":RUN,"purpose":"resolve","pool":"default","timeoutSeconds":900,"cancelRequested":false}})
}
pub struct Stored {
    pub runs: Vec<Value>,
    pub tasks: Vec<Value>,
    pub pods: BTreeMap<String, Value>,
    pub runtime: Value,
    pub events: Vec<(String, Value)>,
    pub creates: usize,
    pub lose_create_ack: bool,
    pub fail_delete: bool,
    pub max_active: u32,
}
pub struct Peer {
    pub stored: Arc<Mutex<Stored>>,
    pub ctx: Arc<Context>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Peer {
    pub async fn new() -> Self {
        let stored = Arc::new(Mutex::new(Stored {
            runs: vec![run()],
            tasks: vec![task()],
            pods: Default::default(),
            runtime: json!({"status":"provisioning","lifecycle":{"generation":1},"execution":null}),
            events: vec![],
            creates: 0,
            lose_create_ack: false,
            fail_delete: false,
            max_active: 1,
        }));
        let app = Router::new()
            .route("/{*path}", any(handle))
            .with_state(stored.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let client = Client::try_from(kube::Config::new(origin.parse().unwrap())).unwrap();
        let mut runtime =
            ServiceClient::new("https://runtime.invalid".into(), "x".repeat(64)).unwrap();
        runtime.origin = format!("{origin}/runtime");
        let ctx = Arc::new(Context {
            client: client.clone(),
            namespace: "test".into(),
            runtime,
            source_admission: tokio::sync::Mutex::new(()),
            leader: Leadership {
                api: Api::namespaced(client, "test"),
                identity: Arc::new("leader".into()),
                name: "operator".into(),
            },
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            stored,
            ctx,
            server,
        }
    }
    pub async fn run_step(&self) -> anyhow::Result<()> {
        let value = self.stored.lock().unwrap().runs[0].clone();
        controller::run_reconcile(
            Arc::new(serde_json::from_value(value).unwrap()),
            self.ctx.clone(),
        )
        .await
        .map(|_| ())
        .map_err(anyhow::Error::new)
    }
    pub async fn task_step(&self) -> anyhow::Result<()> {
        let value = self.stored.lock().unwrap().tasks[0].clone();
        source_tasks::reconcile_task(
            Arc::new(serde_json::from_value(value).unwrap()),
            self.ctx.clone(),
        )
        .await
        .map(|_| ())
    }
    pub fn pod(&self) -> Value {
        self.stored
            .lock()
            .unwrap()
            .pods
            .values()
            .next()
            .unwrap()
            .clone()
    }
    pub fn mutate_pod(&self, f: impl FnOnce(&mut Value)) {
        f(self
            .stored
            .lock()
            .unwrap()
            .pods
            .values_mut()
            .next()
            .unwrap());
    }
}
fn response(code: StatusCode, value: Value) -> Response {
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}
fn missing() -> Response {
    response(
        StatusCode::NOT_FOUND,
        json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"NotFound","message":"fixture object absent","code":404}),
    )
}
fn merge(target: &mut Value, patch: &Value) {
    if let Some(fields) = patch.as_object() {
        if !target.is_object() {
            *target = json!({});
        }
        for (key, value) in fields {
            if value.is_null() {
                target.as_object_mut().unwrap().remove(key);
            } else {
                merge(&mut target[key], value);
            }
        }
    } else {
        *target = patch.clone();
    }
}
async fn handle(
    State(stored): State<Arc<Mutex<Stored>>>,
    Query(query): Query<BTreeMap<String, String>>,
    request: Request,
) -> Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let bytes = axum::body::to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut s = stored.lock().unwrap();
    s.events.push((format!("{method} {path}"), body.clone()));
    if path.contains("/leases/") {
        return response(
            StatusCode::OK,
            json!({"apiVersion":"coordination.k8s.io/v1","kind":"Lease","metadata":{"name":"operator"},"spec":{"holderIdentity":"leader","renewTime":chrono::Utc::now().to_rfc3339(),"leaseDurationSeconds":15}}),
        );
    }
    if let Some(endpoint) = path.strip_prefix("/runtime/v1/") {
        let result = match endpoint {
            "get" => s.runtime.clone(),
            "register" => {
                if body["generation"] != s.runtime["lifecycle"]["generation"] {
                    return response(StatusCode::CONFLICT, json!({"error":"generation fenced"}));
                }
                if !s.runtime["execution"].is_null() && s.runtime["execution"] != body {
                    return response(StatusCode::CONFLICT, json!({"error":"identity changed"}));
                }
                s.runtime["execution"] = body;
                json!({"ok":true})
            }
            "execution-failed" => {
                s.runtime["status"] = json!("failed");
                json!({"status":"failed"})
            }
            "execution-lost" => {
                if body["generation"] != s.runtime["lifecycle"]["generation"] {
                    return response(StatusCode::CONFLICT, json!({"error":"generation fenced"}));
                }
                if body["reason"] == "pod_missing" {
                    s.runtime["lifecycle"]["generation"] =
                        json!(s.runtime["lifecycle"]["generation"].as_i64().unwrap() + 1);
                    s.runtime["status"] = json!("queued");
                    s.runtime["execution"] = Value::Null;
                } else {
                    s.runtime["status"] = json!("failed");
                }
                s.runtime.clone()
            }
            _ => return missing(),
        };
        return response(StatusCode::OK, result);
    }
    if path.contains("/agentpools/") {
        return response(
            StatusCode::OK,
            json!({"apiVersion":"agents.artifactory.dev/v1alpha1","kind":"AgentPool","metadata":{"name":"default"},"spec":{"maxActive":s.max_active,"maxPerProject":1,"image":"worker","runtimeUrl":"https://runtime.invalid","controlUrl":"https://control.invalid","sourcePreparerImage":"source"}}),
        );
    }
    let root = "/api/v1/namespaces/test/pods";
    if path == root {
        return match method.as_str() {
            "GET" => response(
                StatusCode::OK,
                json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":s.pods.values().filter(|pod| query.get("labelSelector").is_none_or(|selector| selector.split(',').all(|term| term.split_once('=').is_some_and(|(key,value)| pod["metadata"]["labels"][key]==value)))).cloned().collect::<Vec<_>>()}),
            ),
            "POST" => {
                let name = body["metadata"]["name"].as_str().unwrap().to_owned();
                if s.pods.contains_key(&name) {
                    return response(
                        StatusCode::CONFLICT,
                        json!({"code":409,"message":"already exists"}),
                    );
                }
                let mut pod = body;
                s.creates += 1;
                pod["metadata"]["uid"] = json!(format!("pod-{}", s.creates));
                pod["metadata"]["resourceVersion"] = json!("1");
                s.pods.insert(name, pod.clone());
                if s.lose_create_ack {
                    s.lose_create_ack = false;
                    response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        json!({"code":500,"message":"ACK lost"}),
                    )
                } else {
                    response(StatusCode::CREATED, pod)
                }
            }
            _ => missing(),
        };
    }
    if let Some(name) = path.strip_prefix(&format!("{root}/")) {
        if method == "DELETE" && s.fail_delete {
            return response(
                StatusCode::FORBIDDEN,
                json!({"code":403,"message":"injected failure"}),
            );
        }
        let Some(pod) = s.pods.get_mut(name) else {
            return missing();
        };
        if method == "DELETE" {
            if body["preconditions"]["uid"] != pod["metadata"]["uid"] {
                return response(
                    StatusCode::CONFLICT,
                    json!({"code":409,"message":"UID fenced"}),
                );
            }
            pod["metadata"]["deletionTimestamp"] = json!(chrono::Utc::now().to_rfc3339());
        } else if method == "PATCH" {
            if body["metadata"]["uid"] != pod["metadata"]["uid"]
                || body["metadata"]["resourceVersion"] != pod["metadata"]["resourceVersion"]
            {
                return response(
                    StatusCode::CONFLICT,
                    json!({"code":409,"message":"Pod UID or version fenced"}),
                );
            }
            merge(pod, &body);
            pod["metadata"]["resourceVersion"] = json!(
                (pod["metadata"]["resourceVersion"]
                    .as_str()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap()
                    + 1)
                .to_string()
            );
        }
        let result = pod.clone();
        if method == "PATCH"
            && pod["metadata"]["finalizers"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && !pod["metadata"]["deletionTimestamp"].is_null()
        {
            s.pods.remove(name);
        }
        return response(StatusCode::OK, result);
    }
    for (plural, kind) in [("agentruns", "AgentRun"), ("sourcetasks", "SourceTask")] {
        let root = format!("/apis/agents.artifactory.dev/v1alpha1/namespaces/test/{plural}");
        let objects = if plural == "agentruns" {
            &mut s.runs
        } else {
            &mut s.tasks
        };
        if path == root {
            return response(
                StatusCode::OK,
                json!({"apiVersion":"agents.artifactory.dev/v1alpha1","kind":format!("{kind}List"),"metadata":{},"items":objects}),
            );
        }
        if let Some(name) = path.strip_prefix(&format!("{root}/")) {
            let name = name.strip_suffix("/status").unwrap_or(name);
            let Some(object) = objects
                .iter_mut()
                .find(|object| object["metadata"]["name"] == name)
            else {
                return missing();
            };
            if method == "PATCH" {
                if body["metadata"]["resourceVersion"] != object["metadata"]["resourceVersion"]
                    || body["metadata"]["uid"] != object["metadata"]["uid"]
                {
                    return response(
                        StatusCode::CONFLICT,
                        json!({"code":409,"message":"version conflict"}),
                    );
                }
                merge(object, &body);
                object["metadata"]["resourceVersion"] = json!(
                    (object["metadata"]["resourceVersion"]
                        .as_str()
                        .unwrap()
                        .parse::<u32>()
                        .unwrap()
                        + 1)
                    .to_string()
                );
            }
            return response(StatusCode::OK, object.clone());
        }
    }
    missing()
}
