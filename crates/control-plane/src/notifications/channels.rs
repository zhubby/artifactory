//! [INPUT]: 依赖事件目录、持久队列和平台邮件配置。
//! [OUTPUT]: 提供邮件绑定、Webhook 与后台投递入口。
//! [POS]: 通知外部渠道边界；业务事务不执行网络请求。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod email;
mod runtime;
pub mod webhook;
pub use runtime::{tick, tick_channel};

pub(crate) fn bad(message: &str) -> crate::error::ApiError {
    crate::error::ApiError::BadRequest(message.into())
}
pub(crate) fn subscribed(preferences: &serde_json::Value, category: &str) -> bool {
    preferences
        .get("email")
        .and_then(|v| v.get(category))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
pub(crate) const CATEGORIES: &[&str] = &[
    "build",
    "release",
    "permission",
    "device",
    "credential",
    "account",
    "workspace",
    "delivery",
];
