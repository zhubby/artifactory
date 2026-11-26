//! [INPUT]: 原节点mTLS证书、当前控制会话与冻结更新资产。
//! [OUTPUT]: 仅为仍获授权的同一资产签发新网关下载票据。
//! [POS]: OTA控制RPC边界，不接受浏览器或任意对象路径。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{protocol, state::AppState};
use tonic::{Request, Response, Status};
use uuid::Uuid;

pub async fn renew_asset(
    state: &AppState,
    request: Request<protocol::EmissaryAssetRenewRequest>,
) -> Result<Response<protocol::EmissaryAssetRenewReply>, Status> {
    let node = crate::grpc::identity(state, &request).await?;
    let input = request.into_inner();
    let session = Uuid::parse_str(&input.session_id)
        .map_err(|_| Status::invalid_argument("invalid control session"))?;
    let url = renew_frozen_asset(state, node, session, &input).await?;
    Ok(Response::new(protocol::EmissaryAssetRenewReply {
        request_id: input.request_id,
        update_id: input.update_id,
        generation: input.generation,
        url,
    }))
}

/// node来自已认证证书；该入口便于对真实数据库事务单独验收。
pub async fn renew_frozen_asset(
    state: &AppState,
    node: Uuid,
    session: Uuid,
    input: &protocol::EmissaryAssetRenewRequest,
) -> Result<String, Status> {
    let key = state
        .store
        .emissary_asset_renewal(node, session, input)
        .await
        .map_err(|_| Status::permission_denied("frozen update is stale or unauthorized"))?;
    let url = state
        .storage
        .emissary_download_url(&key)
        .map_err(|_| Status::unavailable("release ticket unavailable"))?;
    Ok(url)
}
