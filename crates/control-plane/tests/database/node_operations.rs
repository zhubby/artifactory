//! [INPUT]: 依赖真实 PostgreSQL、当前账户 Session 与在线节点。
//! [OUTPUT]: 验证有限操作权限、幂等、节点锁和结果摘要确认。
//! [POS]: 设备只读操作 HTTP 与事务边界回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_emissary_protocol::operations::*;
async fn ready(db: &Database, tenant: Uuid) -> (Uuid, Uuid, Uuid) {
    let node = device(db, tenant).await;
    let startup = Uuid::new_v4();
    let session = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"UPDATE nodes SET startup_id=$2,active_session=$3,capabilities='[\"node-operations-v1\"]' WHERE id=$1",params![node,startup,session]).await.unwrap();
    (node, startup, session)
}
fn input() -> Value {
    json!({"request_id":Uuid::new_v4(),"kind":"collect_diagnostics","schema":1,"input":{"probes":["git"]}})
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn operation_admission_is_idempotent_and_current_admin_only() {
    let db = Database::new().await;
    let (_, owner_cookie) = account(&db, "operations-owner", "viewer").await;
    let tenant = tenant(&db, &owner_cookie, "Ops").await;
    let (owner, cookie) = account(&db, "operations-admin", "viewer").await;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'admin')",
        params![tenant, owner],
    )
    .await
    .unwrap();
    let (node, _, _) = ready(&db, tenant).await;
    let path = format!("/nodes/{node}/operations");
    let first_input = input();
    let (status, first) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &path,
        first_input.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (_, again) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &path,
        first_input.clone(),
    )
    .await;
    assert_eq!(first["id"], again["id"]);
    assert!(first.get("credential_hash").is_none());
    let mut conflict = first_input;
    conflict["input"] = json!({"probes":["cargo"]});
    assert_eq!(
        request(&db, &cookie, Some(tenant), "POST", &path, conflict)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&db, &cookie, Some(tenant), "POST", &path, input())
            .await
            .0,
        StatusCode::CONFLICT
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE tenant_members SET role='member' WHERE tenant_id=$1 AND user_id=$2",
        params![tenant, owner],
    )
    .await
    .unwrap();
    assert_eq!(
        request(&db, &cookie, Some(tenant), "GET", &path, json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let pending = db
        .state
        .store
        .pending_node_operations(db.state.coordination.id(), 20)
        .await
        .unwrap();
    assert!(pending.is_empty(), "revoked authority cannot dispatch");
    db.close().await;
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn terminal_result_requires_digest_and_commits_atomically() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "operations-result", "viewer").await;
    let tenant = tenant(&db, &cookie, "Ops result").await;
    let (node, startup, session) = ready(&db, tenant).await;
    let (_, created) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &format!("/nodes/{node}/operations"),
        input(),
    )
    .await;
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let mut result = OperationResult::default();
    result.probes.insert(
        "git".into(),
        ProbeReport {
            status: ProbeStatus::Unsupported,
            observed_at: chrono::Utc::now(),
            duration_ms: 1,
            probe_version: 1,
            version: None,
            error_code: None,
        },
    );
    let mut update = OperationUpdate {
        operation_id: id,
        startup_id: startup,
        sequence: 3,
        state: OperationState::Succeeded,
        reason: None,
        result: Some(result.clone()),
        result_digest: Some("bad".into()),
    };
    assert!(
        db.state
            .store
            .record_node_operation_update(node, session, &update)
            .await
            .is_err()
    );
    update.result_digest = Some(result.digest().unwrap());
    let ack = db
        .state
        .store
        .record_node_operation_update(node, session, &update)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ack.operation_id, id);
    let (_, detail) = request(
        &db,
        &cookie,
        Some(tenant),
        "GET",
        &format!("/node-operations/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(detail["state"], "succeeded");
    assert_eq!(detail["result"]["probes"]["git"]["status"], "unsupported");
    let duplicate = db
        .state
        .store
        .record_node_operation_update(node, session, &update)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(duplicate.result_digest, ack.result_digest);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn disconnected_outcome_is_unknown_and_late_terminal_still_commits() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "operations-late", "viewer").await;
    let tenant = tenant(&db, &cookie, "Ops late").await;
    let (node, startup, session) = ready(&db, tenant).await;
    let (_, created) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &format!("/nodes/{node}/operations"),
        input(),
    )
    .await;
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    sql::execute(&mut db.pool.clone(),"UPDATE node_operations SET execution_cutoff=clock_timestamp()-interval '6 minutes' WHERE id=$1",params![id]).await.unwrap();
    db.state.store.reconcile_node_operations().await.unwrap();
    let (_, unknown) = request(
        &db,
        &cookie,
        Some(tenant),
        "GET",
        &format!("/node-operations/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(unknown["state"], "requested");
    assert_eq!(unknown["delivery_state"], "outcome_unknown");
    assert_eq!(
        db.state.store.active_node_operations(node).await.unwrap(),
        0
    );
    let result = OperationResult::default();
    let update = OperationUpdate {
        operation_id: id,
        startup_id: startup,
        sequence: 2,
        state: OperationState::Interrupted,
        reason: Some(OperationReason::StartupChanged),
        result_digest: Some(result.digest().unwrap()),
        result: Some(result),
    };
    let new_session = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET startup_id=$2,active_session=$3 WHERE id=$1",
        params![node, Uuid::new_v4(), new_session],
    )
    .await
    .unwrap();
    assert!(
        db.state
            .store
            .record_node_operation_update(node, session, &update)
            .await
            .is_err()
    );
    assert!(
        db.state
            .store
            .record_node_operation_update(node, new_session, &update)
            .await
            .is_err()
    );
    let resumed = db
        .state
        .store
        .pending_node_operations(db.state.coordination.id(), 20)
        .await
        .unwrap();
    assert_eq!(resumed.len(), 1);
    assert!(!resumed[0].execute_allowed);
    assert_ne!(resumed[0].resume.current_startup_id, startup);
    assert!(
        db.state
            .store
            .record_node_operation_update(node, new_session, &update)
            .await
            .unwrap()
            .is_some()
    );
    sql::execute(&mut db.pool.clone(),"UPDATE node_operations SET result_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",params![id]).await.unwrap();
    db.state.store.reconcile_node_operations().await.unwrap();
    let (_, expired) = request(
        &db,
        &cookie,
        Some(tenant),
        "GET",
        &format!("/node-operations/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(expired["state"], "interrupted");
    assert!(expired["result"].is_null());
    assert!(expired["result_digest"].is_string());
    db.close().await;
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_drain_cancels_pending_and_rejects_new_operations() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "operations-drain", "viewer").await;
    let tenant = tenant(&db, &cookie, "Ops drain").await;
    let (node, _, _) = ready(&db, tenant).await;
    let path = format!("/nodes/{node}/operations");
    let (_, created) = request(&db, &cookie, Some(tenant), "POST", &path, input()).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET ota_draining=true WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let deliveries = db
        .state
        .store
        .pending_node_operations(db.state.coordination.id(), 20)
        .await
        .unwrap();
    assert_eq!(deliveries.len(), 1);
    assert!(deliveries[0].cancel_requested);
    assert_eq!(
        deliveries[0].envelope.operation_id.to_string(),
        created["id"].as_str().unwrap()
    );
    assert_eq!(
        request(&db, &cookie, Some(tenant), "POST", &path, input())
            .await
            .0,
        StatusCode::CONFLICT
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dispatch_sends_resume_before_work_and_only_to_current_session() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "operations-dispatch", "viewer").await;
    let tenant = tenant(&db, &cookie, "Ops dispatch").await;
    let (node, startup, session) = ready(&db, tenant).await;
    let (_, created) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &format!("/nodes/{node}/operations"),
        input(),
    )
    .await;
    let (sender, mut receive) = mpsc::channel(8);
    db.state
        .connections
        .write()
        .await
        .insert(node, Connection { session, sender });
    artifactory_control_plane::dispatch::deliver(&db.state)
        .await
        .unwrap();
    let resume = receive.try_recv().unwrap().unwrap();
    assert_eq!(resume.kind, "node_operation_resume");
    let work = receive.try_recv().unwrap().unwrap();
    assert_eq!(work.kind, "node_operation");
    let envelope: OperationEnvelope = serde_json::from_str(&work.payload_json).unwrap();
    assert_eq!(
        envelope.operation_id.to_string(),
        created["id"].as_str().unwrap()
    );
    assert_eq!(envelope.startup_id, startup);
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        request(
            &db,
            &cookie,
            Some(tenant),
            "POST",
            &format!("/node-operations/{id}/cancel"),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    artifactory_control_plane::dispatch::deliver(&db.state)
        .await
        .unwrap();
    assert_eq!(
        receive.try_recv().unwrap().unwrap().kind,
        "node_operation_resume"
    );
    assert_eq!(
        receive.try_recv().unwrap().unwrap().kind,
        "node_operation_cancel"
    );
    assert_eq!(receive.try_recv().unwrap().unwrap().kind, "node_operation");
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET active_session=$2,startup_id=$3 WHERE id=$1",
        params![node, Uuid::new_v4(), Uuid::new_v4()],
    )
    .await
    .unwrap();
    artifactory_control_plane::dispatch::deliver(&db.state)
        .await
        .unwrap();
    assert!(
        receive.try_recv().is_err(),
        "stale process connection cannot receive resume or work"
    );
    db.close().await;
}

async fn blocked(db: &Database, holder: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sql::scalar(
                &mut db.pool.clone(),
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
                params![holder],
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
    .expect("operation must wait on the held row");
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn result_rechecks_session_after_node_lock_wait() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "operations-session-fence", "viewer").await;
    let tenant = tenant(&db, &cookie, "Ops session fence").await;
    let (node, startup, session) = ready(&db, tenant).await;
    let (_, created) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        &format!("/nodes/{node}/operations"),
        input(),
    )
    .await;
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let result = OperationResult::default();
    let update = OperationUpdate {
        operation_id: id,
        startup_id: startup,
        sequence: 2,
        state: OperationState::Canceled,
        reason: Some(OperationReason::Canceled),
        result_digest: Some(result.digest().unwrap()),
        result: Some(result),
    };
    let mut connection = db.pool.clone();
    let mut held = connection.transaction().await.unwrap();
    let holder: i32 = sql::scalar(&mut held, "SELECT pg_backend_pid()", params![])
        .await
        .unwrap();
    let _: Uuid = sql::scalar(
        &mut held,
        "SELECT id FROM nodes WHERE id=$1 FOR UPDATE",
        params![node],
    )
    .await
    .unwrap();
    let store = db.state.store.clone();
    let pending = tokio::spawn(async move {
        store
            .record_node_operation_update(node, session, &update)
            .await
    });
    blocked(&db, holder).await;
    sql::execute(
        &mut held,
        "UPDATE nodes SET active_session=$2 WHERE id=$1",
        params![node, Uuid::new_v4()],
    )
    .await
    .unwrap();
    held.commit().await.unwrap();
    assert!(
        pending.await.unwrap().is_err(),
        "old session must not commit after session replacement"
    );
    let state: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT state FROM node_operations WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(state, "requested");
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn create_and_cancel_recheck_session_expiry_after_row_wait() {
    use artifactory_control_plane::auth::Auth;
    use axum::extract::FromRequestParts;
    for cancel in [false, true] {
        let db = Database::new().await;
        let (_, cookie) = account(&db, "operations-expiry-fence", "viewer").await;
        let tenant = tenant(&db, &cookie, "Ops expiry fence").await;
        let (node, _, _) = ready(&db, tenant).await;
        let id = if cancel {
            let (_, created) = request(
                &db,
                &cookie,
                Some(tenant),
                "POST",
                &format!("/nodes/{node}/operations"),
                input(),
            )
            .await;
            Uuid::parse_str(created["id"].as_str().unwrap()).unwrap()
        } else {
            node
        };
        let (mut parts, _) = Request::builder()
            .uri("/api/v1/nodes")
            .header("cookie", &cookie)
            .header("X-Artifactory-Tenant", tenant.to_string())
            .body(())
            .unwrap()
            .into_parts();
        let auth = Auth::from_request_parts(&mut parts, &db.state)
            .await
            .unwrap();
        let hash = crypto::hash(cookie.split('=').nth(1).unwrap());
        sql::execute(&mut db.pool.clone(),"UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE token_hash=$1",params![&hash]).await.unwrap();
        let mut connection = db.pool.clone();
        let mut held = connection.transaction().await.unwrap();
        let holder: i32 = sql::scalar(&mut held, "SELECT pg_backend_pid()", params![])
            .await
            .unwrap();
        let query = if cancel {
            "SELECT id FROM node_operations WHERE id=$1 FOR UPDATE"
        } else {
            "SELECT id FROM nodes WHERE id=$1 FOR UPDATE"
        };
        let _: Uuid = sql::scalar(&mut held, query, params![id]).await.unwrap();
        let store = db.state.store.clone();
        let create: OperationCreate = serde_json::from_value(input()).unwrap();
        let pending = tokio::spawn(async move {
            if cancel {
                store.cancel_node_operation(&auth, id).await
            } else {
                store.create_node_operation(&auth, node, &create).await
            }
        });
        blocked(&db, holder).await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let expired: bool = sql::scalar(
                    &mut db.pool.clone(),
                    "SELECT expires_at<=clock_timestamp() FROM sessions WHERE token_hash=$1",
                    params![&hash],
                )
                .await
                .unwrap();
                if expired {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        held.commit().await.unwrap();
        assert!(
            pending.await.unwrap().is_err(),
            "expired Session cannot create or cancel after lock wait"
        );
        let changed: bool = if cancel {
            sql::scalar(
                &mut db.pool.clone(),
                "SELECT cancel_requested_at IS NOT NULL FROM node_operations WHERE id=$1",
                params![id],
            )
            .await
            .unwrap()
        } else {
            sql::scalar(
                &mut db.pool.clone(),
                "SELECT EXISTS(SELECT 1 FROM node_operations WHERE node_id=$1)",
                params![node],
            )
            .await
            .unwrap()
        };
        assert!(!changed);
        db.close().await;
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn csrf_cross_tenant_and_node_deletion_bound_operation_data() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "operations-security", "viewer").await;
    let tenant = tenant(&db, &cookie, "Ops security").await;
    let (node, _, _) = ready(&db, tenant).await;
    let path = format!("/nodes/{node}/operations");
    let request_without_csrf = Request::builder()
        .method("POST")
        .uri(format!("/api/v1{path}"))
        .header("cookie", &cookie)
        .header("X-Artifactory-Tenant", tenant.to_string())
        .header("content-type", "application/json")
        .body(Body::from(input().to_string()))
        .unwrap();
    assert_eq!(
        api::router(db.state.clone())
            .oneshot(request_without_csrf)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let (_, created) = request(&db, &cookie, Some(tenant), "POST", &path, input()).await;
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let (_, other) = account(&db, "operations-other", "viewer").await;
    let other_tenant = super::tenant(&db, &other, "Ops other").await;
    assert_eq!(
        request(
            &db,
            &other,
            Some(other_tenant),
            "GET",
            &format!("/node-operations/{id}"),
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM nodes WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM node_operations WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    assert!(
        db.state
            .store
            .pending_node_operations(db.state.coordination.id(), 20)
            .await
            .unwrap()
            .is_empty()
    );
    db.close().await;
}
