//! [INPUT]: 依赖真实租户会话、项目成员授权与概览 HTTP 查询。
//! [OUTPUT]: 验证概览隔离、直接项目授权和非法参数拒绝。
//! [POS]: 租户数据库验收中的概览接口回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn dashboard_http_rechecks_tenant_and_project_permissions() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "dashboard-owner", "viewer").await;
    let (member_id, member) = account(&db, "dashboard-member", "viewer").await;
    let tenant_id = tenant(&db, &owner, "dashboard").await;
    let visible = project(&db, &owner, tenant_id, "visible").await;
    let hidden = project(&db, &owner, tenant_id, "hidden").await;
    let other_tenant = tenant(&db, &owner, "other").await;
    let other = project(&db, &owner, other_tenant, "other").await;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'member')",
        params![tenant_id, member_id],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO project_members(tenant_id,project_id,user_id,role) VALUES($1,$2,$3,'viewer')",
        params![tenant_id, visible, member_id],
    )
    .await
    .unwrap();
    for project_id in [visible, hidden, other] {
        sql::execute(&mut db.pool.clone(),"WITH r AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$1,'1.0.0') RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version,created_at,finished_at) SELECT gen_random_uuid(),$1,'main','succeeded','https://example.com/repo.git',id,version,now()-interval '1 minute',now() FROM r",params![project_id]).await.unwrap();
    }
    let (status, data) = request(
        &db,
        &member,
        Some(tenant_id),
        "GET",
        "/dashboard?days=30&timezone=UTC",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{data}");
    assert_eq!(data["summary"]["total_projects"], 1);
    assert_eq!(data["summary"]["builds_created"], 1);
    assert_eq!(data["recent_projects"][0]["id"], visible.to_string());
    for project_id in [hidden, other] {
        let (status, _) = request(
            &db,
            &member,
            Some(tenant_id),
            "GET",
            &format!("/dashboard?project_id={project_id}"),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (status, _) = request(
        &db,
        &member,
        Some(other_tenant),
        "GET",
        "/dashboard",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for query in [
        "days=1",
        "timezone=Unknown%2FPlace",
        "project_id=invalid",
        "days=7&unexpected=1",
    ] {
        let (status, _) = request(
            &db,
            &member,
            Some(tenant_id),
            "GET",
            &format!("/dashboard?{query}"),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM project_members WHERE project_id=$1 AND user_id=$2",
        params![visible, member_id],
    )
    .await
    .unwrap();
    let (_, data) = request(
        &db,
        &member,
        Some(tenant_id),
        "GET",
        "/dashboard",
        json!({}),
    )
    .await;
    assert_eq!(data["summary"]["total_projects"], 0);
    assert_eq!(data["summary"]["builds_created"], 0);
    db.close().await;
}
