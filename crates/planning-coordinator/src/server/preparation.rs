//! [INPUT]: 依赖 Pod 投影令牌和准备状态。
//! [OUTPUT]: 提供清单、阶段和本地就绪的受权端点。
//! [POS]: 仅向 Coordinator 存储传递已核验的完整执行身份。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
async fn execution(
    s: &Coordinator,
    v: &Value,
) -> artifactory_service_transport::error::Result<Execution> {
    use artifactory_service_transport::error::ApiError;
    let ex = preparation_identity(v)
        .map_err(|_| ApiError::BadRequest("invalid source identity".into()))?;
    let token = v["bootstrap_token"]
        .as_str()
        .ok_or(ApiError::Unauthorized)?;
    token_review(&ex, token)
        .await
        .map_err(authorization_error)?;
    preparation_ready(s, &ex).await?;
    Ok(ex)
}
pub(super) async fn pin(State(s): State<Coordinator>, Json(v): Json<Value>) -> ApiResult<Value> {
    let ex = execution(&s, &v).await?;
    Ok(Json(s.store.preparation_pin(&ex, &v["pin"]).await?))
}
pub(super) async fn commit(State(s): State<Coordinator>, Json(v): Json<Value>) -> ApiResult<Value> {
    let ex = execution(&s, &v).await?;
    let result = s
        .store
        .preparation_commit(&ex, &v["result"]["manifest"])
        .await?;
    Ok(Json(result))
}
pub(super) async fn ready(State(s): State<Coordinator>, Json(v): Json<Value>) -> ApiResult<Value> {
    let ex = execution(&s, &v).await?;
    Ok(Json(
        s.store
            .preparation_ready(
                &ex,
                id(&v, "source_manifest_id")?,
                v["source_digest"]
                    .as_str()
                    .context("source digest required")?,
            )
            .await?,
    ))
}
pub(super) async fn progress(
    State(s): State<Coordinator>,
    Json(v): Json<Value>,
) -> ApiResult<Value> {
    let ex = execution(&s, &v).await?;
    Ok(Json(
        s.store
            .preparation_progress(&ex, &serde_json::from_value(v["progress"].clone())?)
            .await?,
    ))
}
pub(super) async fn fail(State(s): State<Coordinator>, Json(v): Json<Value>) -> ApiResult<Value> {
    let ex = execution(&s, &v).await?;
    Ok(Json(
        s.store
            .preparation_fail(&ex, &serde_json::from_value(v["diagnostic"].clone())?)
            .await?,
    ))
}
