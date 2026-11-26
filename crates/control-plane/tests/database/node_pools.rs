//! [INPUT]: 依赖隔离 PostgreSQL、租户会话与设备夹具。
//! [OUTPUT]: 验证节点池权限、引用保护、注册锁序、审计和设备筛选。
//! [POS]: 租户管理验收；使用真实 HTTP 与数据库约束。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn deleting_a_pool_does_not_deadlock_an_active_enrollment() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let tenant = tenant(&db, &owner, "A").await;
    let (_, value) = request(
        &db,
        &owner,
        Some(tenant),
        "POST",
        "/pools",
        json!({"name":"Builders"}),
    )
    .await;
    let pool = Uuid::parse_str(value["id"].as_str().unwrap()).unwrap();
    let token = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO enrollment_tokens(id,token_hash,pool_id,expires_at) VALUES($1,$2,$3,now()+interval '15 minutes')", params![token,token.to_string(),pool]).await.unwrap();
    let mut connection = db.pool.clone();
    let mut enrollment = connection.transaction().await.unwrap();
    sql::execute(
        &mut enrollment,
        "SELECT id FROM enrollment_tokens WHERE id=$1 FOR UPDATE",
        params![token],
    )
    .await
    .unwrap();
    let path = format!("/pools/{pool}");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        request(&db, &owner, Some(tenant), "DELETE", &path, json!({})),
    )
    .await;
    enrollment.rollback().await.unwrap();
    assert_eq!(
        result
            .expect("pool deletion must not wait for an enrollment token lock")
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&db, &owner, Some(tenant), "DELETE", &path, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_pool_management_enforces_tenant_permissions_and_safe_deletion() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (member_id, member) = account(&db, "member", "admin").await;
    let a = tenant(&db, &owner, "A").await;
    let b = tenant(&db, &owner, "B").await;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'member')",
        params![a, member_id],
    )
    .await
    .unwrap();
    let (status, value) = request(
        &db,
        &owner,
        Some(a),
        "POST",
        "/pools",
        json!({"name":"  Linux builders  "}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let id = Uuid::parse_str(value["id"].as_str().unwrap()).unwrap();
    let path = format!("/pools/{id}");
    for (cookie, tenant, expected) in [
        (&member, a, StatusCode::FORBIDDEN),
        (&owner, b, StatusCode::NOT_FOUND),
    ] {
        for method in ["PATCH", "DELETE"] {
            let (status, body) = request(
                &db,
                cookie,
                Some(tenant),
                method,
                &path,
                json!({"name":"Changed"}),
            )
            .await;
            assert_eq!(status, expected, "{method}: {body}");
        }
    }
    let (status, pools) = request(&db, &owner, Some(a), "GET", "/pools", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let pool = pools
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id.to_string())
        .unwrap();
    assert_eq!(pool["name"], "Linux builders");
    assert_eq!(pool["node_count"], 0);
    for name in ["   ".to_string(), "中".repeat(34)] {
        assert_eq!(
            request(&db, &owner, Some(a), "PATCH", &path, json!({"name":name}))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "PATCH",
            &path,
            json!({"name":"macOS builders"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "POST",
            "/pools",
            json!({"name":"macOS builders"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let default: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM node_pools WHERE tenant_id=$1 AND is_default",
        params![a],
    )
    .await
    .unwrap();
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "DELETE",
            &format!("/pools/{default}"),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let node = device(&db, a).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET pool_id=$2 WHERE id=$1",
        params![node, id],
    )
    .await
    .unwrap();
    assert_eq!(
        request(&db, &owner, Some(a), "DELETE", &path, json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET pool_id=$2 WHERE id=$1",
        params![node, default],
    )
    .await
    .unwrap();
    let project = project(&db, &owner, a, "Project").await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE projects SET pool_ids=$2 WHERE id=$1",
        params![project, vec![id]],
    )
    .await
    .unwrap();
    assert_eq!(
        request(&db, &owner, Some(a), "DELETE", &path, json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE projects SET pool_ids='{}' WHERE id=$1",
        params![project],
    )
    .await
    .unwrap();
    let release = Uuid::new_v4();
    let update = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES($1,$2,'0.2.0','{}','test','hash')", params![a,release]).await.unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status,finished_at) VALUES($1,$2,$3,$4,'succeeded',now())", params![update,node,id,release]).await.unwrap();
    assert_eq!(
        request(&db, &owner, Some(a), "DELETE", &path, json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM emissary_updates WHERE id=$1",
        params![update],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(), "INSERT INTO enrollment_tokens(id,token_hash,pool_id,expires_at) VALUES($1,$2,$3,now()+interval '15 minutes')", params![Uuid::new_v4(),id.to_string(),id]).await.unwrap();
    assert_eq!(
        request(&db, &owner, Some(a), "DELETE", &path, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&db, &owner, Some(a), "DELETE", &path, json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let actions: Vec<String> = sql::scalars(
        &mut db.pool.clone(),
        "SELECT action FROM audit_events WHERE resource_id=$1 ORDER BY id",
        params![id.to_string()],
    )
    .await
    .unwrap();
    assert_eq!(actions, ["pool_created", "pool_renamed", "pool_deleted"]);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_pool_counts_and_device_filters_respect_project_visibility() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (member_id, member) = account(&db, "member", "admin").await;
    let a = tenant(&db, &owner, "A").await;
    let b = tenant(&db, &owner, "B").await;
    let project = project(&db, &owner, a, "Project").await;
    let allowed = device(&db, a).await;
    device(&db, a).await;
    let foreign = device(&db, b).await;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'member')",
        params![a, member_id],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO project_members(tenant_id,project_id,user_id,role) VALUES($1,$2,$3,'viewer')",
        params![a, project, member_id],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE projects SET node_access_mode='nodes',node_ids=$2 WHERE id=$1",
        params![project, vec![allowed]],
    )
    .await
    .unwrap();
    let pool: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT pool_id FROM nodes WHERE id=$1",
        params![allowed],
    )
    .await
    .unwrap();
    let foreign_pool: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT pool_id FROM nodes WHERE id=$1",
        params![foreign],
    )
    .await
    .unwrap();
    let (_, pools) = request(&db, &member, Some(a), "GET", "/pools", json!({})).await;
    assert_eq!(pools[0]["node_count"], 1);
    assert_eq!(pools[0]["online_count"], 1);
    let (_, nodes) = request(
        &db,
        &member,
        Some(a),
        "GET",
        &format!("/nodes?pool_id={pool}"),
        json!({}),
    )
    .await;
    assert_eq!(nodes.as_array().unwrap().len(), 1);
    assert_eq!(nodes[0]["id"], allowed.to_string());
    let (_, nodes) = request(
        &db,
        &owner,
        Some(a),
        "GET",
        &format!("/nodes?pool_id={foreign_pool}"),
        json!({}),
    )
    .await;
    assert_eq!(nodes, json!([]));
    db.close().await;
}
