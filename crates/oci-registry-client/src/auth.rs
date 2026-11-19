//! [INPUT]: 依赖 Registry 鉴权挑战与明确配置的登录凭据。
//! [OUTPUT]: 解析 Bearer 参数并校验认证和上传目标。
//! [POS]: OCI HTTP 客户端的凭据发送边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn challenge(s: &str) -> Result<BTreeMap<String, String>> {
    let (scheme, value) = s
        .split_once(' ')
        .ok_or_else(|| anyhow::anyhow!("invalid registry challenge"))?;
    ensure!(
        scheme.eq_ignore_ascii_case("bearer"),
        "unsupported registry challenge"
    );
    let mut result = BTreeMap::new();
    let mut quoted = false;
    let mut start = 0;
    let mut fields = Vec::new();
    for (i, c) in value.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == ',' && !quoted {
            fields.push(&value[start..i]);
            start = i + 1;
        }
    }
    ensure!(!quoted, "invalid registry challenge quotes");
    fields.push(&value[start..]);
    for field in fields {
        let (k, v) = field
            .trim()
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid challenge field"))?;
        ensure!(
            v.starts_with('"') && v.ends_with('"') && v.len() >= 2,
            "invalid challenge value"
        );
        ensure!(
            result
                .insert(k.to_owned(), v[1..v.len() - 1].to_owned())
                .is_none(),
            "duplicate challenge parameter"
        );
    }
    ensure!(result.contains_key("realm"), "missing registry realm");
    Ok(result)
}
pub fn realm(s: &str, insecure: bool) -> Result<url::Url> {
    let url = url::Url::parse(s)?;
    ensure!(
        (url.scheme() == "https" || (insecure && url.scheme() == "http"))
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "invalid registry realm"
    );
    Ok(url)
}
pub fn upload_location(origin: &url::Url, location: &str) -> Result<url::Url> {
    let url = origin.join(location)?;
    ensure!(
        url.origin() == origin.origin()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.path().starts_with("/v2/"),
        "upload location escaped registry"
    );
    Ok(url)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_challenge_and_upload_origin_preserve_authority() {
        let c = challenge("Bearer realm=\"https://auth.example/token?a=1,2\",service=\"registry\"")
            .unwrap();
        assert_eq!(c["realm"], "https://auth.example/token?a=1,2");
        let origin = url::Url::parse("https://registry.example/").unwrap();
        assert!(upload_location(&origin, "https://evil.example/upload").is_err());
        assert!(upload_location(&origin, "/v2/app/blobs/uploads/id?_state=opaque").is_ok());
    }
}
