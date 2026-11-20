//! [INPUT]: 依赖隔离数据库、租户会话与节点列表。
//! [OUTPUT]: 验证注册改名、任务租约与列表查询预算。
//! [POS]: 复用 tenancy 夹具验证控制台设备身份边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_enrollment_normalizes_names_and_rejects_control_characters() {
    use artifactory_control_plane::{
        grpc::EmissaryService,
        protocol::{EnrollRequest, enrollment_server::Enrollment},
    };
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
    let mut db = Database::with_tenant().await;
    let directory = tempfile::tempdir().unwrap();
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).unwrap().pem();
    tokio::fs::write(directory.path().join("ca.pem"), ca)
        .await
        .unwrap();
    tokio::fs::write(directory.path().join("ca.key"), ca_key.serialize_pem())
        .await
        .unwrap();
    let mut settings = (*db.state.settings).clone();
    settings.pki_dir = directory.path().to_path_buf();
    db.state.settings = Arc::new(settings);
    let key = KeyPair::generate().unwrap();
    let csr = CertificateParams::default()
        .serialize_request(&key)
        .unwrap()
        .pem()
        .unwrap();
    let service = EmissaryService(db.state.clone());
    let input = EnrollRequest {
        token: "a".repeat(64),
        name: "  中文设备  ".into(),
        os: "linux".into(),
        arch: "amd64".into(),
        protocol_major: 1,
        csr_pem: csr,
    };
    for name in [" ", "node\nname", "node\tname", "node\u{7f}name"] {
        let error = service
            .enroll(tonic::Request::new(EnrollRequest {
                name: name.into(),
                ..input.clone()
            }))
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            tonic::Code::InvalidArgument,
            "{name:?}: {error}"
        );
    }
    sql::execute(&mut db.pool.clone(), "INSERT INTO enrollment_tokens(id,token_hash,pool_id,expires_at) VALUES($1,$2,'00000000-0000-0000-0000-000000000001',now()+interval '1 hour')", params![Uuid::new_v4(),crypto::hash(&input.token)]).await.unwrap();
    let enrolled = service
        .enroll(tonic::Request::new(input))
        .await
        .unwrap()
        .into_inner();
    let id = Uuid::parse_str(&enrolled.emissary_id).unwrap();
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT name FROM nodes WHERE id=$1",
            params![id]
        )
        .await
        .unwrap(),
        "中文设备"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_rename_is_tenant_admin_only_atomic_and_preserves_execution_state() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (admin_id, admin) = account(&db, "admin", "viewer").await;
    let (member_id, member) = account(&db, "member", "admin").await;
    let (viewer_id, viewer) = account(&db, "viewer", "viewer").await;
    let a = tenant(&db, &owner, "A").await;
    let b = tenant(&db, &owner, "B").await;
    let project = project(&db, &owner, a, "Project").await;
    let node = device(&db, a).await;
    let foreign = device(&db, b).await;
    for (id, role) in [
        (admin_id, "admin"),
        (member_id, "member"),
        (viewer_id, "member"),
    ] {
        sql::execute(
            &mut db.pool.clone(),
            "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,$3)",
            params![a, id, role],
        )
        .await
        .unwrap();
    }
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO project_members(tenant_id,project_id,user_id,role) VALUES($1,$2,$3,'admin')",
        params![a, project, member_id],
    )
    .await
    .unwrap();
    let path = format!("/nodes/{node}/name");
    let before: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT to_jsonb(n)-'name' FROM nodes n WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let (status, body) = request(
        &db,
        &owner,
        Some(a),
        "PATCH",
        &path,
        json!({"name":"  实验室设备  "}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"ok":true}));
    let after: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT to_jsonb(n)-'name' FROM nodes n WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(
        before, after,
        "rename must preserve pool, slots, certificates and session state"
    );
    let audit: Value = sql::scalar(&mut db.pool.clone(), "SELECT details FROM audit_events WHERE tenant_id=$1 AND resource_id=$2 AND action='node_renamed' ORDER BY id DESC LIMIT 1", params![a,node.to_string()]).await.unwrap();
    assert_eq!(audit, json!({"old_name":"shared","new_name":"实验室设备"}));
    assert_eq!(
        request(
            &db,
            &admin,
            Some(a),
            "PATCH",
            &path,
            json!({"name":"Admin node"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &db,
            &member,
            Some(a),
            "PATCH",
            &path,
            json!({"name":"Forbidden"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &db,
            &viewer,
            Some(a),
            "PATCH",
            &path,
            json!({"name":"Forbidden"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "PATCH",
            &format!("/nodes/{foreign}/name"),
            json!({"name":"Foreign"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for name in [
        " ".to_owned(),
        "x".repeat(101),
        "中".repeat(34),
        "bad\nname".to_owned(),
    ] {
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
            json!({"name":"Valid","slots":4})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1{path}"))
                .header("cookie", &owner)
                .header("x-artifactory-tenant", a.to_string())
                .header("content-type", "application/json")
                .body(Body::from(json!({"name":"No CSRF"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let token = "node-rename-test-token";
    sql::execute(&mut db.pool.clone(), "INSERT INTO api_tokens(id,tenant_id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES($1,$2,$3,'rename',$4,$5,'{read,build,release}',now()+interval '1 hour')", params![Uuid::new_v4(),a,admin_id,crypto::hash(token),vec![project]]).await.unwrap();
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1{path}"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"name":"Token"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE tenants SET status='archived',archived_at=now() WHERE id=$1",
        params![a],
    )
    .await
    .unwrap();
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "PATCH",
            &path,
            json!({"name":"Archived"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_identity_projects_typed_metadata_and_search_respects_visibility() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (member_id, member) = account(&db, "member", "viewer").await;
    let a = tenant(&db, &owner, "A").await;
    let b = tenant(&db, &owner, "B").await;
    let project = project(&db, &owner, a, "Project").await;
    let node = device(&db, a).await;
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
    let metadata = json!({"system.hostname":"hidden-host","hardware.device_model":"Compute Board","hardware.device_vendor":"Example","hardware.cpu_model":"Unique CPU","hardware.physical_memory_bytes":4294967296u64,"custom.large":"x".repeat(60000)});
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET metadata=$2 WHERE id=$1",
        params![node, &metadata],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET metadata=$2 WHERE id=$1",
        params![foreign, json!({"system.hostname":"foreign-secret"})],
    )
    .await
    .unwrap();
    let (status, nodes) = request(&db, &owner, Some(a), "GET", "/nodes", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        nodes[0]["identity"],
        json!({"hostname":"hidden-host","device_model":"Compute Board","device_vendor":"Example","cpu_model":"Unique CPU","memory_total_bytes":4294967296u64})
    );
    assert!(nodes[0].get("metadata").is_none());
    assert!(nodes.to_string().len() < 4096);
    for term in [
        "amd64",
        &node.to_string(),
        "hidden-host",
        "Compute%20Board",
        "Unique%20CPU",
    ] {
        let (_, list) = request(
            &db,
            &owner,
            Some(a),
            "GET",
            &format!("/nodes?q={term}"),
            json!({}),
        )
        .await;
        assert_eq!(list.as_array().unwrap().len(), 1, "search {term}: {list}");
    }
    for memory in [
        json!(-1),
        json!(1.5),
        serde_json::from_str("18446744073709551616").unwrap(),
    ] {
        sql::execute(&mut db.pool.clone(), "UPDATE nodes SET metadata=jsonb_build_object('hardware.physical_memory_bytes',$2::jsonb) WHERE id=$1", params![node,memory]).await.unwrap();
        let (_, list) = request(&db, &owner, Some(a), "GET", "/nodes", json!({})).await;
        assert_eq!(
            list[0]["identity"],
            json!({}),
            "memory capacity must be an unsigned integer"
        );
    }
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET metadata=$2 WHERE id=$1",
        params![node, &metadata],
    )
    .await
    .unwrap();
    let (_, scoped) = request(&db, &member, Some(a), "GET", "/nodes", json!({})).await;
    assert_eq!(scoped[0]["identity"], json!({}));
    for term in ["hidden-host", "Compute%20Board", "Unique%20CPU"] {
        let (_, list) = request(
            &db,
            &member,
            Some(a),
            "GET",
            &format!("/nodes?q={term}"),
            json!({}),
        )
        .await;
        assert_eq!(list, json!([]), "search must not expose hidden identity");
    }
    let (_, list) = request(
        &db,
        &owner,
        Some(a),
        "GET",
        "/nodes?q=foreign-secret",
        json!({}),
    )
    .await;
    assert_eq!(list, json!([]));
    sql::execute(&mut db.pool.clone(), "UPDATE nodes SET metadata=$2 WHERE id=$1", params![node,json!({"system.hostname":12,"hardware.device_model":true,"hardware.device_vendor":{},"hardware.cpu_model":[],"hardware.physical_memory_bytes":"4294967296"})]).await.unwrap();
    let (_, list) = request(&db, &owner, Some(a), "GET", "/nodes", json!({})).await;
    assert_eq!(list[0]["identity"], json!({}));
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET metadata='{}' WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    let (_, list) = request(&db, &owner, Some(a), "GET", "/nodes", json!({})).await;
    assert_eq!(list[0]["identity"], json!({}));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_list_keeps_large_metadata_within_page_and_connection_budget() {
    use artifactory_control_plane::{auth::Auth, store::ControlStore};
    use std::time::{Duration, Instant};
    let db = Database::new().await;
    let (owner_id, owner) = account(&db, "owner", "viewer").await;
    let tenant = tenant(&db, &owner, "Budget").await;
    sql::execute(&mut db.pool.clone(), "INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at,metadata) SELECT gen_random_uuid(),'fleet-'||s,'linux','amd64',p.id,s::text,now()+interval '1 day',jsonb_build_object('system.hostname','host-'||s,'hardware.cpu_model','Budget CPU','custom.large',repeat('x',60000)) FROM generate_series(1,250) s CROSS JOIN node_pools p WHERE p.tenant_id=$1 AND p.is_default",params![tenant]).await.unwrap();
    let pool = connect(
        &db.state.settings.database_url,
        artifactory_control_plane::store::models(),
        PoolSettings::new(1).timeout(Duration::from_millis(500)),
    )
    .await
    .unwrap();
    let mut auth = Auth::scoped(owner_id, "admin", vec![], vec![]);
    auth.token_projects = None;
    auth.tenant_id = tenant;
    db.state
        .store
        .authorize_tenant(&mut auth, tenant)
        .await
        .unwrap();
    let store = ControlStore::new(pool).for_auth(&auth);
    let started = Instant::now();
    let first = tokio::time::timeout(Duration::from_secs(1), store.nodes(25, 0, ""))
        .await
        .expect("node page exceeds the one-second query budget")
        .unwrap();
    let elapsed = started.elapsed();
    let second = store.nodes(25, 25, "").await.unwrap();
    assert_eq!(first.len(), 25);
    assert_eq!(second.len(), 25);
    assert!(
        first
            .iter()
            .all(|node| second.iter().all(|other| node.id != other.id))
    );
    let response = serde_json::to_value(first).unwrap();
    assert!(
        response.to_string().len() < 50000,
        "full metadata must stay in PostgreSQL"
    );
    assert_eq!(response[0]["identity"]["cpu_model"], "Budget CPU");
    assert_eq!(store.nodes(25, 0, "fleet-250").await.unwrap().len(), 1);
    println!(
        "node list: 250 nodes with 60KB metadata, 25-row page, one connection, {}ms",
        elapsed.as_millis()
    );
    drop(store);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_rename_preserves_assigned_tasks_and_leases() {
    use artifactory_control_plane::auth::Auth;
    let db = Database::with_tenant().await;
    let (_, _, job) = db.fixture().await;
    let node = db.node().await;
    scheduler::tick(&db.state).await.unwrap();
    let before: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT to_jsonb(a) FROM attempts a WHERE node_id=$1 AND job_id=$2",
        params![node, job],
    )
    .await
    .unwrap();
    assert_eq!(before["status"], "leased");
    let (owner, _) = account(&db, "task-admin", "admin").await;
    let mut auth = Auth::scoped(owner, "admin", vec![], vec![]);
    auth.token_projects = None;
    auth.tenant_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let tenant = auth.tenant_id;
    db.state
        .store
        .authorize_tenant(&mut auth, tenant)
        .await
        .unwrap();
    db.state
        .store
        .for_auth(&auth)
        .rename_node(node, "Build host")
        .await
        .unwrap();
    let after: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT to_jsonb(a) FROM attempts a WHERE node_id=$1 AND job_id=$2",
        params![node, job],
    )
    .await
    .unwrap();
    assert_eq!(
        before, after,
        "rename must retain task identity, lease token and expiry"
    );
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM jobs WHERE id=$1",
            params![job]
        )
        .await
        .unwrap(),
        "leased"
    );
    db.close().await;
}
