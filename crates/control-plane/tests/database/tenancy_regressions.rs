//! Regressions for authorization normalization and bounded transaction connections.
use super::*;

fn encoded_id(id: Uuid) -> String {
    let value = id.to_string();
    format!("%{:02X}{}", value.as_bytes()[0], &value[1..])
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn encoded_resource_ids_keep_tenant_and_project_authorization() {
    let db = Database::new().await;
    let (_, alice) = account(&db, "alice", "viewer").await;
    let (_, bob) = account(&db, "bob", "viewer").await;
    let (_, reader) = account(&db, "reader", "viewer").await;
    let a = tenant(&db, &alice, "A").await;
    let b = tenant(&db, &bob, "B").await;
    let own_node = device(&db, a).await;
    let other_node = device(&db, b).await;
    assert_eq!(
        request(
            &db,
            &alice,
            Some(a),
            "POST",
            "/tenant/members",
            json!({"username":"reader","role":"member"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let profile = json!({"name":"Protected model","api":"openai-responses",
        "base_url":"https://example.com/v1","model":"protected","api_key":"dummy-review-key"});
    let (status, value) = request(
        &db,
        &bob,
        Some(b),
        "POST",
        "/model-profiles",
        profile.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let model = Uuid::parse_str(value["id"].as_str().unwrap()).unwrap();
    let mut checks = Vec::new();
    for node in [other_node.to_string(), encoded_id(other_node)] {
        let path = format!("/nodes/{node}/metadata");
        checks.push((
            path.clone(),
            request(&db, &alice, Some(a), "GET", &path, json!({}))
                .await
                .0,
            StatusCode::NOT_FOUND,
        ));
    }
    for node in [own_node.to_string(), encoded_id(own_node)] {
        let path = format!("/nodes/{node}/metadata");
        checks.push((
            path.clone(),
            request(&db, &reader, Some(a), "GET", &path, json!({}))
                .await
                .0,
            StatusCode::FORBIDDEN,
        ));
    }
    for id in [model.to_string(), encoded_id(model)] {
        let path = format!("/model-profiles/{id}");
        checks.push((
            path.clone(),
            request(&db, &alice, Some(a), "PATCH", &path, profile.clone())
                .await
                .0,
            StatusCode::NOT_FOUND,
        ));
    }
    let own_path = format!("/nodes/{}/metadata", encoded_id(own_node));
    checks.push((
        own_path.clone(),
        request(&db, &alice, Some(a), "GET", &own_path, json!({}))
            .await
            .0,
        StatusCode::OK,
    ));
    let own_model = format!("/model-profiles/{}", encoded_id(model));
    checks.push((
        own_model.clone(),
        request(&db, &bob, Some(b), "PATCH", &own_model, profile)
            .await
            .0,
        StatusCode::OK,
    ));
    db.close().await;
    for (path, actual, expected) in checks {
        assert_eq!(
            actual, expected,
            "authorization must use the decoded ID: {path}"
        );
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn project_administrators_cannot_redirect_an_existing_credential() {
    let db = Database::new().await;
    let (_, owner) = account(&db, "owner", "viewer").await;
    let (member_id, member) = account(&db, "member", "viewer").await;
    let tenant = tenant(&db, &owner, "Team").await;
    assert_eq!(
        request(
            &db,
            &owner,
            Some(tenant),
            "POST",
            "/tenant/members",
            json!({"username":"member","role":"member"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, value) = request(&db, &owner, Some(tenant), "POST", "/credentials",
        json!({"name":"Root HTTPS","kind":"https","username":"review","secret":"dummy-review-value"})).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let credential = value["id"].clone();
    let input = json!({"name":"Protected project","repository_url":"https://example.com/approved.git",
        "credential_id":credential});
    let (status, value) = request(
        &db,
        &owner,
        Some(tenant),
        "POST",
        "/projects",
        input.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let id = value["id"].as_str().unwrap();
    assert_eq!(
        request(
            &db,
            &owner,
            Some(tenant),
            "PUT",
            &format!("/projects/{id}/members/{member_id}"),
            json!({"role":"admin"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let path = format!("/projects/{id}");
    let mut renamed = input.clone();
    renamed["name"] = json!("Renamed project");
    let allowed = request(&db, &member, Some(tenant), "PATCH", &path, renamed.clone())
        .await
        .0;
    let mut redirected = renamed;
    redirected["repository_url"] = json!("https://review-destination.invalid/repo.git");
    let denied = request(
        &db,
        &member,
        Some(tenant),
        "PATCH",
        &path,
        redirected.clone(),
    )
    .await
    .0;
    let (_, unchanged) = request(&db, &owner, Some(tenant), "GET", &path, json!({})).await;
    let owner_update = request(&db, &owner, Some(tenant), "PATCH", &path, redirected)
        .await
        .0;
    let public = project(&db, &owner, tenant, "No credential").await;
    assert_eq!(
        request(
            &db,
            &owner,
            Some(tenant),
            "PUT",
            &format!("/projects/{public}/members/{member_id}"),
            json!({"role":"admin"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let public_update = request(
        &db,
        &member,
        Some(tenant),
        "PATCH",
        &format!("/projects/{public}"),
        json!({"name":"No credential","repository_url":"https://example.com/new-public.git"}),
    )
    .await
    .0;
    db.close().await;
    assert_eq!(
        allowed,
        StatusCode::OK,
        "project configuration remains editable"
    );
    assert_eq!(
        denied,
        StatusCode::FORBIDDEN,
        "credential destination requires tenant authorization"
    );
    assert_eq!(unchanged["repository_url"], input["repository_url"]);
    assert_eq!(unchanged["credential_id"], credential);
    assert_eq!(owner_update, StatusCode::OK);
    assert_eq!(public_update, StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn project_resource_validation_reuses_the_transaction_connection() {
    use artifactory_control_plane::{
        auth::Auth,
        store::{ControlStore, ProjectInput},
    };
    use std::time::Duration;

    let db = Database::new().await;
    let (user, cookie) = account(&db, "owner", "viewer").await;
    let tenant = tenant(&db, &cookie, "Connection budget").await;
    let (_, credential) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        "/credentials",
        json!({"name":"Source credential","kind":"https","secret":"dummy-review-value"}),
    )
    .await;
    let (_, model) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        "/model-profiles",
        json!({"name":"Model","api":"openai-responses","base_url":"https://example.com/v1",
            "model":"review","api_key":"dummy-review-key"}),
    )
    .await;
    let input = json!({"name":"Before","repository_url":"https://example.com/root.git",
        "credential_id":credential["id"],"model_profile_id":model["id"],
        "source_credentials":[{"repository_url":"https://example.com/sub.git","credential_id":credential["id"]}]});
    let (status, value) = request(
        &db,
        &cookie,
        Some(tenant),
        "POST",
        "/projects",
        input.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let id = Uuid::parse_str(value["id"].as_str().unwrap()).unwrap();
    let mut auth = Auth::scoped(user, "admin", vec![id], vec![]);
    auth.token_projects = None;
    auth.tenant_id = tenant;
    db.state
        .store
        .authorize_tenant(&mut auth, tenant)
        .await
        .unwrap();
    // One remaining connection must suffice even when other requests occupy the budget.
    let pool = connect(
        &db.state.settings.database_url,
        artifactory_control_plane::store::models(),
        PoolSettings::new(1).timeout(Duration::from_millis(500)),
    )
    .await
    .unwrap();
    let control = ControlStore::new(pool).for_auth(&auth);
    let mut changed = input;
    changed["name"] = json!("After");
    let input: ProjectInput = serde_json::from_value(changed).unwrap();
    let result =
        tokio::time::timeout(Duration::from_secs(3), control.update_project(id, &input)).await;
    let (_, current) = request(
        &db,
        &cookie,
        Some(tenant),
        "GET",
        &format!("/projects/{id}"),
        json!({}),
    )
    .await;
    drop(control);
    db.close().await;
    result
        .expect("update must not wait for another pool connection")
        .expect("valid resources use the existing transaction");
    assert_eq!(current["name"], "After");
}
