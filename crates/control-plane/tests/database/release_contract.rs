//! [INPUT]: 依赖隔离数据库、真实 HTTP 鉴权和 Release 领域。
//! [OUTPUT]: 验证版本元数据、并发唯一约束和独立构建归属。
//! [POS]: Release API 合同回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::fixture::build_input;
use super::*;
use serde_json::{Value, json};

async fn grant_release_token(db: &Database, project: Uuid) {
    let user = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO users(id,username,password_hash,role) VALUES($1,'release-api-user','unused','admin')",params![user]).await.unwrap();
    sql::execute(&mut db.pool.clone(),"INSERT INTO api_tokens(id,tenant_id,user_id,name,token_hash,project_ids,scopes,expires_at) VALUES($1,'00000000-0000-0000-0000-000000000001',$2,'release-api',$3,$4,'{read,build,release}',now()+interval '1 day')",params![Uuid::new_v4(),user,crypto::hash("release-api"),vec![project]]).await.unwrap();
}

fn release_input(version: &str, notes: &str) -> Value {
    json!({"version":version,"notes":notes})
}

async fn call(db: &Database, method: &str, path: String, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header("content-type", "application/json")
        .header("authorization", "Bearer release-api")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = api::router(db.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn duplicate_release_version_returns_conflict_and_preserves_original() {
    let db = Database::with_tenant().await;
    let (project, _, _) = db.fixture().await;
    grant_release_token(&db, project).await;
    let path = format!("/projects/{project}/releases");
    let original = release_input("2.3.4", "Original release");
    let (status, created) = call(&db, "POST", path.clone(), original).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let duplicate = release_input("2.3.4", "Overwrite attempt");
    let (status, error) = call(&db, "POST", path, duplicate).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    assert_eq!(error["error"], "resource already exists");
    assert_eq!(error["code"], "request_failed");
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM releases WHERE project_id=$1 AND version=$2",
        params![project, "2.3.4"],
    )
    .await
    .unwrap();
    assert_eq!(count, 1);
    let (status, persisted) = call(
        &db,
        "GET",
        format!("/releases/{}", created["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{persisted}");
    for field in ["id", "version", "notes"] {
        assert_eq!(
            persisted[field], created[field],
            "duplicate changed {field}"
        );
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn concurrent_duplicate_release_versions_return_one_success_and_one_conflict() {
    let db = Database::with_tenant().await;
    let (project, _, _) = db.fixture().await;
    grant_release_token(&db, project).await;
    let path = format!("/projects/{project}/releases");
    let (a, b) = tokio::join!(
        call(
            &db,
            "POST",
            path.clone(),
            release_input("3.4.5", "First creator")
        ),
        call(&db, "POST", path, release_input("3.4.5", "Second creator")),
    );
    let (created, rejected) = match (a.0, b.0) {
        (StatusCode::OK, StatusCode::CONFLICT) => (&a.1, &b.1),
        (StatusCode::CONFLICT, StatusCode::OK) => (&b.1, &a.1),
        _ => panic!("expected one success and one conflict; first={a:?}, second={b:?}"),
    };
    assert_eq!(rejected["error"], "resource already exists");
    assert_eq!(rejected["code"], "request_failed");
    let rows: Vec<Value> = sql::scalars(
        &mut db.pool.clone(),
        "SELECT to_jsonb(r) FROM releases r WHERE project_id=$1 AND version=$2",
        params![project, "3.4.5"],
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    for field in ["id", "version", "notes"] {
        assert_eq!(
            rows[0][field], created[field],
            "concurrent loser changed {field}"
        );
    }
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn release_http_contract_owns_builds_and_rejects_mutable_identity() {
    let db = Database::with_tenant().await;
    let (project, old_build, _) = db.fixture().await;
    let _ = db.release_build_path(project).await;
    grant_release_token(&db, project).await;
    let (status, created) = call(
        &db,
        "POST",
        format!("/projects/{project}/releases"),
        release_input("1.2.3-rc.1", "版本说明"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let release = created["id"].as_str().unwrap();
    let mut alternate = build_input();
    alternate["ref"] = json!("refs/tags/v1");
    alternate["target_requirements"] =
        json!([{"kind":"native","key":"mac","os":"darwin","arch":"arm64"}]);
    alternate["targets"] = json!(["mac"]);
    let (a, b) = tokio::join!(
        call(
            &db,
            "POST",
            format!("/releases/{release}/builds"),
            build_input()
        ),
        call(
            &db,
            "POST",
            format!("/releases/{release}/builds"),
            alternate
        )
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(b.0, StatusCode::OK, "{}", b.1);
    let (status, detail) = call(&db, "GET", format!("/releases/{release}"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["build_count"], 2);
    assert_eq!(detail["notes"], "版本说明");
    for field in [
        "requested_ref",
        "goal",
        "target_requirements",
        "spec_revision",
        "source_manifest_id",
    ] {
        assert!(detail.get(field).is_none(), "Release still exposes {field}");
    }
    let count:i64=sql::scalar(&mut db.pool.clone(),"SELECT count(*) FROM source_tasks s JOIN builds b ON b.id=s.build_id WHERE b.release_id=$1",params![release.parse::<Uuid>().unwrap()]).await.unwrap();
    assert_eq!(count, 2);
    let build = a.1["id"].as_str().unwrap();
    let (status, detail) = call(&db, "GET", format!("/builds/{build}"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["release_id"], release);
    assert_eq!(detail["release_version"], "1.2.3-rc.1");
    assert_eq!(detail["requested_ref"], "main");
    assert_eq!(detail["target_requirements"][0]["key"], "app");
    assert!(detail.get("spec_revision").is_none());
    for body in [
        json!({"version":"2.0.0"}),
        json!({"goal":"forbidden"}),
        json!({"ref":"other"}),
    ] {
        assert_eq!(
            call(&db, "PATCH", format!("/releases/{release}"), body)
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (status, updated) = call(
        &db,
        "PATCH",
        format!("/releases/{release}"),
        json!({"notes":"修正版本说明"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["notes"], "修正版本说明");
    let (_, unchanged) = call(&db, "GET", format!("/builds/{build}"), json!({})).await;
    assert_eq!(unchanged["goal"], detail["goal"]);
    assert_eq!(
        unchanged["target_requirements"],
        detail["target_requirements"]
    );
    assert_eq!(
        call(
            &db,
            "POST",
            format!("/releases/{release}/builds"),
            json!({"version":"9.0.0"})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &db,
            "POST",
            format!("/releases/{release}/publish"),
            json!({"build_id":old_build})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &db,
            "GET",
            format!("/releases/{release}/manifest"),
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &db,
            "POST",
            format!("/projects/{project}/builds"),
            json!({"ref":"main"})
        )
        .await
        .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        call(
            &db,
            "POST",
            format!("/builds/{build}/publish"),
            json!({"version":"9.0.0"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE api_tokens SET scopes='{read,build}' WHERE token_hash=$1",
        params![crypto::hash("release-api")],
    )
    .await
    .unwrap();
    assert_eq!(
        call(
            &db,
            "POST",
            format!("/releases/{release}/builds"),
            build_input()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &db,
            "PATCH",
            format!("/releases/{release}"),
            json!({"notes":"forbidden"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &db,
            "POST",
            format!("/releases/{release}/publish"),
            json!({"build_id":build})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    db.close().await;
}
