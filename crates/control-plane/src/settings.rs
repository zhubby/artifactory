//! [INPUT]: 依赖环境变量、私有签名密钥环与服务地址校验。
//! [OUTPUT]: 提供服务启动设置与资源预算。
//! [POS]: 部署配置进入业务进程的唯一转换边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{env, path::PathBuf};

#[derive(Clone)]
pub struct Settings {
    pub database_url: String,
    pub http_bind: String,
    pub grpc_bind: String,
    pub public_url: String,
    pub insecure_dev: bool,
    pub registration_enabled: bool,
    pub pki_dir: PathBuf,
    pub data_dir: PathBuf,
    pub master_key: [u8; 32],
    pub signing_keys: Option<crate::signing::KeyRing>,
    pub s3_endpoint: String,
    pub s3_public_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub telemetry: Option<TelemetrySettings>,
    pub control: ControlLimits,
}

/// Per-replica resource budgets; database capacity must cover their aggregate.
#[derive(Clone, Debug)]
pub struct ControlLimits {
    pub database_connections: u32,
    pub artifact_verifications: usize,
    pub scheduler_batch: i64,
    pub drain_seconds: u64,
}

impl Default for ControlLimits {
    fn default() -> Self {
        Self {
            database_connections: 20,
            artifact_verifications: 4,
            scheduler_batch: 64,
            drain_seconds: 5,
        }
    }
}

impl ControlLimits {
    fn from_env() -> Result<Self> {
        fn count(name: &str, default: u64, minimum: u64, maximum: u64) -> Result<u64> {
            let value = env::var(name)
                .map_or(Ok(default), |value| value.parse::<u64>())
                .with_context(|| format!("{name} must be an integer"))?;
            ensure!(
                (minimum..=maximum).contains(&value),
                "{name} must be between {minimum} and {maximum}"
            );
            Ok(value)
        }
        Ok(Self {
            database_connections: u32::try_from(count(
                "ARTIFACTORY_DB_MAX_CONNECTIONS",
                20,
                8,
                256,
            )?)?,
            artifact_verifications: usize::try_from(count(
                "ARTIFACTORY_ARTIFACT_VERIFY_CONCURRENCY",
                4,
                1,
                32,
            )?)?,
            scheduler_batch: i64::try_from(count("ARTIFACTORY_SCHEDULER_BATCH_SIZE", 64, 1, 256)?)?,
            drain_seconds: count("ARTIFACTORY_CONTROL_DRAIN_SECONDS", 5, 1, 30)?,
        })
    }
}

#[derive(Clone)]
pub struct TelemetrySettings {
    pub endpoint: String,
    pub query_url: String,
    pub token: String,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let required = |name: &str| env::var(name).with_context(|| format!("{name} is required"));
        let insecure_dev = env::var("ARTIFACTORY_INSECURE_DEV").as_deref() == Ok("true");
        let public_url = required("ARTIFACTORY_PUBLIC_URL")?;
        let s3_endpoint = required("ARTIFACTORY_S3_ENDPOINT")?;
        let s3_public_endpoint =
            env::var("ARTIFACTORY_S3_PUBLIC_ENDPOINT").unwrap_or_else(|_| s3_endpoint.clone());
        validate_endpoint(&public_url, insecure_dev)?;
        let private_http = env::var("ARTIFACTORY_S3_ALLOW_HTTP").as_deref() == Ok("true");
        validate_endpoint(&s3_endpoint, insecure_dev || private_http)?;
        let decoded = STANDARD.decode(required("ARTIFACTORY_MASTER_KEY")?)?;
        let telemetry = match env::var("ARTIFACTORY_TELEMETRY_ENDPOINT") {
            Ok(endpoint) if !endpoint.is_empty() => {
                let address = url::Url::parse(&endpoint)?;
                ensure!(
                    address.scheme() == "https"
                        && address.host_str().is_some()
                        && address.username().is_empty()
                        && address.password().is_none()
                        && address.path() == "/"
                        && address.query().is_none()
                        && address.fragment().is_none(),
                    "telemetry endpoint requires an HTTPS host and port"
                );
                let query_url = required("ARTIFACTORY_TELEMETRY_URL")?;
                artifactory_runtime_protocol::service::endpoint(&query_url)?;
                let token = required("ARTIFACTORY_TELEMETRY_TOKEN")?;
                ensure!(
                    token.len() >= 32,
                    "telemetry service token must contain at least 32 characters"
                );
                Some(TelemetrySettings {
                    endpoint,
                    query_url,
                    token,
                })
            }
            _ => None,
        };
        let master_key: [u8; 32] = decoded.try_into().map_err(|_| {
            anyhow::anyhow!("ARTIFACTORY_MASTER_KEY must contain 32 base64-encoded bytes")
        })?;
        Ok(Self {
            control: ControlLimits::from_env()?,
            telemetry,
            database_url: required("DATABASE_URL")?,
            http_bind: env::var("ARTIFACTORY_HTTP_BIND")
                .unwrap_or_else(|_| "127.0.0.1:8080".into()),
            grpc_bind: env::var("ARTIFACTORY_GRPC_BIND")
                .unwrap_or_else(|_| "127.0.0.1:8443".into()),
            public_url: public_url.trim_end_matches('/').into(),
            insecure_dev,
            registration_enabled: parse_registration_enabled(
                env::var("ARTIFACTORY_REGISTRATION_ENABLED").ok().as_deref(),
            )?,
            pki_dir: env::var("ARTIFACTORY_PKI_DIR")
                .unwrap_or_else(|_| ".local/pki".into())
                .into(),
            data_dir: env::var("ARTIFACTORY_DATA_DIR")
                .unwrap_or_else(|_| ".local/control-plane".into())
                .into(),
            master_key,
            signing_keys: env::var_os("ARTIFACTORY_SIGNING_KEYS_FILE")
                .filter(|p| !p.is_empty())
                .map(|p| crate::signing::KeyRing::load(std::path::Path::new(&p)))
                .transpose()?,
            s3_endpoint,
            s3_public_endpoint,
            s3_bucket: env::var("ARTIFACTORY_S3_BUCKET").unwrap_or_else(|_| "artifactory".into()),
            s3_region: env::var("ARTIFACTORY_S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
            s3_access_key: required("ARTIFACTORY_S3_ACCESS_KEY")?,
            s3_secret_key: required("ARTIFACTORY_S3_SECRET_KEY")?,
        })
    }
}

fn validate_endpoint(endpoint: &str, allow_http: bool) -> Result<()> {
    let url = url::Url::parse(endpoint)?;
    ensure!(
        url.host_str().is_some()
            && (url.scheme() == "https" || (allow_http && url.scheme() == "http")),
        "HTTPS is required unless this fixed endpoint explicitly allows HTTP"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "endpoint must not contain credentials"
    );
    Ok(())
}

fn parse_registration_enabled(value: Option<&str>) -> Result<bool> {
    value
        .unwrap_or("true")
        .parse()
        .context("ARTIFACTORY_REGISTRATION_ENABLED must be true or false")
}

#[cfg(test)]
mod tests {
    use super::{parse_registration_enabled, validate_endpoint};

    #[test]
    fn private_http_requires_an_explicit_endpoint_policy() {
        assert!(validate_endpoint("http://s3.internal:8333", false).is_err());
        assert!(validate_endpoint("http://s3.internal:8333", true).is_ok());
        assert!(validate_endpoint("https://s3.internal", false).is_ok());
        assert!(validate_endpoint("http://user:secret@s3.internal", true).is_err());
    }

    #[test]
    fn registration_defaults_on_and_accepts_explicit_boolean_values() {
        assert!(parse_registration_enabled(None).unwrap());
        assert!(parse_registration_enabled(Some("true")).unwrap());
        assert!(!parse_registration_enabled(Some("false")).unwrap());
        for value in ["", "1", "TRUE", "invalid"] {
            assert!(parse_registration_enabled(Some(value)).is_err());
        }
    }
}
