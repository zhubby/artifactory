// [INPUT]: 依赖 HTTP 业务错误类型。
// [OUTPUT]: 校验内部 Release 版本字符串。
// [POS]: 为构建创建和发布入口共用版本约束。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::error::{ApiError, Result};

pub(crate) fn validate_release_version(version: &str) -> Result<()> {
    if version.is_empty() || version.len() > 100 || version.chars().any(char::is_control) {
        return Err(ApiError::BadRequest("invalid release version".into()));
    }
    Ok(())
}
