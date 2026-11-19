//! [INPUT]: 依赖隔离 PostgreSQL、真实 PKCS12 和 HTTP 租户会话。
//! [OUTPUT]: 验证签名版本冻结、阶段授权、撤权和最终制品证据。
//! [POS]: 托管签名数据库与 API 的集成验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_control_plane::{
    signing::{Evidence, KeyRing, Material},
    store::SigningAccess,
};
use base64::{Engine, engine::general_purpose::STANDARD};

fn material() -> Material {
    use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec!["signing.test".into()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let mut store = KeyStore::new();
    store.add_entry(
        "release",
        KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
            vec![1],
            PrivateKey::from_der(&key.serialize_der()).unwrap(),
            [Certificate::from_der(cert.der()).unwrap()],
        )),
    );
    Material::AndroidKeystore {
        file_base64: STANDARD.encode(store.writer("private-test-password").write().unwrap()),
        store_password: "private-test-password".into(),
        key_password: "private-test-password".into(),
        alias: "release".into(),
        purpose: artifactory_control_plane::signing::AndroidPurpose::Application,
    }
}
fn keys() -> KeyRing {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        json!({"active":"test","keys":{"test":STANDARD.encode([7;32])}}).to_string(),
    )
    .unwrap();
    KeyRing::load(file.path()).unwrap()
}
struct Fixture {
    db: Database,
    keys: KeyRing,
    cookie: String,
    tenant: Uuid,
    project: Uuid,
    credential: Uuid,
    version: Uuid,
    material: Material,
}
impl Fixture {
    async fn new() -> Self {
        let mut db = Database::new().await;
        let keys = keys();
        let mut settings = (*db.state.settings).clone();
        settings.signing_keys = Some(keys.clone());
        db.state.settings = Arc::new(settings);
        let (_, cookie) = account(&db, "signer", "viewer").await;
        let tenant = tenant(&db, &cookie, "signing").await;
        let project = project(&db, &cookie, tenant, "signed").await;
        let material = material();
        let (status, value) = request(
            &db,
            &cookie,
            Some(tenant),
            "POST",
            "/signing/credentials",
            json!({"name":"release","material":material}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let credential = Uuid::parse_str(value["id"].as_str().unwrap()).unwrap();
        let version = Uuid::parse_str(value["version_id"].as_str().unwrap()).unwrap();
        let (status, value) = request(
            &db,
            &cookie,
            Some(tenant),
            "POST",
            "/signing/profiles",
            json!({"name":"release","credential_id":credential}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let (status, value) = request(
            &db,
            &cookie,
            Some(tenant),
            "PUT",
            &format!("/projects/{project}/signing-defaults"),
            json!({"android":"managed:release"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        Self {
            db,
            keys,
            cookie,
            tenant,
            project,
            credential,
            version,
            material,
        }
    }
    async fn attempt(&self) -> (Uuid, Uuid, Uuid, Uuid) {
        let node = device(&self.db, self.tenant).await;
        let build = Uuid::new_v4();
        let job = Uuid::new_v4();
        let attempt = Uuid::new_v4();
        sql::execute(
            &mut self.db.pool.clone(),
            "UPDATE nodes SET signing_profiles='{managed:release}' WHERE id=$1",
            params![node],
        )
        .await
        .unwrap();
        sql::execute(&mut self.db.pool.clone(),"WITH fixture_release AS (INSERT INTO releases(id,project_id,version,requested_ref,goal,target_requirements,targets,source_resolution_id) VALUES(gen_random_uuid(),$2,'0.0.0-test.'||replace($1::text,'-',''),'main','test','[]','{}',$1) RETURNING id,version) INSERT INTO builds(id,project_id,requested_ref,source_repository_url,status,release_id,spec_revision,release_version) VALUES($1,$2,'main','https://example.com/repo','running', (SELECT id FROM fixture_release),1,(SELECT version FROM fixture_release))",params![build,self.project]).await.unwrap();
        sql::execute(&mut self.db.pool.clone(),"INSERT INTO jobs(id,build_id,target,status,target_config) VALUES($1,$2,'android','running',$3)",params![job,build,json!({"os":"linux","arch":"amd64","application_platform":"android","signing_profiles":["managed:release"],"signing_outputs":[{"path":"dist/app.apk"}],"steps":[{"name":"build","phase":"build","run":"build"},{"name":"sign","phase":"sign","run":"sign"},{"name":"verify","phase":"verify","run":"verify"}],"artifacts":["dist/app.apk"]})]).await.unwrap();
        sql::execute(&mut self.db.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id) VALUES($1,$2,$3,'lease',now()+interval '90 seconds','running',$4)",params![attempt,job,node,Uuid::new_v4()]).await.unwrap();
        let session = sql::scalar(
            &mut self.db.pool.clone(),
            "SELECT active_session FROM nodes WHERE id=$1",
            params![node],
        )
        .await
        .unwrap();
        (build, attempt, node, session)
    }
    async fn acquire(
        &self,
        attempt: Uuid,
        node: Uuid,
        session: Uuid,
        step: u32,
    ) -> std::result::Result<String, tonic::Status> {
        self.db
            .state
            .store
            .acquire_signing_material(
                SigningAccess {
                    node,
                    attempt,
                    session,
                    token: "lease",
                    profile: "managed:release",
                    version: self.version,
                },
                step,
                &self.keys,
            )
            .await
    }
}

async fn artifact(f: &Fixture, attempt: Uuid, hash: &str) -> anyhow::Result<u64> {
    let id = Uuid::new_v4();
    let mut connection = f.db.pool.clone();
    let mut tx = connection.transaction().await?;
    sql::execute(&mut tx,"INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'dist/app.apk',$3,1,$4,now()+interval '1 hour')",params![id,attempt,id.to_string(),hash]).await?;
    let changed=sql::execute(&mut tx,"INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'dist/app.apk',$3,1,$4)",params![id,attempt,id.to_string(),hash]).await?;
    tx.commit().await?;
    Ok(changed)
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn managed_signing_freezes_identity_limits_delivery_and_binds_artifact_bytes() {
    let f = Fixture::new().await;
    let (build, attempt, node, session) = f.attempt().await;
    let (status, rotated) = request(
        &f.db,
        &f.cookie,
        Some(f.tenant),
        "POST",
        &format!("/signing/credentials/{}/versions", f.credential),
        json!({"material":f.material}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let fixed: Value = sql::scalar(
        &mut f.db.pool.clone(),
        "SELECT planning_context->'signing_defaults' FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert_eq!(fixed[0]["version_id"], json!(f.version));
    assert_ne!(fixed[0]["version_id"], rotated["version_id"]);
    let (status, preview) = request(
        &f.db,
        &f.cookie,
        Some(f.tenant),
        "GET",
        &format!("/projects/{}/signing-preview", f.project),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview[0]["version_id"], rotated["version_id"]);
    assert!(!preview.to_string().contains("envelope"));
    assert!(f.acquire(attempt, node, session, 0).await.is_err());
    sql::execute(&mut f.db.pool.clone(),"INSERT INTO step_runs(attempt_id,step_index,name,phase,status) VALUES($1,0,'build','build','succeeded'),($1,1,'sign','sign','running')",params![attempt]).await.unwrap();
    assert!(f.acquire(attempt, node, Uuid::new_v4(), 1).await.is_err());
    assert!(
        f.acquire(attempt, Uuid::new_v4(), session, 1)
            .await
            .is_err()
    );
    assert!(f.acquire(attempt, node, session, 2).await.is_err());
    let acquired = f.acquire(attempt, node, session, 1).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&acquired).unwrap(),
        json!(f.material)
    );
    for (revoke, restore, id) in [
        (
            "UPDATE attempts SET lease_expires_at=now()-interval '1 second' WHERE id=$1",
            "UPDATE attempts SET lease_expires_at=now()+interval '90 seconds' WHERE id=$1",
            attempt,
        ),
        (
            "UPDATE builds SET cancel_requested=true WHERE id=$1",
            "UPDATE builds SET cancel_requested=false WHERE id=$1",
            build,
        ),
        (
            "UPDATE nodes SET signing_profiles='{}' WHERE id=$1",
            "UPDATE nodes SET signing_profiles='{managed:release}' WHERE id=$1",
            node,
        ),
        (
            "UPDATE projects SET signing_profiles='{}' WHERE id=$1",
            "UPDATE projects SET signing_profiles='{managed:release}' WHERE id=$1",
            f.project,
        ),
    ] {
        sql::execute(&mut f.db.pool.clone(), revoke, params![id])
            .await
            .unwrap();
        assert!(f.acquire(attempt, node, session, 1).await.is_err());
        sql::execute(&mut f.db.pool.clone(), restore, params![id])
            .await
            .unwrap();
    }
    let fingerprint = fixed[0]["metadata"]["fingerprint_sha256"].as_str().unwrap();
    let mut evidence = Evidence {
        path: "dist/app.apk".into(),
        sha256: "a".repeat(64),
        profile: "managed:release".into(),
        version_id: f.version.to_string(),
        fingerprint_sha256: fingerprint.into(),
        notarized: false,
    };
    assert!(
        f.db.state
            .store
            .record_signing_evidence(node, attempt, session, "lease", evidence.clone())
            .await
            .is_err()
    );
    sql::execute(
        &mut f.db.pool.clone(),
        "UPDATE step_runs SET status='succeeded' WHERE attempt_id=$1",
        params![attempt],
    )
    .await
    .unwrap();
    sql::execute(&mut f.db.pool.clone(),"INSERT INTO step_runs(attempt_id,step_index,name,phase,status) VALUES($1,2,'verify','verify','succeeded')",params![attempt]).await.unwrap();
    assert!(f.acquire(attempt, node, session, 1).await.is_err());
    assert!(artifact(&f, attempt, &evidence.sha256).await.is_err());
    evidence.fingerprint_sha256 = "b".repeat(64);
    assert!(
        f.db.state
            .store
            .record_signing_evidence(node, attempt, session, "lease", evidence.clone())
            .await
            .is_err()
    );
    evidence.fingerprint_sha256 = fingerprint.into();
    f.db.state
        .store
        .record_signing_evidence(node, attempt, session, "lease", evidence.clone())
        .await
        .unwrap();
    evidence.sha256 = "c".repeat(64);
    assert!(
        f.db.state
            .store
            .record_signing_evidence(node, attempt, session, "lease", evidence.clone())
            .await
            .is_err()
    );
    assert!(artifact(&f, attempt, &evidence.sha256).await.is_err());
    evidence.sha256 = "a".repeat(64);
    artifact(&f, attempt, &evidence.sha256).await.unwrap();
    let saved: Value = sql::scalar(
        &mut f.db.pool.clone(),
        "SELECT signing FROM artifacts WHERE attempt_id=$1",
        params![attempt],
    )
    .await
    .unwrap();
    assert_eq!(saved, json!(evidence));
    f.db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn managed_signing_hides_material_isolates_tenants_and_prevents_identity_rotation() {
    let f = Fixture::new().await;
    let (status, public) = request(
        &f.db,
        &f.cookie,
        Some(f.tenant),
        "GET",
        "/signing/credentials",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!public.to_string().contains("private-test-password"));
    assert!(!public.to_string().contains("envelope"));
    assert!(!public.to_string().contains("file_base64"));
    let other = tenant(&f.db, &f.cookie, "other").await;
    let (status, value) = request(
        &f.db,
        &f.cookie,
        Some(other),
        "GET",
        "/signing/credentials",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["credentials"], json!([]));
    let (status, _) = request(
        &f.db,
        &f.cookie,
        Some(other),
        "PATCH",
        &format!("/signing/credentials/{}", f.credential),
        json!({"enabled":false}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(
        &f.db,
        &f.cookie,
        Some(f.tenant),
        "POST",
        &format!("/signing/credentials/{}/versions", f.credential),
        json!({"material":material()}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Nested signing route accepts a request larger than the ordinary 512 KiB cap.
    let mut invalid = json!(f.material);
    invalid["file_base64"] = json!("A".repeat(600_000));
    let (status, _) = request(
        &f.db,
        &f.cookie,
        Some(f.tenant),
        "POST",
        "/signing/credentials",
        json!({"name":"invalid","material":invalid}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    f.db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn managed_signing_rechecks_revocation_after_lock_wait() {
    let f = Fixture::new().await;
    let (_, attempt, node, session) = f.attempt().await;
    sql::execute(&mut f.db.pool.clone(),"INSERT INTO step_runs(attempt_id,step_index,name,phase,status) VALUES($1,0,'sign','sign','running')",params![attempt]).await.unwrap();
    let mut connection = f.db.pool.clone();
    let mut tx = connection.transaction().await.unwrap();
    sql::execute(
        &mut tx,
        "UPDATE signing_versions SET enabled=false WHERE id=$1",
        params![f.version],
    )
    .await
    .unwrap();
    {
        let acquire = f.acquire(attempt, node, session, 0);
        tokio::pin!(acquire);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut acquire)
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
        assert!(acquire.await.is_err());
    }
    f.db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn managed_signing_reports_unavailable_latest_versions_without_fallback() {
    let f = Fixture::new().await;
    for enabled in [false, true] {
        let (status, _) = request(
            &f.db,
            &f.cookie,
            Some(f.tenant),
            "PATCH",
            &format!("/signing/versions/{}", f.version),
            json!({"enabled":enabled}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        for path in [
            "/signing/profiles".to_owned(),
            format!("/projects/{}/signing-preview", f.project),
        ] {
            let (status, value) =
                request(&f.db, &f.cookie, Some(f.tenant), "GET", &path, json!({})).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(value[0]["available"], json!(enabled));
            assert_eq!(value[0]["version_id"], json!(f.version));
        }
    }
    let expired = Uuid::new_v4();
    sql::execute(&mut f.db.pool.clone(), "INSERT INTO signing_versions(id,tenant_id,credential_id,metadata,envelope,created_at) SELECT $2,tenant_id,credential_id,metadata||'{\"not_after\":1}'::jsonb,envelope,clock_timestamp()+interval '1 second' FROM signing_versions WHERE id=$1", params![f.version,expired]).await.unwrap();
    for path in [
        "/signing/profiles".to_owned(),
        format!("/projects/{}/signing-preview", f.project),
    ] {
        let (status, value) =
            request(&f.db, &f.cookie, Some(f.tenant), "GET", &path, json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value[0]["available"], false);
        assert_eq!(value[0]["version_id"], json!(expired));
    }
    f.db.close().await;
}
