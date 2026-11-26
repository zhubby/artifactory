//! [INPUT]: 依赖真实PostgreSQL、租户会话和跨租户资源引用。
//! [OUTPUT]: 验证历史、制品、流与源码计划引用的租户隔离。
//! [POS]: 租户验收中的对象访问隔离回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn cross_tenant_history_artifacts_streams_and_operations_are_rejected() {
    let db = Database::new().await;
    let (_, alice) = account(&db, "alice", "viewer").await;
    let (_, bob) = account(&db, "bob", "viewer").await;
    let a = tenant(&db, &alice, "A").await;
    let b = tenant(&db, &bob, "B").await;
    let p = project(&db, &bob, b, "Private").await;
    let node = device(&db, b).await;
    let build = Uuid::new_v4();
    let job = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    let release = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status,release_id,release_version) VALUES($1,$2,'main','https://example.com/app.git','succeeded', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))",params![build,p]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO jobs(id,build_id,target,target_config) VALUES($1,$2,'app','{}')",
        params![job, build],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id) VALUES($1,$2,$3,'private-lease',now(),'succeeded',$4)",params![attempt,job,node,Uuid::new_v4()]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'app','private/app',1,'hash',now())",params![artifact,attempt]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'app','private/app',1,'hash')",params![artifact,attempt]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO releases(id,project_id,version,notes) VALUES($1,$2,'1.0.0','private')",
        params![release, p],
    )
    .await
    .unwrap();
    for path in [
        format!("/builds/{build}"),
        format!("/builds/{build}/plans"),
        format!("/builds/{build}/events"),
        format!("/attempts/{attempt}/telemetry"),
        format!("/attempts/{attempt}/processes"),
        format!("/artifacts/{artifact}/download"),
        format!("/releases/{release}"),
    ] {
        assert_eq!(
            request(&db, &alice, Some(a), "GET", &path, json!({}))
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    for (method, path, body) in [
        ("POST", format!("/builds/{build}/cancel"), json!({})),
        ("POST", format!("/jobs/{job}/retry"), json!({})),
        (
            "PATCH",
            format!("/nodes/{node}/emissary-update"),
            json!({"release_id":null}),
        ),
    ] {
        assert!(
            [StatusCode::FORBIDDEN, StatusCode::NOT_FOUND]
                .contains(&request(&db, &alice, Some(a), method, &path, body).await.0),
            "{path}"
        );
    }
    for path in [
        "/builds",
        "/releases",
        "/artifacts",
        "/nodes",
        "/credentials",
        "/model-profiles",
        "/emissary-updates",
    ] {
        let (status, list) = request(&db, &alice, Some(a), "GET", path, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{path}: {list}");
        assert_eq!(list, json!([]), "{path}");
    }
    let (_, invite) = request(
        &db,
        &alice,
        Some(a),
        "POST",
        "/tenant/invitations",
        json!({"role":"member"}),
    )
    .await;
    let id = Uuid::parse_str(invite["id"].as_str().unwrap()).unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE tenant_invitations SET expires_at=now()-interval '1 second' WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(
        request(
            &db,
            &bob,
            None,
            "POST",
            &format!("/invitations/{}/accept", invite["token"].as_str().unwrap()),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn source_and_plan_references_cannot_cross_tenant_boundaries() {
    let db = Database::new().await;
    let (_, alice) = account(&db, "alice", "viewer").await;
    let (_, bob) = account(&db, "bob", "viewer").await;
    let a = tenant(&db, &alice, "A").await;
    let b = tenant(&db, &bob, "B").await;
    let pa = project(&db, &alice, a, "A").await;
    let pb = project(&db, &bob, b, "B").await;
    let manifest = Uuid::new_v4();
    assert!(sql::execute(&mut db.pool.clone(),"INSERT INTO source_manifests(id,tenant_id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,'{}',$5)",params![manifest,b,pa,"0".repeat(40),"a".repeat(64)]).await.is_err());
    db.source(pb, manifest).await;
    let build = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,source_repository_url,release_id,release_version) VALUES($1,$2,'main','https://example.com/repo.git', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))", params![build,pa]).await.unwrap();
    assert!(
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET source_manifest_id=$2 WHERE id=$1",
            params![build, manifest]
        )
        .await
        .is_err()
    );
    db.close().await;
}
