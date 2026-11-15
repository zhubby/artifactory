//! Global profile and credential changes must not depend on tenant authority.
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn profile_is_persistent_account_scoped_and_does_not_change_login_identity() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "profile-owner", "viewer").await;
    let (status, profile) = request(
        &db,
        &cookie,
        None,
        "PATCH",
        "/auth/me",
        json!({"display_name":"  林工  "}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{profile}");
    assert_eq!(profile["display_name"], "林工");
    assert_eq!(profile["username"], "profile-owner");
    assert_eq!(
        request(&db, &cookie, None, "GET", "/auth/me", json!({}))
            .await
            .1,
        profile
    );
    assert_eq!(
        request(
            &db,
            &cookie,
            None,
            "PATCH",
            "/auth/me",
            json!({"display_name":" "})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &db,
            &cookie,
            None,
            "PATCH",
            "/auth/me",
            json!({"display_name":"Name","username":"other"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let response = api::router(db.state.clone())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/auth/me")
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"display_name":"CSRF"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn password_change_rejects_wrong_password_and_revokes_every_session() {
    let db = Database::new().await;
    let (id, cookie) = account(&db, "security-owner", "viewer").await;
    let previous = artifactory_control_plane::auth::password_hash("previous-password").unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE users SET password_hash=$2 WHERE id=$1",
        params![id, &previous],
    )
    .await
    .unwrap();
    db.state
        .store
        .create_session(&crypto::hash("second-session"), id)
        .await
        .unwrap();
    let invalid =
        json!({"current_password":"wrong-password","new_password":"replacement-password"});
    assert_eq!(
        request(&db, &cookie, None, "POST", "/auth/password", invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&db, &cookie, None, "GET", "/auth/me", json!({}))
            .await
            .0,
        StatusCode::OK
    );
    let (status, result) = request(
        &db,
        &cookie,
        None,
        "POST",
        "/auth/password",
        json!({"current_password":"previous-password","new_password":"replacement-password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    for session in [&cookie, "artifactory_session=second-session"] {
        assert_eq!(
            request(&db, session, None, "GET", "/auth/me", json!({}))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(
            &db,
            "",
            None,
            "POST",
            "/auth/login",
            json!({"username":"security-owner","password":"previous-password"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &db,
            "",
            None,
            "POST",
            "/auth/login",
            json!({"username":"security-owner","password":"replacement-password"})
        )
        .await
        .0,
        StatusCode::OK
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn credential_rotation_serializes_with_logins_and_security_attempts_are_account_scoped() {
    let db = Database::new().await;
    let (id, cookie) = account(&db, "race-owner", "viewer").await;
    let (other, _) = account(&db, "other-account", "viewer").await;
    let previous = artifactory_control_plane::auth::password_hash("previous-password").unwrap();
    let replacement =
        artifactory_control_plane::auth::password_hash("replacement-password").unwrap();
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE users SET password_hash=$2 WHERE id=$1",
        params![id, &previous],
    )
    .await
    .unwrap();
    let login = |token: &'static str| db.state.store.create_verified_session(token, id, &previous);
    let (a, b, c, rotated) = tokio::join!(
        login("old-login-a"),
        login("old-login-b"),
        login("old-login-c"),
        db.state.store.replace_password(id, &previous, &replacement)
    );
    rotated.unwrap();
    for result in [a, b, c] {
        assert!(
            result.is_ok()
                || matches!(
                    result,
                    Err(artifactory_control_plane::error::ApiError::Unauthorized)
                )
        );
    }
    assert!(
        db.state
            .store
            .create_verified_session("late-old-login", id, &previous)
            .await
            .is_err()
    );
    let count: i64 = sql::scalar(
        &mut db.pool.clone(),
        "SELECT count(*) FROM sessions WHERE user_id=$1",
        params![id],
    )
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "every session admitted before rotation is revoked"
    );
    assert!(
        db.state
            .store
            .replace_password(id, &previous, &previous)
            .await
            .is_err(),
        "a concurrently verified old password cannot rotate credentials again"
    );
    db.state
        .store
        .create_verified_session(&crypto::hash("new-login"), id, &replacement)
        .await
        .unwrap();
    assert_eq!(
        request(&db, &cookie, None, "GET", "/auth/me", json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for _ in 0..10 {
        db.state.store.reserve_security_attempt(id).await.unwrap();
    }
    let response = request(
        &db,
        "artifactory_session=new-login",
        None,
        "POST",
        "/auth/password",
        json!({"current_password":"replacement-password","new_password":"another-password"}),
    )
    .await;
    assert_eq!(response.0, StatusCode::TOO_MANY_REQUESTS, "{}", response.1);
    db.state
        .store
        .reserve_security_attempt(other)
        .await
        .unwrap();
    assert_eq!(
        request(
            &db,
            "artifactory_session=new-login",
            None,
            "GET",
            "/auth/me",
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn account_endpoints_reject_tenant_api_tokens() {
    let db = Database::new().await;
    let (_, cookie) = account(&db, "token-owner", "viewer").await;
    let tenant = tenant(&db, &cookie, "Tokens").await;
    let project = project(&db, &cookie, tenant, "Desktop").await;
    let (_, token) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        "/tokens",
        json!({"name":"CI","project_ids":[project],"scopes":["read"],"days":1}),
    )
    .await;
    for (method, path, body) in [
        ("GET", "/auth/me", json!({})),
        ("PATCH", "/auth/me", json!({"display_name":"CI"})),
        (
            "POST",
            "/auth/password",
            json!({"current_password":"anything","new_password":"another-password"}),
        ),
    ] {
        let response = api::router(db.state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/v1{path}"))
                    .header(
                        "authorization",
                        format!("Bearer {}", token["token"].as_str().unwrap()),
                    )
                    .header("content-type", "application/json")
                    .header("x-artifactory-csrf", "1")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    db.close().await;
}
