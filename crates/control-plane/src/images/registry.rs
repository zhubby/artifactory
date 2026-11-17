//! [INPUT]: 依赖独立仓库 RSA 密钥、平台 API 令牌和镜像执行授权。
//! [OUTPUT]: 校验 RSA 签名材料并签发 Distribution 仓库范围 JWT。
//! [POS]: 内建仓库认证入口；不签发外部仓库凭据。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Config;
use crate::{
    crypto,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{RawQuery, State},
    http::{HeaderMap, header},
};
use serde_json::{Value, json};
use uuid::Uuid;
pub fn basic(headers: &HeaderMap) -> Result<(String, String)> {
    use base64::Engine;
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .ok_or(ApiError::Unauthorized)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ApiError::Unauthorized)?;
    let value = String::from_utf8(bytes).map_err(|_| ApiError::Unauthorized)?;
    let (user, password) = value.split_once(':').ok_or(ApiError::Unauthorized)?;
    if password.len() > 4096 {
        return Err(ApiError::Unauthorized);
    }
    Ok((user.into(), password.into()))
}
pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>> {
    let config = Config::load()?;
    let scope = requested_scope(query.as_deref().unwrap_or_default(), &config.service)?;
    let (username, password) = basic(&headers)?;
    let execution = if let Ok(id) = username.parse::<Uuid>() {
        state
            .store
            .image_token(id, &password)
            .await?
            .map(|publisher| (id, publisher))
    } else {
        None
    };
    let mut access = Vec::new();
    let mut ttl = 60u32;
    let subject;
    if let Some((id, publisher)) = execution {
        if !state.store.image_live(id).await? {
            return Err(ApiError::Unauthorized);
        }
        let e = state.store.image_execution(id).await?;
        ttl = ttl.min(e.timeout_seconds);
        subject = id.to_string();
        let cache = format!("{}/{}/cache/{}-", e.tenant_id, e.project_id, e.spec.name);
        for item in scope.split_whitespace() {
            let (repo, actions) = repository_scope(item)?;
            let candidate = repo == e.candidate_repository;
            let cache = repo == format!("{cache}amd64") || repo == format!("{cache}arm64");
            let final_repo =
                publisher && e.destination_credential.is_none() && repo == e.destination_repository;
            if !(candidate || (!publisher && cache) || final_repo) {
                return Err(ApiError::Forbidden);
            }
            if e.destination_credential.is_some() && actions.iter().any(|s| *s != "pull") {
                return Err(ApiError::Forbidden);
            }
            access.push(json!({"type":"repository","name":repo,"actions":actions}));
        }
    } else {
        let mut auth = state
            .store
            .authenticate_token(&crypto::hash(&password))
            .await?
            .ok_or(ApiError::Unauthorized)?;
        let tenant = auth.tenant_id;
        state.store.authorize_tenant(&mut auth, tenant).await?;
        subject = auth.user_id.to_string();
        for item in scope.split_whitespace() {
            let (repo, actions) = repository_scope(item)?;
            let parts: Vec<_> = repo.split('/').collect();
            if parts.len() != 4 || parts[0] != auth.tenant_id.to_string() || parts[2] != "images" {
                return Err(ApiError::Forbidden);
            }
            let project = Uuid::parse_str(parts[1]).map_err(|_| ApiError::Forbidden)?;
            auth.project(project, "read")?;
            if actions.contains(&"push") {
                auth.project(project, "build")?;
                state
                    .store
                    .ensure_writable(tenant, Some("projects"), Some(project))
                    .await?;
            }
            access.push(json!({"type":"repository","name":repo,"actions":actions}));
        }
    }
    let now = chrono::Utc::now().timestamp();
    let claims = json!({"iss":config.issuer,"sub":subject,"aud":config.service,"exp":now+i64::from(ttl),"nbf":now-5,"iat":now,"jti":Uuid::new_v4(),"access":access});
    let key_bytes = std::fs::read(&config.signing_key).map_err(anyhow::Error::from)?;
    let cert_bytes = std::fs::read(&config.signing_cert).map_err(anyhow::Error::from)?;
    let token = sign(&claims, &key_bytes, &cert_bytes)?;
    Ok(Json(
        json!({"token":token,"access_token":token,"expires_in":ttl,"issued_at":chrono::Utc::now().to_rfc3339()}),
    ))
}
fn sign(claims: &Value, key_bytes: &[u8], cert_bytes: &[u8]) -> Result<String> {
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(key_bytes).map_err(anyhow::Error::from)?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_bytes)
        .map_err(|_| anyhow::anyhow!("invalid registry signing certificate"))?;
    let cert = pem
        .parse_x509()
        .map_err(|_| anyhow::anyhow!("invalid registry signing certificate"))?;
    let kid = key_id(cert.public_key().raw);
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid);
    Ok(jsonwebtoken::encode(&header, claims, &key).map_err(anyhow::Error::from)?)
}
pub(super) fn validate_signing_material(key_bytes: &[u8], cert_bytes: &[u8]) -> anyhow::Result<()> {
    let claims = json!({"exp":chrono::Utc::now().timestamp()+60});
    let token = sign(&claims, key_bytes, cert_bytes)?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_bytes)
        .map_err(|_| anyhow::anyhow!("invalid registry signing certificate"))?;
    let cert = pem
        .parse_x509()
        .map_err(|_| anyhow::anyhow!("invalid registry signing certificate"))?;
    let key =
        jsonwebtoken::DecodingKey::from_rsa_der(cert.public_key().subject_public_key.data.as_ref());
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.validate_aud = false;
    jsonwebtoken::decode::<Value>(&token, &key, &validation)
        .map_err(|_| anyhow::anyhow!("registry signing key does not match certificate"))?;
    Ok(())
}
fn requested_scope(query: &str, service: &str) -> Result<String> {
    if query.len() > 8192 {
        return Err(ApiError::BadRequest("registry query too large".into()));
    }
    let pairs: Vec<_> = url::form_urlencoded::parse(query.as_bytes()).collect();
    if pairs
        .iter()
        .filter(|(key, _)| key == "service")
        .any(|(_, value)| value != service)
    {
        return Err(ApiError::Forbidden);
    }
    let scope = pairs
        .iter()
        .filter(|(key, _)| key == "scope")
        .map(|(_, value)| value.as_ref())
        .collect::<Vec<_>>()
        .join(" ");
    if scope.len() > 4096 {
        return Err(ApiError::BadRequest("registry scope too large".into()));
    }
    Ok(scope)
}
fn repository_scope(scope: &str) -> Result<(&str, Vec<&str>)> {
    let mut parts = scope.split(':');
    if parts.next() != Some("repository") {
        return Err(ApiError::Forbidden);
    }
    let repo = parts.next().ok_or(ApiError::Forbidden)?;
    let actions: Vec<_> = parts
        .next()
        .ok_or(ApiError::Forbidden)?
        .split(',')
        .collect();
    if parts.next().is_some()
        || !artifactory_image_protocol::repository(repo)
        || actions.is_empty()
        || actions.iter().any(|a| !["pull", "push"].contains(a))
    {
        return Err(ApiError::Forbidden);
    }
    Ok((repo, actions))
}
fn key_id(der: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(der);
    let bytes = &digest[..30];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut bits = 0u32;
    let mut count = 0;
    for byte in bytes {
        bits = (bits << 8) | u32::from(*byte);
        count += 8;
        while count >= 5 {
            count -= 5;
            if !out.is_empty() && out.len() % 5 == 4 {
                out.push(':')
            }
            out.push(alphabet[((bits >> count) & 31) as usize] as char);
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_scope_rejects_catalog_delete_and_escapes() {
        for s in [
            "registry:catalog:*",
            "repository:a:delete",
            "repository:../a:pull",
            "repository:a:pull:push",
        ] {
            assert!(repository_scope(s).is_err());
        }
        assert!(repository_scope("repository:a/b:pull,push").is_ok());
    }
    #[test]
    fn libtrust_id_uses_twelve_base32_groups() {
        let id = key_id(b"public key DER");
        assert_eq!(id.len(), 59);
        assert_eq!(id.split(':').count(), 12);
        assert!(id.split(':').all(|s| s.len() == 4));
    }
    #[test]
    fn repeated_scope_parameters_preserve_all_requested_repositories() {
        let scope = requested_scope(
            "service=registry&scope=repository%3Aa%3Apull&scope=repository%3Ab%3Apull%2Cpush",
            "registry",
        )
        .unwrap();
        assert_eq!(scope, "repository:a:pull repository:b:pull,push");
        assert!(requested_scope("service=other&scope=repository%3Aa%3Apull", "registry").is_err());
        assert!(requested_scope("service=registry&service=other", "registry").is_err());
    }
    #[test]
    fn registry_certificate_verifies_claims_and_rejects_wrong_audience_and_expiry() {
        let directory = tempfile::tempdir().unwrap();
        let command = |args: &[&str]| {
            let output = std::process::Command::new("openssl")
                .current_dir(directory.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        command(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "signing.key",
            "-out",
            "signing.crt",
            "-days",
            "1",
            "-subj",
            "/CN=isolated registry signing test",
        ]);
        let key = std::fs::read(directory.path().join("signing.key")).unwrap();
        let cert = std::fs::read(directory.path().join("signing.crt")).unwrap();
        validate_signing_material(&key, &cert).unwrap();
        assert!(validate_signing_material(b"invalid key", &cert).is_err());
        assert!(validate_signing_material(&key, b"invalid certificate").is_err());
        command(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "other.key",
            "-out",
            "other.crt",
            "-days",
            "1",
            "-subj",
            "/CN=other registry signing test",
        ]);
        let other_cert = std::fs::read(directory.path().join("other.crt")).unwrap();
        assert!(validate_signing_material(&key, &other_cert).is_err());
        let public = command(&["x509", "-in", "signing.crt", "-pubkey", "-noout"]);
        let decoding = jsonwebtoken::DecodingKey::from_rsa_pem(&public).unwrap();
        let now = chrono::Utc::now().timestamp();
        let claims = json!({"iss":"artifactory-test","sub":"execution","aud":"registry-test","exp":now+60,"nbf":now-5,"iat":now,"jti":Uuid::new_v4(),"access":[{"type":"repository","name":"tenant/project/images/app","actions":["pull","push"]}]});
        let token = sign(&claims, &key, &cert).unwrap();
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        validation.set_audience(&["registry-test"]);
        validation.set_issuer(&["artifactory-test"]);
        validation.leeway = 0;
        let result = jsonwebtoken::decode::<Value>(&token, &decoding, &validation).unwrap();
        assert_eq!(result.claims, claims);
        assert_eq!(result.header.kid.as_ref().unwrap().split(':').count(), 12);
        validation.set_audience(&["other-registry"]);
        assert!(jsonwebtoken::decode::<Value>(&token, &decoding, &validation).is_err());
        validation.set_audience(&["registry-test"]);
        let mut expired = claims.clone();
        expired["exp"] = json!(now - 1);
        let expired = sign(&expired, &key, &cert).unwrap();
        assert!(jsonwebtoken::decode::<Value>(&expired, &decoding, &validation).is_err());
        let mut modified = token.into_bytes();
        let last = modified.last_mut().unwrap();
        *last = if *last == b'A' { b'B' } else { b'A' };
        assert!(
            jsonwebtoken::decode::<Value>(
                &String::from_utf8(modified).unwrap(),
                &decoding,
                &validation
            )
            .is_err()
        );
    }
}
