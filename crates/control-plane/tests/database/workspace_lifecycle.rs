//! Lifecycle authority, admission and operation progress use the real database.
use super::*;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn production_scheduler_consumes_durable_lifecycle_operations() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "background-owner", "viewer").await;
    let tenant = tenant(&db, &owner, "Background").await;
    let project = project(&db, &owner, tenant, "Desktop").await;
    let (status, operation) = request(
        &db,
        &owner,
        Some(tenant),
        "POST",
        &format!("/projects/{project}/lifecycle"),
        json!({"action":"archive","idempotency_key":Uuid::new_v4()}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    scheduler::start(db.state.clone());
    let path = format!("/operations/{}", operation["id"].as_str().unwrap());
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (_, operation) = request(&db, &owner, None, "GET", &path, json!({})).await;
            if operation["state"] == "succeeded" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    db.close().await;
    result.expect("production scheduler must consume the durable queue without a manual tick");
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn project_archive_fences_admission_and_restore_preserves_the_route() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "lifecycle-owner", "viewer").await;
    let (_, other) = account(&db, "other-owner", "viewer").await;
    let t = tenant(&db, &owner, "Studio").await;
    let p = project(&db, &owner, t, "Desktop").await;
    let input = json!({"action":"archive","idempotency_key":Uuid::new_v4()});
    let (status, op) = request(
        &db,
        &owner,
        Some(t),
        "POST",
        &format!("/projects/{p}/lifecycle"),
        input.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{op}");
    assert_eq!(
        request(
            &db,
            &owner,
            Some(t),
            "POST",
            &format!("/projects/{p}/lifecycle"),
            input
        )
        .await
        .1["id"],
        op["id"]
    );
    assert_eq!(
        request(
            &db,
            &owner,
            Some(t),
            "POST",
            &format!("/projects/{p}/builds"),
            json!({"ref":"main"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    scheduler::tick(&db.state).await.unwrap();
    let (_, detail) = request(
        &db,
        &owner,
        Some(t),
        "GET",
        "/projects/by-slug/desktop",
        json!({}),
    )
    .await;
    assert_eq!(detail["status"], "archived");
    let path = format!("/operations/{}", op["id"].as_str().unwrap());
    assert_eq!(
        request(&db, &owner, None, "GET", &path, json!({})).await.1["state"],
        "succeeded"
    );
    assert_eq!(
        request(&db, &other, None, "GET", &path, json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, value) = request(
        &db,
        &owner,
        Some(t),
        "POST",
        &format!("/projects/{p}/lifecycle"),
        json!({"action":"restore","idempotency_key":Uuid::new_v4()}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{value}");
    assert_eq!(
        request(
            &db,
            &owner,
            Some(t),
            "GET",
            "/projects/by-slug/desktop",
            json!({})
        )
        .await
        .1["status"],
        "active"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn draining_waits_for_existing_work_and_can_be_cancelled() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "drain-owner", "viewer").await;
    let t = tenant(&db, &owner, "Waiting").await;
    let p = project(&db, &owner, t, "Desktop").await;
    let (_, build) = request(
        &db,
        &owner,
        Some(t),
        "POST",
        &format!("/projects/{p}/builds"),
        json!({"ref":"main"}),
    )
    .await;
    let (status, op) = request(
        &db,
        &owner,
        Some(t),
        "POST",
        "/tenant/lifecycle",
        json!({"action":"archive","idempotency_key":Uuid::new_v4()}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{op}");
    scheduler::tick(&db.state).await.unwrap();
    let path = format!("/operations/{}", op["id"].as_str().unwrap());
    let (_, waiting) = request(&db, &owner, None, "GET", &path, json!({})).await;
    assert_eq!(waiting["state"], "waiting");
    assert!(
        waiting["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == build["id"])
    );
    assert_eq!(
        request(
            &db,
            &owner,
            None,
            "POST",
            &format!("{path}/cancel"),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&db, &owner, Some(t), "GET", "/tenant", json!({}))
            .await
            .1["tenant"]["status"],
        "active"
    );
    db.close().await;
}

async fn deletion(
    db: &Database,
    owner: &str,
    tenant: Uuid,
    project: Option<Uuid>,
    slug: &str,
) -> Value {
    let path = project
        .map(|id| format!("/projects/{id}/lifecycle"))
        .unwrap_or("/tenant/lifecycle".into());
    let (status,op)=request(db,owner,Some(tenant),"POST",&path,json!({"action":"delete","idempotency_key":Uuid::new_v4(),"slug":slug,"current_password":"deletion-password"})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{op}");
    op
}
async fn password(db: &Database, id: Uuid) {
    let hash = artifactory_control_plane::auth::password_hash("deletion-password").unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE users SET password_hash=$2 WHERE id=$1",
        params![id, hash],
    )
    .await
    .unwrap();
}
async fn finish(db: &Database, owner: &str, op: &Value) -> Value {
    let path = format!("/operations/{}", op["id"].as_str().unwrap());
    for _ in 0..16 {
        artifactory_control_plane::lifecycle::tick(&db.state)
            .await
            .unwrap();
        let (_, current) = request(db, owner, None, "GET", &path, json!({})).await;
        assert_ne!(current["state"], "failed", "{current}");
        if current["state"] == "succeeded" {
            return current;
        }
    }
    panic!("operation did not complete")
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn permanent_deletion_preserves_other_workspaces_and_tombstones_survive_restart() {
    let db = Database::new().await;
    let (actor, owner) = account(&db, "delete-owner", "viewer").await;
    password(&db, actor).await;
    let a = tenant(&db, &owner, "Delete Team").await;
    let b = tenant(&db, &owner, "Other Team").await;
    let target = project(&db, &owner, a, "Desktop").await;
    let survivor = project(&db, &owner, a, "Survivor").await;
    let other = project(&db, &owner, b, "Desktop").await;
    let device = device(&db, a).await;
    for slug in ["desktop-partial", "Desktop"] {
        assert_eq!(request(&db,&owner,Some(a),"POST",&format!("/projects/{target}/lifecycle"),json!({"action":"delete","idempotency_key":Uuid::new_v4(),"slug":slug,"current_password":"deletion-password"})).await.0,StatusCode::BAD_REQUEST);
    }
    let op = deletion(&db, &owner, a, Some(target), "desktop").await;
    let completed = finish(&db, &owner, &op).await;
    assert!(completed.get("manifest").is_none());
    assert!(completed.get("generation").is_none());
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "GET",
            &format!("/projects/{target}"),
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for (tenant, id) in [(a, survivor), (b, other)] {
        assert_eq!(
            request(
                &db,
                &owner,
                Some(tenant),
                "GET",
                &format!("/projects/{id}"),
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let kept: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1 AND NOT revoked)",
        params![device],
    )
    .await
    .unwrap();
    assert!(kept);
    let replacement = project(&db, &owner, a, "Desktop").await;
    assert_eq!(
        request(
            &db,
            &owner,
            Some(a),
            "GET",
            &format!("/projects/{replacement}"),
            json!({})
        )
        .await
        .1["slug"],
        "desktop-2"
    );
    let op = deletion(&db, &owner, a, None, "delete-team").await;
    finish(&db, &owner, &op).await;
    assert_eq!(
        request(&db, &owner, None, "GET", "/tenants/delete-team", json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &db,
            &owner,
            Some(b),
            "GET",
            &format!("/projects/{other}"),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    let next = tenant(&db, &owner, "Delete Team").await;
    assert_ne!(next, a);
    assert_eq!(
        request(
            &db,
            &owner,
            None,
            "GET",
            "/tenants/delete-team-2",
            json!({})
        )
        .await
        .1["id"],
        next.to_string()
    );
    db.state
        .store
        .bootstrap_admin("delete-owner".into(), "unused".into())
        .await
        .unwrap();
    let default = Uuid::from_u128(1);
    let op = deletion(&db, &owner, default, None, "default").await;
    finish(&db, &owner, &op).await;
    db.state
        .store
        .bootstrap_admin("delete-owner".into(), "unused".into())
        .await
        .unwrap();
    let resurrected: bool = sql::scalar(
        &mut db.pool.clone(),
        "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)",
        params![default],
    )
    .await
    .unwrap();
    assert!(!resurrected);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn expired_leases_are_fenced_and_tenant_restore_preserves_archived_projects() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "takeover-owner", "viewer").await;
    let tenant = tenant(&db, &owner, "Takeover").await;
    let project = project(&db, &owner, tenant, "Archived").await;
    let (_, op) = request(
        &db,
        &owner,
        Some(tenant),
        "POST",
        &format!("/projects/{project}/lifecycle"),
        json!({"action":"archive","idempotency_key":Uuid::new_v4()}),
    )
    .await;
    let old = db
        .state
        .store
        .lease_lifecycle(Uuid::new_v4())
        .await
        .unwrap()
        .unwrap();
    assert!(
        db.state
            .store
            .lease_lifecycle(Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    sql::execute(&mut db.pool.clone(),"UPDATE lifecycle_operations SET lease_until=clock_timestamp()-interval '1 second' WHERE id=$1",params![old.id]).await.unwrap();
    let current = db
        .state
        .store
        .lease_lifecycle(Uuid::new_v4())
        .await
        .unwrap()
        .unwrap();
    assert!(current.generation > old.generation);
    db.state.store.advance_lifecycle_drain(&old).await.unwrap();
    assert_eq!(
        request(
            &db,
            &owner,
            None,
            "GET",
            &format!("/operations/{}", old.id),
            json!({})
        )
        .await
        .1["state"],
        "waiting"
    );
    db.state
        .store
        .advance_lifecycle_drain(&current)
        .await
        .unwrap();
    finish(&db, &owner, &op).await;
    let (_, op) = request(
        &db,
        &owner,
        Some(tenant),
        "POST",
        "/tenant/lifecycle",
        json!({"action":"archive","idempotency_key":Uuid::new_v4()}),
    )
    .await;
    finish(&db, &owner, &op).await;
    assert_eq!(
        request(
            &db,
            &owner,
            Some(tenant),
            "POST",
            "/tenant/lifecycle",
            json!({"action":"restore","idempotency_key":Uuid::new_v4()})
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        request(
            &db,
            &owner,
            Some(tenant),
            "GET",
            &format!("/projects/{project}"),
            json!({})
        )
        .await
        .1["status"],
        "archived"
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and private S3"]
async fn deletion_inventory_cleans_uploads_artifacts_logs_and_survives_a_failed_storage_step() {
    let db = Database::new().await;
    let (actor, owner) = account(&db, "storage-owner", "admin").await;
    password(&db, actor).await;
    db.state
        .store
        .bootstrap_admin("storage-owner".into(), "unused".into())
        .await
        .unwrap();
    let (project, build, job) = db.fixture().await;
    let tenant = Uuid::from_u128(1);
    let node = db.node().await;
    let attempt = Uuid::new_v4();
    let upload = Uuid::new_v4();
    let log = format!("logs/{attempt}/known");
    let late_log = format!("logs/{attempt}/late");
    let incoming = format!("uploads/{upload}");
    let promoted = format!("artifacts/{upload}");
    sql::execute(&mut db.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id) VALUES($1,$2,$3,'expired',now()-interval '1 hour','succeeded',$4)",params![attempt,job,node,Uuid::new_v4()]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'app',$3,1,$4,now()+interval '1 minute')",params![upload,attempt,&incoming,"a".repeat(64)]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'app',$3,1,$4)",params![upload,attempt,&promoted,"a".repeat(64)]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO log_chunks(attempt_id,sequence,step,object_key,sha256,size) VALUES($1,1,'compile',$2,$3,1)",params![attempt,&log,"a".repeat(64)]).await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE jobs SET status='succeeded' WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET status='succeeded',publish_status='not_requested' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    db.state.storage.initialize().await.unwrap();
    let untouched = format!("unrelated/{}/sentinel", Uuid::new_v4());
    for key in [&incoming, &promoted, &log, &late_log, &untouched] {
        db.state.storage.put(key, b"x".to_vec()).await.unwrap();
    }
    let op = deletion(&db, &owner, tenant, Some(project), "test").await;
    let id = Uuid::parse_str(op["id"].as_str().unwrap()).unwrap();
    let drain = db
        .state
        .store
        .lease_lifecycle(Uuid::new_v4())
        .await
        .unwrap()
        .unwrap();
    db.state
        .store
        .advance_lifecycle_drain(&drain)
        .await
        .unwrap();
    artifactory_control_plane::lifecycle::tick(&db.state)
        .await
        .unwrap();
    let (_, waiting) = request(
        &db,
        &owner,
        None,
        "GET",
        &format!("/operations/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(waiting["phase"], "objects");
    assert!(
        db.state.storage.get_log(&promoted).await.is_ok(),
        "issued upload authorization has not expired yet"
    );
    // A real object-store outage retains the inventory and cursor. A replacement
    // replica can retry the same idempotent step after storage recovers.
    sql::execute(&mut db.pool.clone(),"UPDATE lifecycle_operations SET manifest=jsonb_set(manifest,'{uploads_expire_at}',to_jsonb((now()-interval '1 minute')::text)),next_run_at=now() WHERE id=$1",params![id]).await.unwrap();
    let mut unavailable_settings = (*db.state.settings).clone();
    unavailable_settings.s3_endpoint = "http://127.0.0.1:1".into();
    let mut unavailable_replica = db.state.clone();
    unavailable_replica.storage = Arc::new(Storage::new(&unavailable_settings).unwrap());
    artifactory_control_plane::lifecycle::tick(&unavailable_replica)
        .await
        .unwrap();
    let failed: (String, i64) = sql::tuple(
        &mut db.pool.clone(),
        "SELECT state,object_cursor FROM lifecycle_operations WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(failed, ("failed".into(), 0));
    assert!(db.state.storage.get_log(&promoted).await.is_ok());
    assert_eq!(
        request(
            &db,
            &owner,
            None,
            "POST",
            &format!("/operations/{id}/cancel"),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &db,
            &owner,
            None,
            "POST",
            &format!("/operations/{id}/retry"),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    finish(&db, &owner, &op).await;
    for key in [&incoming, &promoted, &log, &late_log] {
        assert!(
            db.state.storage.get_log(key).await.is_err(),
            "target object remains"
        );
    }
    assert_eq!(db.state.storage.get_log(&untouched).await.unwrap(), b"x");
    let rows:i64=sql::scalar(&mut db.pool.clone(),"SELECT (SELECT count(*) FROM builds WHERE id=$1)+(SELECT count(*) FROM attempts WHERE id=$2)+(SELECT count(*) FROM artifacts WHERE id=$3)+(SELECT count(*) FROM uploads WHERE id=$3)",params![build,attempt,upload]).await.unwrap();
    assert_eq!(rows, 0);
    db.state.storage.delete(&untouched).await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL and private S3"]
async fn shared_release_objects_survive_other_tenants_and_concurrent_imports() {
    let db = Database::new().await;
    let (actor, owner) = account(&db, "shared-owner", "viewer").await;
    password(&db, actor).await;
    let a = tenant(&db, &owner, "First Assets").await;
    let b = tenant(&db, &owner, "Shared Assets").await;
    let c = tenant(&db, &owner, "Importing Assets").await;
    let hash = crypto::hash(Uuid::new_v4().as_bytes());
    let key = format!("emissary-releases/{hash}/agent");
    for tenant in [a, b] {
        sql::execute(&mut db.pool.clone(),"INSERT INTO emissary_releases(id,tenant_id,version,manifest,signature,manifest_hash) VALUES($1,$2,'test','{}','test',$3)",params![Uuid::new_v4(),tenant,&hash]).await.unwrap();
    }
    db.state.storage.initialize().await.unwrap();
    db.state
        .storage
        .put(&key, b"shared".to_vec())
        .await
        .unwrap();
    let first = deletion(&db, &owner, a, None, "first-assets").await;
    finish(&db, &owner, &first).await;
    assert_eq!(db.state.storage.get_log(&key).await.unwrap(), b"shared");
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM emissary_releases WHERE tenant_id=$1",
        params![b],
    )
    .await
    .unwrap();
    assert_eq!(count, 1);

    // A different tenant may already be importing the same immutable asset.
    let task = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO workspace_tasks(id,tenant_id,kind,object_hash,expires_at) VALUES($1,$2,'release_import',$3,now()+interval '1 minute')",params![task,c,&hash]).await.unwrap();
    let second = deletion(&db, &owner, b, None, "shared-assets").await;
    for _ in 0..2 {
        artifactory_control_plane::lifecycle::tick(&db.state)
            .await
            .unwrap();
    }
    assert_eq!(db.state.storage.get_log(&key).await.unwrap(), b"shared");
    let id = Uuid::parse_str(second["id"].as_str().unwrap()).unwrap();
    let phase: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT phase FROM lifecycle_operations WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(phase, "objects");
    sql::execute(
        &mut db.pool.clone(),
        "DELETE FROM workspace_tasks WHERE id=$1",
        params![task],
    )
    .await
    .unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE lifecycle_operations SET next_run_at=now() WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    finish(&db, &owner, &second).await;
    assert!(db.state.storage.get_log(&key).await.is_err());
    assert_eq!(
        request(&db, &owner, Some(c), "GET", "/tenant", json!({}))
            .await
            .0,
        StatusCode::OK
    );
    db.close().await;
}
