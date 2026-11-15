//! Real PostgreSQL and HTTP authorization tests. Accounts start with no membership.
use super::*;
use serde_json::{Value, json};

#[path = "tenancy_regressions.rs"]
mod regressions;

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
    for path in [
        "/credentials",
        "/model-profiles",
        "/ai-workers",
        "/audit",
        "/users",
    ] {
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
        &format!("/projects/{pa}/builds"),
        json!({"ref":"main"}),
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
        sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status) VALUES($1,$2,'main','https://example.com/app.git','queued')",params![build,p]).await.unwrap();
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
async fn legacy_upgrade_preserves_resources_and_only_administrators_inherit() {
    use toasty::migration::{MigrationFile, MigrationSet};
    static LEGACY: MigrationSet = MigrationSet::new(&[
        MigrationFile::new(
            2026111301,
            "0000_baseline.sql",
            include_str!("../../migrations/migrations/0000_baseline.sql"),
        ),
        MigrationFile::new(
            2026111401,
            "0001_control_replication.sql",
            include_str!("../../migrations/migrations/0001_control_replication.sql"),
        ),
        MigrationFile::new(
            2026111402,
            "0002_node_placements.sql",
            include_str!("../../migrations/migrations/0002_node_placements.sql"),
        ),
    ]);
    let db = Database::with_migrations(&LEGACY).await;
    let project = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO projects(id,name,repository_url) VALUES($1,'retained','https://example.com/app.git')",params![project]).await.unwrap();
    // A malformed legacy installation must roll back both DDL and migration history.
    assert!(
        artifactory_database::migrate(&db.pool, &artifactory_control_plane::store::MIGRATIONS)
            .await
            .is_err()
    );
    let rolled_back: bool = sql::scalar(&mut db.pool.clone(), "SELECT to_regclass(current_schema()||'.tenants') IS NULL AND (SELECT count(*) FROM __toasty_migrations)=3", params![]).await.unwrap();
    assert!(rolled_back);
    let first = Uuid::new_v4();
    let admin = Uuid::new_v4();
    let reader = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO users(id,username,password_hash,role,created_at) VALUES($1,'first','unused','admin',now()-interval '2 days'),($2,'admin','unused','admin',now()-interval '1 day'),($3,'reader','unused','viewer',now())",params![first,admin,reader]).await.unwrap();
    for user in [admin, reader] {
        sql::execute(
            &mut db.pool.clone(),
            "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
            params![user.to_string(), user],
        )
        .await
        .unwrap();
        sql::execute(&mut db.pool.clone(),"INSERT INTO api_tokens(id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES($1,$2,'legacy',$3,$4,'{read}',now()+interval '1 day')",params![Uuid::new_v4(),user,user.to_string(),vec![project]]).await.unwrap();
    }
    let node = Uuid::new_v4();
    let build = Uuid::new_v4();
    let frozen_empty_build = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at) VALUES($1,'retained','linux','amd64','00000000-0000-0000-0000-000000000001','retained-certificate',now()+interval '1 day')",params![node]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status) VALUES($1,$2,'main','https://example.com/app.git','succeeded')",params![build,project]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status,planning_context) VALUES($1,$2,'main','https://example.com/app.git','succeeded',$3)",params![frozen_empty_build,project,json!({"pool_ids":[]})]).await.unwrap();
    for _ in 0..2 {
        artifactory_database::migrate(&db.pool, &artifactory_control_plane::store::MIGRATIONS)
            .await
            .unwrap();
    }
    let owner: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT owner_id FROM tenants",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(owner, first);
    let members: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM tenant_members",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(members, 2);
    let sessions: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM sessions",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(sessions, 0);
    let tokens: Vec<(Uuid, bool)> = sql::tuples(
        &mut db.pool.clone(),
        "SELECT user_id,revoked FROM api_tokens ORDER BY user_id",
        params![],
    )
    .await
    .unwrap();
    assert!(tokens.contains(&(admin, false)));
    assert!(tokens.contains(&(reader, true)));
    let history:(Uuid,Uuid,String)=sql::tuple(&mut db.pool.clone(),"SELECT b.project_id,b.tenant_id,n.certificate_hash FROM builds b CROSS JOIN nodes n WHERE b.id=$1 AND n.id=$2",params![build,node]).await.unwrap();
    assert_eq!(
        history,
        (project, Uuid::from_u128(1), "retained-certificate".into())
    );
    let allowed: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT build_node_allowed($1,$2)",
        params![build, node],
    )
    .await
    .unwrap();
    assert!(allowed);
    let frozen_allowed: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT build_node_allowed($1,$2)",
        params![frozen_empty_build, node],
    )
    .await
    .unwrap();
    assert!(
        !frozen_allowed,
        "migration must preserve historical frozen grants, including an empty list"
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
    let token_request = || {
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/projects/{p}/builds"))
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"ref":"main"}).to_string()))
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
async fn offline_migration_requires_build_publication_planning_and_ota_to_finish() {
    use artifactory_control_plane::store::maintenance::TenantMigration;

    let db = Database::with_tenant().await;
    let (_, build, _) = db.fixture().await;
    let node = db.node().await;
    let migration = TenantMigration::connect(&db.state.settings.database_url)
        .await
        .unwrap();
    assert!(migration.verify_drained().await.is_err());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='succeeded',publish_status='pending' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert!(
        migration.verify_drained().await.is_err(),
        "publication is still active after compilation finishes"
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET publish_status='succeeded' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    let run = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO planning_runs(id,build_id,purpose,state,model_config) VALUES($1,$2,'initial','leased','{}')", params![run,build]).await.unwrap();
    assert!(migration.verify_drained().await.is_err());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE planning_runs SET state='succeeded' WHERE id=$1",
        params![run],
    )
    .await
    .unwrap();
    let release = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_releases(id,tenant_id,version,manifest,signature,manifest_hash) VALUES($1,'00000000-0000-0000-0000-000000000001','test','{}','test','test')", params![release]).await.unwrap();
    let update = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status) VALUES($1,$2,'00000000-0000-0000-0000-000000000001',$3,'downloading')", params![update,node,release]).await.unwrap();
    assert!(migration.verify_drained().await.is_err());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE emissary_updates SET status='succeeded' WHERE id=$1",
        params![update],
    )
    .await
    .unwrap();
    migration.verify_drained().await.unwrap();
    drop(migration);
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
    sql::execute(&mut db.pool.clone(),"INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status) VALUES($1,$2,'main','https://example.com/app.git','succeeded')",params![build,p]).await.unwrap();
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
    sql::execute(&mut db.pool.clone(),"INSERT INTO releases(id,project_id,build_id,version,commit_sha,idempotency_key) VALUES($1,$2,$3,'v1','private-commit','test-private-release')",params![release,p,build]).await.unwrap();
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
        "/ai-workers",
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
    let repository = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO source_repositories(id,project_id,repository_url,cache_key) VALUES($1,$2,'https://example.com/repo.git',$3)", params![repository,pb,repository.to_string()]).await.unwrap();
    let insert_snapshot = "INSERT INTO source_snapshots(id,project_id,root_repository_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,'{}',$5)";
    assert!(
        sql::execute(
            &mut db.pool.clone(),
            insert_snapshot,
            params![snapshot, pa, repository, "0".repeat(40), "a".repeat(64)]
        )
        .await
        .is_err()
    );
    sql::execute(
        &mut db.pool.clone(),
        insert_snapshot,
        params![snapshot, pb, repository, "0".repeat(40), "a".repeat(64)],
    )
    .await
    .unwrap();
    let build = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO builds(id,project_id,requested_ref,source_repository_url) VALUES($1,$2,'main','https://example.com/repo.git')", params![build,pa]).await.unwrap();
    assert!(
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET source_snapshot_id=$2 WHERE id=$1",
            params![build, snapshot]
        )
        .await
        .is_err()
    );
    db.close().await;
}
