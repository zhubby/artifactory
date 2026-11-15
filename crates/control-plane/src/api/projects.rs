use crate::auth::TenantState;
pub use crate::store::ProjectInput;
use crate::store::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    source, source_cache,
    state::AppState,
};
use axum::{Json, extract::Path};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn list(
    TenantState(state): TenantState,
    auth: Auth,
) -> Result<Json<Vec<dto::ProjectSummary>>> {
    if auth.token_projects.is_some() && !auth.token_scopes.iter().any(|scope| scope == "read") {
        return Err(ApiError::Forbidden);
    }
    Ok(Json(state.store.projects(auth.visible_projects()).await?))
}

pub async fn detail(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<dto::Project>> {
    auth.project(id, "read")?;
    Ok(Json(state.store.project(id).await?))
}

pub async fn resolve(
    TenantState(state): TenantState,
    auth: Auth,
    Path(slug): Path<String>,
) -> Result<Json<dto::Project>> {
    let id = state.store.project_by_slug(&slug).await?;
    auth.project(id, "read")?;
    Ok(Json(state.store.project(id).await?))
}

pub async fn refs(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<source_cache::RepositoryRefs>> {
    auth.project(id, "read")?;
    let project = state.store.project_source(id).await?;
    if project.planner_backend == "kubernetes" {
        let result = crate::runtime_bridge::remote_refs(
            &state,
            id,
            &project.repository_url,
            project.credential_id,
        )
        .await
        .map_err(|_| ApiError::BadGateway("Source Service reference query failed".into()))?;
        return Ok(Json(
            serde_json::from_value(result).map_err(anyhow::Error::from)?,
        ));
    }
    if let Some(client) = crate::local_source::client(&state).await? {
        let result = client
            .post(
                "/internal/local-source",
                &json!({"operation":"refs","project_id":id}),
            )
            .await?;
        return Ok(Json(
            serde_json::from_value(result).map_err(anyhow::Error::from)?,
        ));
    }
    source_cache::list_refs(&state, &project.repository_url, project.credential_id)
        .await
        .map(Json)
        .map_err(|error| match error {
            source_cache::RefQueryError::Timeout => ApiError::GatewayTimeout(error.to_string()),
            _ => ApiError::BadGateway(error.to_string()),
        })
}

async fn validate(_state: &AppState, input: &ProjectInput) -> Result<()> {
    if input.name.is_empty()
        || input.name.len() > 100
        || input.signing_profiles.len() > 32
        || input.build_context.len() > 16000
        || input.source_credentials.len() > 128
    {
        return Err(ApiError::BadRequest("invalid project settings".into()));
    }
    if let Some(backend) = &input.planner_backend
        && (!matches!(backend.as_str(), "poll" | "kubernetes")
            || (backend == "kubernetes" && std::env::var("ARTIFACTORY_RUNTIME_URL").is_err()))
    {
        return Err(ApiError::BadRequest("planner backend unavailable".into()));
    }
    source::validate_repository(&input.repository_url)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    for binding in &input.source_credentials {
        source::validate_repository(&binding.repository_url)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
    Ok(())
}

pub async fn create(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    validate(&state, &input).await?;
    let id = Uuid::new_v4();
    state.store.create_project(id, &input).await?;
    let slug = state.store.project(id).await?.slug;
    state
        .audit(auth.user_id, "project_created", id, json!({}))
        .await?;
    Ok(Json(json!({"id":id,"slug":slug})))
}

pub async fn update(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>> {
    auth.project(id, "admin")?;
    validate(&state, &input).await?;
    state.store.update_project(id, &input).await?;
    state
        .audit(auth.user_id, "project_updated", id, json!({}))
        .await?;
    Ok(Json(json!({"id":id})))
}
