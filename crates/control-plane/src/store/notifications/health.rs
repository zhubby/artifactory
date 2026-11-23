//! [INPUT]: 依赖已授权租户标识、通知事件和渠道投递状态。
//! [OUTPUT]: 提供租户隔离的通知队列健康快照。
//! [POS]: 运维查询存储边界；HTTP 层负责管理员会话校验。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::super::ControlStore;
use artifactory_database::{params, sql};
use serde_json::Value;
use uuid::Uuid;

impl ControlStore {
    pub(crate) async fn notification_health(&self, tenant: Uuid) -> anyhow::Result<Value> {
        Ok(sql::scalar(&mut self.notification_db(), r#"
          SELECT jsonb_build_object(
            'pending_events',(SELECT count(*) FROM notification_events WHERE tenant_id=$1 AND projected_at IS NULL),
            'quarantined_events',(SELECT count(*) FROM notification_events WHERE tenant_id=$1 AND last_error IS NOT NULL),
            'oldest_event_age_seconds',COALESCE((SELECT greatest(0,extract(epoch FROM now()-min(occurred_at))) FROM notification_events WHERE tenant_id=$1 AND projected_at IS NULL),0),
            'queued_email',(SELECT count(*) FROM notification_deliveries d JOIN notification_events e ON e.id=d.event_id WHERE e.tenant_id=$1 AND d.channel='email' AND d.status IN ('queued','sending')),
            'queued_webhook',(SELECT count(*) FROM notification_deliveries d JOIN notification_webhooks w ON w.id=d.endpoint_id WHERE w.tenant_id=$1 AND d.status IN ('queued','sending')),
            'retrying_deliveries',(SELECT count(*) FROM notification_deliveries d LEFT JOIN notification_events e ON e.id=d.event_id LEFT JOIN notification_webhooks w ON w.id=d.endpoint_id WHERE (CASE WHEN d.channel='webhook' THEN w.tenant_id ELSE e.tenant_id END)=$1 AND d.status='queued' AND d.attempts>0),
            'failed_deliveries',(SELECT count(*) FROM notification_deliveries d LEFT JOIN notification_events e ON e.id=d.event_id LEFT JOIN notification_webhooks w ON w.id=d.endpoint_id WHERE (CASE WHEN d.channel='webhook' THEN w.tenant_id ELSE e.tenant_id END)=$1 AND d.status='failed'),
            'paused_endpoints',(SELECT count(*) FROM notification_webhooks WHERE tenant_id=$1 AND paused_until>now()))
        "#,params![tenant]).await?)
    }
}
