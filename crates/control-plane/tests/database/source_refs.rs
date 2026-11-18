//! [INPUT]: 隔离数据库、用户主目录及认证 HTTPS、SSH 夹具。
//! [OUTPUT]: 验证引用查询权限、项目凭据和主机信任。
//! [POS]: 远端引用查询的 HTTP 与真实 Git 集成回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_database::{params, sql};
use serde_json::Value;

async fn read_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn token(database: &Database, role: &str, projects: &[Uuid], scopes: &[&str]) -> String {
    let user = Uuid::new_v4();
    let secret = Uuid::new_v4().to_string();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,'unused',$3)",
        params![user, user.to_string(), role],
    )
    .await
    .unwrap();
    sql::execute(&mut database.pool.clone(), "INSERT INTO api_tokens(tenant_id,id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES('00000000-0000-0000-0000-000000000001',$1,$2,'refs-test',$3,$4,$5,now()+interval '1 hour')", params![Uuid::new_v4(), user, crypto::hash(&secret), projects, scopes]).await.unwrap();
    secret
}

async fn request(
    database: &Database,
    project: Uuid,
    secret: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder().uri(format!("/api/v1/projects/{project}/refs"));
    if let Some(secret) = secret {
        builder = builder.header("authorization", format!("Bearer {secret}"));
    }
    api::router(database.state.clone())
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn references_enforce_read_scope_project_scope_and_not_found() {
    let database = Database::with_tenant().await;
    let project = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(), "INSERT INTO projects(tenant_id,id,name,repository_url) VALUES('00000000-0000-0000-0000-000000000001',$1,'refs','https://127.0.0.1:1/repo.git')", params![project]).await.unwrap();
    assert_eq!(
        request(&database, project, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    for (projects, scopes) in [
        (vec![Uuid::new_v4()], vec!["read"]),
        (vec![project], vec!["build"]),
    ] {
        let secret = token(&database, "operator", &projects, &scopes).await;
        assert_eq!(
            request(&database, project, Some(&secret)).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    let missing = Uuid::new_v4();
    let secret = token(&database, "viewer", &[project, missing], &["read"]).await;
    assert_eq!(
        request(&database, missing, Some(&secret)).await.status(),
        StatusCode::NOT_FOUND
    );
    let router = api::router(database.state.clone());
    let pending = tokio::spawn(async move {
        router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{project}/refs"))
                    .header("authorization", format!("Bearer {secret}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    });
    tokio::time::timeout(std::time::Duration::from_secs(5),async { loop {
        let changed=sql::execute(&mut database.pool.clone(),"UPDATE source_tasks SET status='failed',error='private upstream address' WHERE purpose='refs' AND project_id=$1",params![project]).await.unwrap();
        if changed>0 {break;} tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }}).await.unwrap();
    let response = pending.await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = read_body(response).await.to_string();
    assert!(body.contains("check access"));
    assert!(!body.contains("127.0.0.1"));
    let builds: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM builds",
        params![],
    )
    .await
    .unwrap();
    assert_eq!(builds, 0);
    database.close().await;
}
