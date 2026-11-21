//! [INPUT]: 依赖租户夹具、构建源码任务和 UUID 鉴权。
//! [OUTPUT]: 验证 slug、构建序号与项目权限。
//! [POS]: 可读地址的数据库与 HTTP 回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn fresh_resources_receive_stable_routes_without_identity_changes() {
    let db = Database::with_tenant().await;
    let (project, build, _) = db.fixture().await;
    let second = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,status,source_repository_url,release_id,release_version) VALUES($1,$2,'main','failed','https://example.com/repo.git', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))",
        params![second, project],
    )
    .await
    .unwrap();
    artifactory_database::migrate(&db.pool, &artifactory_control_plane::store::MIGRATIONS)
        .await
        .unwrap();
    let before: Vec<Value> = sql::scalars(&mut db.pool.clone(),"SELECT jsonb_build_object('id',b.id,'number',b.number,'project_slug',p.slug,'tenant_slug',t.slug) FROM builds b JOIN projects p ON p.id=b.project_id JOIN tenants t ON t.id=p.tenant_id ORDER BY b.created_at,b.id",params![]).await.unwrap();
    assert_eq!(before.len(), 2);
    assert_eq!(before[0]["id"], build.to_string());
    assert_eq!(before[0]["number"], 1);
    assert_eq!(before[1]["id"], second.to_string());
    assert_eq!(before[1]["number"], 2);
    assert_eq!(before[0]["project_slug"], "test");
    assert_eq!(before[0]["tenant_slug"], "default");
    artifactory_database::migrate(&db.pool, &artifactory_control_plane::store::MIGRATIONS)
        .await
        .unwrap();
    let third: i64 = sql::scalar(
        &mut db.pool.clone(),
        "WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,source_repository_url,release_id,release_version) VALUES($1,$2,'main','https://example.com/repo.git', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release)) RETURNING number",
        params![Uuid::new_v4(), project],
    )
    .await
    .unwrap();
    assert_eq!(third, 3);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn slugs_are_unique_scoped_and_stable_after_renaming() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "owner", "viewer").await;
    let input = json!({"name":"Desktop Team!","idempotency_key":Uuid::new_v4()});
    let (status, first) = request(&db, &cookie, None, "POST", "/tenants", input.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["slug"], "desktop-team");
    assert_eq!(
        request(&db, &cookie, None, "POST", "/tenants", input)
            .await
            .1,
        first
    );
    let a = Uuid::parse_str(first["id"].as_str().unwrap()).unwrap();
    let b = tenant(&db, &cookie, "Desktop Team!").await;
    let (_, second) = request(
        &db,
        &cookie,
        None,
        "GET",
        "/tenants/desktop-team-2",
        json!({}),
    )
    .await;
    assert_eq!(second["id"], b.to_string());
    let p = project(&db, &cookie, a, "Desktop App").await;
    project(&db, &cookie, a, "Desktop App").await;
    let other = project(&db, &cookie, b, "Desktop App").await;
    for (tenant, slug, expected) in [(a, "desktop-app", p), (b, "desktop-app", other)] {
        let (status, value) = request(
            &db,
            &cookie,
            Some(tenant),
            "GET",
            &format!("/projects/by-slug/{slug}"),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["id"], expected.to_string());
        assert_eq!(value["slug"], slug);
    }
    let (_, list) = request(&db, &cookie, Some(a), "GET", "/projects", json!({})).await;
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|p| p["slug"] == "desktop-app-2")
    );
    let (status, renamed) = request(
        &db,
        &cookie,
        Some(a),
        "PATCH",
        &format!("/projects/{p}"),
        json!({"name":"Renamed product","repository_url":"https://example.com/repository.git"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE tenants SET name='Renamed team' WHERE id=$1",
        params![a],
    )
    .await
    .unwrap();
    let (_, value) = request(
        &db,
        &cookie,
        Some(a),
        "GET",
        "/projects/by-slug/desktop-app",
        json!({}),
    )
    .await;
    assert_eq!(value["name"], "Renamed product");
    let (_, value) = request(
        &db,
        &cookie,
        None,
        "GET",
        "/tenants/desktop-team",
        json!({}),
    )
    .await;
    assert_eq!(value["name"], "Renamed team");
    let c = tenant(&db, &cookie, "开发团队").await;
    let (_, value) = request(&db, &cookie, None, "GET", "/tenants/tenant", json!({})).await;
    assert_eq!(value["id"], c.to_string());
    project(&db, &cookie, c, "桌面应用").await;
    let (_, value) = request(
        &db,
        &cookie,
        Some(c),
        "GET",
        "/projects/by-slug/project",
        json!({}),
    )
    .await;
    assert_eq!(value["slug"], "project");
    assert!(
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE projects SET slug='moved' WHERE id=$1",
            params![p]
        )
        .await
        .is_err()
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn readable_lookup_checks_membership_project_grants_and_token_scopes() {
    let db = Database::new().await;
    let (_, alice) = account(&db, "alice", "viewer").await;
    let (_, bob) = account(&db, "bob", "admin").await;
    let (reader_id, reader) = account(&db, "reader", "admin").await;
    let a = tenant(&db, &alice, "Alice").await;
    let b = tenant(&db, &bob, "Bob").await;
    let p = project(&db, &alice, a, "Visible").await;
    project(&db, &alice, a, "Hidden").await;
    project(&db, &bob, b, "Visible").await;
    let (_, build) = request(
        &db,
        &alice,
        Some(a),
        "POST",
        &db.release_build_path(p).await,
        crate::fixture::build_input(),
    )
    .await;
    assert_eq!(build["number"], 1);
    assert_eq!(build["project_slug"], "visible");
    request(
        &db,
        &alice,
        Some(a),
        "POST",
        "/tenant/members",
        json!({"username":"reader","role":"member"}),
    )
    .await;
    request(
        &db,
        &alice,
        Some(a),
        "PUT",
        &format!("/projects/{p}/members/{reader_id}"),
        json!({"role":"viewer"}),
    )
    .await;
    for slug in ["alice", "%61lice"] {
        assert_eq!(
            request(
                &db,
                &bob,
                None,
                "GET",
                &format!("/tenants/{slug}"),
                json!({})
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    for slug in ["visible", "%76isible"] {
        let path = format!("/projects/by-slug/{slug}/builds/1");
        let (status, value) = request(&db, &reader, Some(a), "GET", &path, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["id"], build["id"]);
        assert_eq!(
            request(&db, &bob, Some(a), "GET", &path, json!({})).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&db, &bob, Some(b), "GET", &path, json!({})).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &db,
            &reader,
            Some(a),
            "GET",
            "/projects/by-slug/hidden",
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for scopes in [json!(["read"]), json!(["build"])] {
        let (_, token) = request(
            &db,
            &alice,
            Some(a),
            "POST",
            "/tokens",
            json!({"name":"CI","project_ids":[p],"scopes":scopes,"days":1}),
        )
        .await;
        for (path, expected) in [
            ("/tenants/alice", StatusCode::FORBIDDEN),
            (
                "/projects/by-slug/visible",
                if scopes == json!(["read"]) {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
            ),
            (
                "/projects/by-slug/visible/builds/1",
                if scopes == json!(["read"]) {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                },
            ),
            ("/projects/by-slug/hidden", StatusCode::FORBIDDEN),
        ] {
            let response = api::router(db.state.clone())
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1{path}"))
                        .header(
                            "authorization",
                            format!("Bearer {}", token["token"].as_str().unwrap()),
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
        }
    }
    request(
        &db,
        &alice,
        Some(a),
        "DELETE",
        &format!("/tenant/members/{reader_id}"),
        json!({}),
    )
    .await;
    assert_eq!(
        request(&db, &reader, None, "GET", "/tenants/alice", json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &db,
            &reader,
            Some(a),
            "GET",
            "/projects/by-slug/visible/builds/1",
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn concurrent_build_numbers_are_project_local_and_not_reused() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "owner", "viewer").await;
    let t = tenant(&db, &cookie, "Team").await;
    let p = project(&db, &cookie, t, "Desktop App").await;
    let other = project(&db, &cookie, t, "Other").await;
    let path = db.release_build_path(p).await;
    let create = || {
        request(
            &db,
            &cookie,
            Some(t),
            "POST",
            &path,
            crate::fixture::build_input(),
        )
    };
    let (a, b, c, d, e, f, g, h) = tokio::join!(
        create(),
        create(),
        create(),
        create(),
        create(),
        create(),
        create(),
        create()
    );
    let mut numbers = Vec::new();
    for (status, value) in [a, b, c, d, e, f, g, h] {
        assert_eq!(status, StatusCode::OK, "{value}");
        numbers.push(
            value["number"]
                .as_i64()
                .expect("build creation returns a number"),
        );
    }
    numbers.sort_unstable();
    assert_eq!(numbers, (1..=8).collect::<Vec<_>>());
    let (_, value) = request(
        &db,
        &cookie,
        Some(t),
        "POST",
        &db.release_build_path(other).await,
        crate::fixture::build_input(),
    )
    .await;
    assert_eq!(value["number"], 1);
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM source_tasks WHERE build_id IN(SELECT id FROM builds WHERE project_id=$1 AND number=8)",
        params![p],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM builds WHERE project_id=$1 AND number=8",
        params![p],
    )
    .await
    .unwrap();
    let (_, value) = create().await;
    assert_eq!(value["number"], 9);
    let (_, detail) = request(
        &db,
        &cookie,
        Some(t),
        "GET",
        "/projects/by-slug/desktop-app/builds/9",
        json!({}),
    )
    .await;
    assert_eq!(detail["id"], value["id"]);
    assert_eq!(detail["number"], 9);
    let (_, projects) = request(&db, &cookie, Some(t), "GET", "/projects", json!({})).await;
    assert_eq!(
        projects
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == p.to_string())
            .unwrap()["last_build"]["number"],
        9
    );
    for number in ["0", "-1", "9007199254740992"] {
        assert_eq!(
            request(
                &db,
                &cookie,
                Some(t),
                "GET",
                &format!("/projects/by-slug/desktop-app/builds/{number}"),
                json!({})
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert!(
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET number=42 WHERE id=$1",
            params![Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()]
        )
        .await
        .is_err()
    );
    db.close().await;
}
