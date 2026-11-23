//! [INPUT]: 依赖隔离 PostgreSQL 与通知渠道 API。
//! [OUTPUT]: 验证单次令牌、订阅重置、绑定代数与撤权取消。
//! [POS]: 外部副作用之前的数据库安全回归；禁止真实邮件和网络发送。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_control_plane::{
    notifications::channels::{email, tick, webhook},
    settings::SmtpSettings,
};
use serde_json::{Value, json};

async fn fixture() -> (Database, Uuid) {
    let mut db = Database::with_tenant().await;
    let id: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT owner_id FROM tenants WHERE id='00000000-0000-0000-0000-000000000001'",
        params![],
    )
    .await
    .unwrap();
    let mut settings = (*db.state.settings).clone();
    settings.notifications.smtp = Some(SmtpSettings {
        host: "unused.invalid".into(),
        port: 465,
        from: "notify@example.org".into(),
        username: None,
        password: None,
        starttls: false,
    });
    db.state.settings = Arc::new(settings);
    (db, id)
}
async fn allow_request(db: &Database, user: Uuid) {
    sql::execute(&mut db.pool.clone(),"UPDATE notification_email_contacts SET last_requested_at=now()-interval '2 minutes' WHERE user_id=$1",params![user]).await.unwrap();
}
async fn install_token(db: &Database, user: Uuid, token: &str) {
    sql::execute(&mut db.pool.clone(),"UPDATE notification_email_contacts SET token_hash=$2,token_expires_at=now()+interval '30 minutes' WHERE user_id=$1",params![user,crypto::hash(token)]).await.unwrap();
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn email_tokens_are_single_use_expiring_and_address_changes_reset_opt_in() {
    let (db, user) = fixture().await;
    let preferences = email::preferences(&db.state, user).await.unwrap();
    assert!(
        preferences["email"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == false)
    );
    assert!(
        email::put_preferences(&db.state, user, json!({"email":{"build":true}}))
            .await
            .is_err()
    );
    email::bind(&db.state, user, Some("first@example.org"))
        .await
        .unwrap();
    assert!(email::bind(&db.state, user, None).await.is_err());
    let payload: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT payload FROM notification_deliveries WHERE user_id=$1",
        params![user],
    )
    .await
    .unwrap();
    assert_eq!(
        payload,
        json!({}),
        "the durable job must never retain a reversible verification token"
    );
    let token = "a".repeat(64);
    install_token(&db, user, &token).await;
    let other = Uuid::new_v4();
    assert!(email::verify(&db.state, other, &token).await.is_err());
    email::verify(&db.state, user, &token).await.unwrap();
    assert!(email::verify(&db.state, user, &token).await.is_err());
    assert!(
        email::put_preferences(&db.state, user, json!({"email":{"delivery":true}}))
            .await
            .is_err(),
        "channel failures remain in-app and cannot create external loops"
    );
    email::put_preferences(
        &db.state,
        user,
        json!({"email":{"build":true},"locale":"en"}),
    )
    .await
    .unwrap();
    allow_request(&db, user).await;
    let preferences = email::bind(&db.state, user, Some("second@example.org"))
        .await
        .unwrap();
    assert_eq!(preferences["email_address"], "second@example.org");
    assert_eq!(preferences["email_verified"], false);
    assert_eq!(preferences["email"]["build"], false);
    install_token(&db, user, &token).await;
    sql::execute(&mut db.pool.clone(),"UPDATE notification_email_contacts SET token_expires_at=now()-interval '1 second' WHERE user_id=$1",params![user]).await.unwrap();
    assert!(email::verify(&db.state, user, &token).await.is_err());
    email::retire(&db.state, user).await.unwrap();
    assert!(email::preferences(&db.state, user).await.unwrap()["email_address"].is_null());
    let queued:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM notification_deliveries WHERE user_id=$1 AND status IN ('queued','sending')",params![user]).await.unwrap();
    assert_eq!(queued, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn unsubscribe_is_binding_scoped_and_rebinding_preserves_monotonic_generation() {
    let (db, user) = fixture().await;
    email::bind(&db.state, user, Some("first@example.org"))
        .await
        .unwrap();
    let gen1: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT generation FROM notification_email_contacts WHERE user_id=$1",
        params![user],
    )
    .await
    .unwrap();
    let old_token = email::unsubscribe_token(&db.state.settings.master_key, user, gen1);
    email::retire(&db.state, user).await.unwrap();
    allow_request(&db, user).await;
    email::bind(&db.state, user, Some("second@example.org"))
        .await
        .unwrap();
    let gen2: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT generation FROM notification_email_contacts WHERE user_id=$1",
        params![user],
    )
    .await
    .unwrap();
    assert!(gen2 > gen1);
    let verify_token = "b".repeat(64);
    install_token(&db, user, &verify_token).await;
    email::verify(&db.state, user, &verify_token).await.unwrap();
    email::put_preferences(&db.state, user, json!({"email":{"build":true}}))
        .await
        .unwrap();
    email::unsubscribe(&db.state, &old_token).await.unwrap();
    assert_eq!(
        email::preferences(&db.state, user).await.unwrap()["email"]["build"],
        true
    );
    let current = email::unsubscribe_token(&db.state.settings.master_key, user, gen2);
    email::unsubscribe(&db.state, &current).await.unwrap();
    assert_eq!(
        email::preferences(&db.state, user).await.unwrap()["email"]["build"],
        false
    );
    // Queued verification tasks are canceled before tick: this test cannot contact SMTP.
    email::retire(&db.state, user).await.unwrap();
    tick(&db.state).await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn webhook_rotation_cancels_old_generation_and_retry_is_bounded() {
    let (db, user) = fixture().await;
    let endpoint = Uuid::new_v4();
    let mut auth = artifactory_control_plane::auth::Auth::scoped(user, "admin", vec![], vec![]);
    auth.tenant_id = Uuid::from_u128(1);
    auth.role = "owner".into();
    auth.token_projects = None;
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_webhooks(id,tenant_id,created_by,name,url,secret_encrypted) VALUES($1,$2,$3,'test','https://127.0.0.1/','unused')",params![endpoint,auth.tenant_id,user]).await.unwrap();
    let test = webhook::mutate(&db.state, &auth, None, endpoint, "test", None)
        .await
        .unwrap();
    let job = Uuid::parse_str(test["id"].as_str().unwrap()).unwrap();
    let rotated = webhook::mutate(&db.state, &auth, None, endpoint, "rotate", None)
        .await
        .unwrap();
    assert_eq!(rotated["generation"], 2);
    assert_eq!(rotated["secret"].as_str().unwrap().len(), 64);
    let status: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT status FROM notification_deliveries WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(status, "canceled");
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_deliveries SET status='failed',generation=2,attempts=10 WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert!(
        webhook::mutate(&db.state, &auth, None, endpoint, "retry", Some(job))
            .await
            .is_err()
    );
    let deliveries = webhook::deliveries(&db.state, &auth, None, endpoint)
        .await
        .unwrap();
    let text = deliveries.to_string();
    assert!(!text.contains("127.0.0.1"));
    assert!(!text.contains(rotated["secret"].as_str().unwrap()));
    webhook::mutate(&db.state, &auth, None, endpoint, "delete", None)
        .await
        .unwrap();
    tick(&db.state).await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn channels_recheck_acl_and_cancel_before_any_external_attempt() {
    let (db, user) = fixture().await;
    email::bind(&db.state, user, Some("first@example.org"))
        .await
        .unwrap();
    let token = "c".repeat(64);
    install_token(&db, user, &token).await;
    email::verify(&db.state, user, &token).await.unwrap();
    email::put_preferences(&db.state, user, json!({"email":{"build":true}}))
        .await
        .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_deliveries SET status='canceled' WHERE channel='verification'",
        params![],
    )
    .await
    .unwrap();
    let event = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(id,event_key,event_type,category,severity,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,channels_routed_at) VALUES($1,$2,'build.status.changed','build','info','account_safe',$3,$2,1,$4,now())",params![event,event.to_string(),Uuid::new_v4(),vec![user]]).await.unwrap();
    let generation: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT generation FROM notification_email_contacts WHERE user_id=$1",
        params![user],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_deliveries(channel,event_id,user_id,generation,event_type) VALUES('email',$1,$2,$3,'build.status.changed')",params![event,user,generation]).await.unwrap();
    tick(&db.state).await.unwrap();
    let result:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',status,'attempts',attempts) FROM notification_deliveries WHERE event_id=$1",params![event]).await.unwrap();
    assert_eq!(result, json!({"status":"canceled","attempts":0}));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn routing_is_future_opt_in_only_and_webhooks_do_not_fan_out_per_candidate() {
    let (mut db, owner) = fixture().await;
    let member = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused','viewer')",
        params![member, member.to_string()],
    )
    .await
    .unwrap();
    for user in [owner, member] {
        email::bind(&db.state, user, Some(&format!("{user}@example.org")))
            .await
            .unwrap();
        let token = "c".repeat(64);
        install_token(&db, user, &token).await;
        email::verify(&db.state, user, &token).await.unwrap();
        email::put_preferences(&db.state, user, json!({"email":{"build":true}}))
            .await
            .unwrap();
    }
    // Cancel explicit verification jobs without retiring the opted-in identities.
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_deliveries SET status='canceled' WHERE channel='verification'",
        params![],
    )
    .await
    .unwrap();
    let endpoint = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_webhooks(id,tenant_id,created_by,name,url,enabled,event_types,secret_encrypted) VALUES($1,$2,$3,'blocked test','https://127.0.0.1/',true,ARRAY['build.status.changed'],'unused')",params![endpoint,Uuid::from_u128(1),owner]).await.unwrap();
    let old = Uuid::new_v4();
    let new = Uuid::new_v4();
    for (id, future) in [(old, false), (new, true)] {
        sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(id,event_key,event_type,category,severity,tenant_id,resource_type,aggregate_key,aggregate_version,candidate_user_ids,attention,occurred_at) VALUES($1,$2,'build.status.changed','build','info',$3,'tenant',$2,1,$4,true,now()+CASE WHEN $5 THEN interval '1 minute' ELSE interval '-10 minutes' END)",params![id,id.to_string(),Uuid::from_u128(1),vec![owner,member],future]).await.unwrap();
    }
    let phase = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(id,event_key,event_type,category,severity,tenant_id,resource_type,aggregate_key,aggregate_version,candidate_user_ids,occurred_at) VALUES($1,$2,'build.phase.changed','build','info',$3,'tenant',$2,1,$4,now()+interval '1 minute')",params![phase,phase.to_string(),Uuid::from_u128(1),vec![owner,member]]).await.unwrap();
    // Disable SMTP after opt-in, so even an unexpected worker bug cannot send mail.
    let mut settings = (*db.state.settings).clone();
    settings.notifications.smtp = None;
    db.state.settings = Arc::new(settings);
    tick(&db.state).await.unwrap();
    let old_count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_deliveries WHERE event_id=$1",
        params![old],
    )
    .await
    .unwrap();
    assert_eq!(old_count, 0);
    let phases: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_deliveries WHERE event_id=$1 AND channel='email'",
        params![phase],
    )
    .await
    .unwrap();
    assert_eq!(phases, 0, "non-attention build phases must not spam email");
    let email_count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_deliveries WHERE event_id=$1 AND channel='email'",
        params![new],
    )
    .await
    .unwrap();
    assert_eq!(email_count, 2);
    let webhook_count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_deliveries WHERE event_id=$1 AND channel='webhook'",
        params![new],
    )
    .await
    .unwrap();
    assert_eq!(webhook_count, 1);
    tick(&db.state).await.unwrap();
    let same_count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM notification_deliveries WHERE event_id=$1",
        params![new],
    )
    .await
    .unwrap();
    assert_eq!(same_count, 3, "routing is idempotent across worker ticks");
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn repeated_webhook_rejections_pause_only_the_endpoint() {
    let (db, user) = fixture().await;
    let endpoint = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_webhooks(id,tenant_id,created_by,name,url,enabled,secret_encrypted) VALUES($1,$2,$3,'blocked test','https://127.0.0.1/',true,'unused')",params![endpoint,Uuid::from_u128(1),user]).await.unwrap();
    for _ in 0..8 {
        sql::execute(&mut db.pool.clone(),"INSERT INTO notification_deliveries(channel,endpoint_id,generation,event_type) VALUES('webhook',$1,1,'notification.test')",params![endpoint]).await.unwrap();
    }
    tick(&db.state).await.unwrap();
    tick(&db.state).await.unwrap();
    let paused:bool=sql::scalar(&mut db.pool.clone(),"SELECT paused_until>now() AND consecutive_failures=5 AND last_status='paused' FROM notification_webhooks WHERE id=$1",params![endpoint]).await.unwrap();
    assert!(paused);
    let attempts: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT sum(attempts) FROM notification_deliveries WHERE endpoint_id=$1",
        params![endpoint],
    )
    .await
    .unwrap();
    assert_eq!(attempts, 5);
    tick(&db.state).await.unwrap();
    let same: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT sum(attempts) FROM notification_deliveries WHERE endpoint_id=$1",
        params![endpoint],
    )
    .await
    .unwrap();
    assert_eq!(same, 5, "the open circuit must not make external attempts");
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn obsolete_attention_and_current_opt_out_cancel_email_before_transport() {
    let (mut db, user) = fixture().await;
    email::bind(&db.state, user, Some("person@example.org"))
        .await
        .unwrap();
    let token = "d".repeat(64);
    install_token(&db, user, &token).await;
    email::verify(&db.state, user, &token).await.unwrap();
    email::put_preferences(&db.state, user, json!({"email":{"build":true}}))
        .await
        .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE notification_deliveries SET status='canceled' WHERE channel='verification'",
        params![],
    )
    .await
    .unwrap();
    let old = Uuid::new_v4();
    let aggregate = old.to_string();
    for (id, version, status, needs_action) in [
        (old, 1_i64, "needs_input", true),
        (Uuid::new_v4(), 2_i64, "succeeded", false),
    ] {
        sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(id,event_key,event_type,category,severity,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,needs_action,payload,occurred_at,channels_routed_at) VALUES($1,$2,'build.status.changed','build','info','account_safe',$3,$4,$5,$6,$7,$8,now()+interval '1 minute',now())",params![id,id.to_string(),user,aggregate,version,vec![user],needs_action,json!({"status":status})]).await.unwrap();
    }
    let generation: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT generation FROM notification_email_contacts WHERE user_id=$1",
        params![user],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_deliveries(channel,event_id,user_id,generation,event_type) VALUES('email',$1,$2,$3,'build.status.changed')",params![old,user,generation]).await.unwrap();
    let mut settings = (*db.state.settings).clone();
    settings.notifications.smtp = None;
    db.state.settings = Arc::new(settings);
    tick(&db.state).await.unwrap();
    let result:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',status,'attempts',attempts) FROM notification_deliveries WHERE event_id=$1",params![old]).await.unwrap();
    assert_eq!(result, json!({"status":"canceled","attempts":0}));
    let next = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_events(id,event_key,event_type,category,severity,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,occurred_at,channels_routed_at) VALUES($1,$2,'build.status.changed','build','info','account_safe',$3,$2,1,$4,now()+interval '1 minute',now())",params![next,next.to_string(),user,vec![user]]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO notification_deliveries(channel,event_id,user_id,generation,event_type) VALUES('email',$1,$2,$3,'build.status.changed')",params![next,user,generation]).await.unwrap();
    email::put_preferences(&db.state, user, json!({"email":{"build":false}}))
        .await
        .unwrap();
    tick(&db.state).await.unwrap();
    let result:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',status,'attempts',attempts) FROM notification_deliveries WHERE event_id=$1",params![next]).await.unwrap();
    assert_eq!(result, json!({"status":"canceled","attempts":0}));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn smtp_outage_allows_unrelated_preferences_without_erasing_existing_email_opt_in() {
    let (mut db, user) = fixture().await;
    email::bind(&db.state, user, Some("person@example.org"))
        .await
        .unwrap();
    let token = "e".repeat(64);
    install_token(&db, user, &token).await;
    email::verify(&db.state, user, &token).await.unwrap();
    email::put_preferences(&db.state, user, json!({"email":{"build":true}}))
        .await
        .unwrap();
    let mut settings = (*db.state.settings).clone();
    settings.notifications.smtp = None;
    db.state.settings = Arc::new(settings);
    let preferences = email::put_preferences(
        &db.state,
        user,
        json!({"in_app":{"device":false},"email":{"build":true},"locale":"en"}),
    )
    .await
    .expect("a temporary SMTP outage must not block unrelated preference changes");
    assert_eq!(preferences["in_app"]["device"], false);
    assert_eq!(preferences["locale"], "en");
    assert_eq!(preferences["email"]["build"], true);
    assert!(
        email::put_preferences(&db.state, user, json!({"email":{"release":true}}))
            .await
            .is_err(),
        "new email subscriptions still require an available SMTP configuration"
    );
    assert_eq!(
        email::preferences(&db.state, user).await.unwrap()["email"]["build"],
        true
    );
    db.close().await;
}
