//! [INPUT]: 依赖隔离数据库、授权设备和租户 HTTP 会话。
//! [OUTPUT]: 验证概览、多租户、成员和签名管理边界。
//! [POS]: 数据库验收中的租户测试入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! Real PostgreSQL and HTTP authorization tests. Accounts start with no membership.
#[path = "build_logs.rs"]
mod build_logs;
#[path = "signing.rs"]
mod signing;
use super::*;
use serde_json::{Value, json};

#[path = "tenancy_regressions.rs"]
mod regressions;

#[path = "dashboard.rs"]
mod dashboard;

#[path = "readable_routes.rs"]
mod readable_routes;

#[path = "account_settings.rs"]
mod account_settings;

#[path = "node_identity.rs"]
mod node_identity;

#[path = "workspace_lifecycle.rs"]
mod workspace_lifecycle;

async fn account(db: &Database, name: &str, legacy_role: &str) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let cookie = format!("artifactory_session={}", Uuid::new_v4());
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused',$3)",
        params![id, name, legacy_role],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash(cookie.split('=').nth(1).unwrap()), id],
    )
    .await
    .unwrap();
    (id, cookie)
}
async fn request(
    db: &Database,
    cookie: &str,
    tenant: Option<Uuid>,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("error")
        .with_test_writer()
        .try_init();
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header("cookie", cookie)
        .header("x-artifactory-csrf", "1")
        .header("content-type", "application/json");
    if let Some(tenant) = tenant {
        req = req.header("X-Artifactory-Tenant", tenant.to_string());
    }
    let response = api::router(db.state.clone())
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let data = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&data).unwrap_or(Value::Null))
}
async fn tenant(db: &Database, cookie: &str, name: &str) -> Uuid {
    let (status, value) = request(
        db,
        cookie,
        None,
        "POST",
        "/tenants",
        json!({"name":name,"idempotency_key":Uuid::new_v4()}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
}
async fn project(db: &Database, cookie: &str, tenant: Uuid, name: &str) -> Uuid {
    let (status, value) = request(
        db,
        cookie,
        Some(tenant),
        "POST",
        "/projects",
        json!({"name":name,"repository_url":"https://example.com/repository.git"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    Uuid::parse_str(value["id"].as_str().unwrap()).unwrap()
}
async fn device(db: &Database, tenant: Uuid) -> Uuid {
    let pool: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM node_pools WHERE tenant_id=$1 AND is_default",
        params![tenant],
    )
    .await
    .unwrap();
    let id = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,capabilities,certificate_hash,certificate_expires_at,active_session,last_seen,connection_owner) VALUES($1,'shared','linux','amd64',$2,'[\"git\",\"bash\"]',$3,now()+interval '1 day',$4,now(),$5)",params![id,pool,id.to_string(),Uuid::new_v4(),db.state.coordination.id()]).await.unwrap();
    id
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn tenant_creation_is_atomic_idempotent_and_context_is_mandatory() {
    let db = Database::new().await;
    let (owner, cookie) = account(&db, "owner", "viewer").await;
    let (_, identity) = request(&db, &cookie, None, "GET", "/auth/me", json!({})).await;
    assert!(identity.get("role").is_none());
    let (_, memberships) = request(&db, &cookie, None, "GET", "/tenants", json!({})).await;
    assert_eq!(memberships, json!([]));
    let (status, error) = request(&db, &cookie, None, "GET", "/projects", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "tenant_required");
    let input = json!({"name":"Workspace","idempotency_key":Uuid::new_v4()});
    let (first, second) = tokio::join!(
        request(&db, &cookie, None, "POST", "/tenants", input.clone()),
        request(&db, &cookie, None, "POST", "/tenants", input)
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first.1, second.1);
    let id = Uuid::parse_str(first.1["id"].as_str().unwrap()).unwrap();
    let counts:(i64,i64)=sql::tuple(&mut db.pool.clone(),"SELECT (SELECT count(*) FROM tenant_members WHERE tenant_id=$1 AND user_id=$2 AND role='owner'),(SELECT count(*) FROM node_pools WHERE tenant_id=$1 AND is_default)",params![id,owner]).await.unwrap();
    assert_eq!(counts, (1, 1));
    let project = project(&db, &cookie, id, "project").await;
    let (_, value) = request(
        &db,
        &cookie,
        Some(id),
        "GET",
        &format!("/projects/{project}"),
        json!({}),
    )
    .await;
    assert_eq!(value["effective_role"], "admin");
    assert_eq!(value["node_access"]["mode"], "pools");
    assert_eq!(value["node_access"]["ids"].as_array().unwrap().len(), 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn tenant_and_project_permissions_cover_direct_ids_lists_and_references() {
    let db = Database::new().await;
    let (_, alice) = account(&db, "alice", "viewer").await;
    let (_, bob) = account(&db, "bob", "admin").await;
    let (reader_id, reader) = account(&db, "reader", "admin").await;
    let a = tenant(&db, &alice, "A").await;
    let b = tenant(&db, &bob, "B").await;
    let pa = project(&db, &alice, a, "A project").await;
    let hidden = project(&db, &alice, a, "Hidden").await;
    let pb = project(&db, &bob, b, "B project").await;
    let na = device(&db, a).await;
    let nb = device(&db, b).await;
    for cookie in [&bob, &reader] {
        let (status, _) = request(&db, cookie, Some(a), "GET", "/projects", json!({})).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "legacy global admin cannot grant authority"
        );
    }
    let (status, _) = request(
        &db,
        &alice,
        Some(a),
        "POST",
        "/tenant/members",
        json!({"username":"reader","role":"member"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, none) = request(&db, &reader, Some(a), "GET", "/projects", json!({})).await;
    assert_eq!(none, json!([]));
    let (status, _) = request(
        &db,
        &alice,
        Some(a),
        "PUT",
        &format!("/projects/{pa}/members/{reader_id}"),
        json!({"role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = request(&db, &reader, Some(a), "GET", "/projects", json!({})).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], pa.to_string());
    for (cookie, tenant, paths) in [
        (
            &alice,
            a,
            vec![
                format!("/projects/{pb}"),
                format!("/nodes/{nb}/metadata"),
                format!("/nodes/{nb}/telemetry"),
                format!("/nodes/{nb}/inventory"),
            ],
        ),
        (
            &reader,
            a,
            vec![format!("/projects/{hidden}"), format!("/projects/{pb}")],
        ),
    ] {
        for path in paths {
            let (status, _) = request(&db, cookie, Some(tenant), "GET", &path, json!({})).await;
            assert!(
                [StatusCode::NOT_FOUND, StatusCode::FORBIDDEN].contains(&status),
                "{path}: {status}"
            );
        }
    }
    for path in ["/credentials", "/model-profiles", "/audit", "/users"] {
        assert_eq!(
            request(&db, &reader, Some(a), "GET", path, json!({}))
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    let (_, nodes) = request(&db, &reader, Some(a), "GET", "/nodes", json!({})).await;
    assert_eq!(nodes.as_array().unwrap().len(), 1);
    assert_eq!(nodes[0]["id"], na.to_string());
    let (status, _) = request(
        &db,
        &reader,
        Some(a),
        "POST",
        &db.release_build_path(pa).await,
        super::fixture::build_input(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &db,
        &alice,
        Some(a),
        "PUT",
        &format!("/projects/{pa}/node-access"),
        json!({"mode":"nodes","ids":[nb]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, overview) = request(&db, &reader, Some(a), "GET", "/overview", json!({})).await;
    assert_eq!(overview["projects"], 1);
    assert_eq!(overview["nodes_registered"], 1);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn invitations_single_use_owner_transfer_and_token_revocation_are_live() {
    let db = Database::new().await;
    let (owner, alice) = account(&db, "alice", "viewer").await;
    let (member, bob) = account(&db, "bob", "viewer").await;
    let (_, carol) = account(&db, "carol", "viewer").await;
    let t = tenant(&db, &alice, "Team").await;
    let p = project(&db, &alice, t, "App").await;
    let (_, invite) = request(
        &db,
        &alice,
        Some(t),
        "POST",
        "/tenant/invitations",
        json!({"role":"member"}),
    )
    .await;
    let token = invite["token"].as_str().unwrap();
    let stored: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT token_hash FROM tenant_invitations WHERE id=$1",
        params![Uuid::parse_str(invite["id"].as_str().unwrap()).unwrap()],
    )
    .await
    .unwrap();
    assert_ne!(stored, token);
    assert_eq!(stored, crypto::hash(token));
    let accept_path = format!("/invitations/{token}/accept");
    let (one, two) = tokio::join!(
        request(&db, &bob, None, "POST", &accept_path, json!({})),
        request(&db, &carol, None, "POST", &accept_path, json!({}))
    );
    assert_eq!(
        [one.0, two.0]
            .iter()
            .filter(|s| **s == StatusCode::OK)
            .count(),
        1
    );
    request(
        &db,
        &alice,
        Some(t),
        "POST",
        "/tenant/members",
        json!({"username":"bob","role":"admin"}),
    )
    .await;
    request(
        &db,
        &alice,
        Some(t),
        "PATCH",
        &format!("/tenant/members/{member}"),
        json!({"role":"admin"}),
    )
    .await;
    assert_eq!(
        request(
            &db,
            &bob,
            Some(t),
            "DELETE",
            &format!("/tenant/members/{owner}"),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &db,
            &alice,
            Some(t),
            "POST",
            "/tenant/owner",
            json!({"user_id":member})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, issued) = request(
        &db,
        &bob,
        Some(t),
        "POST",
        "/tokens",
        json!({"name":"scoped","project_ids":[p],"scopes":["read"],"days":1}),
    )
    .await;
    let bearer = issued["token"].as_str().unwrap();
    async fn token_request(db: &Database, bearer: &str) -> StatusCode {
        api::router(db.state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }
    assert_eq!(token_request(&db, bearer).await, StatusCode::OK);
    // Ownership can move while in-flight service tasks retain their project identity.
    assert_eq!(
        request(
            &db,
            &bob,
            Some(t),
            "POST",
            "/tenant/owner",
            json!({"user_id":owner})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &db,
            &alice,
            Some(t),
            "DELETE",
            &format!("/tenant/members/{member}"),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(token_request(&db, bearer).await, StatusCode::FORBIDDEN);
    assert_eq!(
        request(&db, &bob, Some(t), "GET", "/projects", json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn shared_devices_intersect_frozen_grants_and_serialize_three_replicas() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "owner", "viewer").await;
    let t = tenant(&db, &cookie, "Shared").await;
    let n = device(&db, t).await;
    let projects = [
        project(&db, &cookie, t, "A").await,
        project(&db, &cookie, t, "B").await,
        project(&db, &cookie, t, "C").await,
    ];
    let mut builds = Vec::new();
    for p in projects {
        assert_eq!(
            request(
                &db,
                &cookie,
                Some(t),
                "PUT",
                &format!("/projects/{p}/node-access"),
                json!({"mode":"nodes","ids":[n]})
            )
            .await
            .0,
            StatusCode::OK
        );
        let build = Uuid::new_v4();
        let job = Uuid::new_v4();
        sql::execute(&mut db.pool.clone(),"WITH fixture_release AS (INSERT INTO releases(id,project_id,version) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-','')) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status,release_id,release_version) VALUES($1,$2,'main','https://example.com/app.git','queued', (SELECT id FROM fixture_release),(SELECT version FROM fixture_release))",params![build,p]).await.unwrap();
        sql::execute(
            &mut db.pool.clone(),
            "INSERT INTO jobs(id,build_id,target,target_config) VALUES($1,$2,'app',$3)",
            params![
                job,
                build,
                json!({"os":"linux","arch":"amd64","steps":[{"name":"compile","run":"make"}],"artifacts":["dist/app"]})
            ],
        )
        .await
        .unwrap();
        builds.push(build);
    }
    let new_pool = Uuid::new_v4();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO node_pools(id,tenant_id,name) VALUES($1,$2,'Moved')",
        params![new_pool, t],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET pool_id=$2 WHERE id=$1",
        params![n, new_pool],
    )
    .await
    .unwrap();
    let permitted: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT build_node_allowed($1,$2)",
        params![builds[0], n],
    )
    .await
    .unwrap();
    assert!(permitted, "direct grants survive same-tenant pool moves");
    let second = AppState {
        coordination: Default::default(),
        ..db.state.clone()
    };
    let third = AppState {
        coordination: Default::default(),
        ..db.state.clone()
    };
    let (a, b, c) = tokio::join!(
        scheduler::schedule(&db.state),
        scheduler::schedule(&second),
        scheduler::schedule(&third)
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM attempts WHERE node_id=$1",
        params![n],
    )
    .await
    .unwrap();
    assert_eq!(count, 1);
    for p in projects {
        request(
            &db,
            &cookie,
            Some(t),
            "PUT",
            &format!("/projects/{p}/node-access"),
            json!({"mode":"nodes","ids":[]}),
        )
        .await;
    }
    let allowed: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT build_node_allowed($1,$2)",
        params![builds[0], n],
    )
    .await
    .unwrap();
    assert!(!allowed);
    let active: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM attempts WHERE node_id=$1 AND status='leased'",
        params![n],
    )
    .await
    .unwrap();
    assert_eq!(
        active, 1,
        "revoking device grants preserves existing attempts"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn project_administrators_cannot_expand_resources_and_role_changes_revoke_tokens() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (admin_id, admin) = account(&db, "project-admin", "admin").await;
    let (member_id, _) = account(&db, "member", "viewer").await;
    let (outsider_id, _) = account(&db, "outsider", "admin").await;
    let t = tenant(&db, &owner, "Team").await;
    let p = project(&db, &owner, t, "App").await;
    device(&db, t).await;
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
    request(
        &db,
        &owner,
        Some(t),
        "PUT",
        &format!("/projects/{p}/members/{admin_id}"),
        json!({"role":"admin"}),
    )
    .await;
    let mut input = json!({"name":"Updated","repository_url":"https://example.com/app.git","build_context":"Build release binaries"});
    assert_eq!(
        request(
            &db,
            &admin,
            Some(t),
            "PATCH",
            &format!("/projects/{p}"),
            input.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    for (key, value) in [
        ("signing_profiles", json!(["new-signing"])),
        ("node_access", json!({"mode":"nodes","ids":[]})),
    ] {
        input[key] = value;
        assert_eq!(
            request(
                &db,
                &admin,
                Some(t),
                "PATCH",
                &format!("/projects/{p}"),
                input.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN,
            "{key}"
        );
        input.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(
        request(
            &db,
            &admin,
            Some(t),
            "PUT",
            &format!("/projects/{p}/node-access"),
            json!({"mode":"nodes","ids":[]})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &db,
            &admin,
            Some(t),
            "PUT",
            &format!("/projects/{p}/members/{outsider_id}"),
            json!({"role":"operator"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &db,
            &admin,
            Some(t),
            "PUT",
            &format!("/projects/{p}/members/{member_id}"),
            json!({"role":"operator"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, token) = request(
        &db,
        &admin,
        Some(t),
        "POST",
        "/tokens",
        json!({"name":"CI","project_ids":[p],"scopes":["read","build"],"days":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{token}");
    let bearer = token["token"].as_str().unwrap();
    let release_build_path = db.release_build_path(p).await;
    let token_request = || {
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1{release_build_path}"))
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(crate::fixture::build_input().to_string()))
            .unwrap()
    };
    assert_eq!(
        api::router(db.state.clone())
            .oneshot(token_request())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    request(
        &db,
        &owner,
        Some(t),
        "PUT",
        &format!("/projects/{p}/members/{admin_id}"),
        json!({"role":"viewer"}),
    )
    .await;
    assert_eq!(
        api::router(db.state.clone())
            .oneshot(token_request())
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    db.close().await;
}

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
