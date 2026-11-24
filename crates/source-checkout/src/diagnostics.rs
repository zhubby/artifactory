//! [INPUT]: 依赖有界 Git stderr 和进程终止结果。
//! [OUTPUT]: 提供不含上游内容的失败分类与安全错误。
//! [POS]: stderr 仅在此分类；凭据和原始诊断不越过进程边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFailureKind {
    Timeout,
    Dns,
    Connection,
    Authentication,
    Tls,
    Reference,
    Canceled,
    Unknown,
}

impl GitFailureKind {
    pub fn message(self) -> &'static str {
        match self {
            Self::Timeout => "Git operation timed out; check source pool egress connectivity",
            Self::Dns => "Git upstream hostname could not be resolved",
            Self::Connection => "Git upstream connection could not be established",
            Self::Authentication => "Git upstream authentication was rejected",
            Self::Tls => "Git upstream TLS or SSH host trust could not be verified",
            Self::Reference => "Git reference or pinned object is unavailable",
            Self::Canceled => "source preparation canceled",
            Self::Unknown => {
                "Git operation failed; inspect source access and repository availability"
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GitFailure(pub(crate) GitFailureKind);

impl GitFailure {
    pub fn kind(&self) -> GitFailureKind {
        self.0
    }
}

impl std::fmt::Display for GitFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0.message())
    }
}
impl std::error::Error for GitFailure {}

pub(crate) fn classify_stderr(bytes: &[u8]) -> GitFailureKind {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if text.contains("could not resolve") || text.contains("name or service not known") {
        GitFailureKind::Dns
    } else if text.contains("authentication failed")
        || text.contains("could not read username")
        || text.contains("permission denied (publickey")
        || text.contains("returned error: 401")
        || text.contains("returned error: 403")
    {
        GitFailureKind::Authentication
    } else if text.contains("certificate")
        || text.contains("host key verification failed")
        || text.contains("ssl connect error")
    {
        GitFailureKind::Tls
    } else if text.contains("timed out") || text.contains("timeout was reached") {
        GitFailureKind::Timeout
    } else if text.contains("failed to connect")
        || text.contains("connection refused")
        || text.contains("connection reset")
        || text.contains("network is unreachable")
    {
        GitFailureKind::Connection
    } else if text.contains("couldn't find remote ref")
        || text.contains("not our ref")
        || text.contains("repository not found")
        || text.contains("unadvertised object")
    {
        GitFailureKind::Reference
    } else {
        GitFailureKind::Unknown
    }
}
