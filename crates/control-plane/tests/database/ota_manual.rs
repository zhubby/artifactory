//! [INPUT]: 依赖隔离 PostgreSQL、真实调度入口与租户 HTTP 会话。
//! [OUTPUT]: 验证冻结确认、显式授权、幂等和锁后准入。
//! [POS]: 手动 OTA 的数据库行为回归；不启动设备进程。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::{Value, json};

const TENANT: Uuid = Uuid::from_u128(1);

async fn release(db: &Database, version: &str) -> Uuid {
    let id = Uuid::new_v4();
    let manifest = json!({"version":version,"assets":[{"os":"linux","arch":"amd64","name":"artifactory-emissary-linux-amd64"}]}).to_string();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES($1,$2,$3,$4,'test',$5)", params![TENANT,id,version,manifest,id.to_string()]).await.unwrap();
    id
}

async fn enabled_node(db: &Database) -> Uuid {
    let node = db.node().await;
    sql::execute(&mut db.pool.clone(), "UPDATE nodes SET capabilities=capabilities || '[\"emissary-ota-v1\"]'::jsonb,emissary_current_version='0.1.0' WHERE id=$1", params![node]).await.unwrap();
    node
}

async fn account(db: &Database, role: &str) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let token = Uuid::new_v4().to_string();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused',$3)",
        params![id, format!("manual-{id}"), role],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash(&token), id],
    )
    .await
    .unwrap();
    (id, format!("artifactory_session={token}"))
}

async fn send(db: &Database, builder: axum::http::request::Builder) -> (StatusCode, Value) {
    let node_request = builder.uri_ref().is_some_and(|uri| {
        uri.path().starts_with("/api/v1/nodes/") && uri.path().ends_with("/emissary-update")
    });
    let body = if node_request {
        Some(confirmation(db).await)
    } else {
        None
    };
    send_body(db, builder, body).await
}

async fn confirmation(db: &Database) -> Value {
    let targets: Vec<Uuid> = sql::scalars(
        &mut db.pool.clone(),
        "SELECT id FROM emissary_releases WHERE tenant_id=$1 AND version=ANY($2) ORDER BY version",
        params![
            TENANT,
            vec![
                artifactory_control_plane::VERSION.to_owned(),
                format!("v{}", artifactory_control_plane::VERSION),
                format!("emissary-v{}", artifactory_control_plane::VERSION)
            ]
        ],
    )
    .await
    .unwrap();
    json!({"product_version":artifactory_control_plane::VERSION,"release_id":targets.first().copied().unwrap_or_else(Uuid::new_v4)})
}

async fn send_body(
    db: &Database,
    mut builder: axum::http::request::Builder,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = api::router(db.state.clone())
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn authorized(cookie: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1{path}"))
        .header("cookie", cookie)
        .header("x-artifactory-tenant", TENANT.to_string())
        .header("x-artifactory-csrf", "1")
        .header("origin", "http://localhost:3000")
}

async fn request(db: &Database, cookie: &str, node: Uuid) -> (StatusCode, Value) {
    send(
        db,
        authorized(cookie, &format!("/nodes/{node}/emissary-update")),
    )
    .await
}

async fn counters(db: &Database) -> (i64, i64) {
    sql::tuple(&mut db.pool.clone(), "SELECT (SELECT count(*) FROM emissary_updates),(SELECT count(*) FROM nodes WHERE ota_draining)", params![]).await.unwrap()
}

async fn reset_node(db: &Database, node: Uuid) {
    sql::execute(&mut db.pool.clone(), "UPDATE nodes SET last_seen=clock_timestamp(),certificate_expires_at=clock_timestamp()+interval '1 day',revoked=false,capabilities='[\"emissary-ota-v1\"]',emissary_current_version='0.1.0' WHERE id=$1", params![node]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE node_pools SET emissary_updates_paused=false WHERE id=$1",
        params![TENANT],
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_scheduler_catalog_and_reconnect_never_authorize_an_update() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    for _ in 0..3 {
        scheduler::tick(&db.state).await.unwrap();
    }
    assert_eq!(counters(&db).await, (0, 0));
    release(&db, artifactory_control_plane::VERSION).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE node_pools SET emissary_updates_paused=true WHERE id=$1",
        params![TENANT],
    )
    .await
    .unwrap();
    scheduler::tick(&db.state).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE node_pools SET emissary_updates_paused=false WHERE id=$1",
        params![TENANT],
    )
    .await
    .unwrap();
    let session = Uuid::new_v4();
    let (sender, _receiver) = mpsc::channel::<Result<Command, tonic::Status>>(128);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET active_session=$2,last_seen=clock_timestamp() WHERE id=$1",
        params![node, session],
    )
    .await
    .unwrap();
    db.state
        .connections
        .write()
        .await
        .insert(node, Connection { session, sender });
    for _ in 0..3 {
        scheduler::tick(&db.state).await.unwrap();
        assert!(
            artifactory_control_plane::ota::next(&db.state, node, session)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        counters(&db).await,
        (0, 0),
        "catalog availability, a persisted reconnect and pool resume grant no upgrade authority"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_parallel_requests_share_one_update_and_one_audit() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let target = release(&db, artifactory_control_plane::VERSION).await;
    let (actor, cookie) = account(&db, "admin").await;
    let (first, second) = tokio::join!(request(&db, &cookie, node), request(&db, &cookie, node));
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    assert_eq!(first.1["id"], second.1["id"]);
    let id = Uuid::parse_str(first.1["id"].as_str().unwrap()).unwrap();
    assert_eq!(counters(&db).await, (1, 1));
    let stored: (Uuid, Uuid) = sql::tuple(
        &mut db.pool.clone(),
        "SELECT node_id,release_id FROM emissary_updates WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(stored, (node, target));
    let audits: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM audit_events WHERE action='emissary_update_requested' AND resource_id=$1 AND actor=$2", params![id.to_string(),actor.to_string()]).await.unwrap();
    assert_eq!(audits, 1);
    let repeat = request(&db, &cookie, node).await;
    assert_eq!(repeat.0, StatusCode::OK, "{}", repeat.1);
    assert_eq!(repeat.1["id"], id.to_string());
    assert_eq!(counters(&db).await, (1, 1));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_rejects_unavailable_nodes_and_targets_without_record_or_drain() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let (_, cookie) = account(&db, "admin").await;
    let missing = request(&db, &cookie, node).await;
    assert_eq!(missing.0, StatusCode::CONFLICT, "{}", missing.1);
    assert_eq!(counters(&db).await, (0, 0));
    release(&db, artifactory_control_plane::VERSION).await;
    let alias = release(&db, &format!("v{}", artifactory_control_plane::VERSION)).await;
    let ambiguous = request(&db, &cookie, node).await;
    assert_eq!(ambiguous.0, StatusCode::CONFLICT, "{}", ambiguous.1);
    assert_eq!(counters(&db).await, (0, 0));
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM emissary_releases WHERE id=$1",
        params![alias],
    )
    .await
    .unwrap();
    for (reason, query) in [
        (
            "offline",
            "UPDATE nodes SET last_seen=clock_timestamp()-interval '1 minute' WHERE id=$1",
        ),
        (
            "expired",
            "UPDATE nodes SET certificate_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        ),
        ("revoked", "UPDATE nodes SET revoked=true WHERE id=$1"),
        (
            "no OTA capability",
            "UPDATE nodes SET capabilities='[]' WHERE id=$1",
        ),
        (
            "paused",
            "UPDATE node_pools SET emissary_updates_paused=true WHERE id=$1",
        ),
    ] {
        reset_node(&db, node).await;
        sql::execute(
            &mut db.pool.clone(),
            query,
            params![if reason == "paused" { TENANT } else { node }],
        )
        .await
        .unwrap();
        let result = request(&db, &cookie, node).await;
        assert_eq!(result.0, StatusCode::CONFLICT, "{reason}: {}", result.1);
        assert_eq!(counters(&db).await, (0, 0), "{reason}");
    }
    for version in [
        artifactory_control_plane::VERSION.to_owned(),
        format!("{}+terminal.abc", artifactory_control_plane::VERSION),
        format!(
            "emissary-v{}+terminal.abc",
            artifactory_control_plane::VERSION
        ),
    ] {
        reset_node(&db, node).await;
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE nodes SET emissary_current_version=$2 WHERE id=$1",
            params![node, &version],
        )
        .await
        .unwrap();
        let aligned = request(&db, &cookie, node).await;
        assert_eq!(aligned.0, StatusCode::CONFLICT, "{version}: {}", aligned.1);
        assert_eq!(counters(&db).await, (0, 0));
    }
    let audits: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM audit_events WHERE action='emissary_update_requested'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(audits, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_pool_capacity_rejects_a_second_node_without_fake_queue() {
    let db = Database::with_tenant().await;
    let first = enabled_node(&db).await;
    let second = enabled_node(&db).await;
    release(&db, artifactory_control_plane::VERSION).await;
    let (_, cookie) = account(&db, "admin").await;
    assert_eq!(request(&db, &cookie, first).await.0, StatusCode::OK);
    let full = request(&db, &cookie, second).await;
    assert_eq!(full.0, StatusCode::CONFLICT, "{}", full.1);
    assert_eq!(counters(&db).await, (1, 1));
    let untouched: bool = sql::scalar(&mut db.pool.clone(), "SELECT NOT ota_draining AND NOT EXISTS(SELECT 1 FROM emissary_updates WHERE node_id=$1) FROM nodes WHERE id=$1", params![second]).await.unwrap();
    assert!(untouched);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_preserves_an_active_old_target_and_rejects_replacement() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let old = release(&db, "0.1.0").await;
    release(&db, artifactory_control_plane::VERSION).await;
    let existing = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status) VALUES($1,$2,$3,$4,'downloading')", params![existing,node,TENANT,old]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET ota_draining=true WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let (_, cookie) = account(&db, "admin").await;
    let result = request(&db, &cookie, node).await;
    assert_eq!(result.0, StatusCode::CONFLICT, "{}", result.1);
    assert_eq!(counters(&db).await, (1, 1));
    let frozen: (Uuid, String) = sql::tuple(
        &mut db.pool.clone(),
        "SELECT release_id,status FROM emissary_updates WHERE id=$1",
        params![existing],
    )
    .await
    .unwrap();
    assert_eq!(frozen, (old, "downloading".into()));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_requires_admin_session_tenant_and_csrf_origin() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    release(&db, artifactory_control_plane::VERSION).await;
    let (actor, cookie) = account(&db, "admin").await;
    let (_, member) = account(&db, "viewer").await;
    assert_eq!(request(&db, &member, node).await.0, StatusCode::FORBIDDEN);
    let path = format!("/api/v1/nodes/{node}/emissary-update");
    for (csrf, origin) in [
        (false, "http://localhost:3000"),
        (true, "https://other.invalid"),
    ] {
        let mut builder = Request::builder()
            .method("POST")
            .uri(&path)
            .header("cookie", &cookie)
            .header("x-artifactory-tenant", TENANT.to_string())
            .header("origin", origin);
        if csrf {
            builder = builder.header("x-artifactory-csrf", "1");
        }
        assert_eq!(send(&db, builder).await.0, StatusCode::FORBIDDEN);
    }
    let no_tenant = Request::builder()
        .method("POST")
        .uri(&path)
        .header("cookie", &cookie)
        .header("x-artifactory-csrf", "1")
        .header("origin", "http://localhost:3000");
    assert_eq!(send(&db, no_tenant).await.0, StatusCode::BAD_REQUEST);
    let foreign = Uuid::new_v4();
    let mut connection = db.pool.clone();
    let mut transaction = connection.transaction().await.unwrap();
    sql::execute(&mut transaction, "INSERT INTO tenants(id,name,owner_id,created_by,creation_key,slug) VALUES($1,'Foreign',$2,$2,$3,$4)", params![foreign,actor,Uuid::new_v4(),format!("foreign-{}",foreign.simple())]).await.unwrap();
    sql::execute(
        &mut transaction,
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'owner')",
        params![foreign, actor],
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let cross_tenant = Request::builder()
        .method("POST")
        .uri(&path)
        .header("cookie", &cookie)
        .header("x-artifactory-tenant", foreign.to_string())
        .header("x-artifactory-csrf", "1")
        .header("origin", "http://localhost:3000");
    assert_eq!(send(&db, cross_tenant).await.0, StatusCode::NOT_FOUND);
    let (project, _, _) = db.fixture().await;
    let token = Uuid::new_v4().to_string();
    sql::execute(&mut db.pool.clone(), "INSERT INTO api_tokens(tenant_id,id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES($1,$2,$3,'manual-test',$4,$5,'{read,build,release}',now()+interval '1 hour')", params![TENANT,Uuid::new_v4(),actor,crypto::hash(&token),vec![project]]).await.unwrap();
    let token_request = Request::builder()
        .method("POST")
        .uri(&path)
        .header("authorization", format!("Bearer {token}"));
    assert_eq!(send(&db, token_request).await.0, StatusCode::FORBIDDEN);
    assert_eq!(counters(&db).await, (0, 0));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_retry_obeys_pause_and_node_admission_without_resuming_pool() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let target = release(&db, artifactory_control_plane::VERSION).await;
    let prior = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status,finished_at) VALUES($1,$2,$3,$4,'failed',now())", params![prior,node,TENANT,target]).await.unwrap();
    let (_, cookie) = account(&db, "admin").await;
    let path = format!("/emissary-updates/{prior}/retry");
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE node_pools SET emissary_updates_paused=true WHERE id=$1",
        params![TENANT],
    )
    .await
    .unwrap();
    let paused = send(&db, authorized(&cookie, &path)).await;
    assert_eq!(paused.0, StatusCode::CONFLICT, "{}", paused.1);
    let remains_paused: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT emissary_updates_paused FROM node_pools WHERE id=$1",
        params![TENANT],
    )
    .await
    .unwrap();
    assert!(remains_paused);
    for (reason, query) in [
        (
            "offline",
            "UPDATE nodes SET last_seen=clock_timestamp()-interval '1 minute' WHERE id=$1",
        ),
        (
            "expired",
            "UPDATE nodes SET certificate_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        ),
        ("revoked", "UPDATE nodes SET revoked=true WHERE id=$1"),
        (
            "no capability",
            "UPDATE nodes SET capabilities='[]' WHERE id=$1",
        ),
    ] {
        reset_node(&db, node).await;
        sql::execute(&mut db.pool.clone(), query, params![node])
            .await
            .unwrap();
        let rejected = send(&db, authorized(&cookie, &path)).await;
        assert_eq!(rejected.0, StatusCode::CONFLICT, "{reason}: {}", rejected.1);
        assert_eq!(counters(&db).await, (1, 0));
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_rejects_a_stale_confirmation_without_record_or_drain() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let target = release(&db, artifactory_control_plane::VERSION).await;
    let old = release(&db, "0.1.0").await;
    let (_, cookie) = account(&db, "admin").await;
    let path = format!("/nodes/{node}/emissary-update");
    for body in [
        json!({"product_version":"0.1.0","release_id":target}),
        json!({"product_version":artifactory_control_plane::VERSION,"release_id":old}),
        json!({"product_version":artifactory_control_plane::VERSION,"release_id":Uuid::new_v4()}),
    ] {
        let rejected = send_body(&db, authorized(&cookie, &path), Some(body)).await;
        assert_eq!(rejected.0, StatusCode::CONFLICT, "{}", rejected.1);
        assert_eq!(counters(&db).await, (0, 0));
    }
    let accepted = send_body(
        &db,
        authorized(&cookie, &path),
        Some(json!({"product_version":artifactory_control_plane::VERSION,"release_id":target})),
    )
    .await;
    assert_eq!(accepted.0, StatusCode::OK, "{}", accepted.1);
    assert_eq!(counters(&db).await, (1, 1));
    db.close().await;
}

async fn wait_until_blocked(db: &Database, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sql::scalar(
                &mut db.pool.clone(),
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
                params![blocker],
            )
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("upgrade request must reach the held row lock before expiration");
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_rechecks_certificate_time_after_node_lock_wait() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    release(&db, artifactory_control_plane::VERSION).await;
    let (_, cookie) = account(&db, "admin").await;
    let mut connection = db.pool.clone();
    let mut lock = connection.transaction().await.unwrap();
    let blocker: i32 = sql::scalar(&mut lock, "SELECT pg_backend_pid()", params![])
        .await
        .unwrap();
    sql::execute(
        &mut lock,
        "SELECT id FROM nodes WHERE id=$1 FOR UPDATE",
        params![node],
    )
    .await
    .unwrap();
    let pending = request(&db, &cookie, node);
    let expire = async {
        wait_until_blocked(&db, blocker).await;
        sql::execute(&mut lock, "UPDATE nodes SET name=name,certificate_expires_at=clock_timestamp()+interval '200 milliseconds' WHERE id=$1", params![node]).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        lock.commit().await.unwrap();
    };
    let (rejected, ()) = tokio::join!(pending, expire);
    assert_eq!(rejected.0, StatusCode::CONFLICT, "{}", rejected.1);
    assert_eq!(counters(&db).await, (0, 0));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_rechecks_login_session_time_after_node_lock_wait() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    release(&db, artifactory_control_plane::VERSION).await;
    let (_, cookie) = account(&db, "admin").await;
    let token_hash = crypto::hash(cookie.strip_prefix("artifactory_session=").unwrap());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE sessions SET expires_at=clock_timestamp()+interval '3 seconds' WHERE token_hash=$1",
        params![&token_hash],
    )
    .await
    .unwrap();
    let mut connection = db.pool.clone();
    let mut lock = connection.transaction().await.unwrap();
    let blocker: i32 = sql::scalar(&mut lock, "SELECT pg_backend_pid()", params![])
        .await
        .unwrap();
    sql::execute(
        &mut lock,
        "SELECT id FROM nodes WHERE id=$1 FOR UPDATE",
        params![node],
    )
    .await
    .unwrap();
    let pending = request(&db, &cookie, node);
    let expire = async {
        wait_until_blocked(&db, blocker).await;
        loop {
            let expired: bool = sql::scalar(
                &mut db.pool.clone(),
                "SELECT expires_at<=clock_timestamp() FROM sessions WHERE token_hash=$1",
                params![&token_hash],
            )
            .await
            .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        lock.commit().await.unwrap();
    };
    let (rejected, ()) = tokio::join!(pending, expire);
    assert!(
        [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN].contains(&rejected.0),
        "{}",
        rejected.1
    );
    assert_eq!(counters(&db).await, (0, 0));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_manual_requires_a_strict_confirmation_document() {
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let target = release(&db, artifactory_control_plane::VERSION).await;
    let (_, cookie) = account(&db, "admin").await;
    let path = format!("/nodes/{node}/emissary-update");
    for body in [
        None,
        Some(json!({})),
        Some(
            json!({"product_version":artifactory_control_plane::VERSION,"release_id":target,"shell":"ignored"}),
        ),
    ] {
        let rejected = send_body(&db, authorized(&cookie, &path), body).await;
        assert!(
            rejected.0.is_client_error(),
            "{}: {}",
            rejected.0,
            rejected.1
        );
        assert_eq!(counters(&db).await, (0, 0));
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_asset_renewal_is_bound_to_frozen_asset_generation_and_current_session() {
    use sha2::{Digest, Sha256};
    let db = Database::with_tenant().await;
    let node = enabled_node(&db).await;
    let session = db.state.connections.read().await[&node].session;
    let release = Uuid::new_v4();
    let update = Uuid::new_v4();
    let asset_sha = "a".repeat(64);
    let manifest=json!({"version":artifactory_control_plane::VERSION,"commit":"b".repeat(40),"protocol_min":1,"protocol_max":1,"assets":[{"os":"linux","arch":"amd64","name":"artifactory-emissary-linux-amd64","size":10,"sha256":asset_sha}]}).to_string();
    let hash = hex::encode(Sha256::digest(manifest.as_bytes()));
    sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES($1,$2,$3,$4,'test',$5)",params![TENANT,release,artifactory_control_plane::VERSION,manifest,&hash]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_updates(id,node_id,pool_id,release_id,generation,status) VALUES($1,$2,$3,$4,42,'downloading')",params![update,node,TENANT,release]).await.unwrap();
    let input = artifactory_control_plane::protocol::EmissaryAssetRenewRequest {
        session_id: session.to_string(),
        request_id: Uuid::new_v4().to_string(),
        update_id: update.to_string(),
        generation: 42,
        manifest_sha256: hash,
        asset_sha256: asset_sha,
        asset_size: 10,
        os: "linux".into(),
        arch: "amd64".into(),
    };
    let renew =
        |input| artifactory_control_plane::ota::renew_frozen_asset(&db.state, node, session, input);
    assert!(renew(&input).await.is_ok());
    let progress = |sequence, done| json!({"diagnostics":{"emissary_version":"0.2.2"},"ota":{"update_id":update,"generation":42,"phase":"downloading","launcher_version":"0.2.3","observability":{"schema_version":1,"sequence":sequence,"launcher_version":"0.2.3","launcher_capabilities":["ota-progress-v1"],"progress":{"update_id":update,"generation":42,"asset_sha256":"a".repeat(64),"sequence":sequence,"subphase":"download","bytes_done":done,"bytes_total":10,"download_attempt":1,"last_progress_at":chrono::Utc::now(),"updated_at":chrono::Utc::now(),"error_code":null},"drain":null,"last_progress_at":chrono::Utc::now(),"stalled":false}}});
    artifactory_control_plane::ota::report(&db.state, node, session, &progress(2, 8))
        .await
        .unwrap();
    artifactory_control_plane::ota::report(&db.state, node, session, &progress(1, 3))
        .await
        .unwrap();
    let stored: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT emissary_ota_status FROM nodes WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(stored["observability"]["progress"]["bytes_done"], 8);
    let mut wrong_generation = progress(3, 9);
    wrong_generation["ota"]["generation"] = json!(41);
    assert!(
        artifactory_control_plane::ota::report(&db.state, node, session, &wrong_generation)
            .await
            .is_err()
    );
    let throttled = json!({"diagnostics":{"emissary_version":"0.2.2"},"ota":{"update_id":update,"generation":42,"phase":"downloading","launcher_version":"0.2.3"}});
    artifactory_control_plane::ota::report(&db.state, node, session, &throttled)
        .await
        .unwrap();
    let stored: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT emissary_ota_status FROM nodes WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(stored["observability"]["progress"]["sequence"], 2);
    let mut wrong = input.clone();
    wrong.generation = 41;
    assert!(renew(&wrong).await.is_err());
    let mut wrong = input.clone();
    wrong.asset_sha256 = "c".repeat(64);
    assert!(renew(&wrong).await.is_err());
    assert!(
        artifactory_control_plane::ota::renew_frozen_asset(&db.state, node, Uuid::new_v4(), &input)
            .await
            .is_err()
    );
    for change in ["revoked", "session", "cancel", "certificate"] {
        sql::execute(&mut db.pool.clone(), "UPDATE nodes SET revoked=false,active_session=$2,certificate_expires_at=clock_timestamp()+interval '1 day',last_seen=clock_timestamp() WHERE id=$1", params![node,session]).await.unwrap();
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE emissary_updates SET cancel_requested=false WHERE id=$1",
            params![update],
        )
        .await
        .unwrap();
        let mut connection = db.pool.clone();
        let mut holder = connection.transaction().await.unwrap();
        sql::execute(
            &mut holder,
            "SELECT id FROM nodes WHERE id=$1 FOR UPDATE",
            params![node],
        )
        .await
        .unwrap();
        let pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![])
            .await
            .unwrap();
        let state = db.state.clone();
        let request = input.clone();
        let waiting = tokio::spawn(async move {
            artifactory_control_plane::ota::renew_frozen_asset(&state, node, session, &request)
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3),async {
            loop {
                let blocked:bool=sql::scalar(&mut db.pool.clone(),"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",params![pid]).await.unwrap();
                if blocked {break;}
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        match change {
            "revoked" => {
                sql::execute(
                    &mut holder,
                    "UPDATE nodes SET revoked=true WHERE id=$1",
                    params![node],
                )
                .await
                .unwrap();
            }
            "session" => {
                sql::execute(
                    &mut holder,
                    "UPDATE nodes SET active_session=$2 WHERE id=$1",
                    params![node, Uuid::new_v4()],
                )
                .await
                .unwrap();
            }
            "certificate" => {
                sql::execute(&mut holder,"UPDATE nodes SET certificate_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",params![node]).await.unwrap();
            }
            _ => {
                sql::execute(
                    &mut holder,
                    "UPDATE emissary_updates SET cancel_requested=true WHERE id=$1",
                    params![update],
                )
                .await
                .unwrap();
            }
        }
        holder.commit().await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            "{change} while node lock waited must reject renewal"
        );
    }
    sql::execute(&mut db.pool.clone(), "UPDATE nodes SET revoked=false,active_session=$2,certificate_expires_at=clock_timestamp()+interval '1 day',last_seen=clock_timestamp() WHERE id=$1", params![node,session]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE emissary_updates SET cancel_requested=true WHERE id=$1",
        params![update],
    )
    .await
    .unwrap();
    assert!(renew(&input).await.is_err());
    sql::execute(&mut db.pool.clone(),"UPDATE emissary_updates SET cancel_requested=false,status='succeeded',finished_at=now() WHERE id=$1",params![update]).await.unwrap();
    assert!(renew(&input).await.is_err());
    db.close().await;
}
