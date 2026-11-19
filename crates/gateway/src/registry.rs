//! [INPUT]: 依赖冻结发布授权、实际 Manifest 正文和固定仓库后端。
//! [OUTPUT]: 正文缓冲上限内计算 SHA256，授权成功后执行单次写入。
//! [POS]: 只截取正式镜像 Manifest；镜像层保持原有流式代理。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use pingora::{Result, http::ResponseHeader, proxy::Session};
use sha2::{Digest, Sha256};

pub const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

pub struct ManifestWrite<'a> {
    pub repository: &'a str,
    pub reference: &'a str,
}

pub fn formal_manifest(path: &str) -> Option<ManifestWrite<'_>> {
    let path = path.strip_prefix("/v2/")?;
    let (repository, reference) = path.rsplit_once("/manifests/")?;
    let parts: Vec<_> = repository.split('/').collect();
    (parts.len() == 4 && parts[2] == "images").then_some(ManifestWrite {
        repository,
        reference,
    })
}

fn failure(message: &'static str) -> Box<pingora::Error> {
    pingora::Error::explain(pingora::ErrorType::HTTPStatus(502), message)
}

async fn bounded_response(
    mut response: reqwest::Response,
    limit: usize,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= limit as u64)
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        anyhow::ensure!(bytes.len() + chunk.len() <= limit);
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn challenge(
    session: &mut Session,
    client: &reqwest::Client,
    registry: &url::Url,
) -> Result<()> {
    // 只读探测保留 Distribution 的认证挑战，不发送待授权正文。
    let target = registry
        .join("/v2/")
        .map_err(|_| failure("invalid registry challenge route"))?;
    let response = client
        .get(target)
        .send()
        .await
        .map_err(|_| failure("registry authentication unavailable"))?;
    let mut header = ResponseHeader::build(401, Some(3))?;
    if let Some(value) = response.headers().get("www-authenticate") {
        header.insert_header("WWW-Authenticate", value)?;
    }
    header.insert_header("Content-Length", "0")?;
    header.insert_header("Cache-Control", "no-store")?;
    session.write_response_header(Box::new(header), true).await
}

pub async fn write_manifest(
    session: &mut Session,
    client: &reqwest::Client,
    control: url::Url,
    registry: url::Url,
    gateway_token: &str,
) -> Result<()> {
    let path = session.req_header().uri.path().to_owned();
    let write = formal_manifest(&path).ok_or_else(|| failure("invalid manifest route"))?;
    let Some(authorization) = session
        .req_header()
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= 16384)
        .map(str::to_owned)
    else {
        return challenge(session, client, &registry).await;
    };
    if !authorization.starts_with("Bearer ") {
        return challenge(session, client, &registry).await;
    }
    if session
        .req_header()
        .headers
        .get("content-encoding")
        .is_some_and(|value| value.as_bytes() != b"identity")
    {
        return session.respond_error(415).await;
    }
    if session
        .req_header()
        .headers
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|size| size > MAX_MANIFEST_BYTES)
    {
        return session.respond_error(413).await;
    }
    let read = async {
        let mut bytes = Vec::new();
        while let Some(chunk) = session.read_request_body().await? {
            if bytes.len() + chunk.len() > MAX_MANIFEST_BYTES {
                return Ok::<_, Box<pingora::Error>>(None);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(bytes))
    };
    let body = match tokio::time::timeout(std::time::Duration::from_secs(30), read).await {
        Ok(Ok(Some(body))) => body,
        Ok(Ok(None)) => return session.respond_error(413).await,
        Ok(Err(error)) => return Err(error),
        Err(_) => return session.respond_error(408).await,
    };
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&body)));
    let resolver = control
        .join("/internal/registry-manifest-authorize")
        .map_err(|_| failure("invalid manifest authorization route"))?;
    let response = client.post(resolver).bearer_auth(gateway_token).json(&serde_json::json!({
        "authorization":authorization,"repository":write.repository,"reference":write.reference,"digest":digest,
    })).send().await.map_err(|_| failure("manifest authorization unavailable"))?;
    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return challenge(session, client, &registry).await;
        }
        let status = match response.status().as_u16() {
            403 => 403,
            409 => 409,
            _ => 502,
        };
        return session.respond_error(status).await;
    }
    let authorized = bounded_response(response, 16384)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|value| value["authorized"] == true);
    if !authorized {
        return session.respond_error(502).await;
    }
    // 正文已固定，授权后只发送一次，禁止重试有副作用的请求。
    let target = registry
        .join(&path)
        .map_err(|_| failure("invalid manifest backend"))?;
    let mut request = client
        .put(target)
        .header("authorization", authorization)
        .body(body);
    if let Some(value) = session.req_header().headers.get("content-type") {
        request = request.header("content-type", value);
    }
    let response = request
        .send()
        .await
        .map_err(|_| failure("manifest write unavailable"))?;
    let mut header = ResponseHeader::build(response.status().as_u16(), Some(8))?;
    for name in [
        "content-type",
        "docker-content-digest",
        "docker-distribution-api-version",
        "location",
        "www-authenticate",
    ] {
        if let Some(value) = response.headers().get(name) {
            header.insert_header(name, value)?;
        }
    }
    let body = bounded_response(response, 65536)
        .await
        .map_err(|_| failure("oversized manifest response"))?;
    header.insert_header("Content-Length", body.len().to_string())?;
    header.insert_header("Cache-Control", "no-store")?;
    session
        .write_response_header(Box::new(header), body.is_empty())
        .await?;
    if !body.is_empty() {
        session.write_response_body(Some(body.into()), true).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formal_manifest_routes_exclude_candidate_and_blob_writes() {
        let write = formal_manifest("/v2/tenant/project/images/app/manifests/1.2.3-rc.1").unwrap();
        assert_eq!(write.repository, "tenant/project/images/app");
        assert_eq!(write.reference, "1.2.3-rc.1");
        assert!(formal_manifest("/v2/tenant/project/builds/id/manifests/root").is_none());
        assert!(formal_manifest("/v2/tenant/project/images/app/blobs/uploads/uuid").is_none());
        // 空引用和尾斜杠同样进入围栏，禁止交给上游路径规范化。
        assert_eq!(
            formal_manifest("/v2/tenant/project/images/app/manifests/")
                .unwrap()
                .reference,
            ""
        );
        assert_eq!(
            formal_manifest("/v2/tenant/project/images/app/manifests/1.2.3/")
                .unwrap()
                .reference,
            "1.2.3/"
        );
    }
}
