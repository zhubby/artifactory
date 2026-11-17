//! [INPUT]: 依赖项目权限、执行令牌和仓库内容校验。
//! [OUTPUT]: 提供镜像凭据、无秘密仓库来源、日志与独立发布接口。
//! [POS]: 人员 API 与工作 Pod API 使用独立认证边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{Auth, TenantState},
    crypto,
    error::{ApiError, Result},
    images,
    state::AppState,
};
use artifactory_image_protocol::{RegistryLogin, Report};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
fn bearer(headers: &HeaderMap) -> Result<&str> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)
}
async fn task(state: &AppState, id: Uuid, headers: &HeaderMap) -> Result<(bool, String)> {
    let token = bearer(headers)?.to_owned();
    let publisher = state
        .store
        .image_token(id, &token)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok((publisher, token))
}
pub async fn report(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(report): Json<Report>,
) -> Result<Json<Value>> {
    let (publisher, token) = task(&state, id, &headers).await?;
    if matches!(report, Report::Published { .. }) && !publisher
        || matches!(report, Report::Platform { .. }) && publisher
    {
        return Err(ApiError::Forbidden);
    }
    let e = state.store.image_execution(id).await?;
    if let Report::Published { digest } = &report
        && publisher
        && e.status == "succeeded"
        && e.root_digest.as_ref() == Some(digest)
    {
        return Ok(Json(json!({"ok":true})));
    }
    if !state.store.image_live(id).await? {
        return Err(ApiError::Conflict(
            "image execution is stale or canceled".into(),
        ));
    }
    images::verify_report(&state, &e, &token, &report).await?;
    let object = if let Report::Log {
        platform,
        sequence,
        text,
    } = &report
    {
        if publisher || !e.spec.platforms.contains(platform) || *sequence <= 0 || text.len() > 16384
        {
            return Err(ApiError::BadRequest("invalid image log".into()));
        }
        let hash = crypto::hash(text);
        let key = format!(
            "{}/{}/image-logs/{id}/{platform}/{sequence}-{hash}",
            e.tenant_id, e.project_id
        );
        state.storage.put(&key, text.as_bytes().to_vec()).await?;
        Some((key, hash))
    } else {
        None
    };
    state
        .store
        .image_report(
            id,
            publisher,
            &report,
            object.as_ref().map(|(k, h)| (k.as_str(), h.as_str())),
        )
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn publication(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (publisher, _) = task(&state, id, &headers).await?;
    if !publisher || !state.store.image_live(id).await? {
        return Err(ApiError::Forbidden);
    }
    let e = state.store.image_execution(id).await?;
    artifactory_image_protocol::index(&e.spec.platforms, &e.results)?;
    Ok(Json(
        serde_json::to_value(e.results).map_err(anyhow::Error::from)?,
    ))
}
#[derive(Deserialize)]
pub struct ImageQuery {
    project_id: Option<Uuid>,
}
pub async fn list(
    TenantState(state): TenantState,
    auth: Auth,
    Query(query): Query<ImageQuery>,
) -> Result<Json<Vec<Value>>> {
    if let Some(project) = query.project_id {
        auth.project(project, "read")?;
    }
    let projects = auth
        .visible_projects()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| auth.project(*p, "read").is_ok())
        .collect::<Vec<_>>();
    Ok(Json(state.store.images(&projects, query.project_id).await?))
}
pub async fn credentials(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Value>>> {
    auth.project(id, "read")?;
    Ok(Json(state.store.image_credentials(id).await?))
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Material {
    Registry { name: String, login: RegistryLogin },
    Secret { name: String, value: String },
}
pub async fn create_credential(
    TenantState(state): TenantState,
    auth: Auth,
    Path(project): Path<Uuid>,
    Json(material): Json<Material>,
) -> Result<Json<Value>> {
    auth.project(project, "admin")?;
    let id = Uuid::new_v4();
    let (name, registry_origin, bytes) = match material {
        Material::Registry { name, login } => {
            login.validate(state.settings.insecure_dev)?;
            (
                name,
                Some(
                    url::Url::parse(&login.endpoint)
                        .map_err(anyhow::Error::from)?
                        .origin()
                        .ascii_serialization(),
                ),
                serde_json::to_vec(&login).map_err(anyhow::Error::from)?,
            )
        }
        Material::Secret { name, value } => (name, None, value.into_bytes()),
    };
    if name.trim().is_empty() || name.len() > 100 || bytes.is_empty() || bytes.len() > 128 * 1024 {
        return Err(ApiError::BadRequest("invalid image credential".into()));
    }
    let envelope = crypto::seal_context(
        &state.settings.master_key,
        &bytes,
        format!("image-credential/{project}/{id}").as_bytes(),
    )?;
    state
        .store
        .save_image_credential(project, id, &name, registry_origin.as_deref(), &envelope)
        .await?;
    state
        .audit(
            auth.user_id,
            "image_credential.create",
            id,
            json!({"project":project,"kind":if registry_origin.is_some(){"registry"}else{"secret"}}),
        )
        .await?;
    Ok(Json(json!({"id":id})))
}
pub async fn disable_credential(
    TenantState(state): TenantState,
    auth: Auth,
    Path((project, credential)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    auth.project(project, "admin")?;
    state
        .store
        .disable_image_credential(project, credential)
        .await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    credential_id: Option<Uuid>,
    repository: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}
pub async fn publish(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Publish>,
) -> Result<Json<Value>> {
    let e = state.store.image_execution(id).await?;
    auth.project(e.project_id, "release")?;
    if !images::enabled() {
        return Err(ApiError::Conflict(images::DISABLED_MESSAGE.into()));
    }
    if input.tags.len() > 16
        || input
            .tags
            .iter()
            .any(|s| !artifactory_image_protocol::tag(s))
        || input.credential_id.is_some()
            && input
                .repository
                .as_deref()
                .is_none_or(|s| !artifactory_image_protocol::repository(s))
        || input.credential_id.is_none() && (input.repository.is_some() || !input.tags.is_empty())
    {
        return Err(ApiError::BadRequest(
            "invalid publication destination".into(),
        ));
    }
    let publication = Uuid::new_v4();
    let token = crypto::token();
    state
        .store
        .request_image_publication(
            id,
            publication,
            input.credential_id,
            input.repository.as_deref(),
            &input.tags,
            &crypto::hash(&token),
        )
        .await?;
    if let Err(error) = images::save_publication(&state, publication, &token).await {
        state.store.finish_image(publication, "failed").await?;
        return Err(error.into());
    }
    Ok(Json(json!({"id":publication})))
}
#[derive(Deserialize)]
pub struct Cursor {
    #[serde(default)]
    after: i64,
}
pub async fn logs(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>> {
    let e = state.store.image_execution(id).await?;
    auth.project(e.project_id, "read")?;
    let mut result = Vec::new();
    for row in state.store.image_log_keys(id, cursor.after).await? {
        let bytes = state
            .storage
            .get_log(row["object_key"].as_str().ok_or(ApiError::NotFound)?)
            .await?;
        result.push(json!({"id":row["id"],"platform":row["platform"],"sequence":row["sequence"],"text":String::from_utf8_lossy(&bytes)}));
    }
    Ok(Json(json!(result)))
}
