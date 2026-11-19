//! [INPUT]: 依赖业务状态、模型配置和隔离数据库。
//! [OUTPUT]: 验证注册、预算分类与规划终态。
//! [POS]: 数据库验收中的规划与身份基础回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn model_profile_queries_mask_keys_and_validate_planning_parameters() {
    let database = Database::with_tenant().await;
    let user = Uuid::new_v4();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,'admin','unused','admin')",
        params![user],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO sessions VALUES($1,$2,now()+interval '1 hour')",
        params![crypto::hash("ai-session"), user],
    )
    .await
    .unwrap();
    let router = api::router(database.state.clone());
    for (parameters, expected) in [
        (
            serde_json::json!({"max_tokens":-1}),
            StatusCode::BAD_REQUEST,
        ),
        (
            serde_json::json!({"thinking_level":"invalid"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            serde_json::json!({"headers":{"Authorization":"override"}}),
            StatusCode::BAD_REQUEST,
        ),
        (
            serde_json::json!({"max_tokens":1024,"requests_per_minute":3}),
            StatusCode::OK,
        ),
    ] {
        let body = serde_json::json!({"name":"real","api":"openai-completions","base_url":"https://model.example/v1","model":"test","api_key":"private-model-key","parameters":parameters,"is_default":true});
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/model-profiles")
                    .header(
                        "X-Artifactory-Tenant",
                        "00000000-0000-0000-0000-000000000001",
                    )
                    .header("cookie", "artifactory_session=ai-session")
                    .header("origin", "http://localhost:3000")
                    .header("x-artifactory-csrf", "1")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/model-profiles")
                .header(
                    "X-Artifactory-Tenant",
                    "00000000-0000-0000-0000-000000000001",
                )
                .header("cookie", "artifactory_session=ai-session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(!body.contains("private-model-key"));
    assert!(!body.contains("key_ciphertext"));
    assert!(body.contains("********"));
    let ciphertext: String = sql::scalar(
        &mut database.pool.clone(),
        "SELECT key_ciphertext FROM model_profiles",
        params![],
    )
    .await
    .unwrap();
    assert_ne!(ciphertext, "private-model-key");
    assert_eq!(
        crypto::open(&database.state.settings.master_key, &ciphertext).unwrap(),
        b"private-model-key"
    );
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn registration_creates_viewer_and_standard_session() {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    let database = Database::new().await;
    let router = api::router(database.state.clone());
    let password = "a-new-reader-password";
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .header("x-artifactory-csrf", "1")
                .header("origin", "http://localhost:3000")
                .body(Body::from(
                    serde_json::json!({"username":"  new-reader  ","password":password})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let set_cookie = response.headers()["set-cookie"].to_str().unwrap();
    assert!(set_cookie.contains("HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"));
    assert!(!set_cookie.contains("Secure"));
    let cookie = set_cookie.split(';').next().unwrap().to_owned();
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .unwrap();
    let user: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(user["username"], "new-reader");
    assert!(
        user.get("role").is_none(),
        "account identity has no global business role"
    );
    assert!(user.get("password_hash").is_none());
    let (encoded, role): (String, String) = sql::tuple(
        &mut database.pool.clone(),
        "SELECT password_hash,role FROM users WHERE username='new-reader'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(role, "viewer");
    assert_ne!(encoded, password);
    Argon2::default()
        .verify_password(password.as_bytes(), &PasswordHash::new(&encoded).unwrap())
        .unwrap();
    let sessions: i64 = sql::scalar(&mut database.pool.clone(), "SELECT count(*) FROM sessions WHERE expires_at>now()+interval '23 hours' AND expires_at<=now()+interval '24 hours'", params![])
    .await
    .unwrap();
    assert_eq!(sessions, 1);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let audit: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM audit_events WHERE action='user_registered'",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(audit, 1);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/logout")
                .header("cookie", &cookie)
                .header("x-artifactory-csrf", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .header("x-artifactory-csrf", "1")
                .body(Body::from(
                    serde_json::json!({"username":"new-reader","password":password}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("set-cookie"));
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn revision_budget_excludes_source_authorization_and_transport_failures() {
    let database = Database::new().await;
    let node = database.node().await;
    for failure in [
        "execution",
        "source",
        "authorization",
        "artifact_transport",
        "emissary",
    ] {
        let (project, build, job) = database.fixture().await;
        let snapshot = Uuid::new_v4();
        let run = Uuid::new_v4();
        let plan = Uuid::new_v4();
        database.source(project, snapshot).await;
        sql::execute(&mut database.pool.clone(), "INSERT INTO planning_runs(id,build_id,purpose,state,model_config) VALUES($1,$2,'initial','succeeded','{}')", params![run, build]).await.unwrap();
        sql::execute(&mut database.pool.clone(), "INSERT INTO plan_revisions(id,build_id,run_id,revision,source_manifest_id,contents,sha256) VALUES($1,$2,$3,0,$4,'{}','digest')", params![plan, build, run, snapshot]).await.unwrap();
        sql::execute(&mut database.pool.clone(), "UPDATE builds SET goal='Build CLI',model_config_snapshot='{}',current_plan_id=$2,source_manifest_id=$3,status='failed' WHERE id=$1", params![build, plan, snapshot]).await.unwrap();
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE jobs SET status='failed',plan_revision_id=$2 WHERE id=$1",
            params![job, plan],
        )
        .await
        .unwrap();
        sql::execute(&mut database.pool.clone(), "INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id,error,failure_kind) VALUES($1,$2,$3,'test',now(),'failed',$4,'controlled failure',$5)", params![Uuid::new_v4(), job, node, Uuid::new_v4(), failure]).await.unwrap();
        scheduler::queue_revisions(&database.state).await.unwrap();
        let (status, count): (String, i32) = sql::tuple(
            &mut database.pool.clone(),
            "SELECT status,revision_count FROM builds WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        if failure == "execution" {
            assert_eq!((status, count), ("replanning".into(), 1));
            // A later execution failure can consume the second revision, then
            // the same scheduler must leave the terminal failure untouched.
            sql::execute(
                &mut database.pool.clone(),
                "UPDATE planning_runs SET state='succeeded' WHERE build_id=$1",
                params![build],
            )
            .await
            .unwrap();
            sql::execute(
                &mut database.pool.clone(),
                "UPDATE builds SET status='failed' WHERE id=$1",
                params![build],
            )
            .await
            .unwrap();
            scheduler::queue_revisions(&database.state).await.unwrap();
            sql::execute(
                &mut database.pool.clone(),
                "UPDATE planning_runs SET state='succeeded' WHERE build_id=$1",
                params![build],
            )
            .await
            .unwrap();
            sql::execute(
                &mut database.pool.clone(),
                "UPDATE builds SET status='failed' WHERE id=$1",
                params![build],
            )
            .await
            .unwrap();
            scheduler::queue_revisions(&database.state).await.unwrap();
            let count: i32 = sql::scalar(
                &mut database.pool.clone(),
                "SELECT revision_count FROM builds WHERE id=$1",
                params![build],
            )
            .await
            .unwrap();
            assert_eq!(count, 2);
        } else {
            assert_eq!((status, count), ("failed".into(), 0));
        }
    }
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn build_summary_cannot_overwrite_retry_or_planning_committed_while_waiting() {
    let database = Database::with_tenant().await;
    for next in ["queued", "planning"] {
        let (_, build, job) = database.fixture().await;
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE jobs SET status='failed' WHERE id=$1",
            params![job],
        )
        .await
        .unwrap();
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE builds SET status='running',finished_at=NULL WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();

        let mut writer_db = database.pool.clone();
        let mut writer = writer_db.transaction().await.unwrap();
        let backend: i32 = sql::scalar(&mut writer, "SELECT pg_backend_pid()", params![])
            .await
            .unwrap();
        sql::execute(
            &mut writer,
            "SELECT id FROM builds WHERE id=$1 FOR UPDATE",
            params![build],
        )
        .await
        .unwrap();
        let store = database.state.store.clone();
        let summary = tokio::spawn(async move { store.refresh_builds().await });
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                let blocked: bool = sql::scalar(&mut database.pool.clone(), "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) AND query LIKE 'WITH summaries%')", params![backend]).await.unwrap();
                if blocked { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        // The summary already read the old failed jobs. Commit the same atomic
        // job/build transition used by a retry while it waits for the build.
        sql::execute(
            &mut writer,
            "UPDATE jobs SET status='queued' WHERE id=$1",
            params![job],
        )
        .await
        .unwrap();
        sql::execute(
            &mut writer,
            "UPDATE builds SET status=$2,finished_at=NULL WHERE id=$1",
            params![build, next],
        )
        .await
        .unwrap();
        writer.commit().await.unwrap();
        summary.await.unwrap().unwrap();
        let current: (String, Option<chrono::DateTime<chrono::Utc>>) = sql::tuple(
            &mut database.pool.clone(),
            "SELECT status,finished_at FROM builds WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        assert_eq!(
            current,
            (next.into(), None),
            "a stale summary must not replace a newly committed state"
        );
        database.state.store.refresh_builds().await.unwrap();
        let status: String = sql::scalar(
            &mut database.pool.clone(),
            "SELECT status FROM builds WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        assert_eq!(status, next);
    }
    database.close().await;
}
