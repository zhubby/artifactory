//! [INPUT]: 依赖 Registry 凭据、Bearer 挑战和受限 HTTP 客户端。
//! [OUTPUT]: 提供 Manifest 请求与校验后流式 Blob 复制。
//! [POS]: 镜像网络传输边界，不跟随携带凭据的重定向。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::auth;
use anyhow::{Context, Result, ensure};
use artifactory_image_protocol::{RegistryLogin, digest, repository, sha256, tag};
use futures::StreamExt;
use reqwest::{Method, Response, StatusCode};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

#[derive(Clone)]
pub struct Registry(Arc<Inner>);
struct Inner {
    login: RegistryLogin,
    origin: url::Url,
    insecure: bool,
    http: reqwest::Client,
    tokens: tokio::sync::Mutex<BTreeMap<String, String>>,
}
pub(crate) async fn bounded(response: Response, maximum: usize) -> Result<Vec<u8>> {
    ensure!(
        response.status().is_success(),
        "registry returned HTTP {}",
        response.status()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= maximum as u64),
        "registry response exceeds limit"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(reqwest::Error::without_url)?;
        ensure!(
            bytes.len() + chunk.len() <= maximum,
            "registry response exceeds limit"
        );
        bytes.extend(chunk);
    }
    Ok(bytes)
}
impl Registry {
    /// 拒绝将现有版本标签指向另一个镜像。
    pub async fn ensure_tag_available(
        &self,
        repo: &str,
        reference: &str,
        expected: &str,
    ) -> Result<()> {
        ensure!(
            repository(repo) && tag(reference) && digest(expected),
            "invalid immutable tag"
        );
        let response = self
            .request(
                Method::GET,
                &format!("/v2/{repo}/manifests/{reference}"),
                &format!("repository:{repo}:pull"),
                None,
                None,
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        let bytes = bounded(response, 4 * 1024 * 1024).await?;
        ensure!(
            sha256(&bytes) == expected,
            "version tag is already bound to different content"
        );
        Ok(())
    }
    pub fn new(login: RegistryLogin, insecure: bool) -> Result<Self> {
        login.validate(insecure)?;
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(1800));
        if let Ok(path) = std::env::var("ARTIFACTORY_SERVICE_CA") {
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        Ok(Self(Arc::new(Inner {
            origin: url::Url::parse(&login.endpoint)?,
            login,
            insecure,
            http: builder.build()?,
            tokens: tokio::sync::Mutex::new(BTreeMap::new()),
        })))
    }
    pub fn host(&self) -> &str {
        self.0.origin.authority()
    }
    async fn bearer(&self, challenge: &str, scope: &str) -> Result<String> {
        let fields = auth::challenge(challenge)?;
        let mut realm = auth::realm(&fields["realm"], self.0.insecure)?;
        {
            let mut query = realm.query_pairs_mut();
            if let Some(service) = fields.get("service") {
                query.append_pair("service", service);
            }
            for item in scope.split(' ') {
                query.append_pair("scope", item);
            }
        }
        let response = self
            .0
            .http
            .get(realm)
            .basic_auth(&self.0.login.username, Some(&self.0.login.password))
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        let value: serde_json::Value =
            serde_json::from_slice(&bounded(response, 64 * 1024).await?)?;
        let token = value["token"]
            .as_str()
            .or_else(|| value["access_token"].as_str())
            .context("registry token missing")?
            .to_owned();
        ensure!(token.len() <= 32768, "oversized registry token");
        self.0
            .tokens
            .lock()
            .await
            .insert(scope.to_owned(), token.clone());
        Ok(token)
    }
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
        scope: &str,
        body: Option<Vec<u8>>,
        media: Option<&str>,
    ) -> Result<Response> {
        let url = self.0.origin.join(path)?;
        ensure!(
            url.origin() == self.0.origin.origin() && path.starts_with("/v2/"),
            "invalid registry path"
        );
        let mut token = self.0.tokens.lock().await.get(scope).cloned();
        for attempt in 0..2 {
            let mut req=self.0.http.request(method.clone(),url.clone()).header("Accept","application/vnd.oci.image.index.v1+json,application/vnd.oci.image.manifest.v1+json,application/vnd.docker.distribution.manifest.list.v2+json,application/vnd.docker.distribution.manifest.v2+json");
            req = if let Some(token) = &token {
                req.bearer_auth(token)
            } else {
                req.basic_auth(&self.0.login.username, Some(&self.0.login.password))
            };
            if let Some(body) = &body {
                req = req.body(body.clone());
            }
            if let Some(media) = media {
                req = req.header("Content-Type", media);
            }
            let response = req.send().await.map_err(reqwest::Error::without_url)?;
            if response.status() != StatusCode::UNAUTHORIZED || attempt == 1 {
                return Ok(response);
            }
            let challenge = response
                .headers()
                .get("www-authenticate")
                .context("registry challenge missing")?
                .to_str()?
                .to_owned();
            token = Some(self.bearer(&challenge, scope).await?);
        }
        unreachable!()
    }
    pub async fn manifest(&self, repo: &str, reference: &str) -> Result<Vec<u8>> {
        ensure!(
            repository(repo) && (digest(reference) || tag(reference)),
            "invalid manifest reference"
        );
        let response = self
            .request(
                Method::GET,
                &format!("/v2/{repo}/manifests/{reference}"),
                &format!("repository:{repo}:pull"),
                None,
                None,
            )
            .await?;
        let bytes = bounded(response, 4 * 1024 * 1024).await?;
        if digest(reference) {
            ensure!(sha256(&bytes) == reference, "manifest digest mismatch");
        }
        Ok(bytes)
    }
    pub async fn put_manifest(&self, repo: &str, reference: &str, bytes: &[u8]) -> Result<()> {
        ensure!(
            repository(repo) && (digest(reference) || tag(reference)),
            "invalid manifest destination"
        );
        let v: serde_json::Value = serde_json::from_slice(bytes)?;
        let media = v["mediaType"]
            .as_str()
            .context("manifest media type missing")?;
        let response = self
            .request(
                Method::PUT,
                &format!("/v2/{repo}/manifests/{reference}"),
                &format!("repository:{repo}:pull,push"),
                Some(bytes.to_vec()),
                Some(media),
            )
            .await?;
        ensure!(
            response.status() == StatusCode::CREATED,
            "manifest upload returned HTTP {}",
            response.status()
        );
        if let Some(remote) = response.headers().get("docker-content-digest") {
            ensure!(
                remote.to_str()? == sha256(bytes),
                "uploaded manifest digest mismatch"
            );
        }
        Ok(())
    }
    pub async fn small_blob(&self, repo: &str, id: &str) -> Result<Vec<u8>> {
        ensure!(repository(repo) && digest(id), "invalid blob reference");
        let r = self
            .request(
                Method::GET,
                &format!("/v2/{repo}/blobs/{id}"),
                &format!("repository:{repo}:pull"),
                None,
                None,
            )
            .await?;
        let bytes = bounded(r, 4 * 1024 * 1024).await?;
        ensure!(sha256(&bytes) == id, "blob digest mismatch");
        Ok(bytes)
    }
    pub(crate) async fn copy_blob(
        &self,
        source: &Registry,
        source_repo: &str,
        destination_repo: &str,
        id: &str,
        size: u64,
        temp: &Path,
    ) -> Result<()> {
        ensure!(
            repository(source_repo)
                && repository(destination_repo)
                && digest(id)
                && size <= 16 * 1024 * 1024 * 1024,
            "invalid blob declaration"
        );
        let scope = format!("repository:{destination_repo}:pull,push");
        let exists = self
            .request(
                Method::HEAD,
                &format!("/v2/{destination_repo}/blobs/{id}"),
                &scope,
                None,
                None,
            )
            .await?;
        if exists.status() == StatusCode::OK {
            return Ok(());
        }
        ensure!(
            exists.status() == StatusCode::NOT_FOUND,
            "blob lookup returned HTTP {}",
            exists.status()
        );
        let response = source
            .request(
                Method::GET,
                &format!("/v2/{source_repo}/blobs/{id}"),
                &format!("repository:{source_repo}:pull"),
                None,
                None,
            )
            .await?;
        ensure!(
            response.status().is_success(),
            "blob download returned HTTP {}",
            response.status()
        );
        let mut file = tokio::fs::File::from_std(tempfile::tempfile_in(temp)?);
        let mut stream = response.bytes_stream();
        let mut count = 0u64;
        let mut hash = Sha256::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(reqwest::Error::without_url)?;
            count += chunk.len() as u64;
            ensure!(count <= size, "blob exceeds declaration");
            hash.update(&chunk);
            file.write_all(&chunk).await?;
        }
        ensure!(
            count == size && format!("sha256:{}", hex::encode(hash.finalize())) == id,
            "blob integrity verification failed"
        );
        file.seek(std::io::SeekFrom::Start(0)).await?;
        let start = self
            .request(
                Method::POST,
                &format!("/v2/{destination_repo}/blobs/uploads/"),
                &scope,
                Some(Vec::new()),
                None,
            )
            .await?;
        ensure!(
            start.status() == StatusCode::ACCEPTED,
            "blob upload start returned HTTP {}",
            start.status()
        );
        let mut location = auth::upload_location(
            &self.0.origin,
            start
                .headers()
                .get("location")
                .context("upload location missing")?
                .to_str()?,
        )?;
        location.query_pairs_mut().append_pair("digest", id);
        let token = self.0.tokens.lock().await.get(&scope).cloned();
        let req = self
            .0
            .http
            .put(location)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", size);
        let req = if let Some(token) = token {
            req.bearer_auth(token)
        } else {
            req.basic_auth(&self.0.login.username, Some(&self.0.login.password))
        };
        let response = req
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(file),
            ))
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        ensure!(
            response.status() == StatusCode::CREATED,
            "blob upload returned HTTP {}",
            response.status()
        );
        Ok(())
    }
}
