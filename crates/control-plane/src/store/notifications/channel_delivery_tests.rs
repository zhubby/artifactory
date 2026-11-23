//! [INPUT]: 依赖隔离 PostgreSQL、真实 SMTP 连接与邮箱退役。
//! [OUTPUT]: 验证 SMTP 总超时、重试及退役后的晚结果围栏。
//! [POS]: 渠道事务和网络边界的数据库并发回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use crate::notifications::channels::{email, tick_channel};
use artifactory_database::{PoolSettings, connect, with_options};
use std::{collections::HashMap, sync::Arc, time::Duration};

struct SmtpFixture {
    admin: toasty::Db,
    pool: toasty::Db,
    schema: String,
    state: AppState,
    user: Uuid,
}

async fn smtp_fixture(port: u16) -> anyhow::Result<SmtpFixture> {
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
    let admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
    let schema = format!("notification_network_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut admin.clone(),
        &format!("CREATE SCHEMA {schema}"),
        params![],
    )
    .await?;
    let scoped = with_options(&url, &[("search_path", &schema)])?;
    let pool = connect(&scoped, crate::store::models(), PoolSettings::new(8)).await?;
    artifactory_database::migrate(&pool, &crate::store::MIGRATIONS).await?;
    let user = Uuid::new_v4();
    sql::execute(
        &mut pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused','viewer')",
        params![user, user.to_string()],
    )
    .await?;
    let settings = crate::settings::Settings {
        notifications: crate::settings::NotificationSettings {
            smtp: Some(crate::settings::SmtpSettings {
                host: "127.0.0.1".into(),
                port,
                from: "notify@example.org".into(),
                username: None,
                password: None,
                starttls: false,
            }),
            ..Default::default()
        },
        database_url: scoped,
        http_bind: "127.0.0.1:0".into(),
        grpc_bind: "127.0.0.1:0".into(),
        public_url: "https://example.org".into(),
        insecure_dev: true,
        registration_enabled: true,
        pki_dir: ".local/pki".into(),
        data_dir: std::env::temp_dir(),
        master_key: [1; 32],
        signing_keys: None,
        s3_endpoint: "http://127.0.0.1:19000".into(),
        s3_public_endpoint: "http://127.0.0.1:19000".into(),
        s3_bucket: "test".into(),
        s3_region: "us-east-1".into(),
        s3_access_key: "test".into(),
        s3_secret_key: "test".into(),
        telemetry: None,
        control: Default::default(),
    };
    let state = AppState {
        coordination: Default::default(),
        store: crate::store::ControlStore::new(pool.clone()),
        storage: Arc::new(crate::storage::Storage::new(&settings)?),
        settings: Arc::new(settings),
        connections: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
    };
    Ok(SmtpFixture {
        admin,
        pool,
        schema,
        state,
        user,
    })
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn stalled_smtp_does_not_lock_binding_or_overwrite_retirement() -> anyhow::Result<()> {
    // 暂停真实 TLS 握手，证明网络等待期间绑定可撤销。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let _ = accepted_tx.send(());
        let _ = release_rx.await;
        drop(socket);
    });
    let SmtpFixture {
        admin,
        pool,
        schema,
        state,
        user,
    } = smtp_fixture(port).await?;
    email::bind(&state, user, Some("person@example.org")).await?;
    let job_id: Uuid = sql::scalar(
        &mut pool.clone(),
        "SELECT id FROM notification_deliveries WHERE user_id=$1",
        params![user],
    )
    .await?;
    let attempt_state = state.clone();
    let attempt = tokio::spawn(async move { tick_channel(&attempt_state, "verification").await });
    tokio::time::timeout(Duration::from_secs(5), accepted_rx).await??;
    let retirement =
        tokio::time::timeout(Duration::from_secs(1), email::retire(&state, user)).await;
    let completed_without_network = retirement.is_ok();
    let _ = release_tx.send(());
    server.await?;
    tokio::time::timeout(Duration::from_secs(5), attempt).await???;
    if !completed_without_network {
        email::retire(&state, user).await?;
    }
    let result: Value = sql::scalar(&mut pool.clone(),"SELECT jsonb_build_object('status',status,'attempts',attempts) FROM notification_deliveries WHERE id=$1",params![job_id]).await?;
    sql::execute(
        &mut admin.clone(),
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    assert!(
        completed_without_network,
        "SMTP wait must not hold the address binding transaction"
    );
    assert_eq!(
        result,
        json!({"status":"canceled","attempts":0}),
        "late transport results must not revive a retired binding"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn stalled_smtp_exits_before_lease_expiry_and_preserves_retry() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let _ = accepted_tx.send(());
        let _ = release_rx.await;
        drop(socket);
    });
    let SmtpFixture {
        admin,
        pool,
        schema,
        state,
        user,
    } = smtp_fixture(port).await?;
    email::bind(&state, user, Some("person@example.org")).await?;
    let attempt_state = state.clone();
    let mut attempt =
        tokio::spawn(async move { tick_channel(&attempt_state, "verification").await });
    tokio::time::timeout(Duration::from_secs(5), accepted_rx).await??;
    let completed = tokio::time::timeout(Duration::from_secs(22), &mut attempt).await;
    let bounded_without_peer_release = completed.is_ok();
    let _ = release_tx.send(());
    server.await?;
    if let Ok(result) = completed {
        result??;
    } else {
        tokio::time::timeout(Duration::from_secs(5), attempt).await???;
    }
    let result: Value = sql::scalar(&mut pool.clone(),"SELECT jsonb_build_object('status',status,'attempts',attempts,'leased',lease_until IS NOT NULL) FROM notification_deliveries WHERE user_id=$1",params![user]).await?;
    sql::execute(
        &mut admin.clone(),
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    assert!(
        bounded_without_peer_release,
        "SMTP TLS wait must time out before the 90-second delivery lease expires"
    );
    assert_eq!(
        result,
        json!({"status":"queued","attempts":1,"leased":false}),
        "SMTP timeout must release the lease and retain a retryable job"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn verification_retry_preserves_accepted_link_and_original_expiry() -> anyhow::Result<()> {
    let SmtpFixture {
        admin,
        pool,
        schema,
        state,
        user,
    } = smtp_fixture(465).await?;
    email::bind(&state, user, Some("first@example.org")).await?;
    let expires_at: chrono::DateTime<chrono::Utc> = sql::scalar(
        &mut pool.clone(),
        "SELECT token_expires_at FROM notification_email_contacts WHERE user_id=$1",
        params![user],
    )
    .await?;
    let first_lease = Uuid::new_v4();
    let first = claim(&state, "verification", first_lease).await?.unwrap();
    let first_token = prepare(&state, &first, first_lease)
        .await?
        .unwrap()
        .verification_token
        .unwrap();
    // 收件端接受邮件后，丢失成功响应。队列只能重试，不能撤销已收到的链接。
    finish(&state, &first, first_lease, Err(true)).await?;
    sql::execute(
        &mut pool.clone(),
        "UPDATE notification_deliveries SET next_attempt_at=now() WHERE id=$1",
        params![uuid(&first, "id")?],
    )
    .await?;
    let retry_lease = Uuid::new_v4();
    let retry = claim(&state, "verification", retry_lease).await?.unwrap();
    let retry_token = prepare(&state, &retry, retry_lease)
        .await?
        .unwrap()
        .verification_token
        .unwrap();
    let retry_expiry: chrono::DateTime<chrono::Utc> = sql::scalar(
        &mut pool.clone(),
        "SELECT token_expires_at FROM notification_email_contacts WHERE user_id=$1",
        params![user],
    )
    .await?;
    assert_eq!(
        first_token, retry_token,
        "an accepted verification link must survive retries with the same Message-ID"
    );
    assert_eq!(
        expires_at, retry_expiry,
        "retries must not extend the original verification window"
    );
    email::verify(&state, user, &first_token).await?;
    assert!(
        email::verify(&state, user, &first_token).await.is_err(),
        "verification remains single-use"
    );
    sql::execute(&mut pool.clone(), "UPDATE notification_email_contacts SET last_requested_at=now()-interval '2 minutes' WHERE user_id=$1", params![user]).await?;
    email::bind(&state, user, Some("second@example.org")).await?;
    let new_lease = Uuid::new_v4();
    let new_job = claim(&state, "verification", new_lease).await?.unwrap();
    let new_token = prepare(&state, &new_job, new_lease)
        .await?
        .unwrap()
        .verification_token
        .unwrap();
    assert_ne!(
        first_token, new_token,
        "a new binding and job must have a different token"
    );
    assert!(
        prepare(&state, &retry, retry_lease).await?.is_none(),
        "a canceled old job cannot supersede the new token"
    );
    assert!(email::verify(&state, user, &first_token).await.is_err());
    email::verify(&state, user, &new_token).await?;
    sql::execute(
        &mut admin.clone(),
        &format!("DROP SCHEMA {schema} CASCADE"),
        params![],
    )
    .await?;
    Ok(())
}
