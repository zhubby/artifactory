//! [INPUT]: 依赖私有 S3、主密钥和网关公共入口。
//! [OUTPUT]: 提供对象校验、私有签名和绑定交付文件名的网关票据。
//! [POS]: 所有客户端对象流量通过网关，内部校验直连 S3。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::settings::Settings;
use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub struct Storage {
    bucket: Bucket,
    public_url: String,
    ticket_key: [u8; 32],
    credentials: Credentials,
    client: reqwest::Client,
    temporary_dir: std::path::PathBuf,
    verifications: tokio::sync::Semaphore,
}

#[derive(serde::Serialize)]
pub struct ResolvedArtifact {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

fn valid_delivery_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= 255
        && filename.as_bytes()[0].is_ascii_alphanumeric()
        && filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

impl Storage {
    pub async fn delete_prefix(&self, prefix: &str) -> Result<()> {
        let mut continuation = None;
        loop {
            let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
            action.with_prefix(prefix);
            if let Some(token) = &continuation {
                action.with_continuation_token(token);
            }
            let body = self
                .client
                .get(action.sign(Duration::from_secs(60)))
                .send()
                .await
                .map_err(reqwest::Error::without_url)?
                .error_for_status()
                .map_err(reqwest::Error::without_url)?
                .text()
                .await?;
            let page = rusty_s3::actions::ListObjectsV2::parse_response(&body)?;
            for object in page.contents {
                ensure!(
                    object.key.starts_with(prefix),
                    "object listing escaped prefix"
                );
                self.delete(&object.key).await?;
            }
            continuation = page.next_continuation_token;
            if continuation.is_none() {
                break;
            }
        }
        Ok(())
    }
    pub async fn delete(&self, key: &str) -> Result<()> {
        self.client
            .delete(
                self.bucket
                    .delete_object(Some(&self.credentials), key)
                    .sign(Duration::from_secs(60)),
            )
            .send()
            .await
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(reqwest::Error::without_url)?;
        Ok(())
    }
    pub fn new(settings: &Settings) -> Result<Self> {
        Ok(Self {
            verifications: tokio::sync::Semaphore::new(settings.control.artifact_verifications),
            bucket: Bucket::new(
                settings.s3_endpoint.parse()?,
                UrlStyle::Path,
                settings.s3_bucket.clone(),
                settings.s3_region.clone(),
            )?,
            public_url: settings.public_url.clone(),
            ticket_key: settings.master_key,
            credentials: Credentials::new(&settings.s3_access_key, &settings.s3_secret_key),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()?,
            temporary_dir: settings.data_dir.clone(),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        let url = self
            .bucket
            .create_bucket(&self.credentials)
            .sign(Duration::from_secs(60));
        let response = self
            .client
            .put(url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("object store connection failed")?;
        ensure!(
            response.status().is_success() || response.status().as_u16() == 409,
            "object store bucket initialization failed: {}",
            response.status()
        );
        Ok(())
    }

    fn ticket(&self, key: &str, method: &str, seconds: i64) -> Result<String> {
        self.named_ticket(key, method, seconds, None)
    }
    fn named_ticket(
        &self,
        key: &str,
        method: &str,
        seconds: i64,
        filename: Option<&str>,
    ) -> Result<String> {
        ensure!(
            filename.is_none_or(|name| method == "GET" && valid_delivery_filename(name)),
            "invalid delivery filename"
        );
        let bytes = serde_json::to_vec(
            &serde_json::json!({"key":key,"method":method,"filename":filename,"expires":chrono::Utc::now().timestamp()+seconds}),
        )?;
        let sealed = crate::crypto::seal_context(&self.ticket_key, &bytes, b"artifact-gateway/v1")?;
        let mut url = url::Url::parse(&format!(
            "{}/artifacts/object",
            self.public_url.trim_end_matches('/')
        ))?;
        url.query_pairs_mut().append_pair("ticket", &sealed);
        Ok(url.to_string())
    }
    pub fn upload_url(&self, key: &str) -> Result<String> {
        self.ticket(key, "PUT", 900)
    }
    pub fn download_url(&self, key: &str) -> Result<String> {
        self.ticket(key, "GET", 60)
    }
    pub fn delivery_download_url(&self, key: &str, filename: &str) -> Result<String> {
        self.named_ticket(key, "GET", 60, Some(filename))
    }
    pub fn emissary_download_url(&self, key: &str) -> Result<String> {
        self.ticket(key, "GET", 900)
    }
    pub fn resolve_ticket(&self, ticket: &str, method: &str) -> Result<String> {
        Ok(self.resolve_object_ticket(ticket, method)?.url)
    }
    pub fn resolve_object_ticket(&self, ticket: &str, method: &str) -> Result<ResolvedArtifact> {
        ensure!(ticket.len() <= 8192, "invalid artifact ticket");
        let bytes = crate::crypto::open_context(&self.ticket_key, ticket, b"artifact-gateway/v1")?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let expected = value["method"].as_str().unwrap_or("");
        ensure!(
            method == expected || (method == "HEAD" && expected == "GET"),
            "artifact method not authorized"
        );
        let expires = value["expires"].as_i64().unwrap_or(0);
        let remaining = expires - chrono::Utc::now().timestamp();
        ensure!(remaining > 0 && remaining <= 900, "artifact ticket expired");
        let key = value["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid artifact key"))?;
        let filename = match value.get("filename") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(filename))
                if expected == "GET" && valid_delivery_filename(filename) =>
            {
                Some(filename.clone())
            }
            _ => anyhow::bail!("invalid delivery filename"),
        };
        let duration = Duration::from_secs(remaining as u64);
        let url = match method {
            "PUT" => self
                .bucket
                .put_object(Some(&self.credentials), key)
                .sign(duration),
            "HEAD" => self
                .bucket
                .head_object(Some(&self.credentials), key)
                .sign(duration),
            "GET" => self
                .bucket
                .get_object(Some(&self.credentials), key)
                .sign(duration),
            _ => anyhow::bail!("artifact method not authorized"),
        }
        .to_string();
        Ok(ResolvedArtifact { url, filename })
    }

    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<()> {
        let url = self
            .bucket
            .put_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(60));
        self.client
            .put(url)
            .body(body)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(|error| error.without_url())?;
        Ok(())
    }

    pub async fn get_log(&self, key: &str) -> Result<Vec<u8>> {
        let url = self
            .bucket
            .get_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(60));
        let mut stream = self
            .client
            .get(url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(|error| error.without_url())?
            .bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk?);
            ensure!(bytes.len() <= 65536, "oversized log object");
        }
        Ok(bytes)
    }

    pub async fn verify_and_promote(
        &self,
        key: &str,
        final_key: &str,
        expected_size: i64,
        expected_hash: &str,
    ) -> Result<()> {
        use tokio::io::{AsyncSeekExt, AsyncWriteExt};
        let _permit = self.verifications.acquire().await?;
        let url = self
            .bucket
            .get_object(Some(&self.credentials), key)
            .sign(Duration::from_secs(300));
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(|error| error.without_url())?;
        if let Some(size) = response.content_length() {
            ensure!(size == expected_size as u64, "artifact size does not match");
        }
        let mut size = 0_u64;
        let mut digest = Sha256::new();
        let mut verified = tokio::fs::File::from_std(tempfile::tempfile_in(&self.temporary_dir)?);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            size += chunk.len() as u64;
            ensure!(
                size <= expected_size as u64,
                "artifact exceeds expected size"
            );
            digest.update(&chunk);
            verified.write_all(&chunk).await?;
        }
        ensure!(
            size == expected_size as u64 && hex::encode(digest.finalize()) == expected_hash,
            "artifact integrity verification failed"
        );
        verified.seek(std::io::SeekFrom::Start(0)).await?;
        let url = self
            .bucket
            .put_object(Some(&self.credentials), final_key)
            .sign(Duration::from_secs(300));
        self.client
            .put(url)
            .header(reqwest::header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(verified),
            ))
            .send()
            .await
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(|error| error.without_url())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn storage() -> Storage {
        Storage {
            bucket: Bucket::new(
                "https://private.invalid:9000".parse().unwrap(),
                UrlStyle::Path,
                "artifacts",
                "us-east-1",
            )
            .unwrap(),
            public_url: "https://build.example.com/".into(),
            ticket_key: [42; 32],
            credentials: Credentials::new("fixture-access", "fixture-secret"),
            client: reqwest::Client::new(),
            temporary_dir: std::env::temp_dir(),
            verifications: tokio::sync::Semaphore::new(1),
        }
    }
    fn ticket(url: &str) -> String {
        url::Url::parse(url)
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "ticket")
            .unwrap()
            .1
            .into_owned()
    }
    #[tokio::test]
    async fn gateway_tickets_bind_object_and_method_and_hide_s3() {
        let storage = storage();
        let public = storage.download_url("private/object").unwrap();
        assert!(public.starts_with("https://build.example.com/artifacts/object?ticket="));
        assert!(!public.contains("private.invalid") && !public.contains("fixture-access"));
        let token = ticket(&public);
        for method in ["GET", "HEAD"] {
            let private =
                url::Url::parse(&storage.resolve_ticket(&token, method).unwrap()).unwrap();
            assert_eq!(private.host_str(), Some("private.invalid"));
            assert_eq!(private.path(), "/artifacts/private/object");
            assert!(private.query().unwrap().contains("X-Amz-Signature="));
        }
        assert!(storage.resolve_ticket(&token, "PUT").is_err());
        let upload = ticket(&storage.upload_url("uploads/new").unwrap());
        assert!(storage.resolve_ticket(&upload, "PUT").is_ok());
        assert!(storage.resolve_ticket(&upload, "GET").is_err());
        assert!(storage.resolve_ticket(&upload, "DELETE").is_err());
    }
    #[tokio::test]
    async fn delivery_tickets_bind_the_filename_for_get_and_head() {
        let storage = storage();
        let filename = "sample-server-1.2.3-rc.1-linux-amd64.tar.gz";
        let token = ticket(
            &storage
                .delivery_download_url("objects/signed", filename)
                .unwrap(),
        );
        for method in ["GET", "HEAD"] {
            let resolved = storage.resolve_object_ticket(&token, method).unwrap();
            assert_eq!(resolved.filename.as_deref(), Some(filename));
            assert_eq!(
                url::Url::parse(&resolved.url).unwrap().path(),
                "/artifacts/objects/signed"
            );
        }
        assert!(storage.resolve_object_ticket(&token, "PUT").is_err());
        for invalid in [
            "",
            "../object",
            "a/b",
            "a\\b",
            "a\r\nX:bad",
            "a\"b",
            "中文.zip",
        ] {
            assert!(
                storage
                    .delivery_download_url("objects/signed", invalid)
                    .is_err()
            );
        }
        assert!(
            storage
                .delivery_download_url("object", &"a".repeat(255))
                .is_ok()
        );
        assert!(
            storage
                .delivery_download_url("object", &"a".repeat(256))
                .is_err()
        );
        assert!(
            storage
                .named_ticket("object", "PUT", 60, Some(filename))
                .is_err()
        );
    }
    #[tokio::test]
    async fn gateway_tickets_reject_expiry_tampering_and_other_keys() {
        let storage = storage();
        assert!(
            storage
                .resolve_ticket(
                    &ticket(&storage.ticket("object", "GET", -1).unwrap()),
                    "GET"
                )
                .is_err()
        );
        assert!(
            storage
                .resolve_ticket(
                    &ticket(&storage.ticket("object", "GET", 901).unwrap()),
                    "GET"
                )
                .is_err()
        );
        let valid = ticket(&storage.download_url("object").unwrap());
        let mut changed = valid.as_bytes().to_vec();
        changed[10] = if changed[10] == b'A' { b'B' } else { b'A' };
        assert!(
            storage
                .resolve_ticket(std::str::from_utf8(&changed).unwrap(), "GET")
                .is_err()
        );
        let mut other = storage;
        other.ticket_key = [43; 32];
        assert!(other.resolve_ticket(&valid, "GET").is_err());
    }
}
