// [INPUT]: 依赖 HTTP 业务错误与构建目标合同。
// [OUTPUT]: 校验严格 SemVer、比较版本并生成正式交付文件名。
// [POS]: Release 创建、列表和发布共用确定性的版本规则。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::error::{ApiError, Result};
use std::cmp::Ordering;

pub(crate) fn validate_release_version(version: &str) -> Result<()> {
    let invalid = || {
        ApiError::BadRequest("version must be strict SemVer without v prefix or build metadata (max 100 ASCII bytes)".into())
    };
    if version.is_empty() || version.len() > 100 || !version.is_ascii() || version.contains('+') {
        return Err(invalid());
    }
    let (core, pre) = version
        .split_once('-')
        .map_or((version, None), |(c, p)| (c, Some(p)));
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| !numeric(p)) {
        return Err(invalid());
    }
    if pre.is_some_and(|p| {
        p.split('.').any(|s| {
            s.is_empty()
                || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                || (s.bytes().all(|c| c.is_ascii_digit()) && !numeric(s))
        })
    }) {
        return Err(invalid());
    }
    Ok(())
}
fn numeric(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}
fn number_cmp(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
pub(crate) fn compare_versions(a: &str, b: &str) -> Ordering {
    let (ac, ap) = a.split_once('-').map_or((a, None), |(c, p)| (c, Some(p)));
    let (bc, bp) = b.split_once('-').map_or((b, None), |(c, p)| (c, Some(p)));
    for (x, y) in ac.split('.').zip(bc.split('.')) {
        let o = number_cmp(x, y);
        if o != Ordering::Equal {
            return o;
        }
    }
    match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let mut xs = x.split('.');
            let mut ys = y.split('.');
            loop {
                match (xs.next(), ys.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(x), Some(y)) => {
                        let xn = x.bytes().all(|c| c.is_ascii_digit());
                        let yn = y.bytes().all(|c| c.is_ascii_digit());
                        let o = match (xn, yn) {
                            (true, true) => number_cmp(x, y),
                            (true, false) => Ordering::Less,
                            (false, true) => Ordering::Greater,
                            _ => x.cmp(y),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                }
            }
        }
    }
}
pub(crate) fn safe_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
}
pub(crate) fn artifact_suffix(path: &str) -> &str {
    let filename = path.rsplit('/').next().unwrap_or(path);
    if let Some(suffix) = [".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst"]
        .into_iter()
        .find(|suffix| filename.ends_with(suffix))
    {
        suffix
    } else {
        filename
            .rfind('.')
            .map(|index| &filename[index..])
            .unwrap_or(".bin")
    }
}
pub(crate) fn delivery_filename(
    project: &str,
    key: &str,
    version: &str,
    platform: &str,
    arch: &str,
    suffix: &str,
) -> Result<String> {
    validate_release_version(version)?;
    if !safe_key(project)
        || !safe_key(key)
        || !safe_key(platform)
        || !safe_key(arch)
        || suffix.len() > 32
        || !suffix.starts_with('.')
        || suffix
            .split('.')
            .skip(1)
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return Err(ApiError::BadRequest(
            "invalid delivery filename component".into(),
        ));
    }
    let name = format!("{project}-{key}-{version}-{platform}-{arch}{suffix}");
    if name.len() > 255 {
        return Err(ApiError::BadRequest(
            "delivery filename exceeds 255 bytes".into(),
        ));
    }
    Ok(name)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_semver() {
        for v in ["0.0.0", "1.2.3", "1.2.3-rc.1", "999999999999999999999.0.0"] {
            assert!(validate_release_version(v).is_ok(), "{v}");
        }
        for v in [
            "v1.0.0",
            "1.0",
            "01.0.0",
            "1.0.0+build",
            "1.0.0-01",
            "1.0.0-",
            "1.0.0-a..b",
            "1.0.0  ",
            "一.0.0",
        ] {
            assert!(validate_release_version(v).is_err(), "{v}");
        }
    }
    #[test]
    fn semver_order() {
        let v = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "2.0.0",
        ];
        for w in v.windows(2) {
            assert_eq!(compare_versions(w[0], w[1]), Ordering::Less);
        }
    }
    #[test]
    fn delivery_names() {
        assert_eq!(
            delivery_filename("project", "cli", "1.2.3-rc.1", "linux", "amd64", ".tar.gz").unwrap(),
            "project-cli-1.2.3-rc.1-linux-amd64.tar.gz"
        );
        assert!(delivery_filename("project", "../x", "1.0.0", "linux", "amd64", ".zip").is_err());
    }
    #[test]
    fn artifact_suffix_uses_only_the_filename() {
        assert_eq!(artifact_suffix("dist/v1.2/my-cli"), ".bin");
        assert_eq!(artifact_suffix("dist/v1.2/my-cli.zip"), ".zip");
        assert_eq!(artifact_suffix("dist/v1.2/my-cli.tar.gz"), ".tar.gz");
        assert_eq!(
            delivery_filename(
                "project",
                "cli",
                "1.2.3",
                "linux",
                "amd64",
                artifact_suffix("dist/v1.2/my-cli")
            )
            .unwrap(),
            "project-cli-1.2.3-linux-amd64.bin"
        );
    }
}
