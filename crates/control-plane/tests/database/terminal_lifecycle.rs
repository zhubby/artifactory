//! [INPUT]: 依赖真实 PostgreSQL、租户操作和终端持久状态。
//! [OUTPUT]: 验证租户排空、设备租约等待、删除清扫和项目隔离。
//! [POS]: 终端与工作区生命周期的组合回归；不替代设备进程回收验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

async fn terminal_device(db: &Database, tenant: Uuid) -> Uuid {
    let node = device(db, tenant).await;
    sql::execute(&mut db.pool.clone(),"UPDATE nodes SET startup_id=$2,capabilities='[\"terminal-v1\"]',metadata=$3 WHERE id=$1",params![node,Uuid::new_v4(),json!({"emissary.terminal":{"enabled":true,"profiles":[{"id":"default","label":"Shell"}],"account":"emissary"}})]).await.unwrap();
    node
}

async fn create(db: &Database, cookie: &str, tenant: Uuid, node: Uuid) -> Uuid {
    let (status, value) = request(db,cookie,Some(tenant),"POST",&format!("/nodes/{node}/terminal-sessions"),json!({"idempotency_key":Uuid::new_v4(),"renderer_id":Uuid::new_v4(),"profile":"default","columns":80,"rows":24,"term":"xterm-256color"})).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
}

async fn start(
    db: &Database,
    cookie: &str,
    tenant: Uuid,
    project: Option<Uuid>,
    action: &str,
) -> Uuid {
    let path = project
        .map(|id| format!("/projects/{id}/lifecycle"))
        .unwrap_or_else(|| "/tenant/lifecycle".into());
    let slug: String = if let Some(project) = project {
        sql::scalar(
            &mut db.pool.clone(),
            "SELECT slug FROM projects WHERE id=$1",
            params![project],
        )
        .await
        .unwrap()
    } else {
        sql::scalar(
            &mut db.pool.clone(),
            "SELECT slug FROM tenants WHERE id=$1",
            params![tenant],
        )
        .await
        .unwrap()
    };
    let (status, value) = request(db,cookie,Some(tenant),"POST",&path,json!({"action":action,"idempotency_key":Uuid::new_v4(),"slug":slug,"current_password":"terminal-deletion-password"})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{value}");
    Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
}

async fn finish(db: &Database, operation: Uuid) {
    for _ in 0..8 {
        artifactory_control_plane::lifecycle::tick(&db.state)
            .await
            .unwrap();
        let state: String = sql::scalar(
            &mut db.pool.clone(),
            "SELECT state FROM lifecycle_operations WHERE id=$1",
            params![operation],
        )
        .await
        .unwrap();
        assert_ne!(
            state, "failed",
            "terminal cleanup must not poison the tenant operation"
        );
        if state == "succeeded" {
            return;
        }
    }
    panic!("tenant operation did not finish");
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn tenant_delete_waits_for_terminal_lease_and_leaves_no_orphans() {
    let db = Database::new().await;
    let (actor, cookie) = account(&db, "terminal-deletion", "viewer").await;
    let encoded =
        artifactory_control_plane::auth::password_hash("terminal-deletion-password").unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE users SET password_hash=$2 WHERE id=$1",
        params![actor, encoded],
    )
    .await
    .unwrap();
    let tenant = tenant(&db, &cookie, "Terminal deletion").await;
    let node = terminal_device(&db, tenant).await;
    let pending = create(&db, &cookie, tenant, node).await;
    let active = create(&db, &cookie, tenant, node).await;
    sql::execute(&mut db.pool.clone(),"UPDATE terminal_sessions SET ticket_hash='unconsumed',ticket_expires_at=clock_timestamp()+interval '30 seconds' WHERE id=$1",params![pending]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"UPDATE terminal_sessions SET state='active',generation=1,attached_at=clock_timestamp() WHERE id=$1",params![active]).await.unwrap();
    let operation = start(&db, &cookie, tenant, None, "delete").await;
    let stopped: (String, bool, bool) = sql::tuple(&mut db.pool.clone(),"SELECT state,finished_at IS NOT NULL,ticket_hash IS NULL FROM terminal_sessions WHERE id=$1",params![pending]).await.unwrap();
    assert_eq!(
        stopped,
        ("closed".into(), true, true),
        "an unattached terminal must close with the tenant admission fence"
    );
    assert!(!db.state.store.terminal_authorized(active).await.unwrap());
    assert_eq!(
        db.state.store.terminal_session(active).await.unwrap().state,
        "closing"
    );
    let blockers = db
        .state
        .store
        .lifecycle_blockers(tenant, None)
        .await
        .unwrap();
    assert!(
        blockers
            .iter()
            .any(|item| item["kind"] == "terminal" && item["id"] == active.to_string()),
        "active terminal must retain a device lease blocker"
    );
    db.state
        .store
        .close_terminal(active, "authorization_expired", None, None)
        .await
        .unwrap();
    assert!(
        db.state
            .store
            .lifecycle_blockers(tenant, None)
            .await
            .unwrap()
            .iter()
            .any(|item| item["id"] == active.to_string()),
        "owner closure alone is not a device cleanup acknowledgement"
    );
    artifactory_control_plane::lifecycle::tick(&db.state)
        .await
        .unwrap();
    let state: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT state FROM lifecycle_operations WHERE id=$1",
        params![operation],
    )
    .await
    .unwrap();
    assert_eq!(
        state, "waiting",
        "tenant deletion must wait for the device lease"
    );
    let deadline: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT manifest->>'terminal_drain_until' FROM lifecycle_operations WHERE id=$1",
        params![operation],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE lifecycle_operations SET next_run_at=clock_timestamp() WHERE id=$1",
        params![operation],
    )
    .await
    .unwrap();
    artifactory_control_plane::lifecycle::tick(&db.state)
        .await
        .unwrap();
    let same: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT manifest->>'terminal_drain_until' FROM lifecycle_operations WHERE id=$1",
        params![operation],
    )
    .await
    .unwrap();
    assert_eq!(
        deadline, same,
        "repeated drain observations must not extend the device lease barrier"
    );
    sql::execute(&mut db.pool.clone(),"UPDATE lifecycle_operations SET manifest=jsonb_set(manifest,'{terminal_drain_until}',to_jsonb((clock_timestamp()-interval '1 second')::text)),next_run_at=clock_timestamp() WHERE id=$1",params![operation]).await.unwrap();
    finish(&db, operation).await;
    let remaining: (i64,i64,i64) = sql::tuple(&mut db.pool.clone(),"SELECT (SELECT count(*) FROM tenants WHERE id=$1),(SELECT count(*) FROM terminal_sessions WHERE tenant_id=$1),(SELECT count(*) FROM audit_events WHERE tenant_id=$1)",params![tenant]).await.unwrap();
    assert_eq!(
        remaining,
        (0, 0, 0),
        "tenant cleanup must remove terminal tickets, sessions and tenant audit records"
    );
    db.state
        .store
        .terminal_maintenance()
        .await
        .expect("deleted tenants must not poison terminal maintenance");
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn tenant_archive_closes_pending_terminals_but_project_delete_preserves_them() {
    let db = Database::new().await;
    let (actor, cookie) = account(&db, "terminal-project-isolation", "viewer").await;
    let encoded =
        artifactory_control_plane::auth::password_hash("terminal-deletion-password").unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE users SET password_hash=$2 WHERE id=$1",
        params![actor, encoded],
    )
    .await
    .unwrap();
    let tenant = tenant(&db, &cookie, "Terminal project isolation").await;
    let project = project(&db, &cookie, tenant, "Project").await;
    let node = terminal_device(&db, tenant).await;
    let terminal = create(&db, &cookie, tenant, node).await;
    let operation = start(&db, &cookie, tenant, Some(project), "delete").await;
    assert!(
        db.state.store.terminal_authorized(terminal).await.unwrap(),
        "project deletion must not revoke a tenant device terminal"
    );
    finish(&db, operation).await;
    assert_eq!(
        db.state
            .store
            .terminal_session(terminal)
            .await
            .unwrap()
            .state,
        "pending"
    );
    assert!(db.state.store.terminal_authorized(terminal).await.unwrap());
    let archive = start(&db, &cookie, tenant, None, "archive").await;
    assert_eq!(
        db.state
            .store
            .terminal_session(terminal)
            .await
            .unwrap()
            .state,
        "closed"
    );
    finish(&db, archive).await;
    let audits: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM audit_events WHERE action='terminal_closed' AND resource_id=$1",
        params![terminal.to_string()],
    )
    .await
    .unwrap();
    assert_eq!(
        audits, 1,
        "closing an unattached terminal must create one audit event"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn a_recent_control_close_still_waits_for_the_device_lease() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "terminal-recent-close", "viewer").await;
    let tenant = tenant(&db, &cookie, "Terminal recent close").await;
    let node = terminal_device(&db, tenant).await;
    let terminal = create(&db, &cookie, tenant, node).await;
    sql::execute(&mut db.pool.clone(),"UPDATE terminal_sessions SET state='active',generation=1,attached_at=clock_timestamp() WHERE id=$1",params![terminal]).await.unwrap();
    db.state
        .store
        .close_terminal(terminal, "user_closed", None, None)
        .await
        .unwrap();
    let operation = start(&db, &cookie, tenant, None, "archive").await;
    assert!(
        db.state
            .store
            .lifecycle_blockers(tenant, None)
            .await
            .unwrap()
            .iter()
            .any(|item| item["id"] == terminal.to_string()),
        "a control close immediately before tenant drain is not device cleanup evidence"
    );
    artifactory_control_plane::lifecycle::tick(&db.state)
        .await
        .unwrap();
    let recorded: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT manifest ? 'terminal_drain_until' FROM lifecycle_operations WHERE id=$1",
        params![operation],
    )
    .await
    .unwrap();
    assert!(
        recorded,
        "the lease barrier must survive scheduler replacement"
    );
    sql::execute(&mut db.pool.clone(),"UPDATE lifecycle_operations SET manifest=jsonb_set(manifest,'{terminal_drain_until}',to_jsonb((clock_timestamp()-interval '1 second')::text)),next_run_at=clock_timestamp() WHERE id=$1",params![operation]).await.unwrap();
    finish(&db, operation).await;
    db.close().await;
}
