//! [INPUT]: 依赖隔离 PostgreSQL、真实调度入口和租户 HTTP 会话。
//! [OUTPUT]: 验证产品版本固定目标、版本兼容和历史重试围栏。
//! [POS]: OTA 产品版本合同的数据库与接口回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::{Value, json};

const TENANT: Uuid = Uuid::from_u128(1);

async fn release(db: &Database, version: &str) -> Uuid {
    let id = Uuid::new_v4();
    let manifest = json!({"version":version,"assets":[{"os":"linux","arch":"amd64","name":"artifactory-emissary-linux-amd64"}]}).to_string();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_releases(tenant_id,id,version,manifest,signature,manifest_hash) VALUES($1,$2,$3,$4,'test',$5)", params![TENANT,id,version,manifest,id.to_string()]).await.unwrap();
    id
}

async fn enabled_node(db: &Database, version: &str) -> (Uuid, Uuid) {
    let node = db.node().await;
    sql::execute(&mut db.pool.clone(), "UPDATE nodes SET capabilities=capabilities || '[\"emissary-ota-v1\"]'::jsonb,emissary_current_version=$2 WHERE id=$1", params![node,version]).await.unwrap();
    let session = db.state.connections.read().await[&node].session;
    (node, session)
}

async fn counters(db: &Database) -> (i64, i64) {
    sql::tuple(&mut db.pool.clone(), "SELECT (SELECT count(*) FROM emissary_updates),(SELECT count(*) FROM nodes WHERE ota_draining)", params![]).await.unwrap()
}

async fn admin(db: &Database) -> String {
    let id = Uuid::new_v4();
    let token = Uuid::new_v4().to_string();
    sql::execute(&mut db.pool.clone(), "INSERT INTO users(id,username,password_hash,role) VALUES($1,'version-admin','unused','admin')", params![id]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash(&token), id],
    )
    .await
    .unwrap();
    format!("artifactory_session={token}")
}

async fn request(
    db: &Database,
    cookie: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1{path}"))
                .header("cookie", cookie)
                .header("x-artifactory-tenant", TENANT.to_string())
                .header("x-artifactory-csrf", "1")
                .header("content-type", "application/json")
                .header("origin", "http://localhost:3000")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_versions_follow_product_version_without_target_and_ignore_old_pins() {
    let db = Database::with_tenant().await;
    let old = release(&db, "0.1.0").await;
    let target = release(&db, artifactory_control_plane::VERSION).await;
    let (node, session) = enabled_node(&db, "0.1.0").await;
    let pins:(Option<Uuid>,Option<Uuid>)=sql::tuple(&mut db.pool.clone(),"SELECT n.emissary_target_release,p.emissary_target_release FROM nodes n JOIN node_pools p ON p.id=n.pool_id WHERE n.id=$1",params![node]).await.unwrap();
    assert_eq!(pins, (None, None));
    scheduler::tick(&db.state).await.unwrap();
    assert_eq!(
        counters(&db).await,
        (1, 1),
        "full scheduler must discover unpinned nodes"
    );
    let phase: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT status FROM emissary_updates WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(phase, "offered");
    assert!(
        artifactory_control_plane::ota::next(&db.state, node, session)
            .await
            .unwrap()
            .is_none(),
        "recent command is not immediately replayed"
    );
    let selected: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT release_id FROM emissary_updates WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(selected, target);
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE emissary_updates SET status='succeeded',finished_at=now()",
        params![],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET ota_draining=false",
        params![],
    )
    .await
    .unwrap();
    let (other, other_session) = enabled_node(&db, "0.0.1").await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET emissary_target_release=$1 WHERE id=$2",
        params![old, other],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE node_pools SET emissary_target_release=$1",
        params![old],
    )
    .await
    .unwrap();
    assert!(
        artifactory_control_plane::ota::next(&db.state, other, other_session)
            .await
            .unwrap()
            .is_some()
    );
    let selected: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT release_id FROM emissary_updates WHERE node_id=$1",
        params![other],
    )
    .await
    .unwrap();
    assert_eq!(
        selected, target,
        "historical pins cannot select an independent version"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_versions_same_product_metadata_and_missing_or_ambiguous_assets_do_not_drain() {
    let db = Database::with_tenant().await;
    let (node, session) = enabled_node(&db, "0.1.0").await;
    release(&db, "0.3.0").await;
    assert!(
        artifactory_control_plane::ota::next(&db.state, node, session)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(counters(&db).await, (0, 0));
    let target = release(&db, artifactory_control_plane::VERSION).await;
    for version in [
        artifactory_control_plane::VERSION.to_owned(),
        format!("v{}", artifactory_control_plane::VERSION),
        format!("emissary-v{}", artifactory_control_plane::VERSION),
        format!("{}+terminal.abc", artifactory_control_plane::VERSION),
    ] {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE nodes SET emissary_current_version=$1 WHERE id=$2",
            params![version, node],
        )
        .await
        .unwrap();
        assert!(
            artifactory_control_plane::ota::next(&db.state, node, session)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(counters(&db).await, (0, 0));
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE nodes SET emissary_current_version='0.1.0' WHERE id=$1",
        params![node],
    )
    .await
    .unwrap();
    release(&db, &format!("v{}", artifactory_control_plane::VERSION)).await;
    assert!(
        artifactory_control_plane::ota::next(&db.state, node, session)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(counters(&db).await, (0, 0));
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM emissary_releases WHERE id=$1",
        params![target],
    )
    .await
    .unwrap();
    assert!(
        artifactory_control_plane::ota::next(&db.state, node, session)
            .await
            .unwrap()
            .is_some(),
        "one legacy same-version release is supported"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_versions_keep_inflight_manifest_frozen_after_product_version_changes() {
    let db = Database::with_tenant().await;
    let old = release(&db, "0.1.0").await;
    let (node, session) = enabled_node(&db, "0.0.1").await;
    let update = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status) VALUES($1,$2,$3,$4,'downloading')",params![update,node,TENANT,old]).await.unwrap();
    release(&db, artifactory_control_plane::VERSION).await;
    let offer = artifactory_control_plane::ota::next(&db.state, node, session)
        .await
        .unwrap()
        .unwrap();
    let payload: Value = serde_json::from_str(&offer.payload_json).unwrap();
    let manifest: Value = serde_json::from_str(payload["manifest"].as_str().unwrap()).unwrap();
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(payload["id"], update.to_string());
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_versions_health_matches_frozen_manifest_for_canonical_and_legacy_assets() {
    let db = Database::with_tenant().await;
    let canonical = artifactory_control_plane::VERSION;
    for manifest_version in [canonical.to_owned(), format!("emissary-v{canonical}")] {
        let release = release(&db, &format!("test-{manifest_version}")).await;
        sql::execute(&mut db.pool.clone(),"UPDATE emissary_releases SET manifest=jsonb_set(manifest::jsonb,'{version}',to_jsonb($2::text))::text WHERE id=$1",params![release,&manifest_version]).await.unwrap();
        let (node, session) = enabled_node(&db, "0.1.0").await;
        let update = Uuid::new_v4();
        sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status,previous_version) VALUES($1,$2,$3,$4,'activating','0.1.0')",params![update,node,TENANT,release]).await.unwrap();
        let wrong = json!({"diagnostics":{"emissary_version":"0.3.0"},"ota":{"update_id":update,"phase":"succeeded","launcher_version":"0.1.0"}});
        assert!(
            !artifactory_control_plane::ota::report(&db.state, node, session, &wrong)
                .await
                .unwrap()
        );
        let payload = json!({"diagnostics":{"emissary_version":manifest_version},"ota":{"update_id":update,"phase":"succeeded","launcher_version":"0.1.0"}});
        assert!(
            artifactory_control_plane::ota::report(&db.state, node, session, &payload)
                .await
                .unwrap()
        );
        let done:bool=sql::scalar(&mut db.pool.clone(),"SELECT finished_at IS NOT NULL AND status='succeeded' FROM emissary_updates WHERE id=$1",params![update]).await.unwrap();
        assert!(done);
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ota_versions_http_status_targets_and_stale_retry_use_one_product_release() {
    let db = Database::with_tenant().await;
    let cookie = admin(&db).await;
    let (node, _) = enabled_node(&db, "0.1.0").await;
    let old = release(&db, "0.1.0").await;
    let (status, version) = request(&db, &cookie, "GET", "/emissary-version", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        version["product_version"],
        artifactory_control_plane::VERSION
    );
    assert_eq!(version["release_status"], "missing");
    assert!(version["release_id"].is_null());
    let target = release(&db, artifactory_control_plane::VERSION).await;
    let (_, version) = request(&db, &cookie, "GET", "/emissary-version", json!({})).await;
    assert_eq!(version["release_status"], "ready");
    assert_eq!(version["release_id"], target.to_string());
    for path in [
        format!("/nodes/{node}/emissary-update"),
        format!("/pools/{TENANT}/emissary-update"),
    ] {
        assert_eq!(
            request(&db, &cookie, "PATCH", &path, json!({"release_id":old}))
                .await
                .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            request(&db, &cookie, "PATCH", &path, json!({"release_id":target}))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&db, &cookie, "PATCH", &path, json!({"release_id":null}))
                .await
                .0,
            StatusCode::OK
        );
    }
    let prior = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status,finished_at) VALUES($1,$2,$3,$4,'failed',now())",params![prior,node,TENANT,old]).await.unwrap();
    assert_eq!(
        request(
            &db,
            &cookie,
            "POST",
            &format!("/emissary-updates/{prior}/retry"),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(counters(&db).await, (1, 0));
    let current_prior = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_updates(id,node_id,pool_id,release_id,status,finished_at) VALUES($1,$2,$3,$4,'failed',now())",params![current_prior,node,TENANT,target]).await.unwrap();
    for current in [
        artifactory_control_plane::VERSION.to_owned(),
        format!("{}+terminal.abc", artifactory_control_plane::VERSION),
    ] {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE nodes SET emissary_current_version=$1 WHERE id=$2",
            params![current, node],
        )
        .await
        .unwrap();
        assert_eq!(
            request(
                &db,
                &cookie,
                "POST",
                &format!("/emissary-updates/{current_prior}/retry"),
                json!({})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    }
    assert_eq!(
        counters(&db).await,
        (2, 0),
        "aligned history cannot create a redundant retry"
    );
    release(&db, &format!("v{}", artifactory_control_plane::VERSION)).await;
    let (_, version) = request(&db, &cookie, "GET", "/emissary-version", json!({})).await;
    assert_eq!(version["release_status"], "ambiguous");
    assert!(version["release_id"].is_null());
    assert_eq!(
        request(
            &db,
            &cookie,
            "PATCH",
            &format!("/nodes/{node}/emissary-update"),
            json!({"release_id":target})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    db.close().await;
}
