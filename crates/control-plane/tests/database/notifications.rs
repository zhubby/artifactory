//! [INPUT]: 依赖隔离数据库及通知事件投影。
//! [OUTPUT]: 验证聚合版本、已读水位和实时权限。
//! [POS]: 通知核心的真实 PostgreSQL 回归测试。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_control_plane::store::notifications::{NotificationFilter, ReadAllInput};

#[path = "notification_followup.rs"]
mod followup;

async fn user(db: &Database) -> Uuid {
    let id = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused','viewer')",
        params![id, id.to_string()],
    )
    .await
    .unwrap();
    id
}
async fn event(db: &Database, user: Uuid, aggregate: &str, version: i64, attention: bool) {
    sql::execute(&mut db.pool.clone(), "INSERT INTO notification_events(event_key,event_type,category,severity,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,attention) VALUES($1,'build.phase.changed','build','info','account',$6,$2,$3,$4,$5)", params![format!("{aggregate}:{version}"),aggregate,version,vec![user],attention,user]).await.unwrap();
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_aggregation_preserves_newer_attention_and_read_watermark() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    event(&db, uid, "a", 1, true).await;
    assert_eq!(db.state.store.project_notifications(100).await.unwrap(), 1);
    let first = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    let item = &first.items[0];
    event(&db, uid, "a", 2, true).await;
    db.state.store.project_notifications(100).await.unwrap();
    db.state
        .store
        .read_notification(uid, item.id, item.attention_revision)
        .await
        .unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .unread_count,
        1
    );
    db.state
        .store
        .read_all_notifications(
            uid,
            ReadAllInput {
                watermark: first.watermark,
                category: None,
                tenant_id: None,
                project_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .unread_count,
        1
    );
    event(&db, uid, "a", 3, false).await;
    db.state.store.project_notifications(100).await.unwrap();
    let latest = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(latest.items.len(), 1);
    assert_eq!(latest.items[0].content_revision, 3);
    assert_eq!(latest.items[0].attention_revision, 2);
    db.state
        .store
        .read_notification(uid, item.id, 2)
        .await
        .unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .unread_count,
        0
    );
    db.close().await;
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_candidates_never_override_current_project_acl() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    let (project, build, _) = db.fixture().await;
    sql::execute(&mut db.pool.clone(),"INSERT INTO tenant_members(tenant_id,user_id,role) SELECT tenant_id,$2,'member' FROM projects WHERE id=$1 ON CONFLICT(tenant_id,user_id) DO UPDATE SET role='member'",params![project,uid]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO project_members(tenant_id,project_id,user_id,role) SELECT tenant_id,id,$2,'viewer' FROM projects WHERE id=$1",params![project,uid]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(event_key,event_type,category,severity,tenant_id,project_id,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,attention) SELECT $1,'build.status.changed','build','success',tenant_id,id,'build',$2,$1,1,$3,true FROM projects WHERE id=$4",params![build.to_string(),build,vec![uid],project]).await.unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    let inbox = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    let item_id = inbox.items[0].id;
    assert!(
        !db.state
            .store
            .notification_subscription(uid, project)
            .await
            .unwrap()
            .following
    );
    assert!(
        db.state
            .store
            .set_notification_subscription(uid, project, true)
            .await
            .unwrap()
            .following
    );
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .unread_count,
        1
    );
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM project_members WHERE project_id=$1 AND user_id=$2",
        params![project, uid],
    )
    .await
    .unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .unread_count,
        0
    );
    assert!(
        db.state
            .store
            .list_notifications(uid, NotificationFilter::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        db.state
            .store
            .notification_detail(uid, item_id, None)
            .await
            .is_err()
    );
    assert!(
        db.state
            .store
            .notification_subscription(uid, project)
            .await
            .is_err()
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_http_uses_account_sessions_and_rejects_csrf_mutations() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    event(&db, uid, "http", 1, true).await;
    db.state.store.project_notifications(100).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("notification-session"), uid],
    )
    .await
    .unwrap();
    let router = api::router(db.state.clone());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/notifications")
                .header("cookie", "artifactory_session=notification-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "personal inbox needs no selected tenant"
    );
    let body = axum::body::to_bytes(response.into_body(), 128 * 1024)
        .await
        .unwrap();
    let page: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let id = page["items"][0]["id"].as_str().unwrap();
    for (credential, csrf, status) in [
        (false, false, StatusCode::UNAUTHORIZED),
        (true, false, StatusCode::FORBIDDEN),
        (true, true, StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/notifications/{id}/read"))
            .header("content-type", "application/json");
        if credential {
            request = request.header("cookie", "artifactory_session=notification-session")
        }
        // CSRF is checked before session lookup for mutations.
        if csrf || !credential {
            request = request.header("x-artifactory-csrf", "1");
        }
        let response = router
            .clone()
            .oneshot(
                request
                    .body(Body::from("{\"attention_revision\":1}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let denied = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/notifications")
                .header("authorization", "Bearer notification-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_old_events_cannot_rewind_content_or_reopen_attention() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    event(&db, uid, "ordered", 4, true).await;
    db.state.store.project_notifications(100).await.unwrap();
    let first = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    let original = &first.items[0];
    db.state
        .store
        .read_notification(uid, original.id, 4)
        .await
        .unwrap();
    event(&db, uid, "ordered", 2, true).await;
    db.state.store.project_notifications(100).await.unwrap();
    let latest = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(latest.items[0].content_revision, 4);
    assert_eq!(latest.items[0].updated_at, original.updated_at);
    assert_eq!(latest.items[0].attention_revision, 4);
    assert_eq!(latest.items[0].read_revision, 4);
    assert_eq!(latest.unread_count, 0);
    event(&db, uid, "ordered", 5, true).await;
    sql::execute(&mut db.pool.clone(),"UPDATE notification_events SET event_type='future.unrecognized' WHERE event_key='ordered:5'",params![]).await.unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    let detail = db
        .state
        .store
        .notification_detail(uid, original.id, None)
        .await
        .unwrap();
    assert_eq!(detail.events.len(), 2);
    assert!(
        db.state
            .store
            .notification_detail(Uuid::new_v4(), original.id, None)
            .await
            .is_err()
    );
    assert_eq!(db.state.store.project_notifications(100).await.unwrap(), 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_unsupported_event_is_quarantined_and_personal_notice_is_mandatory() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_preferences(user_id,preferences) VALUES($1,'{\"in_app\":{\"permission\":false,\"account\":false}}')",params![uid]).await.unwrap();
    event(&db, uid, "unsupported", 1, true).await;
    sql::execute(&mut db.pool.clone(),"UPDATE notification_events SET event_type='future.unrecognized' WHERE aggregate_key='unsupported'",params![]).await.unwrap();
    event(&db, uid, "personal", 1, true).await;
    sql::execute(&mut db.pool.clone(),"UPDATE notification_events SET event_type='permission.member.changed',category='permission',resource_type='account_safe' WHERE aggregate_key='personal'",params![]).await.unwrap();
    assert_eq!(db.state.store.project_notifications(100).await.unwrap(), 2);
    let page = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(
        page.items.len(),
        1,
        "unsupported events cannot enter the inbox"
    );
    assert_eq!(
        page.items[0].category, "permission",
        "mandatory personal notice bypasses preferences"
    );
    let reason: Option<String> = sql::scalar(
        &mut db.pool.clone(),
        "SELECT last_error FROM notification_events WHERE aggregate_key='unsupported'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("unsupported_event_schema"));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_phase_resolves_action_without_creating_new_attention() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    event(&db, uid, "action", 1, true).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_events SET needs_action=true WHERE aggregate_key='action'",
        params![],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .needs_action_count,
        1
    );
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_preferences(user_id,preferences) VALUES($1,'{\"in_app\":{\"build\":false}}')",params![uid]).await.unwrap();
    event(&db, uid, "action", 2, false).await;
    db.state.store.project_notifications(100).await.unwrap();
    let page = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(
        page.needs_action_count, 0,
        "existing cards must resolve even after disabling new notifications"
    );
    assert_eq!(page.items[0].attention_revision, 1);
    assert_eq!(page.items[0].content_revision, 2);
    assert_eq!(page.unread_count, 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_admin_downgrade_hides_administrative_history() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    let (project, _, _) = db.fixture().await;
    sql::execute(&mut db.pool.clone(),"INSERT INTO tenant_members(tenant_id,user_id,role) SELECT tenant_id,$2,'member' FROM projects WHERE id=$1 ON CONFLICT(tenant_id,user_id) DO UPDATE SET role='member'",params![project,uid]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO project_members(tenant_id,project_id,user_id,role) SELECT tenant_id,id,$2,'admin' FROM projects WHERE id=$1",params![project,uid]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(event_key,event_type,category,severity,tenant_id,project_id,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,attention) SELECT 'admin-test','permission.resources.changed','permission','info',tenant_id,id,'project_admin',id,'admin-test',1,$2,true FROM projects WHERE id=$1",params![project,vec![uid]]).await.unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    let page = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    let id = page.items[0].id;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE project_members SET role='viewer' WHERE project_id=$1 AND user_id=$2",
        params![project, uid],
    )
    .await
    .unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .unread_count,
        0
    );
    assert!(
        db.state
            .store
            .list_notifications(uid, NotificationFilter::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        db.state
            .store
            .notification_detail(uid, id, None)
            .await
            .is_err()
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_scopes_include_old_pages_and_exclude_revoked_admin_scopes() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    let (old, _, _) = db.fixture().await;
    let (recent, _, _) = db.fixture().await;
    let other = user(&db).await;
    let tenant = Uuid::new_v4();
    let admin = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"WITH new_tenant AS (INSERT INTO tenants(id,name,owner_id,created_by,creation_key) VALUES($1,'Private scope',$2,$2,$3) RETURNING id) INSERT INTO tenant_members(tenant_id,user_id,role) SELECT id,$2,'owner' FROM new_tenant",params![tenant,other,Uuid::new_v4()]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO projects(id,tenant_id,name,repository_url) VALUES($1,$2,'Private admin project','https://example.com/test')",params![admin,tenant]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'member')",
        params![tenant, uid],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO project_members(tenant_id,project_id,user_id,role) SELECT tenant_id,id,$1,CASE WHEN id=$3 THEN 'admin' ELSE 'viewer' END FROM projects WHERE id=ANY($2)",params![uid,vec![old,recent,admin],admin]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    for (project, kind, age) in [
        (old, "project", 2i64),
        (recent, "project", 0),
        (admin, "project_admin", 1),
    ] {
        sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(event_key,event_type,category,severity,tenant_id,project_id,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,attention,occurred_at) SELECT id::text,'permission.resources.changed','permission','info',tenant_id,id,$3,id,id::text,1,$2,true,now()-make_interval(days=>$4::integer) FROM projects WHERE id=$1",params![project,vec![uid],kind,age as i32]).await.unwrap();
    }
    db.state.store.project_notifications(100).await.unwrap();
    let first = db
        .state
        .store
        .list_notifications(
            uid,
            NotificationFilter {
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(first.items[0].project_id, Some(recent));
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("scope-session"), uid],
    )
    .await
    .unwrap();
    let scopes = notification_scopes_http(&db).await;
    assert_eq!(scopes["projects"].as_array().unwrap().len(), 3);
    assert!(
        scopes["projects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == old.to_string()),
        "scope selector includes projects outside the current page"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE project_members SET role='viewer' WHERE project_id=$1 AND user_id=$2",
        params![admin, uid],
    )
    .await
    .unwrap();
    let scopes = notification_scopes_http(&db).await;
    assert_eq!(scopes["projects"].as_array().unwrap().len(), 2);
    assert!(
        !scopes["tenants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == tenant.to_string()),
        "a tenant with only hidden administrative notifications is excluded"
    );
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM project_members WHERE project_id=$1 AND user_id=$2",
        params![old, uid],
    )
    .await
    .unwrap();
    let scopes = notification_scopes_http(&db).await;
    assert_eq!(scopes["projects"].as_array().unwrap().len(), 1);
    assert_eq!(scopes["projects"][0]["id"], recent.to_string());
    db.close().await;
}
async fn notification_scopes_http(db: &Database) -> serde_json::Value {
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/v1/notifications/scopes")
                .header("cookie", "artifactory_session=scope-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 128 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_personal_delivery_action_is_visible_without_resource_context() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    event(&db, uid, "delivery-action", 1, true).await;
    event(&db, uid, "permission-no-action", 1, true).await;
    sql::execute(&mut db.pool.clone(),"UPDATE notification_events SET resource_type='account_safe',needs_action=true,category=CASE WHEN aggregate_key='delivery-action' THEN 'delivery' ELSE 'permission' END,event_type=CASE WHEN aggregate_key='delivery-action' THEN 'delivery.channel.changed' ELSE 'permission.member.changed' END",params![]).await.unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    let page = db
        .state
        .store
        .list_notifications(
            uid,
            NotificationFilter {
                needs_action: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.needs_action_count, 1);
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].needs_action);
    assert_eq!(page.items[0].category, "delivery");
    assert!(
        page.items[0].tenant_id.is_none()
            && page.items[0].project_id.is_none()
            && page.items[0].resource_id.is_none()
    );
    let all = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert!(
        !all.items
            .iter()
            .find(|n| n.category == "permission")
            .unwrap()
            .needs_action
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_in_app_optout_updates_content_without_new_attention() {
    let db = Database::with_tenant().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    event(&db, uid, "muted", 1, true).await;
    db.state.store.project_notifications(100).await.unwrap();
    let first = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    let id = first.items[0].id;
    db.state.store.read_notification(uid, id, 1).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_preferences(user_id,preferences) VALUES($1,'{\"in_app\":{\"build\":false}}')",params![uid]).await.unwrap();
    event(&db, uid, "muted", 2, true).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_events SET needs_action=true WHERE event_key='muted:2'",
        params![],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    let muted = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(muted.items[0].content_revision, 2);
    assert_eq!(
        muted.items[0].attention_revision, 1,
        "opt-out prevents a new attention revision"
    );
    assert_eq!(muted.items[0].read_revision, 1);
    assert_eq!(muted.unread_count, 0);
    assert_eq!(
        muted.needs_action_count, 1,
        "current action state still updates"
    );
    event(&db, uid, "muted", 3, false).await;
    db.state.store.project_notifications(100).await.unwrap();
    assert_eq!(
        db.state
            .store
            .notification_summary(uid)
            .await
            .unwrap()
            .needs_action_count,
        0
    );
    db.close().await;
}
