//! [INPUT]: 依赖真实构建事件、项目关注和个人聚合收件箱。
//! [OUTPUT]: 验证取消关注后静默更新已有事项，不增加未读。
//! [POS]: 通知订阅边界的 PostgreSQL 回归测试。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn notification_unfollow_resolves_existing_action_without_new_attention() {
    let db = Database::with_tenant().await;
    let (project, build, _) = db.fixture().await;
    let uid = user(&db).await;
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events",
        params![],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='running' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    db.state
        .store
        .set_notification_subscription(uid, project, true)
        .await
        .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='needs_input' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    db.state.store.project_notifications(100).await.unwrap();
    let first = db
        .state
        .store
        .list_notifications(uid, NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.needs_action_count, 1);
    let original = &first.items[0];
    db.state
        .store
        .read_notification(uid, original.id, original.attention_revision)
        .await
        .unwrap();
    db.state
        .store
        .set_notification_subscription(uid, project, false)
        .await
        .unwrap();

    for status in ["running", "succeeded"] {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET status=$2 WHERE id=$1",
            params![build, status],
        )
        .await
        .unwrap();
        db.state.store.project_notifications(100).await.unwrap();
        let current = db
            .state
            .store
            .list_notifications(uid, NotificationFilter::default())
            .await
            .unwrap();
        assert_eq!(current.items.len(), 1);
        assert_eq!(
            current.needs_action_count, 0,
            "unfollow must not strand an action after it resolves"
        );
        assert_eq!(current.unread_count, 0, "unfollow prevents new attention");
        assert_eq!(
            current.items[0].attention_revision,
            original.attention_revision
        );
        assert!(current.items[0].content_revision > original.content_revision);
    }
    let detail = db
        .state
        .store
        .notification_detail(uid, original.id, None)
        .await
        .unwrap();
    assert_eq!(
        detail.events.len(),
        3,
        "timeline includes silent resolution but excludes pre-follow history"
    );
    let candidates: Vec<Uuid> = sql::scalar(&mut db.pool.clone(), "SELECT candidate_user_ids FROM notification_events WHERE aggregate_key=$1 ORDER BY aggregate_version DESC LIMIT 1", params![format!("build:{build}")]).await.unwrap();
    assert!(
        !candidates.contains(&uid),
        "silent updates must not enroll external delivery candidates"
    );
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM notification_events WHERE aggregate_key=$1 AND aggregate_version<$2",
        params![format!("build:{build}"), detail.item.content_revision],
    )
    .await
    .unwrap();
    let retained = db
        .state
        .store
        .notification_detail(uid, original.id, None)
        .await
        .unwrap();
    assert_eq!(
        retained.events.len(),
        1,
        "retention of the first candidate event must not hide the current silent update"
    );
    db.close().await;
}
