//! [INPUT]: 依赖任务专属令牌、控制资源 URL 与服务 CA。
//! [OUTPUT]: 提供有界报告、脱敏失败日志、发布材料读取和心跳。
//! [POS]: 工作 Pod 到控制平面的认证边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use artifactory_image_protocol::Report;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Control {
    pub http: reqwest::Client,
    url: String,
    token: String,
}
impl Control {
    pub fn new(url: String, token: String, insecure: bool) -> Result<Self> {
        let parsed = url::Url::parse(&url)?;
        ensure!(
            (parsed.scheme() == "https" || (insecure && parsed.scheme() == "http"))
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none(),
            "invalid control resource URL"
        );
        let mut http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20));
        if let Ok(path) = std::env::var("ARTIFACTORY_SERVICE_CA") {
            http =
                http.add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        Ok(Self {
            http: http.build()?,
            url: url.trim_end_matches('/').into(),
            token,
        })
    }
    pub async fn report(&self, report: &Report) -> Result<()> {
        for attempt in 0..3 {
            let response = self
                .http
                .post(format!("{}/report", self.url))
                .bearer_auth(&self.token)
                .json(report)
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) if response.status().is_client_error() => {
                    anyhow::bail!("execution report rejected: HTTP {}", response.status())
                }
                _ if attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await
                }
                _ => anyhow::bail!("execution report unavailable"),
            }
        }
        unreachable!()
    }
    pub(crate) async fn failure(
        &self,
        platform: &str,
        error: &anyhow::Error,
        secrets: artifactory_emissary::signing::redact::Secrets,
        sequence: &AtomicI64,
    ) -> String {
        let text = crate::diagnostics::error_text(error, secrets)
            .await
            .unwrap_or_else(|_| "failure diagnostic unavailable".into());
        let stage = if platform == "publish" {
            "publication"
        } else {
            "build"
        };
        let message = crate::diagnostics::summary(stage, &text);
        let _ = self
            .report(&Report::Log {
                platform: platform.into(),
                sequence: sequence.fetch_add(1, Ordering::Relaxed) + 1,
                text,
            })
            .await;
        let _ = self
            .report(&Report::Failed {
                message: message.clone(),
            })
            .await;
        message
    }
    pub async fn publication(&self) -> Result<Vec<artifactory_image_protocol::PlatformResult>> {
        let response = self
            .http
            .get(format!("{}/publication", self.url))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        ensure!(
            response.status().is_success(),
            "publication not authorized: HTTP {}",
            response.status()
        );
        ensure!(
            response.content_length().is_none_or(|n| n <= 32768),
            "oversized publication context"
        );
        let bytes = response
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)?;
        ensure!(bytes.len() <= 32768, "oversized publication context");
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub fn heartbeat(
        &self,
        platform: String,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let control = self.clone();
        tokio::spawn(async move {
            let report = Report::Heartbeat {
                platform,
                pod_uid: std::env::var("POD_UID").unwrap_or_default(),
            };
            loop {
                if control.report(&report).await.is_err() {
                    cancel.cancel();
                    return;
                }
                tokio::select! { _=cancel.cancelled()=>return, _=tokio::time::sleep(Duration::from_secs(10))=>{} }
            }
        })
    }
}
