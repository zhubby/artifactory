//! [INPUT]: 依赖真实PostgreSQL、原登录Session和租户资源锁。
//! [OUTPUT]: 验证撤权后复验与设备授权的行锁顺序。
//! [POS]: 租户验收中的权限等待和外键死锁回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn project_member_mutations_recheck_authority_after_revocation() {
    use artifactory_control_plane::auth::Auth;
    use axum::extract::FromRequestParts;

    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (admin_id, admin) = account(&db, "project-admin", "viewer").await;
    let (member_id, _) = account(&db, "member", "viewer").await;
    let t = tenant(&db, &owner, "Team").await;
    let p = project(&db, &owner, t, "App").await;
    for name in ["project-admin", "member"] {
        assert_eq!(
            request(
                &db,
                &owner,
                Some(t),
                "POST",
                "/tenant/members",
                json!({"username":name,"role":"member"})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let member_path = format!("/projects/{p}/members/{admin_id}");
    assert_eq!(
        request(
            &db,
            &owner,
            Some(t),
            "PUT",
            &member_path,
            json!({"role":"admin"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (mut parts, _) = Request::builder()
        .uri(format!("/api/v1/projects/{p}/members/{member_id}"))
        .header("cookie", &admin)
        .header("X-Artifactory-Tenant", t.to_string())
        .body(())
        .unwrap()
        .into_parts();
    let auth = Auth::from_request_parts(&mut parts, &db.state)
        .await
        .unwrap();
    auth.project(p, "admin").unwrap();
    let stale_store = db.state.store.for_auth(&auth);
    assert_eq!(
        request(&db, &owner, Some(t), "DELETE", &member_path, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert!(matches!(
        stale_store
            .set_project_member(p, member_id, Some("admin"))
            .await,
        Err(artifactory_control_plane::error::ApiError::Forbidden)
    ));
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM project_members WHERE project_id=$1 AND user_id=$2",
        params![p, member_id],
    )
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "a queued mutation cannot restore revoked project authority"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn device_grant_changes_do_not_deadlock_task_tenant_foreign_keys() {
    use artifactory_control_plane::auth::Auth;
    use artifactory_control_plane::store::{NodeAccess, NodeAccessMode};
    use axum::extract::FromRequestParts;

    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let t = tenant(&db, &owner, "Team").await;
    let p = project(&db, &owner, t, "App").await;
    let (mut parts, _) = Request::builder()
        .uri(format!("/api/v1/projects/{p}/node-access"))
        .header("cookie", owner)
        .header("X-Artifactory-Tenant", t.to_string())
        .body(())
        .unwrap()
        .into_parts();
    let auth = Auth::from_request_parts(&mut parts, &db.state)
        .await
        .unwrap();
    let store = db.state.store.for_auth(&auth);
    let mut pool = db.pool.clone();
    let mut assignment = pool.transaction().await.unwrap();
    let pid: i32 = sql::scalar(&mut assignment, "SELECT pg_backend_pid()", params![])
        .await
        .unwrap();
    sql::execute(
        &mut assignment,
        "SELECT id FROM projects WHERE id=$1 FOR SHARE",
        params![p],
    )
    .await
    .unwrap();
    let changing = tokio::spawn(async move {
        store
            .set_node_access(
                p,
                &NodeAccess {
                    mode: NodeAccessMode::Nodes,
                    ids: vec![],
                },
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sql::scalar(&mut db.pool.clone(), "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE 'UPDATE projects SET node_access_mode%')", params![pid]).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    // Inserting an attempt takes this same lock for its tenant foreign key.
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        sql::execute(
            &mut assignment,
            "SELECT id FROM tenants WHERE id=$1 FOR KEY SHARE",
            params![t],
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assignment.commit().await.unwrap();
    changing.await.unwrap().unwrap();
    db.close().await;
}
