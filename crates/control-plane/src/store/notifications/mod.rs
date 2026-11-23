//! [INPUT]: 依赖持久通知事件、当前项目授权和安全文案目录。
//! [OUTPUT]: 提供个人收件箱、渠道配置、投递租约与维护查询。
//! [POS]: 账户级通知边界，不继承请求的租户作用域。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub(crate) mod channel_delivery;
pub(crate) mod channel_email;
pub(crate) mod channel_webhook;
mod health;
pub(crate) mod maintenance;
mod projection;
mod reads;
use crate::{
    error::{ApiError, Result},
    notifications::catalog::LocalizedText,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) fn db(state: &crate::state::AppState) -> toasty::Db {
    state.store.notification_db()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationItem {
    pub id: Uuid,
    pub category: String,
    pub event_type: String,
    pub severity: String,
    pub title: LocalizedText,
    pub summary: LocalizedText,
    pub tenant_id: Option<Uuid>,
    pub tenant_name: Option<String>,
    pub tenant_slug: Option<String>,
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub project_slug: Option<String>,
    pub resource_id: Option<Uuid>,
    pub resource_type: String,
    pub url: Option<String>,
    pub occurred_at: String,
    pub updated_at: String,
    pub content_revision: i64,
    pub attention_revision: i64,
    pub read_revision: i64,
    pub needs_action: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationSummary {
    pub unread_count: i64,
    pub needs_action_count: i64,
    pub watermark: i64,
}
#[derive(Debug, Serialize)]
pub struct NotificationPage {
    pub items: Vec<NotificationItem>,
    pub next_cursor: Option<String>,
    pub watermark: i64,
    pub unread_count: i64,
    pub needs_action_count: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationScopes {
    pub tenants: Vec<NotificationTenantScope>,
    pub projects: Vec<NotificationProjectScope>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationTenantScope {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationProjectScope {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub tenant_id: Uuid,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationFilter {
    pub category: Option<String>,
    pub tenant_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub unread: Option<bool>,
    pub needs_action: Option<bool>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadAllInput {
    pub watermark: i64,
    pub category: Option<String>,
    pub tenant_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}
#[derive(Debug, Serialize)]
pub struct NotificationDetail {
    pub item: NotificationItem,
    pub events: Vec<NotificationTimelineEvent>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct NotificationTimelineEvent {
    pub id: Uuid,
    pub event_type: String,
    pub title: LocalizedText,
    pub summary: LocalizedText,
    pub occurred_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectSubscription {
    pub following: bool,
}

fn category(value: Option<&str>) -> Result<()> {
    if value.is_some_and(|v| {
        ![
            "build",
            "release",
            "permission",
            "device",
            "credential",
            "account",
            "workspace",
            "delivery",
        ]
        .contains(&v)
    }) {
        return Err(ApiError::BadRequest("invalid notification category".into()));
    }
    Ok(())
}
