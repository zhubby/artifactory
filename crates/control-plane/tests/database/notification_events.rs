//! [INPUT]: 依赖真实业务事务及持久通知事件。
//! [OUTPUT]: 验证事务回滚、权限边界及设备与项目签名告警恢复。
//! [POS]: 事件生产者的数据库集成回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn business_status_changes_enqueue_once_and_rollback_with_business() {
    let db = Database::with_tenant().await;
    let (_, build, _) = db.fixture().await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='running' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    let count: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_events WHERE resource_id=$1 AND event_type='build.status.changed'", params![build]).await.unwrap();
    assert!(
        count >= 2,
        "creation and actual status transition must enqueue events"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='running' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    let same: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_events WHERE resource_id=$1 AND event_type='build.status.changed'", params![build]).await.unwrap();
    assert_eq!(same, count, "same-value updates must not create messages");
    let mut pool = db.pool.clone();
    let mut tx = pool.transaction().await.unwrap();
    sql::execute(
        &mut tx,
        "UPDATE builds SET status='failed',error='secret-upstream-diagnostic' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    drop(tx);
    let remaining: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_events WHERE resource_id=$1 AND event_type='build.status.changed'", params![build]).await.unwrap();
    assert_eq!(remaining, count);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='failed',error='secret-upstream-diagnostic' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    let leaked: bool = sql::scalar(&mut db.pool.clone(), "SELECT EXISTS(SELECT 1 FROM notification_events WHERE payload::text LIKE '%secret-upstream-diagnostic%')", params![]).await.unwrap();
    assert!(!leaked, "free-form errors must never enter notifications");
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn removed_member_gets_only_a_personal_safe_notice() {
    let db = Database::with_tenant().await;
    let user = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused','viewer')",
        params![user, user.to_string()],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM tenant_members WHERE user_id=$1",
        params![user],
    )
    .await
    .unwrap();
    let safe: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_events WHERE resource_type='account_safe' AND resource_id=$1 AND payload->>'status'='removed' AND candidate_user_ids=ARRAY[$1]", params![user]).await.unwrap();
    assert_eq!(safe, 1);
    db.state.store.project_notifications(100).await.unwrap();
    let inbox = db
        .state
        .store
        .list_notifications(user, Default::default())
        .await
        .unwrap();
    let notice = inbox
        .items
        .iter()
        .find(|i| i.resource_type == "account_safe")
        .unwrap();
    assert!(notice.tenant_name.is_none());
    assert!(notice.project_name.is_none());
    assert!(notice.url.is_none());
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn device_offline_is_debounced_and_recovery_updates_same_incident() {
    let db = Database::with_tenant().await;
    let node = db.node().await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET last_seen=now()-interval '90 seconds' WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    let before: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_events WHERE resource_id=$1 AND category='device'",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(before, 0);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET last_seen=now()-interval '130 seconds' WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    for _ in 0..2 {
        let _: i32 = sql::scalar(
            &mut db.pool.clone(),
            "SELECT notification_scan_anomalies()",
            params![],
        )
        .await
        .unwrap();
    }
    let offline:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM notification_events WHERE resource_id=$1 AND payload->>'status'='offline'",params![node]).await.unwrap();
    assert_eq!(offline, 1);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET last_seen=now() WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    let early:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM notification_events WHERE resource_id=$1 AND payload->>'status'='online'",params![node]).await.unwrap();
    assert_eq!(early, 0);
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    let events:serde_json::Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_agg(jsonb_build_object('key',aggregate_key,'attention',attention,'action',needs_action) ORDER BY aggregate_version) FROM notification_events WHERE resource_id=$1 AND category='device'",params![node]).await.unwrap();
    assert_eq!(events.as_array().unwrap().len(), 2);
    assert_eq!(events[0]["key"], events[1]["key"]);
    assert_eq!(events[0]["attention"], true);
    assert_eq!(events[1]["attention"], false);
    assert_eq!(events[1]["action"], false);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn signing_parent_cascade_produces_one_logical_credential_notice() {
    let db = Database::with_tenant().await;
    let credential = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO signing_credentials(id,tenant_id,name,platform) VALUES($1,'00000000-0000-0000-0000-000000000001','certificate','macos')",params![credential]).await.unwrap();
    for _ in 0..2 {
        sql::execute(&mut db.pool.clone(),"INSERT INTO signing_versions(id,tenant_id,credential_id,metadata,envelope) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,'{}','{}')",params![Uuid::new_v4(),credential]).await.unwrap();
    }
    sql::execute(&mut db.pool.clone(),"INSERT INTO signing_profiles(tenant_id,name,credential_id) VALUES('00000000-0000-0000-0000-000000000001','profile',$1)",params![credential]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM signing_credentials WHERE id=$1",
        params![credential],
    )
    .await
    .unwrap();
    let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM notification_events WHERE resource_id=$1 AND event_type='credential.status.changed'",params![credential]).await.unwrap();
    assert_eq!(count, 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn signing_expiry_reopens_after_reenable_and_resolves_after_deletion() {
    let db = Database::with_tenant().await;
    let (project, _, _) = db.fixture().await;
    let credential = Uuid::new_v4();
    let version = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO signing_credentials(id,tenant_id,name,platform) VALUES($1,'00000000-0000-0000-0000-000000000001','certificate','macos')", params![credential]).await.unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO signing_versions(id,tenant_id,credential_id,metadata,envelope) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,jsonb_build_object('not_after',extract(epoch FROM now()+interval '12 hours')::bigint),'{}')", params![version,credential]).await.unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO signing_profiles(tenant_id,name,credential_id) VALUES('00000000-0000-0000-0000-000000000001','expiry-profile',$1)", params![credential]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE projects SET signing_defaults='{\"macos\":\"expiry-profile\"}' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    let scan = || async {
        sql::scalar::<i32>(
            &mut db.pool.clone(),
            "SELECT notification_scan_anomalies()",
            params![],
        )
        .await
        .unwrap()
    };
    scan().await;
    let affected: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_events WHERE aggregate_key IN ($1,$2) AND needs_action",
        params![
            format!("signing:{version}"),
            format!("signing-project:{project}:{version}")
        ],
    )
    .await
    .unwrap();
    assert_eq!(affected, 2);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE projects SET signing_defaults='{}' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    scan().await;
    db.state.store.project_notifications(128).await.unwrap();
    let pending: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT needs_action FROM notification_inbox WHERE aggregate_key=$1 LIMIT 1",
        params![format!("signing-project:{project}:{version}")],
    )
    .await
    .unwrap();
    assert!(
        !pending,
        "removing the project default resolves its old alert"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE projects SET signing_defaults='{\"macos\":\"expiry-profile\"}' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    scan().await;
    db.state.store.project_notifications(128).await.unwrap();
    let pending: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT needs_action FROM notification_inbox WHERE aggregate_key=$1 LIMIT 1",
        params![format!("signing-project:{project}:{version}")],
    )
    .await
    .unwrap();
    assert!(
        pending,
        "restoring the expiring project default reopens its alert"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE signing_credentials SET enabled=false WHERE id=$1",
        params![credential],
    )
    .await
    .unwrap();
    scan().await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE signing_credentials SET enabled=true WHERE id=$1",
        params![credential],
    )
    .await
    .unwrap();
    scan().await;
    let reopened: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_events WHERE aggregate_key=$1 AND payload->>'status'='expiring_1d'", params![format!("signing:{version}")]).await.unwrap();
    assert_eq!(
        reopened, 2,
        "re-enabling the same expired version reopens its alert"
    );
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM signing_credentials WHERE id=$1",
        params![credential],
    )
    .await
    .unwrap();
    scan().await;
    db.state.store.project_notifications(128).await.unwrap();
    let stale: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_inbox WHERE aggregate_key IN ($1,$2) AND needs_action",
        params![
            format!("signing:{version}"),
            format!("signing-project:{project}:{version}")
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        stale, 0,
        "deleted certificate and project alerts must resolve"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn never_connected_device_still_reports_expired_certificate_without_offline_alert() {
    let db = Database::with_tenant().await;
    let node = db.node().await;
    sql::execute(&mut db.pool.clone(), "UPDATE nodes SET last_seen=NULL,active_session=NULL,connection_owner=NULL,certificate_expires_at=now()-interval '1 second' WHERE id=$1", params![node]).await.unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    let statuses: Vec<String> = sql::scalars(&mut db.pool.clone(), "SELECT payload->>'status' FROM notification_events WHERE resource_id=$1 AND event_type='device.status.changed'", params![node]).await.unwrap();
    assert_eq!(statuses, vec!["certificate_expired"]);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn channel_failure_is_actionable_and_health_includes_explicit_test_jobs() {
    let db = Database::with_tenant().await;
    let user: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM users WHERE username='fixture-owner'",
        params![],
    )
    .await
    .unwrap();
    let endpoint = Uuid::new_v4();
    let delivery = Uuid::new_v4();
    let tenant = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO notification_webhooks(id,tenant_id,created_by,name,url,secret_encrypted,last_status) VALUES($1,$2,$3,'Endpoint','https://example.com/private-path','unused','failed')", params![endpoint,tenant,user]).await.unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO notification_deliveries(id,channel,endpoint_id,generation,event_type,status,completed_at) VALUES($1,'webhook',$2,1,'notification.test','failed',now())", params![delivery,endpoint]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("health-fixture"), user],
    )
    .await
    .unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(128).await.unwrap();
    let actions: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_inbox WHERE user_id=$1 AND category='delivery' AND needs_action", params![user]).await.unwrap();
    assert_eq!(actions, 1);
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/v1/tenant/notification-health")
                .header("cookie", "artifactory_session=health-fixture")
                .header("x-artifactory-tenant", tenant.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        health["failed_deliveries"], 1,
        "test jobs have no event_id but belong to their endpoint tenant"
    );
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/v1/tenant/notification-metrics")
                .header("cookie", "artifactory_session=health-fixture")
                .header("x-artifactory-tenant", tenant.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let metrics = std::str::from_utf8(&body).unwrap();
    assert!(metrics.contains("artifactory_notifications_failed_deliveries 1"));
    assert!(!metrics.contains("Endpoint") && !metrics.contains("private-path"));
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_webhooks SET last_status='succeeded' WHERE id=$1",
        params![endpoint],
    )
    .await
    .unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(128).await.unwrap();
    let actions: i64 = sql::scalar(&mut db.pool.clone(), "SELECT count(*) FROM notification_inbox WHERE user_id=$1 AND category='delivery' AND needs_action", params![user]).await.unwrap();
    assert_eq!(actions, 0);
    let outsider = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused','viewer')",
        params![outsider, outsider.to_string()],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("health-outsider"), outsider],
    )
    .await
    .unwrap();
    for cookie in ["artifactory_session=health-outsider", ""] {
        let response = api::router(db.state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenant/notification-health")
                    .header("cookie", cookie)
                    .header("x-artifactory-tenant", tenant.to_string())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!([StatusCode::FORBIDDEN, StatusCode::UNAUTHORIZED].contains(&response.status()));
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn resource_cleanup_waits_for_events_before_locking_projected_inbox() {
    let db = Database::with_tenant().await;
    let user: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM users WHERE username='fixture-owner'",
        params![],
    )
    .await
    .unwrap();
    let project = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO projects(id,tenant_id,name,slug,repository_url) VALUES($1,'00000000-0000-0000-0000-000000000001','cleanup','cleanup','https://example.com/source.git')", params![project]).await.unwrap();
    sql::execute(&mut db.pool.clone(), "SELECT notification_emit('build.status.changed','build','info','00000000-0000-0000-0000-000000000001',$1,'project',$1,$2,'{}',ARRAY[$3],true,false)", params![project,format!("cleanup:{project}"),user]).await.unwrap();
    db.state.store.project_notifications(128).await.unwrap();
    let event: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT event_id FROM notification_inbox WHERE project_id=$1",
        params![project],
    )
    .await
    .unwrap();
    let mut connection = db.pool.clone();
    let mut projector = connection.transaction().await.unwrap();
    sql::execute(
        &mut projector,
        "SELECT id FROM notification_events WHERE id=$1 FOR UPDATE",
        params![event],
    )
    .await
    .unwrap();
    let application = format!("cleanup-lock-{project}");
    let probe = application.clone();
    let mut delete_pool = db.pool.clone();
    let deletion = tokio::spawn(async move {
        let mut tx = delete_pool.transaction().await.unwrap();
        let _: String = sql::scalar(
            &mut tx,
            "SELECT set_config('application_name',$1,true)",
            params![application],
        )
        .await
        .unwrap();
        sql::execute(
            &mut tx,
            "DELETE FROM projects WHERE id=$1",
            params![project],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked: bool = sql::scalar(&mut db.pool.clone(), "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')", params![probe.clone()]).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("cleanup must be waiting for the projector's event lock");
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        sql::execute(
            &mut projector,
            "UPDATE notification_inbox SET updated_at=clock_timestamp() WHERE event_id=$1",
            params![event],
        ),
    )
    .await
    .expect("cleanup must not take inbox locks before event locks")
    .unwrap();
    projector.commit().await.unwrap();
    deletion.await.unwrap();
    let remaining: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_events WHERE project_id=$1",
        params![project],
    )
    .await
    .unwrap();
    assert_eq!(remaining, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_recovery_resolves_the_device_aggregate_without_new_attention() {
    let db = Database::with_tenant().await;
    let node = db.node().await;
    let release = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_releases(id,tenant_id,version,manifest,signature,manifest_hash) VALUES($1,$2,'1.2.3','{}','unused',$3)",params![release,Uuid::from_u128(1),"a".repeat(64)]).await.unwrap();
    for (status, expected) in [
        ("succeeded", 0_i64),
        ("failed", 1),
        ("succeeded", 2),
        ("succeeded", 2),
    ] {
        let update = Uuid::new_v4();
        sql::execute(
            &mut db.pool.clone(),
            "INSERT INTO emissary_updates(id,node_id,pool_id,release_id) VALUES($1,$2,$3,$4)",
            params![update, node, Uuid::from_u128(1), release],
        )
        .await
        .unwrap();
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE emissary_updates SET status=$2,finished_at=now() WHERE id=$1",
            params![update, status],
        )
        .await
        .unwrap();
        let count: i64 = sql::scalar(
            &mut db.pool.clone(),
            "SELECT count(*) FROM notification_events WHERE aggregate_key=$1",
            params![format!("device-update:{node}")],
        )
        .await
        .unwrap();
        assert_eq!(count, expected);
    }
    db.state.store.project_notifications(128).await.unwrap();
    let latest:serde_json::Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',payload->>'status','attention',attention,'action',needs_action) FROM notification_events WHERE aggregate_key=$1 ORDER BY aggregate_version DESC LIMIT 1",params![format!("device-update:{node}")]).await.unwrap();
    assert_eq!(
        latest,
        serde_json::json!({"status":"resolved","attention":false,"action":false})
    );
    let pending: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_inbox WHERE aggregate_key=$1 AND needs_action",
        params![format!("device-update:{node}")],
    )
    .await
    .unwrap();
    assert_eq!(pending, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn credential_reenable_and_delete_clear_the_same_actionable_aggregate() {
    let db = Database::with_tenant().await;
    let credential = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO signing_credentials(id,tenant_id,name,platform) VALUES($1,$2,'test','macos')",
        params![credential, Uuid::from_u128(1)],
    )
    .await
    .unwrap();
    for (enabled, status, action) in [
        (false, "disabled", true),
        (true, "resolved", false),
        (false, "disabled", true),
    ] {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE signing_credentials SET enabled=$2 WHERE id=$1",
            params![credential, enabled],
        )
        .await
        .unwrap();
        let latest:serde_json::Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',payload->>'status','action',needs_action,'attention',attention) FROM notification_events WHERE aggregate_key=$1 ORDER BY aggregate_version DESC LIMIT 1",params![format!("credential:{credential}")]).await.unwrap();
        assert_eq!(
            latest,
            serde_json::json!({"status":status,"action":action,"attention":action})
        );
    }
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM signing_credentials WHERE id=$1",
        params![credential],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(128).await.unwrap();
    let pending: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_inbox WHERE aggregate_key=$1 AND needs_action",
        params![format!("credential:{credential}")],
    )
    .await
    .unwrap();
    assert_eq!(pending, 0);
    let action:bool=sql::scalar(&mut db.pool.clone(),"SELECT needs_action FROM notification_events WHERE aggregate_key=$1 ORDER BY aggregate_version DESC LIMIT 1",params![format!("credential:{credential}")]).await.unwrap();
    assert!(!action);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn deleting_failed_webhook_resolves_its_station_alert() {
    let db = Database::with_tenant().await;
    let owner: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT owner_id FROM tenants WHERE id=$1",
        params![Uuid::from_u128(1)],
    )
    .await
    .unwrap();
    let endpoint = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_webhooks(id,tenant_id,created_by,name,url,secret_encrypted,last_status) VALUES($1,$2,$3,'test','https://example.org/','unused','failed')",params![endpoint,Uuid::from_u128(1),owner]).await.unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(128).await.unwrap();
    let before:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM notification_inbox WHERE resource_id=$1 AND category='delivery' AND needs_action",params![endpoint]).await.unwrap();
    assert!(before > 0);
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_webhooks WHERE id=$1",
        params![endpoint],
    )
    .await
    .unwrap();
    let _: i32 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT notification_scan_anomalies()",
        params![],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(128).await.unwrap();
    let after:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM notification_inbox WHERE resource_id=$1 AND category='delivery' AND needs_action",params![endpoint]).await.unwrap();
    assert_eq!(after, 0);
    db.close().await;
}
