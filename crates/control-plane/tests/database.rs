//! [INPUT]: 依赖隔离 PostgreSQL、授权设备和业务服务。
//! [OUTPUT]: 提供数据库夹具、查池并发行锁验证和跨模块测试入口。
//! [POS]: 真实数据库测试总入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_control_plane::{
    api, crypto,
    protocol::Command,
    recipe::Recipe,
    scheduler,
    settings::Settings,
    state::{AppState, Connection},
    storage::Storage,
};
use artifactory_database::{PoolSettings, connect, with_options};
use artifactory_database::{params, sql};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{RwLock, mpsc};
use tower::ServiceExt;
use uuid::Uuid;

#[path = "database/fixture.rs"]
mod fixture;
#[path = "database/image_pool_admission.rs"]
mod image_pool_admission;
#[path = "database/images.rs"]
mod images;
#[path = "database/planning_basics.rs"]
mod planning_basics;
#[path = "database/planning_bridge.rs"]
mod planning_bridge;
#[path = "database/registration.rs"]
mod registration;
#[path = "database/release_contract.rs"]
mod release_contract;
#[path = "database/replication.rs"]
mod replication;
#[path = "database/source_refs.rs"]
mod source_refs;
#[path = "database/tenancy.rs"]
mod tenancy;
#[path = "database/transport.rs"]
mod transport;

struct Database {
    state: AppState,
    pool: toasty::Db,
    admin: toasty::Db,
    schema: String,
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn concurrent_schedulers_do_not_double_assign_or_exceed_slots() {
    let database = Database::with_tenant().await;
    let (_, _, job) = database.fixture().await;
    sql::execute(&mut database.pool.clone(), "INSERT INTO jobs(id,build_id,target,target_config) SELECT $1,build_id,'second',target_config FROM jobs WHERE id=$2", params![Uuid::new_v4(), job]).await.unwrap();
    database.node().await;
    let (first, second) = tokio::join!(
        scheduler::tick(&database.state),
        scheduler::tick(&database.state)
    );
    first.unwrap();
    second.unwrap();
    let attempts: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(attempts, 1);
    let total: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(total, 1);
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn queued_signing_authorization_is_rechecked() {
    let database = Database::with_tenant().await;
    let (project, _, _) = database.fixture().await;
    let node = database.node().await;
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE jobs SET target_config=jsonb_set(target_config,'{signing_profiles}','[\"apple\"]')",
        params![],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE nodes SET signing_profiles='{apple}' WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    scheduler::tick(&database.state).await.unwrap();
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE projects SET signing_profiles='{apple}' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    scheduler::tick(&database.state).await.unwrap();
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 1);
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn build_scope_can_retry_release_drafts_and_source_is_snapshotted() {
    let database = Database::with_tenant().await;
    let (project, _, job) = database.fixture().await;
    database.node().await;
    let user = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO users(id,username,password_hash,role) VALUES($1,'builder','unused','operator')", params![user]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO api_tokens(tenant_id,id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES('00000000-0000-0000-0000-000000000001',$1,$2,'build-only',$3,$4,'{read,build}',now()+interval '1 hour')", params![Uuid::new_v4(), user, crypto::hash("builder"), vec![project]]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE jobs SET status='failed' WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    let router = api::router(database.state.clone());
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/jobs/{job}/retry"))
        .header("content-type", "application/json")
        .header("authorization", "Bearer builder")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::OK
    );
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1{}",
            database.release_build_path(project).await
        ))
        .header("content-type", "application/json")
        .header("authorization", "Bearer builder")
        .body(Body::from(fixture::build_input().to_string()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let build = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE projects SET repository_url='https://example.com/changed.git' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    let repository: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT source_repository_url FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert_eq!(repository, "https://example.com/repo.git");
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn incomplete_artifacts_never_make_a_release_visible() {
    let database = Database::with_tenant().await;
    let (_, build, _) = database.fixture().await;
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE builds SET status='succeeded' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert!(
        scheduler::publish_build(&database.state, build)
            .await
            .is_err()
    );
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM releases WHERE status='published'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM publish_attempts WHERE status='failed'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn expired_started_tasks_are_lost_and_unstarted_tasks_requeue() {
    let database = Database::with_tenant().await;
    let (_, _, job) = database.fixture().await;
    database.node().await;
    scheduler::tick(&database.state).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE attempts SET lease_expires_at=now()-interval '1 second'",
        params![],
    )
    .await
    .unwrap();
    scheduler::expire(&database.state).await.unwrap();
    let state: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT status FROM jobs WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(state, "queued");
    scheduler::tick(&database.state).await.unwrap();
    sql::execute(&mut database.pool.clone(), "UPDATE attempts SET status='running',started_at=now(),lease_expires_at=now()-interval '1 second' WHERE status='leased'", params![]).await.unwrap();
    scheduler::expire(&database.state).await.unwrap();
    let state: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT status FROM jobs WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(state, "lost");
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn publication_is_atomic_immutable_and_idempotent() {
    let database = Database::with_tenant().await;
    let (_, build, job) = database.fixture().await;
    let node = database.node().await;
    let attempt = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE jobs SET status='succeeded'",
        params![],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE builds SET status='succeeded'",
        params![],
    )
    .await
    .unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id,source_manifest_id) SELECT $1,$2,$3,'test',now(),'succeeded',$4,source_manifest_id FROM builds WHERE id=$5", params![attempt, job, node, Uuid::new_v4(),build]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'dist/app',$3,5,$4,now())", params![artifact, attempt, format!("upload/{artifact}"), "a".repeat(64)]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'dist/app',$3,5,$4)", params![artifact, attempt, format!("artifact/{artifact}"), "a".repeat(64)]).await.unwrap();
    let (first, second) = tokio::join!(
        scheduler::publish_build(&database.state, build),
        scheduler::publish_build(&database.state, build)
    );
    assert_eq!(first.unwrap(), second.unwrap());
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM releases",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 1);
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM release_artifacts",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 1);
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn real_http_authorization_rejects_csrf_and_scope_escalation() {
    let database = Database::with_tenant().await;
    let (project, _, _) = database.fixture().await;
    let user = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO users(id,username,password_hash,role) VALUES($1,'operator','unused','operator')", params![user]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("session"), user],
    )
    .await
    .unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO api_tokens(tenant_id,id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES('00000000-0000-0000-0000-000000000001',$1,$2,'scoped',$3,$4,'{read}',now()+interval '1 hour')", params![Uuid::new_v4(), user, crypto::hash("api-token"), vec![project]]).await.unwrap();
    let router = api::router(database.state.clone());
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1{}",
            database.release_build_path(project).await
        ))
        .header("content-type", "application/json")
        .header(
            "X-Artifactory-Tenant",
            "00000000-0000-0000-0000-000000000001",
        )
        .header("cookie", "artifactory_session=session")
        .body(Body::from(fixture::build_input().to_string()))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1{}",
            database.release_build_path(project).await
        ))
        .header("content-type", "application/json")
        .header("authorization", "Bearer api-token")
        .body(Body::from(fixture::build_input().to_string()))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let request = Request::builder()
        .uri("/api/v1/nodes")
        .header("authorization", "Bearer api-token")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_revocation_audit_records_effective_irreversible_state() {
    let database = Database::with_tenant().await;
    let node = database.node().await;
    let user = Uuid::new_v4();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,'admin','unused','admin')",
        params![user],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("admin-session"), user],
    )
    .await
    .unwrap();
    let router = api::router(database.state.clone());
    for revoked in [true, false] {
        let request = Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/nodes/{node}"))
            .header("content-type", "application/json")
            .header(
                "X-Artifactory-Tenant",
                "00000000-0000-0000-0000-000000000001",
            )
            .header("cookie", "artifactory_session=admin-session")
            .header("x-artifactory-csrf", "1")
            .header("origin", "http://localhost:3000")
            .body(Body::from(
                serde_json::json!({
                    "draining": true,
                    "revoked": revoked,
                    "slots": 1,
                    "labels": {},
                    "signing_profiles": [],
                    "pool_id": "00000000-0000-0000-0000-000000000001"
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
    }
    let revoked: bool = sql::scalar(
        &mut database.pool.clone(),
        "SELECT revoked FROM nodes WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let states: Vec<bool> = sql::scalars(&mut database.pool.clone(), "SELECT (details->>'revoked')::boolean FROM audit_events WHERE action='node_updated' AND resource_id=$1 ORDER BY id", params![node.to_string()])
    .await
    .unwrap();
    assert!(revoked);
    assert_eq!(states, vec![true, true]);
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_uses_node_lock_and_pool_limit_and_never_changes_admin_drain() {
    let database = Database::with_tenant().await;
    let (_, _, _job) = database.fixture().await;
    let first = database.node().await;
    let second = database.node().await;
    let release = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES('00000000-0000-0000-0000-000000000001',$1,'v-next',$2,'test','hash')", params![release, serde_json::json!({"assets":[{"os":"linux","arch":"amd64","name":"artifactory-emissary-linux-amd64"}]}).to_string()]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "UPDATE nodes SET capabilities=capabilities || '[\"emissary-ota-v1\"]'::jsonb,emissary_current_version='v-old',emissary_launcher_version='v-old',draining=true", params![]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE node_pools SET emissary_target_release=$1",
        params![release],
    )
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        scheduler::tick(&database.state),
        scheduler::tick(&database.state)
    );
    a.unwrap();
    b.unwrap();
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM emissary_updates WHERE status='offered'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "pool concurrency must be one across racing schedulers"
    );
    let updates: Vec<Uuid> = sql::scalars(
        &mut database.pool.clone(),
        "SELECT node_id FROM emissary_updates",
        params![],
    )
    .await
    .unwrap();
    assert!(updates[0] == first || updates[0] == second);
    let attempts: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(attempts, 0);
    let drained: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM nodes WHERE draining",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(drained, 2, "OTA must preserve administrator drain");
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_management_rejects_operator_and_project_tokens() {
    let database = Database::with_tenant().await;
    let (project, _, _) = database.fixture().await;
    let user = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO users(id,username,password_hash,role) VALUES($1,'ota-operator','unused','operator')", params![user]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("ota-session"), user],
    )
    .await
    .unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO api_tokens(tenant_id,id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES('00000000-0000-0000-0000-000000000001',$1,$2,'scoped',$3,$4,'{read,build,release}',now()+interval '1 hour')", params![Uuid::new_v4(), user, crypto::hash("ota-token"), vec![project]]).await.unwrap();
    let router = api::router(database.state.clone());
    for (method, path, body) in [
        (
            "POST",
            "/api/v1/emissary-releases".to_string(),
            r#"{"tag":"v-next"}"#.to_string(),
        ),
        (
            "PATCH",
            format!("/api/v1/nodes/{}/emissary-update", Uuid::new_v4()),
            r#"{"release_id":null}"#.to_string(),
        ),
        (
            "PATCH",
            "/api/v1/pools/00000000-0000-0000-0000-000000000001/emissary-update".into(),
            r#"{"release_id":null}"#.into(),
        ),
        (
            "POST",
            format!("/api/v1/emissary-updates/{}/retry", Uuid::new_v4()),
            "{}".into(),
        ),
        (
            "POST",
            format!("/api/v1/emissary-updates/{}/cancel", Uuid::new_v4()),
            "{}".into(),
        ),
    ] {
        for token in [false, true] {
            let mut request = Request::builder()
                .method(method)
                .uri(&path)
                .header("content-type", "application/json")
                .header("origin", "http://localhost:3000")
                .header("x-artifactory-csrf", "1");
            request = if token {
                request.header("authorization", "Bearer ota-token")
            } else {
                request
                    .header(
                        "X-Artifactory-Tenant",
                        "00000000-0000-0000-0000-000000000001",
                    )
                    .header("cookie", "artifactory_session=ota-session")
            };
            let status = router
                .clone()
                .oneshot(request.body(Body::from(body.clone())).unwrap())
                .await
                .unwrap()
                .status();
            assert!(
                [StatusCode::FORBIDDEN, StatusCode::NOT_FOUND].contains(&status),
                "{method} {path} token={token}"
            );
        }
    }
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_failed_activation_pauses_pool_and_fences_stale_sessions() {
    let database = Database::with_tenant().await;
    let node = database.node().await;
    let release = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES('00000000-0000-0000-0000-000000000001',$1,'v-next',$2,'test','hash')", params![release, serde_json::json!({"assets":[{"os":"linux","arch":"amd64","name":"artifactory-emissary-linux-amd64"}]}).to_string()]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "UPDATE nodes SET capabilities=capabilities || '[\"emissary-ota-v1\"]'::jsonb,emissary_current_version='v-old',emissary_target_release=$1,draining=true WHERE id=$2", params![release, node]).await.unwrap();
    scheduler::tick(&database.state).await.unwrap();
    let id: Uuid = sql::scalar(
        &mut database.pool.clone(),
        "SELECT id FROM emissary_updates WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    let session = database.state.connections.read().await[&node].session;
    let payload = serde_json::json!({"diagnostics":{"emissary_version":"v-old"},"ota":{"update_id":id,"phase":"rolled_back","launcher_version":"v-old","error":"candidate crashed"}});
    assert!(
        artifactory_control_plane::ota::report(&database.state, node, Uuid::new_v4(), &payload)
            .await
            .is_err()
    );
    let status: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT status FROM emissary_updates WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(status, "offered");
    let preparing = serde_json::json!({"diagnostics":{"emissary_version":"v-old"},"ota":{"update_id":id,"phase":"activating","activation_pending":true,"launcher_version":"v-old"}});
    assert!(
        artifactory_control_plane::ota::report(&database.state, node, session, &preparing)
            .await
            .unwrap(),
        "old runtime must receive acknowledgment before yielding to a candidate"
    );
    assert!(
        artifactory_control_plane::ota::report(&database.state, node, session, &payload)
            .await
            .unwrap()
    );
    let flags:(bool,bool,bool)=sql::tuple(&mut database.pool.clone(), "SELECT n.draining,n.ota_draining,p.emissary_updates_paused FROM nodes n JOIN node_pools p ON p.id=n.pool_id WHERE n.id=$1", params![node]).await.unwrap();
    assert_eq!(flags, (true, false, true));
    scheduler::tick(&database.state).await.unwrap();
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM emissary_updates",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 1, "failed release must never loop automatically");
    // Revocation and certificate expiry are enforced within the status transaction.
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE nodes SET revoked=true WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert!(
        artifactory_control_plane::ota::report(&database.state, node, session, &payload)
            .await
            .is_err()
    );
    sql::execute(&mut database.pool.clone(), "UPDATE nodes SET revoked=false,certificate_expires_at=now()-interval '1 minute' WHERE id=$1", params![node]).await.unwrap();
    assert!(
        artifactory_control_plane::ota::report(&database.state, node, session, &payload)
            .await
            .is_err()
    );
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_retry_after_pool_move_respects_new_pool_capacity() {
    let database = Database::with_tenant().await;
    let node = database.node().await;
    let other = database.node().await;
    let release = Uuid::new_v4();
    let prior = Uuid::new_v4();
    let new_pool = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES('00000000-0000-0000-0000-000000000001',$1,'v-next','{}','test','hash')", params![release]).await.unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status,finished_at) VALUES($1,$2,'00000000-0000-0000-0000-000000000001',$3,'failed',now())", params![prior, node, release]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO node_pools(tenant_id,id,name) VALUES('00000000-0000-0000-0000-000000000001',$1,'new pool')",
        params![new_pool],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE nodes SET pool_id=$1",
        params![new_pool],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO emissary_updates(id,node_id,pool_id,release_id) VALUES($1,$2,$3,$4)",
        params![Uuid::new_v4(), other, new_pool, release],
    )
    .await
    .unwrap();
    let user = Uuid::new_v4();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,'admin','unused','admin')",
        params![user],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("admin-session"), user],
    )
    .await
    .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/emissary-updates/{prior}/retry"))
        .header("content-type", "application/json")
        .header(
            "X-Artifactory-Tenant",
            "00000000-0000-0000-0000-000000000001",
        )
        .header("cookie", "artifactory_session=admin-session")
        .header("x-artifactory-csrf", "1")
        .header("origin", "http://localhost:3000")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        api::router(database.state.clone())
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn unchanged_terminal_builds_do_not_generate_new_row_versions() {
    let database = Database::with_tenant().await;
    let (_, build, job) = database.fixture().await;
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE jobs SET status='succeeded' WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    scheduler::refresh_builds(&database.state).await.unwrap();
    let before: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT ctid::text FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    scheduler::refresh_builds(&database.state).await.unwrap();
    let after: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT ctid::text FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert_eq!(
        before, after,
        "an unchanged terminal build must not be rewritten"
    );
    database.close().await;
}
