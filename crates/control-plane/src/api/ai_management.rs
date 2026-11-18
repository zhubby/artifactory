//! [INPUT]: 依赖租户鉴权、模型配置存储与规划状态。
//! [OUTPUT]: 提供模型配置、澄清、计划和分页对话接口。
//! [POS]: AI 规划的管理入口；构建执行由独立节点负责。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::auth::TenantState;
pub use crate::store::ProfileInput;
use crate::store::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query},
};
use serde_json::{Value, json};
use uuid::Uuid;

fn validate(state: &AppState, input: &ProfileInput) -> Result<()> {
    let url = url::Url::parse(&input.base_url)
        .map_err(|_| ApiError::BadRequest("invalid model endpoint".into()))?;
    if input.name.trim().is_empty()
        || input.name.len() > 100
        || input.model.is_empty()
        || input.model.len() > 200
        || ![
            "openai-completions",
            "openai-responses",
            "anthropic-messages",
        ]
        .contains(&input.api.as_str())
        || url.host_str().is_none()
        || !(url.scheme() == "https" || state.settings.insecure_dev && url.scheme() == "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !input.parameters.is_object()
        || input.parameters.to_string().len() > 8192
        || input.api_key.len() > 8192
        || input.api_key.contains(['\0', '\r', '\n'])
        || !(1..=100).contains(&input.max_calls)
        || !(30..=3600).contains(&input.timeout_seconds)
    {
        return Err(ApiError::BadRequest("invalid model configuration".into()));
    }
    // Parameters are model inference options, never an alternate auth/endpoint channel.
    let allowed = [
        "thinking_level",
        "max_tokens",
        "context_window",
        "temperature",
        "requests_per_minute",
    ];
    if input
        .parameters
        .as_object()
        .unwrap()
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(ApiError::BadRequest("unsupported model parameter".into()));
    }
    for (key, value) in input.parameters.as_object().unwrap() {
        let valid = match key.as_str() {
            "thinking_level" => value.as_str().is_some_and(|level| {
                ["off", "minimal", "low", "medium", "high", "xhigh", "max"].contains(&level)
            }),
            "max_tokens" => value.as_u64().is_some_and(|n| (1..=1_000_000).contains(&n)),
            "context_window" => value
                .as_u64()
                .is_some_and(|n| (1024..=10_000_000).contains(&n)),
            "requests_per_minute" => value.as_u64().is_some_and(|n| (1..=600).contains(&n)),
            "temperature" => value.as_f64().is_some_and(|n| (0.0..=2.0).contains(&n)),
            _ => false,
        };
        if !valid {
            return Err(ApiError::BadRequest(format!(
                "invalid model parameter: {key}"
            )));
        }
    }
    Ok(())
}

pub async fn profiles(
    TenantState(state): TenantState,
    auth: Auth,
) -> Result<Json<Vec<dto::ModelProfile>>> {
    auth.session_role("admin")?;
    Ok(Json(state.store.model_profiles().await?))
}

pub async fn model_options(
    TenantState(state): TenantState,
    auth: Auth,
) -> Result<Json<Vec<dto::ModelOption>>> {
    auth.session_role("viewer")?;
    Ok(Json(state.store.model_options().await?))
}
pub async fn create_profile(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<ProfileInput>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    validate(&state, &input)?;
    if input.api_key.is_empty() {
        return Err(ApiError::BadRequest("model API key is required".into()));
    }
    let id = Uuid::new_v4();
    state
        .store
        .save_model_profile(&state.settings.master_key, id, input, false)
        .await?;
    state
        .audit(auth.user_id, "model_profile_created", id, json!({}))
        .await?;
    Ok(Json(json!({"id":id})))
}
pub async fn update_profile(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<ProfileInput>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    validate(&state, &input)?;
    state
        .store
        .save_model_profile(&state.settings.master_key, id, input, true)
        .await?;
    state
        .audit(auth.user_id, "model_profile_updated", id, json!({}))
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn delete_profile(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    state.store.delete_model_profile(id).await?;
    state
        .audit(auth.user_id, "model_profile_deleted", id, json!({}))
        .await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn clarify(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Value>,
) -> Result<Json<Value>> {
    let answer = input
        .get("answer")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty() && v.len() <= 8000)
        .ok_or_else(|| ApiError::BadRequest("answer is required".into()))?;
    state.store.clarify_build(id, answer, &auth).await?;
    state
        .audit(auth.user_id, "build_clarified", id, json!({}))
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn plans(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<dto::BuildPlans>> {
    Ok(Json(state.store.plans(id, &auth).await?))
}

pub async fn conversations(
    TenantState(state): TenantState,
    auth: Auth,
    Path(build): Path<Uuid>,
) -> Result<Json<Value>> {
    let context = state.store.conversation_context(build).await?;
    let project: Uuid = context["project_id"]
        .as_str()
        .ok_or(ApiError::NotFound)?
        .parse()
        .map_err(anyhow::Error::from)?;
    auth.project(project, "read")?;
    let mut conversations = Vec::new();
    for run in context["runs"].as_array().ok_or(ApiError::NotFound)? {
        let result: Value = crate::runtime_bridge::runtime()?
            .post(
                "/v1/conversations",
                &json!({"run_id":run["run_id"],"tenant_id":auth.tenant_id,"project_id":project}),
            )
            .await?;
        if let Some(items) = result["conversations"].as_array() {
            for item in items {
                let mut item = item.clone();
                item["purpose"] = run["purpose"].clone();
                conversations.push(item);
            }
        }
    }
    Ok(Json(json!({"conversations":conversations})))
}

#[derive(serde::Deserialize)]
pub struct MessagePage {
    after: Option<i64>,
    limit: Option<usize>,
}
pub async fn messages(
    TenantState(state): TenantState,
    auth: Auth,
    Path((build, run, generation)): Path<(Uuid, Uuid, i64)>,
    Query(page): Query<MessagePage>,
) -> Result<Json<Value>> {
    let context = state.store.conversation_context(build).await?;
    let project: Uuid = context["project_id"]
        .as_str()
        .ok_or(ApiError::NotFound)?
        .parse()
        .map_err(anyhow::Error::from)?;
    auth.project(project, "read")?;
    if generation < 1
        || !context["runs"]
            .as_array()
            .is_some_and(|runs| runs.iter().any(|item| item["run_id"] == run.to_string()))
    {
        return Err(ApiError::NotFound);
    }
    Ok(Json(crate::runtime_bridge::runtime()?.post("/v1/conversation/messages",&json!({"run_id":run,"tenant_id":auth.tenant_id,"project_id":project,"generation":generation,"after":page.after.unwrap_or(0),"limit":page.limit.unwrap_or(50).min(100)})).await?))
}
