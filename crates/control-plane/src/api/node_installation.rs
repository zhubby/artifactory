//! [INPUT]: 依赖真实账户授权、签名发行、设备 CA 和网关票据。
//! [OUTPUT]: 提供系统设备地址、安装选项、短期材料与固定安装脚本。
//! [POS]: 首次安装的 HTTP 边界；材料就绪后才签发单用凭证。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{AccountAuth, Auth, TenantState},
    crypto,
    error::{ApiError, Result},
    ota::{self, Manifest, ReleaseSource},
    state::AppState,
};
use axum::{Json, extract::State, http::header, response::IntoResponse};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::Ipv6Addr;
use uuid::Uuid;

const SHELL_INSTALLER: &str = include_str!("../installation/installer.sh");
const POWERSHELL_INSTALLER: &str = include_str!("../installation/installer.ps1");

fn digest(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}

fn endpoint_valid(endpoint: &str) -> bool {
    if endpoint.is_empty()
        || endpoint.len() > 320
        || endpoint
            .bytes()
            .any(|c| c.is_ascii_whitespace() || c.is_ascii_control())
    {
        return false;
    }
    let Some((host, port)) = endpoint.rsplit_once(':') else {
        return false;
    };
    if port.is_empty()
        || port.starts_with('0')
        || port.len() > 5
        || !port.bytes().all(|c| c.is_ascii_digit())
        || !port.parse::<u16>().is_ok_and(|p| p > 0)
    {
        return false;
    }
    if let Some(ip) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return ip.parse::<Ipv6Addr>().is_ok();
    }
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            && label
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    })
}

fn installation_keys(keys: &[String]) -> Result<Vec<String>> {
    let valid: Vec<_> = keys
        .iter()
        .filter(|key| {
            key.len() == 44
                && STANDARD
                    .decode(key)
                    .ok()
                    .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
                    .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok())
                    .is_some()
        })
        .cloned()
        .collect();
    if valid.is_empty() {
        return Err(ApiError::Conflict(
            "Emissary release verification keys are not configured".into(),
        ));
    }
    Ok(valid)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsInput {
    device_endpoint: String,
}

pub async fn settings(TenantState(state): TenantState, auth: Auth) -> Result<impl IntoResponse> {
    auth.session_role("admin")?;
    let can_edit = state.store.system_administrator(auth.user_id).await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "device_endpoint":state.store.device_public_endpoint().await?, "can_edit":can_edit,
        })),
    ))
}

pub async fn update_settings(
    State(state): State<AppState>,
    account: AccountAuth,
    Json(input): Json<SettingsInput>,
) -> Result<impl IntoResponse> {
    if !state.store.system_administrator(account.user_id).await? {
        return Err(ApiError::Forbidden);
    }
    if !input.device_endpoint.is_empty() && !endpoint_valid(&input.device_endpoint) {
        return Err(ApiError::BadRequest(
            "device endpoint must be host:port or [IPv6]:port".into(),
        ));
    }
    state
        .store
        .set_device_public_endpoint(account.user_id, &input.device_endpoint)
        .await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"device_endpoint":input.device_endpoint,"can_edit":true})),
    ))
}

fn verified_manifest(
    release: &crate::store::dto::EmissaryRelease,
    keys: &[String],
) -> Result<Manifest> {
    let manifest =
        ota::verify(release.manifest.as_bytes(), &release.signature, keys).map_err(|_| {
            ApiError::Conflict(
                "Emissary release verification failed; import a trusted release".into(),
            )
        })?;
    if digest(release.manifest.as_bytes()) != release.manifest_hash
        || manifest.version != release.version
    {
        return Err(ApiError::Conflict(
            "Emissary release identity does not match its signed manifest".into(),
        ));
    }
    Ok(manifest)
}

pub async fn options(TenantState(state): TenantState, auth: Auth) -> Result<impl IntoResponse> {
    auth.session_role("admin")?;
    let releases = state.store.emissary_releases().await?;
    let mut available = Vec::new();
    if !releases.is_empty() {
        let source = ReleaseSource::load(&state).await.map_err(|_| {
            ApiError::Conflict("Emissary release verification keys are not configured".into())
        })?;
        let keys = installation_keys(&source.public_keys)?;
        for release in releases {
            let Ok(manifest) = verified_manifest(&release, &keys) else {
                continue;
            };
            let platforms: Vec<_> = manifest
                .assets
                .iter()
                .map(|a| format!("{}/{}", a.os, a.arch))
                .collect();
            available
                .push(json!({"id":release.id,"version":release.version,"platforms":platforms}));
        }
    }
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            json!({"device_endpoint":state.store.device_public_endpoint().await?,"releases":available}),
        ),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationInput {
    pool_id: Uuid,
    release_id: Uuid,
    os: String,
    arch: String,
}

pub async fn create(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<InstallationInput>,
) -> Result<impl IntoResponse> {
    auth.session_role("admin")?;
    let platform = format!("{}/{}", input.os, input.arch);
    if ![
        "linux/amd64",
        "linux/arm64",
        "darwin/amd64",
        "darwin/arm64",
        "windows/amd64",
        "windows/arm64",
    ]
    .contains(&platform.as_str())
    {
        return Err(ApiError::BadRequest(
            "unsupported installation platform".into(),
        ));
    }
    let endpoint = state.store.device_public_endpoint().await?;
    if !endpoint_valid(&endpoint) {
        return Err(ApiError::Conflict(
            "configure the device public endpoint in system settings first".into(),
        ));
    }
    state.store.installation_pool(input.pool_id).await?;
    let release = state.store.installation_release(input.release_id).await?;
    let source = ReleaseSource::load(&state).await.map_err(|_| {
        ApiError::Conflict("Emissary release verification keys are not configured".into())
    })?;
    let keys = installation_keys(&source.public_keys)?;
    let manifest = verified_manifest(&release, &keys)?;
    let asset = manifest
        .assets
        .iter()
        .find(|a| a.os == input.os && a.arch == input.arch)
        .ok_or_else(|| ApiError::Conflict("the release has no asset for this platform".into()))?;
    let ca = tokio::fs::read_to_string(state.settings.pki_dir.join("ca.pem"))
        .await
        .map_err(|_| ApiError::Conflict("the device CA is unavailable".into()))?;
    valid_ca(&ca)?;
    let object = format!("emissary-releases/{}/{}", release.manifest_hash, asset.name);
    // 票据按整秒签发；凭证截止不晚于随后签发的下载票据。
    let deadline = DateTime::from_timestamp(Utc::now().timestamp() + 900, 0)
        .ok_or_else(|| ApiError::Conflict("invalid installation deadline".into()))?;
    let download_url = state.storage.emissary_download_url(&object)?;
    let (installer, extension) = if input.os == "windows" {
        (POWERSHELL_INSTALLER, "ps1")
    } else {
        (SHELL_INSTALLER, "sh")
    };
    let installer_url = format!(
        "{}/api/v1/node-installation/installer.{extension}",
        state.settings.public_url.trim_end_matches('/')
    );
    // 最后签发凭证；读取、验证或构造材料失败时不留下不可用的凭证。
    let token = crypto::token();
    let expires_at = state
        .store
        .installation_credential(
            Uuid::new_v4(),
            &crypto::hash(&token),
            input.pool_id,
            input.release_id,
            (&input.os, &input.arch),
            deadline,
        )
        .await?;
    let response: Value = json!({
        "token":token,"expires_at":expires_at,"device_endpoint":endpoint,
        "ca_sha256":digest(ca.as_bytes()),"ca_pem":ca,
        "version":manifest.version,"os":input.os,"arch":input.arch,
        "download_url":download_url,"sha256":asset.sha256,"size":asset.size,
        "public_keys":keys,"installer_url":installer_url,"installer_sha256":digest(installer.as_bytes()),
    });
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)))
}

fn valid_ca(pem: &str) -> Result<()> {
    use x509_parser::{pem::parse_x509_pem, prelude::FromDer};
    if pem.len() > 65536 {
        return Err(ApiError::Conflict("the device CA exceeds 64 KiB".into()));
    }
    let (_, certificate) = parse_x509_pem(pem.as_bytes())
        .map_err(|_| ApiError::Conflict("the device CA is invalid".into()))?;
    let (_, certificate) =
        x509_parser::certificate::X509Certificate::from_der(&certificate.contents)
            .map_err(|_| ApiError::Conflict("the device CA is invalid".into()))?;
    if !certificate.is_ca() {
        return Err(ApiError::Conflict(
            "the device CA certificate must permit certificate signing".into(),
        ));
    }
    if !certificate.validity().is_valid() {
        return Err(ApiError::Conflict(
            "the device CA has expired or is not valid yet".into(),
        ));
    }
    Ok(())
}

pub async fn shell_installer() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        SHELL_INSTALLER,
    )
}

pub async fn powershell_installer() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        POWERSHELL_INSTALLER,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_require_explicit_port_and_reject_shell_or_url_syntax() {
        for valid in [
            "agents.example.com:31443",
            "localhost:8443",
            "192.168.1.2:443",
            "[2001:db8::1]:8443",
            "[::1]:1",
        ] {
            assert!(endpoint_valid(valid), "{valid}");
        }
        for invalid in [
            "",
            "example.com",
            "https://a:443",
            "a:0",
            "a:0443",
            "a:000000000000443",
            "a:65536",
            "a:+443",
            "a:443/path",
            "a:443?x=1",
            "a@b:443",
            "a:443\n",
            "2001:db8::1:443",
            "a;echo:443",
            "a:443 ",
            "a..b:443",
            "-a:443",
            "a_:443",
        ] {
            assert!(!endpoint_valid(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn installation_rejects_leaf_certificate_as_device_ca() {
        let leaf = rcgen::generate_simple_self_signed(vec!["agents.example.com".into()]).unwrap();
        assert!(valid_ca(&leaf.cert.pem()).is_err());
        assert!(valid_ca(&"x".repeat(65537)).is_err());
    }
}
