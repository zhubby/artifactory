//! [INPUT]: 依赖持久异常周期、业务状态和通知事件保留约束。
//! [OUTPUT]: 扫描设备与证书异常，分批清理过期通知。
//! [POS]: 通知存储维护；运行时只调用此入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::state::AppState;
use artifactory_database::{params, sql};

pub(crate) async fn tick(state: &AppState) -> anyhow::Result<()> {
    let mut db = state.store.notification_db();
    let _: i32 = sql::scalar(&mut db, "SELECT notification_scan_anomalies()", params![]).await?;
    retain(&db).await
}

async fn retain(pool: &toasty::Db) -> anyhow::Result<()> {
    let mut db = pool.clone();
    // 保留待处理状态；先清收件引用，再清不再被引用的事件。
    sql::execute(&mut db, "DELETE FROM notification_inbox WHERE updated_at<now()-interval '90 days' AND NOT needs_action AND id IN (SELECT id FROM notification_inbox WHERE updated_at<now()-interval '90 days' AND NOT needs_action ORDER BY updated_at LIMIT 500 FOR UPDATE SKIP LOCKED)", params![]).await?;
    sql::execute(&mut db, "DELETE FROM notification_events WHERE id IN (SELECT e.id FROM notification_events e WHERE e.occurred_at<now()-interval '90 days' AND e.projected_at IS NOT NULL AND e.channels_routed_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM notification_inbox i WHERE i.event_id=e.id OR (i.aggregate_key=e.aggregate_key AND i.needs_action)) AND NOT EXISTS(SELECT 1 FROM notification_deliveries d WHERE d.event_id=e.id AND d.status IN ('queued','sending')) ORDER BY e.occurred_at LIMIT 500 FOR UPDATE OF e SKIP LOCKED)",params![]).await?;
    // 项目与租户已清除时，不再保留可识别的业务快照或待发外部任务。
    sql::execute(&mut db, "DELETE FROM notification_inbox WHERE resource_type<>'account_safe' AND ((project_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM projects WHERE id=notification_inbox.project_id)) OR (tenant_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM tenants WHERE id=notification_inbox.tenant_id)))",params![]).await?;
    sql::execute(&mut db, "UPDATE notification_deliveries d SET status='canceled',payload='{}',last_error=NULL,lease_until=NULL WHERE d.status IN ('queued','sending') AND EXISTS(SELECT 1 FROM notification_events e WHERE e.id=d.event_id AND ((e.project_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM projects WHERE id=e.project_id)) OR (e.tenant_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM tenants WHERE id=e.tenant_id))))",params![]).await?;
    sql::execute(&mut db, "UPDATE notification_events e SET payload='{}',candidate_user_ids='{}' WHERE e.resource_type<>'account_safe' AND e.payload<>'{}' AND ((e.project_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM projects WHERE id=e.project_id)) OR (e.tenant_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM tenants WHERE id=e.tenant_id)))",params![]).await?;
    Ok(())
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
