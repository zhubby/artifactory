//! [INPUT]: 依赖真实 PostgreSQL、生产投影与保留清理函数。
//! [OUTPUT]: 验证并发行锁、待处理保留和事件引用安全。
//! [POS]: 通知存储维护的隔离数据库回归，不复制清理 SQL。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::retain;
use crate::store::ControlStore;
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use std::time::Duration;
use uuid::Uuid;

struct Fixture {
    admin: toasty::Db,
    pool: toasty::Db,
    store: ControlStore,
    schema: String,
    user: Uuid,
}
impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")
            .expect("explicit test database required");
        let admin = connect(&url, Default::default(), PoolSettings::new(2))
            .await
            .unwrap();
        let schema = format!("notification_retention_{}", Uuid::new_v4().simple());
        sql::execute(
            &mut admin.clone(),
            &format!("CREATE SCHEMA {schema}"),
            params![],
        )
        .await
        .unwrap();
        let pool = connect(
            &with_options(&url, &[("search_path", &schema)]).unwrap(),
            crate::store::models(),
            PoolSettings::new(8),
        )
        .await
        .unwrap();
        artifactory_database::migrate(&pool, &crate::store::MIGRATIONS)
            .await
            .unwrap();
        let store = ControlStore::new(pool.clone());
        store
            .bootstrap_admin("retention-owner".into(), "unused".into())
            .await
            .unwrap();
        let user: Uuid = sql::scalar(
            &mut pool.clone(),
            "SELECT id FROM users WHERE username='retention-owner'",
            params![],
        )
        .await
        .unwrap();
        sql::execute(
            &mut pool.clone(),
            "DELETE FROM notification_events",
            params![],
        )
        .await
        .unwrap();
        Self {
            admin,
            pool,
            store,
            schema,
            user,
        }
    }
    async fn event(&self, key: &str, version: i64, days: i32, action: bool) -> Uuid {
        let id:Uuid=sql::scalar(&mut self.pool.clone(),"INSERT INTO notification_events(event_key,event_type,category,severity,resource_type,resource_id,aggregate_key,aggregate_version,candidate_user_ids,attention,needs_action,occurred_at,channels_routed_at) VALUES($1,'build.status.changed','build','info','account',$2,$3,$4,$5,true,$6,now()-make_interval(days=>$7),now()) RETURNING id",params![format!("{key}:{version}"),self.user,key,version,vec![self.user],action,days]).await.unwrap();
        id
    }
    async fn projected(&self, key: &str, version: i64, days: i32, action: bool) -> Uuid {
        let id = self.event(key, version, days, action).await;
        assert_eq!(self.store.project_notifications(128).await.unwrap(), 1);
        id
    }
    async fn event_exists(&self, id: Uuid) -> bool {
        sql::scalar(
            &mut self.pool.clone(),
            "SELECT EXISTS(SELECT 1 FROM notification_events WHERE id=$1)",
            params![id],
        )
        .await
        .unwrap()
    }
    async fn inbox_exists(&self, key: &str) -> bool {
        sql::scalar(
            &mut self.pool.clone(),
            "SELECT EXISTS(SELECT 1 FROM notification_inbox WHERE aggregate_key=$1)",
            params![key],
        )
        .await
        .unwrap()
    }
    async fn close(self) {
        sql::execute(
            &mut self.admin.clone(),
            &format!("DROP SCHEMA {} CASCADE", self.schema),
            params![],
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn retention_keeps_actions_current_references_and_pending_deliveries() {
    let f = Fixture::new().await;
    let complete = f.projected("complete", 1, 91, false).await;
    let action_history = f.projected("action", 1, 95, false).await;
    let action = f.projected("action", 2, 91, true).await;
    let current = f.projected("current", 1, 91, false).await;
    sql::execute(
        &mut f.pool.clone(),
        "UPDATE notification_inbox SET updated_at=now() WHERE aggregate_key='current'",
        params![],
    )
    .await
    .unwrap();
    let queued = f.projected("queued", 1, 91, false).await;
    let sending = f.projected("sending", 1, 91, false).await;
    let recent = f.projected("recent", 1, 89, false).await;
    for (event, status) in [(queued, "queued"), (sending, "sending")] {
        sql::execute(&mut f.pool.clone(),"INSERT INTO notification_deliveries(channel,event_id,user_id,generation,event_type,status,lease_until) VALUES('email',$1,$2,1,'build.status.changed',$3,now()+interval '90 seconds')",params![event,f.user,status]).await.unwrap();
    }
    let unprojected = f.event("unprojected", 1, 91, false).await;
    let unrouted = f.event("unrouted", 1, 91, false).await;
    sql::execute(
        &mut f.pool.clone(),
        "UPDATE notification_events SET projected_at=now(),channels_routed_at=NULL WHERE id=$1",
        params![unrouted],
    )
    .await
    .unwrap();
    retain(&f.pool).await.unwrap();
    assert!(!f.inbox_exists("complete").await);
    assert!(!f.event_exists(complete).await);
    for key in ["action", "current", "recent"] {
        assert!(f.inbox_exists(key).await, "must retain {key}");
    }
    for id in [
        action_history,
        action,
        current,
        queued,
        sending,
        recent,
        unprojected,
        unrouted,
    ] {
        assert!(
            f.event_exists(id).await,
            "live reference or unfinished work must preserve event"
        );
    }
    assert!(!f.inbox_exists("queued").await && !f.inbox_exists("sending").await);
    sql::execute(
        &mut f.pool.clone(),
        "UPDATE notification_deliveries SET status='succeeded'",
        params![],
    )
    .await
    .unwrap();
    retain(&f.pool).await.unwrap();
    assert!(!f.event_exists(queued).await && !f.event_exists(sending).await);
    assert!(f.event_exists(current).await);
    f.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn retention_skips_a_locked_card_while_projection_adds_new_action() {
    let f = Fixture::new().await;
    f.projected("race", 1, 91, false).await;
    let mut connection = f.pool.clone();
    let mut holder = connection.transaction().await.unwrap();
    let holder_pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![])
        .await
        .unwrap();
    let _: Uuid = sql::scalar(
        &mut holder,
        "SELECT id FROM notification_inbox WHERE aggregate_key='race' FOR UPDATE",
        params![],
    )
    .await
    .unwrap();
    f.event("race", 2, 0, true).await;
    let store = f.store.clone();
    let projection = tokio::spawn(async move { store.project_notifications(128).await });
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let blocked:bool=sql::scalar(&mut f.pool.clone(),"SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE $1=ANY(pg_blocking_pids(a.pid)))",params![holder_pid]).await.unwrap();
            if blocked {break}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("real projector must wait behind the inbox row lock");
    tokio::time::timeout(Duration::from_secs(2), retain(&f.pool))
        .await
        .expect("cleanup must skip the locked card without waiting")
        .unwrap();
    assert!(f.inbox_exists("race").await);
    holder.commit().await.unwrap();
    assert_eq!(projection.await.unwrap().unwrap(), 1);
    let state: (i64, bool) = sql::tuple(
        &mut f.pool.clone(),
        "SELECT content_revision,needs_action FROM notification_inbox WHERE aggregate_key='race'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(state, (2, true));
    sql::execute(&mut f.pool.clone(),"UPDATE notification_inbox SET updated_at=now()-interval '91 days' WHERE aggregate_key='race'",params![]).await.unwrap();
    retain(&f.pool).await.unwrap();
    assert!(
        f.inbox_exists("race").await,
        "pending action survives even beyond retention age"
    );
    f.close().await;
}
