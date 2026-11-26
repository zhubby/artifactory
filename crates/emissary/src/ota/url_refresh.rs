//! [INPUT]: 已验签的冻结更新身份与runtime提供的私有邮箱回复。
//! [OUTPUT]: 请求同一资产的新票据，拒绝迟到或错代回复。
//! [POS]: launcher与runtime的URL刷新边界；不持有平台登录凭据。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Asset, OtaConfig, Request};
use crate::config::{atomic_json, read_json};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlRefreshRequest {
    pub request_id: String,
    pub update_id: String,
    pub generation: i64,
    pub manifest_sha256: String,
    pub asset_sha256: String,
    pub asset_size: u64,
    pub os: String,
    pub arch: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlRefreshReply {
    pub request: UrlRefreshRequest,
    pub url: Option<String>,
}

pub fn url_refresh_request(directory: &Path) -> Result<Option<UrlRefreshRequest>> {
    let path = directory.join("url-refresh-request.json");
    if !path.try_exists()? {
        return Ok(None);
    }
    let request: UrlRefreshRequest = read_json(&path)?;
    ensure!(
        serde_json::to_vec(&request)?.len() <= 4096
            && uuid::Uuid::parse_str(&request.request_id).is_ok()
            && request.update_id.len() <= 100
            && request.manifest_sha256.len() == 64
            && request.asset_sha256.len() == 64,
        "invalid URL refresh request"
    );
    Ok(Some(request))
}
/// runtime只能答复其刚读取的冻结请求；Control仍必须复核授权。
pub fn complete_url_refresh(
    directory: &Path,
    request: UrlRefreshRequest,
    url: Option<String>,
) -> Result<()> {
    if url_refresh_request(directory)?.as_ref() != Some(&request) {
        return Ok(());
    }
    let reply = UrlRefreshReply { request, url };
    ensure!(
        serde_json::to_vec(&reply)?.len() <= 4096,
        "oversized URL refresh reply"
    );
    atomic_json(&directory.join("url-refresh-reply.json"), &reply)
}

struct Mailbox {
    request: PathBuf,
    reply: PathBuf,
}
impl Drop for Mailbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.request);
        let _ = std::fs::remove_file(&self.reply);
    }
}

pub(super) async fn refresh(cfg: &OtaConfig, request: &Request, asset: &Asset) -> Result<url::Url> {
    let (os, arch) = crate::platform();
    let frozen = UrlRefreshRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        update_id: request.id.clone(),
        generation: request.generation,
        manifest_sha256: hex::encode(Sha256::digest(request.manifest.as_bytes())),
        asset_sha256: asset.sha256.clone(),
        asset_size: asset.size,
        os: os.into(),
        arch: arch.into(),
    };
    let mailbox = Mailbox {
        request: cfg.directory.join("url-refresh-request.json"),
        reply: cfg.directory.join("url-refresh-reply.json"),
    };
    atomic_json(&mailbox.request, &frozen)?;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(reply) = read_json::<UrlRefreshReply>(&mailbox.reply)
                && reply.request == frozen
            {
                let Some(url) = reply.url else {
                    bail!("release URL refresh denied");
                };
                return crate::source::validate_url(&url, cfg.allow_http);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => bail!("release URL refresh unavailable"),
    }
}
