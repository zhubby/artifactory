//! [INPUT]: 依赖隔离数据库、真实账户角色与签名发行夹具。
//! [OUTPUT]: 验证全局地址、统一版本安装材料和租户边界。
//! [POS]: 设备安装 HTTP 接口的集成验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

async fn pool(db: &Database, tenant: Uuid) -> Uuid {
    sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM node_pools WHERE tenant_id=$1",
        params![tenant],
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_settings_use_global_account_role_and_survive_tenant_switch() {
    let db = Database::new().await;
    let (_, admin) = account(&db, "system-admin", "admin").await;
    let (_, owner) = account(&db, "ordinary-owner", "viewer").await;
    let first = tenant(&db, &admin, "First").await;
    let second = tenant(&db, &owner, "Second").await;
    let (status, settings) = request(
        &db,
        &admin,
        Some(first),
        "PATCH",
        "/system/node-installation",
        json!({"device_endpoint":"agents.example.com:31443"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(
        settings,
        json!({"device_endpoint":"agents.example.com:31443","can_edit":true})
    );
    let (status, settings) = request(
        &db,
        &owner,
        Some(second),
        "GET",
        "/system/node-installation",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert_eq!(
        settings,
        json!({"device_endpoint":"agents.example.com:31443","can_edit":false})
    );
    assert_eq!(
        request(
            &db,
            &owner,
            Some(second),
            "PATCH",
            "/system/node-installation",
            json!({"device_endpoint":"attacker.example.com:443"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for endpoint in [
        "https://agents.example.com:443",
        "a:443/path",
        "a:0",
        "a:0443",
        "a:65536",
        "a:443?x=1",
        "a@b:443",
        "a:443\n",
        "2001:db8::1:443",
        "a;echo:443",
    ] {
        assert_eq!(
            request(
                &db,
                &admin,
                Some(first),
                "PATCH",
                "/system/node-installation",
                json!({"device_endpoint":endpoint})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST,
            "{endpoint}"
        );
    }
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/system/node-installation")
                .header("cookie", &admin)
                .header("X-Artifactory-Tenant", first.to_string())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"device_endpoint":"a:443"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let (member_id, member) = account(&db, "ordinary-member", "viewer").await;
    sql::execute(
        &mut db.pool.clone(),
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'member')",
        params![first, member_id],
    )
    .await
    .unwrap();
    for path in ["/system/node-installation", "/node-installation/options"] {
        assert_eq!(
            request(&db, &member, Some(first), "GET", path, json!({}))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/system/node-installation")
                .header("authorization", "Bearer not-a-session")
                .header("x-artifactory-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"device_endpoint":"a:443"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    db.close().await;
}

async fn signed_release(db: &mut Database, tenant: Uuid) -> (tempfile::TempDir, Uuid) {
    signed_release_version(db, tenant, artifactory_control_plane::VERSION).await
}

async fn signed_release_version(
    db: &mut Database,
    tenant: Uuid,
    version: &str,
) -> (tempfile::TempDir, Uuid) {
    let directory = tempfile::tempdir().unwrap();
    let signing = SigningKey::from_bytes(&[21; 32]);
    let public_key = STANDARD.encode(signing.verifying_key().as_bytes());
    let mut ca_params = rcgen::CertificateParams::default();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = ca_params
        .self_signed(&rcgen::KeyPair::generate().unwrap())
        .unwrap()
        .pem();
    tokio::fs::write(directory.path().join("ca.pem"), ca)
        .await
        .unwrap();
    tokio::fs::write(
        directory.path().join("emissary-releases.json"),
        json!({"repository":"fixture/artifactory","public_keys":[public_key]}).to_string(),
    )
    .await
    .unwrap();
    let settings = Arc::make_mut(&mut db.state.settings);
    settings.pki_dir = directory.path().into();
    settings.data_dir = directory.path().into();
    let assets = [("linux","amd64"),("linux","arm64"),("darwin","amd64"),("darwin","arm64"),("windows","amd64"),("windows","arm64")].map(|(os,arch)|json!({"os":os,"arch":arch,"name":format!("artifactory-emissary-{os}-{arch}{}",if os=="windows" {".exe"} else {""}),"size":10,"sha256":"b".repeat(64)}));
    let manifest = json!({"version":version,"commit":"a".repeat(40),"protocol_min":1,"protocol_max":1,"assets":assets}).to_string();
    let hash = hex::encode(Sha256::digest(manifest.as_bytes()));
    let signature = STANDARD.encode(signing.sign(manifest.as_bytes()).to_bytes());
    let id = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(), "INSERT INTO emissary_releases(id,tenant_id,version,manifest,signature,manifest_hash) VALUES($1,$2,$3,$4,$5,$6)", params![id,tenant,version,manifest,signature,hash]).await.unwrap();
    (directory, id)
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_prepares_fixed_material_before_issuing_single_use_credential() {
    let mut db = Database::new().await;
    let (_, admin) = account(&db, "install-admin", "admin").await;
    let tenant_id = tenant(&db, &admin, "Install").await;
    let pool_id = pool(&db, tenant_id).await;
    let (_directory, release) = signed_release(&mut db, tenant_id).await;
    let input = json!({"pool_id":pool_id,"release_id":release,"os":"linux","arch":"arm64"});
    assert_eq!(
        request(
            &db,
            &admin,
            Some(tenant_id),
            "POST",
            "/node-installation",
            input.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM enrollment_tokens",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        request(
            &db,
            &admin,
            Some(tenant_id),
            "PATCH",
            "/system/node-installation",
            json!({"device_endpoint":"agents.example.com:31443"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let ca_path = db.state.settings.pki_dir.join("ca.pem");
    let ca = tokio::fs::read(&ca_path).await.unwrap();
    tokio::fs::remove_file(&ca_path).await.unwrap();
    assert_eq!(
        request(
            &db,
            &admin,
            Some(tenant_id),
            "POST",
            "/node-installation",
            input.clone()
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM enrollment_tokens",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 0, "missing CA must not consume enrollment capacity");
    tokio::fs::write(&ca_path, ca).await.unwrap();
    let mut unsupported = input.clone();
    unsupported["arch"] = json!("riscv64");
    assert_eq!(
        request(
            &db,
            &admin,
            Some(tenant_id),
            "POST",
            "/node-installation",
            unsupported
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let before = chrono::Utc::now();
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/node-installation")
                .header("cookie", &admin)
                .header("X-Artifactory-Tenant", tenant_id.to_string())
                .header("x-artifactory-csrf", "1")
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let material: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(material["device_endpoint"], "agents.example.com:31443");
    assert_eq!(material["version"], artifactory_control_plane::VERSION);
    assert_eq!(material["sha256"], "b".repeat(64));
    assert_eq!(material["size"], 10);
    assert_eq!(
        material["ca_sha256"],
        hex::encode(Sha256::digest(
            material["ca_pem"].as_str().unwrap().as_bytes()
        ))
    );
    assert!(
        material["download_url"]
            .as_str()
            .unwrap()
            .starts_with("http://localhost:3000/artifacts/object?ticket=")
    );
    assert_eq!(
        material["installer_url"],
        "http://localhost:3000/api/v1/node-installation/installer.sh"
    );
    let installer = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/v1/node-installation/installer.sh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(installer.status(), StatusCode::OK);
    let script = axum::body::to_bytes(installer.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(
        material["installer_sha256"],
        hex::encode(Sha256::digest(&script))
    );
    let (status, options) = request(
        &db,
        &admin,
        Some(tenant_id),
        "GET",
        "/node-installation/options",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(
        options["product_version"],
        artifactory_control_plane::VERSION
    );
    assert_eq!(options["releases"][0]["id"], release.to_string());
    assert_eq!(
        options["releases"][0]["platforms"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    let token = material["token"].as_str().unwrap();
    let expiry: chrono::DateTime<chrono::Utc> =
        material["expires_at"].as_str().unwrap().parse().unwrap();
    assert!((expiry - before).num_seconds() >= 899 && (expiry - before).num_seconds() <= 901);
    let download = url::Url::parse(material["download_url"].as_str().unwrap()).unwrap();
    let ticket = download
        .query_pairs()
        .find(|(key, _)| key == "ticket")
        .unwrap()
        .1;
    let raw = crypto::open_context(
        &db.state.settings.master_key,
        &ticket,
        b"artifact-gateway/v1",
    )
    .unwrap();
    let ticket: Value = serde_json::from_slice(&raw).unwrap();
    assert!(expiry.timestamp() <= ticket["expires"].as_i64().unwrap());
    let stored: Value = sql::scalar(&mut db.pool.clone(), "SELECT jsonb_build_object('pool_id',pool_id,'expires_at',expires_at,'used_at',used_at) FROM enrollment_tokens WHERE token_hash=$1", params![crypto::hash(token)]).await.unwrap();
    assert_eq!(stored["pool_id"], pool_id.to_string());
    assert_eq!(
        stored["expires_at"]
            .as_str()
            .unwrap()
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap(),
        expiry
    );
    assert!(stored["used_at"].is_null());
    let audit: Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT details FROM audit_events WHERE action='node_installation_created'",
        params![],
    )
    .await
    .unwrap();
    assert!(!audit.to_string().contains(token));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_rejects_cross_tenant_assets_and_invalid_signature_without_token() {
    let mut db = Database::new().await;
    let (_, admin) = account(&db, "asset-admin", "admin").await;
    let (_, other) = account(&db, "asset-owner", "viewer").await;
    let a = tenant(&db, &admin, "A").await;
    let b = tenant(&db, &other, "B").await;
    let (_directory, release) = signed_release(&mut db, a).await;
    let pool_a = pool(&db, a).await;
    let pool_b = pool(&db, b).await;
    assert_eq!(
        request(
            &db,
            &admin,
            Some(a),
            "PATCH",
            "/system/node-installation",
            json!({"device_endpoint":"agents.example.com:31443"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &db,
            &other,
            Some(b),
            "POST",
            "/node-installation",
            json!({"pool_id":pool_b,"release_id":release,"os":"linux","arch":"amd64"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &db,
            &admin,
            Some(a),
            "POST",
            "/node-installation",
            json!({"pool_id":pool_b,"release_id":release,"os":"linux","arch":"amd64"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, options) = request(
        &db,
        &other,
        Some(b),
        "GET",
        "/node-installation/options",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert!(options["releases"].as_array().unwrap().is_empty());
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE emissary_releases SET signature=$1 WHERE id=$2",
        params![STANDARD.encode([0; 64]), release],
    )
    .await
    .unwrap();
    assert_eq!(
        request(
            &db,
            &admin,
            Some(a),
            "POST",
            "/node-installation",
            json!({"pool_id":pool_a,"release_id":release,"os":"linux","arch":"amd64"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM enrollment_tokens",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_filters_untrusted_history_and_malformed_rotation_keys() {
    let mut db = Database::new().await;
    let (_, admin) = account(&db, "rotation-admin", "admin").await;
    let tenant_id = tenant(&db, &admin, "Rotation").await;
    let (directory, release) = signed_release(&mut db, tenant_id).await;
    let pool_id = pool(&db, tenant_id).await;
    sql::execute(&mut db.pool.clone(),
        "INSERT INTO emissary_releases(id,tenant_id,version,manifest,signature,manifest_hash) SELECT $1,tenant_id,'emissary-v0.0.1',manifest,$2,repeat('0',64) FROM emissary_releases WHERE id=$3",
        params![Uuid::new_v4(),STANDARD.encode([0;64]),release]).await.unwrap();
    let source_path = directory.path().join("emissary-releases.json");
    let mut source: Value =
        serde_json::from_slice(&tokio::fs::read(&source_path).await.unwrap()).unwrap();
    source["public_keys"]
        .as_array_mut()
        .unwrap()
        .push(json!("broken-rotation-key"));
    tokio::fs::write(&source_path, source.to_string())
        .await
        .unwrap();
    let (status, options) = request(
        &db,
        &admin,
        Some(tenant_id),
        "GET",
        "/node-installation/options",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["releases"].as_array().unwrap().len(), 1);
    assert_eq!(
        options["product_version"],
        artifactory_control_plane::VERSION
    );
    assert_eq!(options["releases"][0]["id"], release.to_string());
    request(
        &db,
        &admin,
        Some(tenant_id),
        "PATCH",
        "/system/node-installation",
        json!({"device_endpoint":"agents.example.com:443"}),
    )
    .await;
    let (status, material) = request(
        &db,
        &admin,
        Some(tenant_id),
        "POST",
        "/node-installation",
        json!({"pool_id":pool_id,"release_id":release,"os":"linux","arch":"amd64"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{material}");
    assert_eq!(material["public_keys"].as_array().unwrap().len(), 1);
    assert_eq!(
        STANDARD
            .decode(material["public_keys"][0].as_str().unwrap())
            .unwrap()
            .len(),
        32
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_lock_wait_does_not_extend_download_deadline() {
    let mut db = Database::new().await;
    let (_, admin) = account(&db, "waiting-admin", "admin").await;
    let tenant_id = tenant(&db, &admin, "Waiting").await;
    let (_directory, release) = signed_release(&mut db, tenant_id).await;
    let pool_id = pool(&db, tenant_id).await;
    request(
        &db,
        &admin,
        Some(tenant_id),
        "PATCH",
        "/system/node-installation",
        json!({"device_endpoint":"agents.example.com:443"}),
    )
    .await;
    let mut connection = db.pool.clone();
    let mut tx = connection.transaction().await.unwrap();
    sql::execute(
        &mut tx,
        "SELECT id FROM tenants WHERE id=$1 FOR NO KEY UPDATE",
        params![tenant_id],
    )
    .await
    .unwrap();
    let pending = request(
        &db,
        &admin,
        Some(tenant_id),
        "POST",
        "/node-installation",
        json!({"pool_id":pool_id,"release_id":release,"os":"linux","arch":"amd64"}),
    );
    let release_lock = async {
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
        tx.commit().await.unwrap();
    };
    let ((status, material), ()) = tokio::join!(pending, release_lock);
    assert_eq!(status, StatusCode::OK, "{material}");
    let expiry: chrono::DateTime<chrono::Utc> =
        material["expires_at"].as_str().unwrap().parse().unwrap();
    let download = url::Url::parse(material["download_url"].as_str().unwrap()).unwrap();
    let ticket = download
        .query_pairs()
        .find(|(key, _)| key == "ticket")
        .unwrap()
        .1;
    let raw = crypto::open_context(
        &db.state.settings.master_key,
        &ticket,
        b"artifact-gateway/v1",
    )
    .unwrap();
    let ticket: Value = serde_json::from_slice(&raw).unwrap();
    assert!(
        expiry.timestamp() <= ticket["expires"].as_i64().unwrap(),
        "credential cannot outlive its download ticket"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_rejects_other_versions_and_ambiguous_aliases_without_token() {
    let mut db = Database::new().await;
    let (_, admin) = account(&db, "version-admin", "admin").await;
    let tenant_id = tenant(&db, &admin, "Version").await;
    let pool_id = pool(&db, tenant_id).await;
    let (_old_directory, old) = signed_release_version(&mut db, tenant_id, "emissary-v0.0.1").await;
    request(
        &db,
        &admin,
        Some(tenant_id),
        "PATCH",
        "/system/node-installation",
        json!({"device_endpoint":"agents.example.com:443"}),
    )
    .await;
    let (status, options) = request(
        &db,
        &admin,
        Some(tenant_id),
        "GET",
        "/node-installation/options",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(
        options["product_version"],
        artifactory_control_plane::VERSION
    );
    assert!(options["releases"].as_array().unwrap().is_empty());
    let input =
        |release| json!({"pool_id":pool_id,"release_id":release,"os":"linux","arch":"amd64"});
    let (status, error) = request(
        &db,
        &admin,
        Some(tenant_id),
        "POST",
        "/node-installation",
        input(old),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    let (_directory, current) = signed_release(&mut db, tenant_id).await;
    let (_alias_directory, alias) = signed_release_version(
        &mut db,
        tenant_id,
        &format!("emissary-v{}", artifactory_control_plane::VERSION),
    )
    .await;
    let (status, options) = request(
        &db,
        &admin,
        Some(tenant_id),
        "GET",
        "/node-installation/options",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert!(
        options["releases"].as_array().unwrap().is_empty(),
        "ambiguous release aliases cannot be selected"
    );
    for release in [current, alias] {
        let (status, error) = request(
            &db,
            &admin,
            Some(tenant_id),
            "POST",
            "/node-installation",
            input(release),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{error}");
    }
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM enrollment_tokens",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "version rejection must precede enrollment credential issuance"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn node_installation_accepts_canonical_manifest_with_legacy_release_tag() {
    let mut db = Database::new().await;
    let (_, admin) = account(&db, "canonical-admin", "admin").await;
    let tenant_id = tenant(&db, &admin, "Canonical").await;
    let pool_id = pool(&db, tenant_id).await;
    let (_directory, release) = signed_release(&mut db, tenant_id).await;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE emissary_releases SET version=$1 WHERE id=$2",
        params![
            format!("emissary-v{}", artifactory_control_plane::VERSION),
            release
        ],
    )
    .await
    .unwrap();
    request(
        &db,
        &admin,
        Some(tenant_id),
        "PATCH",
        "/system/node-installation",
        json!({"device_endpoint":"agents.example.com:443"}),
    )
    .await;
    let (status, options) = request(
        &db,
        &admin,
        Some(tenant_id),
        "GET",
        "/node-installation/options",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["releases"][0]["id"], release.to_string());
    let (status, material) = request(
        &db,
        &admin,
        Some(tenant_id),
        "POST",
        "/node-installation",
        json!({"pool_id":pool_id,"release_id":release,"os":"linux","arch":"amd64"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{material}");
    assert_eq!(material["version"], artifactory_control_plane::VERSION);
    db.close().await;
}
