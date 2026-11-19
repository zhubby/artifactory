//! [INPUT]: 依赖隔离 PostgreSQL、Coordinator HTTP 和模型桩。
//! [OUTPUT]: 验证预算、审计、故障终态与租户边界。
//! [POS]: 新初始化基线的持久状态回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_database::{PoolSettings, connect, with_options};
use artifactory_database::{params, sql};
use artifactory_planning_coordinator::{
    lifecycle::Lifecycle,
    server::{self, Coordinator},
};
use artifactory_planning_protocol::{Execution, Lease, ModelConfiguration, RunIntent};
use artifactory_service_transport::{crypto, service::Client};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
#[path = "database/conversation_chunks.rs"]
mod conversation_chunks;
#[path = "database/execution_failure.rs"]
mod execution_failure;
#[path = "database/source_payload.rs"]
mod source_payload;
struct TestCoordinator {
    coordinator: Coordinator,
    pool: toasty::Db,
}
impl std::ops::Deref for TestCoordinator {
    type Target = Coordinator;
    fn deref(&self) -> &Coordinator {
        &self.coordinator
    }
}
impl TestCoordinator {
    fn clone(&self) -> Coordinator {
        self.coordinator.clone()
    }
}
async fn state() -> TestCoordinator {
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")
        .expect("ARTIFACTORY_TEST_DATABASE_URL required");
    let admin = connect(&url, Default::default(), PoolSettings::new(2))
        .await
        .unwrap();
    let schema = format!("rt_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut admin.clone(),
        &format!("CREATE SCHEMA {schema}"),
        params![],
    )
    .await
    .unwrap();
    let scoped = with_options(&url, &[("search_path", &schema)]).unwrap();
    let pool = connect(
        &scoped,
        artifactory_planning_coordinator::store::models(),
        PoolSettings::new(12),
    )
    .await
    .unwrap();
    artifactory_database::migrate(&pool, &artifactory_planning_coordinator::store::MIGRATIONS)
        .await
        .unwrap();
    let coordinator = Coordinator {
        store: artifactory_planning_coordinator::store::PlanningStore::new(
            pool.clone(),
            Arc::new([9; 32]),
        ),
        key: Arc::new([9; 32]),
        control: Client::new("https://control.invalid".into(), "x".repeat(64)).unwrap(),
        model_http: artifactory_service_transport::service::client().unwrap(),
        public_url: "https://planning.invalid".into(),
    };
    TestCoordinator { coordinator, pool }
}
fn intent(run: Uuid) -> RunIntent {
    RunIntent {
        tenant_id: Uuid::from_u128(1),
        run_id: run,
        build_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        version: 1,
        source_manifest_id: Some(Uuid::new_v4()),
        source_digest: Some("a".repeat(64)),
        pool: "default".into(),
        context: json!({}),
        model: ModelConfiguration {
            id: Uuid::new_v4(),
            api: "openai-completions".into(),
            base_url: "https://model.invalid/v1".into(),
            model: "fixture".into(),
            api_key: "private-provider-key".into(),
            parameters: json!({}),
            max_calls: 2,
            timeout_seconds: 600,
        },
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn retired_project_purge_waits_for_work_preserves_other_projects_and_blocks_recreation() {
    let state = state().await;
    let target = intent(Uuid::new_v4());
    let other = intent(Uuid::new_v4());
    server::put_intent(&state, serde_json::to_value(&target).unwrap())
        .await
        .unwrap();
    server::put_intent(&state, serde_json::to_value(&other).unwrap())
        .await
        .unwrap();
    assert!(state.store.purge_project(target.project_id).await.is_err());
    sql::execute(
        &mut state.pool.clone(),
        "UPDATE planning_runs SET status='failed' WHERE id=$1",
        params![target.run_id],
    )
    .await
    .unwrap();
    state.store.purge_project(target.project_id).await.unwrap();
    state.store.purge_project(target.project_id).await.unwrap();
    let remaining: Vec<Uuid> = sql::scalars(
        &mut state.pool.clone(),
        "SELECT id FROM planning_runs",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(remaining, vec![other.run_id]);
    let mut late = target.clone();
    late.run_id = Uuid::new_v4();
    assert!(
        server::put_intent(&state, serde_json::to_value(late).unwrap())
            .await
            .is_err()
    );
}
async fn owner(s: &TestCoordinator, run: Uuid) -> Lease {
    let execution = Execution {
        run_id: run,
        generation: 1,
        uid: "pod-a".into(),
        name: "pod-a".into(),
        namespace: "test".into(),
    };
    server::register_execution(s, execution.clone())
        .await
        .unwrap();
    sql::execute(
        &mut s.pool.clone(),
        "UPDATE planning_runs SET status='provisioning' WHERE id=$1",
        params![run],
    )
    .await
    .unwrap();
    let acquired = s
        .store
        .acquire_execution(execution, "https://planning.invalid")
        .await
        .unwrap();
    serde_json::from_value(acquired["lease"].clone()).unwrap()
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn intents_are_idempotent_conflicting_payloads_fail_and_cancel_before_create_cannot_resurrect()
 {
    let s = state().await;
    let run = Uuid::new_v4();
    let value = serde_json::to_value(intent(run)).unwrap();
    server::put_intent(&s, value.clone()).await.unwrap();
    server::put_intent(&s, value.clone()).await.unwrap();
    let mut other = value.clone();
    other["pool"] = json!("other");
    assert!(server::put_intent(&s, other).await.is_err());
    let ciphertext: String = sql::scalar(
        &mut s.pool.clone(),
        "SELECT intent_ciphertext FROM planning_runs WHERE id=$1",
        params![run],
    )
    .await
    .unwrap();
    assert!(!ciphertext.contains("private-provider-key"));
    let canceled = Uuid::new_v4();
    server::put_intent(&s, json!({"run_id":canceled,"version":2,"canceled":true}))
        .await
        .unwrap();
    server::put_intent(&s, serde_json::to_value(intent(canceled)).unwrap())
        .await
        .unwrap();
    assert_eq!(
        server::read_run(&s, canceled).await.unwrap()["status"],
        "canceled"
    );
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn tenants_cannot_rebind_projects() {
    let s = state().await;
    let first = intent(Uuid::new_v4());
    server::put_intent(&s, serde_json::to_value(&first).unwrap())
        .await
        .unwrap();
    let mut other = intent(Uuid::new_v4());
    other.project_id = first.project_id;
    other.tenant_id = Uuid::new_v4();
    assert!(
        server::put_intent(&s, serde_json::to_value(&other).unwrap())
            .await
            .is_err()
    );
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn duplicate_pod_and_old_generation_cannot_report_or_submit_and_proposal_survives_ack_loss() {
    for name in ["submit_plan", "submit_placements"] {
        let s = state().await;
        let run = Uuid::new_v4();
        server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
            .await
            .unwrap();
        let lease = owner(&s, run).await;
        assert!(
            server::register_execution(
                &s,
                Execution {
                    run_id: run,
                    generation: 1,
                    uid: "pod-b".into(),
                    name: "pod-b".into(),
                    namespace: "test".into()
                }
            )
            .await
            .is_err()
        );
        let id = Uuid::new_v4();
        let arguments = if name == "submit_plan" {
            json!({"plan":{"summary":"test"}})
        } else {
            json!({"placements":{"app":{"candidates":[]}}})
        };
        let value = json!({"lease":lease,"request_id":id,"name":name,"arguments":arguments});
        assert_eq!(
            server::tool_call(&s, value.clone()).await.unwrap()["pending"],
            true
        );
        assert_eq!(
            server::tool_call(&s, value.clone()).await.unwrap()["proposal_id"],
            id.to_string()
        );
        let mut conflict = value;
        conflict["arguments"] = json!({});
        assert!(server::tool_call(&s, conflict).await.is_err());
        let proposals: i64 = sql::scalar(
            &mut s.pool.clone(),
            "SELECT count(*) FROM planning_proposals WHERE run_id=$1",
            params![run],
        )
        .await
        .unwrap();
        assert_eq!(proposals, 1);
        let mut stale = lease.clone();
        stale.uid = "pod-b".into();
        assert!(server::validate_grant(&s, stale).await.is_err());
        server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
            .await
            .unwrap();
        server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
            .await
            .expect("replaying cancellation must be idempotent");
        assert!(server::heartbeat_run(&s, lease).await.is_err());
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn conversations_preserve_generation_messages_without_replaying_them() {
    let s = state().await;
    let run = Uuid::new_v4();
    let input = intent(run);
    server::put_intent(&s, serde_json::to_value(&input).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    let event = json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":1,"kind":"conversation","payload":{"kind":"message","content":{"role":"toolResult","content":"repository-source-excerpt"}}});
    server::report_event(&s, event.clone()).await.unwrap();
    server::report_event(&s, event).await.unwrap();
    let scope = json!({"run_id":run,"tenant_id":input.tenant_id,"project_id":input.project_id,"generation":1,"limit":1});
    let messages = s.store.conversation_messages(scope.clone()).await.unwrap();
    assert_eq!(messages["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        messages["messages"][0]["payload"]["content"],
        "repository-source-excerpt"
    );
    let second = json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":2,"kind":"conversation","payload":{"kind":"message","content":{"role":"assistant","content":"completed answer"}}});
    server::report_event(&s, second).await.unwrap();
    let page = s.store.conversation_messages(scope.clone()).await.unwrap();
    assert_eq!(page["next_cursor"], 1);
    let mut after = scope.clone();
    after["after"] = json!(1);
    let page = s.store.conversation_messages(after).await.unwrap();
    assert_eq!(page["messages"][0]["sequence"], 2);
    assert!(page["next_cursor"].is_null());
    let mut denied = scope.clone();
    denied["tenant_id"] = json!(Uuid::new_v4());
    assert!(s.store.conversation_messages(denied).await.is_err());
    sql::execute(&mut s.pool.clone(),"UPDATE planning_runs SET lifecycle=jsonb_set(lifecycle,'{lease_until_ms}','0'::jsonb) WHERE id=$1",params![run]).await.unwrap();
    s.store.maintain().await.unwrap();
    let execution = Execution {
        run_id: run,
        generation: 2,
        uid: "new-pod".into(),
        name: "new-pod".into(),
        namespace: "test".into(),
    };
    server::register_execution(&s, execution.clone())
        .await
        .unwrap();
    sql::execute(
        &mut s.pool.clone(),
        "UPDATE planning_runs SET status='provisioning' WHERE id=$1",
        params![run],
    )
    .await
    .unwrap();
    let acquired = s
        .store
        .acquire_execution(execution, "https://planning.invalid")
        .await
        .unwrap();
    assert!(acquired["task"]["context"].get("checkpoint").is_none());
    let sessions = s.store.conversations(scope).await.unwrap();
    assert_eq!(sessions["conversations"][0]["status"], "interrupted");
    assert_eq!(sessions["conversations"].as_array().unwrap().len(), 2);
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn source_preparation_does_not_start_the_model_deadline() {
    let s = state().await;
    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    let execution = Execution {
        run_id: run,
        generation: 1,
        uid: "init".into(),
        name: "init".into(),
        namespace: "test".into(),
    };
    server::register_execution(&s, execution.clone())
        .await
        .unwrap();
    sql::execute(
        &mut s.pool.clone(),
        "UPDATE planning_runs SET status='provisioning' WHERE id=$1",
        params![run],
    )
    .await
    .unwrap();
    s.store.prepare_execution(&execution).await.unwrap();
    let state = server::read_run(&s, run).await.unwrap();
    assert!(state["lifecycle"]["deadline_ms"].is_null());
    assert_eq!(state["lifecycle"]["calls"], 0);
}

type Requests = Arc<std::sync::Mutex<Vec<(String, String, Value)>>>;
async fn upstream() -> (String, Requests, tokio::task::JoinHandle<()>) {
    use axum::{
        Router,
        extract::{Path, State},
        http::HeaderMap,
        routing::post,
    };
    let requests: Requests = Default::default();
    let captured = requests.clone();
    let app = Router::new()
        .route(
            "/{*path}",
            post(
                |State(seen): State<Requests>,
                 Path(path): Path<String>,
                 headers: HeaderMap,
                 axum::Json(value): axum::Json<Value>| async move {
                    let key = headers
                        .get("authorization")
                        .or_else(|| headers.get("x-api-key"))
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string();
                    seen.lock().unwrap().push((path, key, value));
                    (
                        [("content-type", "text/event-stream")],
                        "event: tool\ndata: {\"arguments\":\"partial\"}\n\ndata: [DONE]\n\n",
                    )
                },
            ),
        )
        .with_state(captured);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (origin, requests, handle)
}
async fn fixture_run(s: &TestCoordinator, origin: &str, api: &str) -> (Uuid, Lease) {
    let run = Uuid::new_v4();
    let mut input = intent(run);
    input.model.api = api.into();
    server::put_intent(s, serde_json::to_value(&input).unwrap())
        .await
        .unwrap();
    // Only the test upstream is plaintext; production endpoint validation remains enabled.
    input.model.base_url = format!("{origin}/v1");
    sql::execute(
        &mut s.pool.clone(),
        "UPDATE planning_runs SET intent_ciphertext=$2 WHERE id=$1",
        params![
            run,
            crypto::seal(&s.key, &serde_json::to_vec(&input).unwrap()).unwrap()
        ],
    )
    .await
    .unwrap();
    (run, owner(s, run).await)
}
fn model_request(lease: &Lease, request_id: Uuid) -> axum::extract::Request {
    axum::http::Request::builder()
        .header("authorization", format!("Bearer {}", lease.token))
        .header("x-artifactory-request-id", request_id.to_string())
        .body(axum::body::Body::from(
            json!({"model":"fixture","stream":true,"messages":[],"tools":[]}).to_string(),
        ))
        .unwrap()
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn all_model_protocols_stream_with_private_credentials_and_replays_never_resend() {
    let s = state().await;
    let (origin, requests, task) = upstream().await;
    for (api, path) in [
        ("openai-completions", "v1/chat/completions"),
        ("openai-responses", "v1/responses"),
        ("anthropic-messages", "v1/messages"),
    ] {
        let (run, lease) = fixture_run(&s, &origin, api).await;
        let request_id = Uuid::new_v4();
        let reply = server::model_request(
            s.clone(),
            run,
            1,
            lease.uid.clone(),
            path.into(),
            model_request(&lease, request_id),
        )
        .await
        .unwrap();
        assert_eq!(reply.status(), 200);
        assert_eq!(reply.headers()["content-type"], "text/event-stream");
        let body = axum::body::to_bytes(reply.into_body(), 4096).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("partial"));
        let replay = server::model_request(
            s.clone(),
            run,
            1,
            lease.uid.clone(),
            path.into(),
            model_request(&lease, request_id),
        )
        .await
        .unwrap();
        assert_eq!(replay.status(), 409);
        assert!(
            server::model_request(
                s.clone(),
                run,
                1,
                lease.uid.clone(),
                "v1/other".into(),
                model_request(&lease, Uuid::new_v4())
            )
            .await
            .is_err()
        );
        assert_eq!(
            server::read_run(&s, run).await.unwrap()["lifecycle"]["calls"],
            1
        );
    }
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0].0, "v1/chat/completions");
    assert_eq!(captured[1].0, "v1/responses");
    assert_eq!(captured[2].0, "v1/messages");
    assert_eq!(captured[0].1, "Bearer private-provider-key");
    assert_eq!(captured[2].1, "private-provider-key");
    task.abort();
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn concurrent_model_requests_cannot_overspend_and_unknown_outcomes_remain_consumed() {
    let s = state().await;
    let (origin, requests, task) = upstream().await;
    let (run, lease) = fixture_run(&s, &origin, "openai-completions").await;
    let results = futures::future::join_all((0..12).map(|_| {
        server::model_request(
            s.clone(),
            run,
            1,
            lease.uid.clone(),
            "v1/chat/completions".into(),
            model_request(&lease, Uuid::new_v4()),
        )
    }))
    .await;
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 2);
    assert_eq!(requests.lock().unwrap().len(), 2);
    task.abort();
    let (run, lease) = fixture_run(&s, &origin, "openai-completions").await;
    let request_id = Uuid::new_v4();
    assert!(
        server::model_request(
            s.clone(),
            run,
            1,
            lease.uid.clone(),
            "v1/chat/completions".into(),
            model_request(&lease, request_id)
        )
        .await
        .is_err()
    );
    let replay = server::model_request(
        s.clone(),
        run,
        1,
        lease.uid.clone(),
        "v1/chat/completions".into(),
        model_request(&lease, request_id),
    )
    .await
    .unwrap();
    assert_eq!(replay.status(), 409);
    assert_eq!(
        server::read_run(&s, run).await.unwrap()["lifecycle"]["calls"],
        1
    );
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn ten_thousand_queued_runs_admit_only_pool_capacity_even_with_concurrent_dispatchers() {
    let s = state().await;
    let projects: Vec<_> = (0..4).map(|_| Uuid::new_v4()).collect();
    let mut ids = Vec::new();
    let mut encrypted = Vec::new();
    let mut project_ids = Vec::new();
    let mut model = None;
    for i in 0..10_000 {
        let run = Uuid::new_v4();
        let mut input = intent(run);
        input.project_id = projects[i % 4];
        model = Some(input.source_manifest_id);
        ids.push(run);
        project_ids.push(input.project_id);
        encrypted.push(crypto::seal(&s.key, &serde_json::to_vec(&input).unwrap()).unwrap());
    }
    let tenant = Uuid::from_u128(1);
    sql::execute(&mut s.pool.clone(), "INSERT INTO planning_project_tenants(project_id,tenant_id) SELECT project,$2 FROM unnest($1::uuid[]) project", params![&projects, tenant]).await.unwrap();
    sql::execute(&mut s.pool.clone(), "INSERT INTO planning_runs(id,version,intent_hash,intent_ciphertext,project_id,pool,source_manifest_id,source_digest,lifecycle,status,tenant_id) SELECT id,1,'fixture',ciphertext,project,'default',$4,$5,$6,'queued',$7 FROM unnest($1::uuid[],$2::text[],$3::uuid[]) AS t(id,ciphertext,project)", params![&ids, &encrypted, &project_ids, model.unwrap(), "a".repeat(64), serde_json::to_value(Lifecycle::new(2)).unwrap(),tenant]).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/queue", listener.local_addr().unwrap());
    let app = server::router(s.clone(), "x".repeat(64));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = reqwest::Client::new();
    for _ in 0..4 {
        let replies = futures::future::join_all((0..3).map(|_| {
            http.post(&url)
                .bearer_auth("x".repeat(64))
                .json(&json!({"pool":"default","limit":8,"per_project":2}))
                .send()
        }))
        .await;
        for reply in replies {
            let value: Value = reply
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(value["runs"].as_array().unwrap().len() <= 8);
        }
    }
    let active: i64 = sql::scalar(
        &mut s.pool.clone(),
        "SELECT count(*) FROM planning_runs WHERE status='provisioning'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(active, 8);
    let highest:i64=sql::scalar(&mut s.pool.clone(), "SELECT max(n)::bigint FROM (SELECT count(*) n FROM planning_runs WHERE status='provisioning' GROUP BY project_id) c", params![]).await.unwrap();
    assert_eq!(highest, 2);
    let untouched:i64=sql::scalar(&mut s.pool.clone(), "SELECT count(*) FROM planning_runs WHERE status='queued' AND (lifecycle->>'deadline_ms') IS NULL AND (lifecycle->>'calls')::int=0", params![]).await.unwrap();
    assert_eq!(untouched, 9_992);
    task.abort();
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn cancellation_interrupts_a_provider_that_has_not_returned_response_headers() {
    let s = state().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            "late response"
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let (run, lease) = fixture_run(&s, &origin, "openai-completions").await;
    let (result, ()) = tokio::join!(
        tokio::time::timeout(
            std::time::Duration::from_secs(4),
            server::model_request(
                s.clone(),
                run,
                1,
                lease.uid.clone(),
                "v1/chat/completions".into(),
                model_request(&lease, Uuid::new_v4())
            )
        ),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
                .await
                .unwrap();
        }
    );
    assert!(
        result
            .expect("cancel must interrupt the upstream wait")
            .is_err()
    );
    assert_eq!(
        server::read_run(&s, run).await.unwrap()["lifecycle"]["calls"],
        1
    );
    task.abort();
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn profile_rate_reservations_are_shared_across_different_runs() {
    let s = state().await;
    let (origin, _, task) = upstream().await;
    let profile = Uuid::new_v4();
    let mut runs = Vec::new();
    for _ in 0..3 {
        let (run, lease) = fixture_run(&s, &origin, "openai-completions").await;
        let ciphertext: String = sql::scalar(
            &mut s.pool.clone(),
            "SELECT intent_ciphertext FROM planning_runs WHERE id=$1",
            params![run],
        )
        .await
        .unwrap();
        let mut cfg: RunIntent =
            serde_json::from_slice(&crypto::open(&s.key, &ciphertext).unwrap()).unwrap();
        cfg.model.id = profile;
        cfg.model.parameters = json!({"requests_per_minute":600});
        sql::execute(
            &mut s.pool.clone(),
            "UPDATE planning_runs SET intent_ciphertext=$2 WHERE id=$1",
            params![
                run,
                crypto::seal(&s.key, &serde_json::to_vec(&cfg).unwrap()).unwrap()
            ],
        )
        .await
        .unwrap();
        runs.push((run, lease));
    }
    let began = std::time::Instant::now();
    let results = futures::future::join_all(runs.iter().map(|(run, lease)| {
        server::model_request(
            s.clone(),
            *run,
            1,
            lease.uid.clone(),
            "v1/chat/completions".into(),
            model_request(lease, Uuid::new_v4()),
        )
    }))
    .await;
    assert!(results.iter().all(|result| result.is_ok()));
    assert!(
        began.elapsed() >= std::time::Duration::from_secs(2),
        "a shared profile must serialize reserved rate slots"
    );
    task.abort();
}
