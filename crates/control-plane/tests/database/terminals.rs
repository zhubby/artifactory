//! [INPUT]: 依赖隔离数据库、登录会话与当前设备启动身份。
//! [OUTPUT]: 验证终端原子准入、原 Session 归属和一次性票据。
//! [POS]: 终端真实 PostgreSQL/HTTP 权限回归，不使用构建授权替代 Shell 权限。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

async fn ready_device(db: &Database, tenant: Uuid) -> Uuid {
    let node = device(db, tenant).await;
    sql::execute(&mut db.pool.clone(),"UPDATE nodes SET startup_id=$2,capabilities='[\"terminal-v1\"]',metadata=$3 WHERE id=$1",params![node,Uuid::new_v4(),json!({"emissary.terminal":{"enabled":true,"profiles":[{"id":"default","label":"Shell"}],"account":"emissary"}})]).await.unwrap();
    node
}

fn create_input() -> Value {
    json!({"idempotency_key":Uuid::new_v4(),"renderer_id":Uuid::new_v4(),"profile":"default","columns":80,"rows":24,"term":"xterm-256color"})
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminals_are_device_scoped_idempotent_and_tickets_are_single_use() {
    let db = Database::new().await;
    let (owner, cookie) = account(&db, "terminal-owner", "viewer").await;
    let tenant = tenant(&db, &cookie, "Terminals").await;
    let node = ready_device(&db, tenant).await;
    let input = create_input();
    let path = format!("/nodes/{node}/terminal-sessions");
    let (status, first) = request(&db, &cookie, Some(tenant), "POST", &path, input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (_, again) = request(&db, &cookie, Some(tenant), "POST", &path, input.clone()).await;
    assert_eq!(first["id"], again["id"]);
    assert!(first.get("credential_hash").is_none());
    let id = Uuid::parse_str(first["id"].as_str().unwrap()).unwrap();
    let renderer = Uuid::parse_str(input["renderer_id"].as_str().unwrap()).unwrap();
    let (status, attach) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &format!("/terminal-sessions/{id}/attach"),
        json!({"renderer_id":renderer,"offset":0}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{attach}");
    let ticket = attach["ticket"].as_str().unwrap();
    assert_eq!(
        request(
            &db,
            &cookie,
            Some(tenant),
            "POST",
            &format!("/terminal-sessions/{id}/attach"),
            json!({"renderer_id":Uuid::new_v4(),"offset":0})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let credential = crypto::hash(cookie.split('=').nth(1).unwrap());
    let consumed = db
        .state
        .store
        .consume_terminal_ticket(id, owner, &credential, renderer, ticket)
        .await
        .unwrap();
    assert_eq!(consumed.generation, 1);
    assert!(
        db.state
            .store
            .consume_terminal_ticket(id, owner, &credential, renderer, ticket)
            .await
            .is_err()
    );
    let (status, _) = request(&db, &cookie, Some(tenant), "POST", &path, create_input()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        request(&db, &cookie, Some(tenant), "POST", &path, create_input())
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let audits: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM audit_events WHERE action='terminal_created'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(audits, 2);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_concurrent_admission_enforces_the_persisted_limit() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "terminal-concurrency", "viewer").await;
    let tenant = tenant(&db, &cookie, "Terminal concurrency").await;
    let node = ready_device(&db, tenant).await;
    let path = format!("/nodes/{node}/terminal-sessions");
    let (a, b, c) = tokio::join!(
        request(&db, &cookie, Some(tenant), "POST", &path, create_input()),
        request(&db, &cookie, Some(tenant), "POST", &path, create_input()),
        request(&db, &cookie, Some(tenant), "POST", &path, create_input()),
    );
    let statuses = [a.0, b.0, c.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        2
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::TOO_MANY_REQUESTS)
            .count(),
        1
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn device_authority_does_not_allow_stealing_another_session() {
    let db = Database::new().await;
    let (_, alice) = account(&db, "terminal-alice", "viewer").await;
    let (bob_id, bob) = account(&db, "terminal-bob", "admin").await;
    let tenant = tenant(&db, &alice, "Terminal authority").await;
    request(
        &db,
        &alice,
        Some(tenant),
        "POST",
        "/tenant/members",
        json!({"username":"terminal-bob","role":"admin"}),
    )
    .await;
    let node = ready_device(&db, tenant).await;
    let input = create_input();
    let (_, created) = request(
        &db,
        &alice,
        Some(tenant),
        "POST",
        &format!("/nodes/{node}/terminal-sessions"),
        input.clone(),
    )
    .await;
    let id = created["id"].as_str().unwrap();
    for (method, path, body) in [
        ("GET", format!("/terminal-sessions/{id}"), json!({})),
        (
            "POST",
            format!("/terminal-sessions/{id}/attach"),
            json!({"renderer_id":input["renderer_id"],"offset":0}),
        ),
        ("POST", format!("/terminal-sessions/{id}/close"), json!({})),
    ] {
        assert_eq!(
            request(&db, &bob, Some(tenant), method, &path, body)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE tenant_members SET role='member' WHERE tenant_id=$1 AND user_id=$2",
        params![tenant, bob_id],
    )
    .await
    .unwrap();
    assert_eq!(
        request(
            &db,
            &bob,
            Some(tenant),
            "POST",
            &format!("/nodes/{node}/terminal-sessions"),
            create_input()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, new_session) = account(&db, "terminal-other", "viewer").await;
    let raw = new_session.split('=').nth(1).unwrap();
    let alice_id: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM users WHERE username='terminal-alice'",
        params![],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE sessions SET user_id=$2 WHERE token_hash=$1",
        params![crypto::hash(raw), alice_id],
    )
    .await
    .unwrap();
    assert_eq!(
        request(
            &db,
            &new_session,
            Some(tenant),
            "GET",
            &format!("/terminal-sessions/{id}"),
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_revalidation_fences_revocation_restart_and_closed_tenants() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "terminal-fences", "viewer").await;
    let tenant = tenant(&db, &cookie, "Terminal fences").await;
    let node = ready_device(&db, tenant).await;
    let (_, created) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &format!("/nodes/{node}/terminal-sessions"),
        create_input(),
    )
    .await;
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    assert!(db.state.store.terminal_authorized(id).await.unwrap());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET revoked=true WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert!(!db.state.store.terminal_authorized(id).await.unwrap());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET revoked=false,active_session=$2 WHERE id=$1",
        params![node, Uuid::new_v4()],
    )
    .await
    .unwrap();
    assert!(!db.state.store.terminal_authorized(id).await.unwrap());
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_tail_drain_requires_normal_exit_and_current_authority() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "terminal-tail-permission", "viewer").await;
    let tenant = tenant(&db, &cookie, "Terminal tail permission").await;
    let node = ready_device(&db, tenant).await;
    for (reason, allowed) in [("user_closed", false), ("shell_exited", true)] {
        let (status, created) = request(
            &db,
            &cookie,
            Some(tenant),
            "POST",
            &format!("/nodes/{node}/terminal-sessions"),
            create_input(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
        db.state
            .store
            .close_terminal(id, reason, Some(7), None)
            .await
            .unwrap();
        assert!(!db.state.store.terminal_authorized(id).await.unwrap());
        assert_eq!(
            db.state.store.terminal_access_valid(id).await.unwrap(),
            allowed
        );
        if allowed {
            sql::execute(
                &mut db.pool.clone(),
                "DELETE FROM sessions WHERE token_hash=$1",
                params![crypto::hash(cookie.split('=').nth(1).unwrap())],
            )
            .await
            .unwrap();
            assert!(!db.state.store.terminal_access_valid(id).await.unwrap());
        }
    }
    db.close().await;
}
