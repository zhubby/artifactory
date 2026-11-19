//! [INPUT]: 依赖隔离数据库与镜像执行存储。
//! [OUTPUT]: 验证无设备执行、候选确认和 Release 外仓分发。
//! [POS]: 镜像执行状态机的数据库集成验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_image_protocol::{MANIFEST_MEDIA_TYPE, PlatformResult, Report, index, sha256};
use serde_json::json;
#[path = "image_completion.rs"]
mod completion;
#[path = "image_regressions.rs"]
mod regressions;
async fn image(db: &Database) -> (Uuid, Uuid, Uuid, Uuid) {
    let (project, build, job) = db.fixture().await;
    let snapshot = sql::scalar(
        &mut db.pool.clone(),
        "SELECT source_manifest_id FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(), "UPDATE releases SET target_requirements='[{\"kind\":\"image\",\"key\":\"app\",\"platforms\":[\"linux/amd64\",\"linux/arm64\"]}]' WHERE id=(SELECT release_id FROM builds WHERE id=$1)", params![build]).await.unwrap();
    let target = json!({"executor":"buildkit","os":"linux","arch":"amd64","timeout_seconds":600,"image":{"name":"app","platforms":["linux/amd64","linux/arm64"]}});
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE jobs SET target_config=$2 WHERE id=$1",
        params![job, target],
    )
    .await
    .unwrap();
    (project, build, job, snapshot)
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_build_has_no_device_attempt_and_fences_canceled_source() {
    let db = Database::with_tenant().await;
    let (_, build, job, _manifest) = image(&db).await;
    let id = Uuid::new_v4();
    let store = &db.state.store;
    assert!(
        store
            .claim_image(job, id, &crypto::hash("build"), &crypto::hash("publish"))
            .await
            .unwrap()
    );
    assert!(
        !store
            .claim_image(job, Uuid::new_v4(), "other", "other")
            .await
            .unwrap()
    );
    store.save_image_task(id, "sealed").await.unwrap();
    assert!(store.image_live(id).await.unwrap());
    assert_eq!(store.image_token(id, "build").await.unwrap(), Some(false));
    assert_eq!(store.image_token(id, "publish").await.unwrap(), Some(true));
    assert_eq!(store.image_token(id, "wrong").await.unwrap(), None);
    let heartbeat = Report::Heartbeat {
        platform: "linux/amd64".into(),
        pod_uid: Uuid::new_v4().to_string(),
    };
    store
        .image_report(id, false, &heartbeat, None)
        .await
        .unwrap();
    assert!(
        store
            .image_report(
                id,
                false,
                &Report::Heartbeat {
                    platform: "linux/amd64".into(),
                    pod_uid: Uuid::new_v4().to_string()
                },
                None
            )
            .await
            .is_err()
    );
    assert!(
        store
            .image_report(id, true, &heartbeat, None)
            .await
            .is_err()
    );
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM attempts WHERE job_id=$1",
            params![job]
        )
        .await
        .unwrap(),
        0
    );
    store.cancel_build(build).await.unwrap();
    assert!(!store.image_live(id).await.unwrap());
    assert!(!store.image_live(id).await.unwrap());
    assert!(
        store
            .image_report(id, false, &heartbeat, None)
            .await
            .is_err()
    );
    store.finish_image(id, "canceled").await.unwrap();
    store.refresh_builds().await.unwrap();
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM builds WHERE id=$1",
            params![build]
        )
        .await
        .unwrap(),
        "canceled"
    );
    db.close().await;
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_publication_is_complete_and_idempotent() {
    let db = Database::with_tenant().await;
    let (project, build, job, _) = image(&db).await;
    let id = Uuid::new_v4();
    let store = &db.state.store;
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    let expected: Vec<String> = vec!["linux/amd64".into(), "linux/arm64".into()];
    let results: Vec<_> = expected
        .iter()
        .map(|p: &String| PlatformResult {
            platform: p.clone(),
            digest: sha256(p.as_bytes()),
            size: 42,
            media_type: MANIFEST_MEDIA_TYPE.into(),
        })
        .collect();
    for result in &results {
        store
            .image_report(
                id,
                false,
                &Report::Platform {
                    result: result.clone(),
                },
                None,
            )
            .await
            .unwrap();
    }
    let digest = sha256(&index(&expected, &results).unwrap());
    let published = Report::Published {
        digest: digest.clone(),
    };
    assert!(
        store
            .image_report(id, false, &published, None)
            .await
            .is_err()
    );
    store
        .image_report(id, true, &published, None)
        .await
        .unwrap();
    confirm_image(store, id).await;
    store
        .image_report(id, true, &published, None)
        .await
        .unwrap();
    store
        .image_report(id, true, &published, None)
        .await
        .unwrap();
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM image_artifacts WHERE execution_id=$1",
            params![id]
        )
        .await
        .unwrap(),
        1
    );
    assert!(
        store
            .image_report(
                id,
                true,
                &Report::Published {
                    digest: sha256(b"changed")
                },
                None
            )
            .await
            .is_err()
    );
    store.refresh_builds().await.unwrap();
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM builds WHERE id=$1",
            params![build]
        )
        .await
        .unwrap(),
        "succeeded"
    );
    let tenant = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let owner: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT owner_id FROM tenants WHERE id=$1",
        params![tenant],
    )
    .await
    .unwrap();
    let mut auth = artifactory_control_plane::auth::Auth::scoped(owner, "admin", vec![], vec![]);
    auth.token_projects = None;
    store.authorize_tenant(&mut auth, tenant).await.unwrap();
    let visible = auth.visible_projects().unwrap();
    assert!(visible.contains(&project));
    let catalog = store.for_auth(&auth);
    let images = catalog.images(&visible, Some(project)).await.unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0]["name"], "app");
    assert_eq!(images[0]["build_id"], build.to_string());
    assert!(images[0]["build_number"].is_number());
    assert!(images[0]["project_slug"].is_string());
    assert!(catalog.images(&[], Some(project)).await.unwrap().is_empty());
    assert!(
        catalog
            .images(&visible, Some(Uuid::new_v4()))
            .await
            .unwrap()
            .is_empty()
    );
    db.close().await;
}

fn publication_store(
    db: &Database,
    project: Uuid,
) -> artifactory_control_plane::store::ControlStore {
    let mut auth = artifactory_control_plane::auth::Auth::scoped(
        Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap(),
        "admin",
        vec![project],
        vec!["read".into(), "release".into()],
    );
    auth.tenant_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    db.state.store.for_auth(&auth)
}
async fn platform_results(
    store: &artifactory_control_plane::store::ControlStore,
    id: Uuid,
) -> String {
    let expected: Vec<String> = vec!["linux/amd64".into(), "linux/arm64".into()];
    let mut results = Vec::new();
    for platform in &expected {
        let result = PlatformResult {
            platform: platform.clone(),
            digest: sha256(platform.as_bytes()),
            size: 42,
            media_type: MANIFEST_MEDIA_TYPE.into(),
        };
        store
            .image_report(
                id,
                false,
                &Report::Platform {
                    result: result.clone(),
                },
                None,
            )
            .await
            .unwrap();
        results.push(result);
    }
    sha256(&index(&expected, &results).unwrap())
}

// 存储夹具代表已通过 CR 校验的 Control；完整校验由 completion 模块覆盖。
async fn confirm_image(store: &artifactory_control_plane::store::ControlStore, id: Uuid) {
    let uid = format!("fixture-{id}");
    store.bind_image_build(id, &uid).await.unwrap();
    store.complete_image(id, &uid).await.unwrap();
}

// 外仓测试只模拟已完成的正式推广；真实仓库推广由 OCI 与集群验收覆盖。
async fn freeze_published_image(db: &Database, build: Uuid, execution: Uuid) -> String {
    let repository: String = sql::scalar(&mut db.pool.clone(), "SELECT b.tenant_id::text||'/'||b.project_id::text||'/images/app' FROM builds b WHERE b.id=$1",params![build]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO release_deliveries(release_id,build_id,source_manifest_id,spec_revision,manifest) SELECT release_id,id,source_manifest_id,spec_revision,'{}' FROM builds WHERE id=$1",params![build]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO release_images(release_id,image_id,repository,version,root_digest,manifest_digests) SELECT b.release_id,i.id,$3,b.release_version,i.digest,jsonb_build_array(i.digest) FROM image_artifacts i JOIN jobs j ON j.id=i.job_id JOIN builds b ON b.id=j.build_id WHERE b.id=$1 AND i.execution_id=$2",params![build,execution,&repository]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"UPDATE releases SET status='published',published_at=now() WHERE id=(SELECT release_id FROM builds WHERE id=$1)",params![build]).await.unwrap();
    repository
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn unpublished_candidates_cannot_distribute_and_external_failure_preserves_result() {
    let db = Database::with_tenant().await;
    let (project, build, job, _) = image(&db).await;
    let store = publication_store(&db, project);
    let id = Uuid::new_v4();
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    assert_eq!(
        store
            .image_execution(id)
            .await
            .unwrap()
            .task_timeout_seconds,
        1200
    );
    let digest = platform_results(&store, id).await;
    assert!(
        store
            .request_image_publication(
                id,
                Uuid::new_v4(),
                None,
                Some("foreign-tenant/foreign-project/images/app"),
                &[],
                "forged"
            )
            .await
            .is_err()
    );
    assert!(
        store
            .request_image_publication(id, Uuid::new_v4(), None, None, &[], "retry")
            .await
            .is_err()
    );
    assert!(
        !store
            .claim_image(job, Uuid::new_v4(), "other", "other")
            .await
            .unwrap()
    );
    store.save_image_task(id, "sealed-publisher").await.unwrap();
    store
        .image_report(
            id,
            true,
            &Report::Published {
                digest: digest.clone(),
            },
            None,
        )
        .await
        .unwrap();
    confirm_image(&store, id).await;
    assert_eq!(store.image_execution(id).await.unwrap().status, "succeeded");
    let credential = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO image_credentials(id,tenant_id,project_id,name,kind,envelope,registry_origin) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,'destination','registry','sealed','https://external.example.com:5443')",params![credential,project]).await.unwrap();
    assert!(
        store
            .request_image_publication(
                id,
                Uuid::new_v4(),
                Some(credential),
                Some("external/app"),
                &[],
                "unpublished"
            )
            .await
            .is_err()
    );
    let formal_repository = freeze_published_image(&db, build, id).await;
    assert!(
        store
            .request_image_publication(
                id,
                Uuid::new_v4(),
                Some(credential),
                Some("external/app"),
                &["other".into()],
                "override"
            )
            .await
            .is_err()
    );
    let external = Uuid::new_v4();
    store
        .request_image_publication(
            id,
            external,
            Some(credential),
            Some("external/app"),
            &[],
            "external",
        )
        .await
        .unwrap();
    let execution = store.image_execution(external).await.unwrap();
    assert_eq!(execution.root_digest.as_ref(), Some(&digest));
    assert_eq!(execution.candidate_repository, formal_repository);
    let version: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT release_version FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert_eq!(execution.tags, vec![version]);
    store
        .save_image_task(external, "sealed-external")
        .await
        .unwrap();
    store
        .image_report(
            external,
            true,
            &Report::Failed {
                message: "failure".into(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(store.image_execution(id).await.unwrap().status, "succeeded");
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM jobs WHERE id=$1",
            params![job]
        )
        .await
        .unwrap(),
        "succeeded"
    );
    let external_retry = Uuid::new_v4();
    store
        .request_image_publication(
            id,
            external_retry,
            Some(credential),
            Some("external/app"),
            &[],
            "retry-external",
        )
        .await
        .unwrap();
    store
        .save_image_task(external_retry, "sealed-external-retry")
        .await
        .unwrap();
    store
        .image_report(
            external_retry,
            true,
            &Report::Published {
                digest: digest.clone(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        store.image_execution(external_retry).await.unwrap().status,
        "running"
    );
    confirm_image(&store, external_retry).await;
    assert_eq!(
        store.image_execution(external_retry).await.unwrap().status,
        "succeeded"
    );
    assert_eq!(
        store
            .image_execution(id)
            .await
            .unwrap()
            .root_digest
            .as_ref(),
        Some(&digest)
    );
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM image_artifacts WHERE execution_id=$1",
            params![id]
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM attempts WHERE job_id=$1",
            params![job]
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sql::scalar::<i64>(
            &mut db.pool.clone(),
            "SELECT count(*) FROM image_executions WHERE job_id=$1 AND operation='build'",
            params![job]
        )
        .await
        .unwrap(),
        1
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn publication_rejects_other_project_and_revoked_credentials() {
    let db = Database::with_tenant().await;
    let (project, _, job, _) = image(&db).await;
    let (other, _, _) = db.fixture().await;
    let store = publication_store(&db, project);
    let id = Uuid::new_v4();
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    let digest = platform_results(&store, id).await;
    store
        .image_report(id, true, &Report::Published { digest }, None)
        .await
        .unwrap();
    confirm_image(&store, id).await;
    for (credential_project, enabled) in [(other, true), (project, false)] {
        let credential = Uuid::new_v4();
        sql::execute(&mut db.pool.clone(),"INSERT INTO image_credentials(id,tenant_id,project_id,name,kind,envelope,enabled,registry_origin) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,$3,'registry','sealed',$4,'https://external.example.com:5443')",params![credential,credential_project,credential.to_string(),enabled]).await.unwrap();
        assert!(
            store
                .image_credential(project, credential, "registry")
                .await
                .is_err()
        );
        assert!(
            store
                .request_image_publication(
                    id,
                    Uuid::new_v4(),
                    Some(credential),
                    Some("external/app"),
                    &["v1".into()],
                    "external"
                )
                .await
                .is_err()
        );
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn deadline_revokes_source_and_all_worker_reports() {
    let db = Database::with_tenant().await;
    let (_, _, job, _manifest) = image(&db).await;
    let store = &db.state.store;
    let id = Uuid::new_v4();
    store
        .claim_image(job, id, "build", "publish")
        .await
        .unwrap();
    store.save_image_task(id, "sealed").await.unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE image_executions SET deadline_at=now()-interval '1 second' WHERE id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert!(!store.image_live(id).await.unwrap());
    assert!(!store.image_live(id).await.unwrap());
    assert!(
        store
            .image_report(
                id,
                false,
                &Report::Heartbeat {
                    platform: "linux/amd64".into(),
                    pod_uid: Uuid::new_v4().to_string()
                },
                None
            )
            .await
            .is_err()
    );
    assert!(store.save_image_task(id, "new-envelope").await.is_err());
    store.finish_image(id, "failed").await.unwrap();
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_dispatch_rotates_more_than_one_batch_and_prioritizes_cancellation() {
    let db = Database::with_tenant().await;
    let (_, _, seed, _) = image(&db).await;
    let store = &db.state.store;
    let mut executions = Vec::new();
    for n in 0..40 {
        let job = if n == 0 {
            seed
        } else {
            let job = Uuid::new_v4();
            sql::execute(&mut db.pool.clone(),"INSERT INTO jobs(id,build_id,target,target_config) SELECT $1,build_id,$2,target_config FROM jobs WHERE id=$3",params![job,format!("image-{n}"),seed]).await.unwrap();
            job
        };
        let id = Uuid::new_v4();
        assert!(
            store
                .claim_image(job, id, "build", "publish")
                .await
                .unwrap()
        );
        store.save_image_task(id, "sealed").await.unwrap();
        executions.push(id);
    }
    let first = store.image_pending().await.unwrap();
    assert_eq!(first.len(), 32);
    let second = store.image_pending().await.unwrap();
    let visited: std::collections::BTreeSet<_> =
        first.iter().chain(second.iter()).copied().collect();
    assert_eq!(visited.len(), 40);
    let canceled = executions[39];
    sql::execute(&mut db.pool.clone(),"UPDATE image_executions SET cancel_requested=true,last_dispatched_at=clock_timestamp() WHERE id=$1",params![canceled]).await.unwrap();
    assert!(store.image_pending().await.unwrap().contains(&canceled));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL and unset ARTIFACTORY_IMAGE_NAMESPACE"]
async fn disabled_namespace_rejects_image_retry_but_preserves_native_retry_and_admission_snapshot()
{
    assert!(
        !artifactory_control_plane::images::enabled(),
        "run this test with ARTIFACTORY_IMAGE_NAMESPACE unset"
    );
    let db = Database::with_tenant().await;
    let (image_project, image_build, image_job, _) = image(&db).await;
    let (native_project, native_build, native_job) = db.fixture().await;
    for build in [image_build, native_build] {
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET status='failed' WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE jobs SET status='failed' WHERE build_id=$1",
            params![build],
        )
        .await
        .unwrap();
    }
    let tenant = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let owner: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT owner_id FROM tenants WHERE id=$1",
        params![tenant],
    )
    .await
    .unwrap();
    let mut auth = artifactory_control_plane::auth::Auth::scoped(
        owner,
        "admin",
        vec![image_project, native_project],
        vec!["read".into(), "build".into(), "release".into()],
    );
    auth.tenant_id = tenant;
    let error = db
        .state
        .store
        .retry_job(image_job, &auth)
        .await
        .unwrap_err();
    assert!(
        matches!(error,artifactory_control_plane::error::ApiError::Conflict(ref message) if message==artifactory_control_plane::images::DISABLED_MESSAGE)
    );
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM jobs WHERE id=$1",
            params![image_job]
        )
        .await
        .unwrap(),
        "failed"
    );
    db.state.store.retry_job(native_job, &auth).await.unwrap();
    assert_eq!(
        sql::scalar::<String>(
            &mut db.pool.clone(),
            "SELECT status FROM jobs WHERE id=$1",
            params![native_job]
        )
        .await
        .unwrap(),
        "queued"
    );
    let input = serde_json::from_value(json!({"parameters":{}})).unwrap();
    let release: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT release_id FROM builds WHERE id=$1",
        params![native_build],
    )
    .await
    .unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO model_profiles(id,tenant_id,name,api,base_url,model,key_ciphertext,is_default) VALUES($1,$2,'test','openai-responses','https://example.com','test','unused',true)",params![Uuid::new_v4(),tenant]).await.unwrap();
    let build = db
        .state
        .store
        .for_auth(&auth)
        .create_release_build(release, owner, &input)
        .await
        .unwrap();
    let context: serde_json::Value = sql::scalar(
        &mut db.pool.clone(),
        "SELECT planning_context FROM builds WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    assert_eq!(context["image_builds_enabled"], false);
    db.close().await;
}
