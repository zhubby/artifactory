//! [INPUT]: 依赖事务事件、账户水位和当前授权函数。
//! [OUTPUT]: 保存首次接收版本，幂等聚合并静默更新已有事项。
//! [POS]: 多副本共享的持久收件箱投影器。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::super::ControlStore;
use artifactory_database::{params, sql};
use uuid::Uuid;

impl ControlStore {
    pub async fn project_notifications(&self, batch: i64) -> anyhow::Result<usize> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // Serializing this short database-only batch prevents out-of-order aggregate
        // writes and ensures committed per-account watermarks cannot skip receipts.
        let locked: bool = sql::scalar(
            &mut tx,
            "SELECT pg_try_advisory_xact_lock(hashtextextended('notification-projection:'||current_database()||':'||current_schema(),70236112301))",
            params![],
        )
        .await?;
        if !locked {
            tx.commit().await?;
            return Ok(0);
        }
        let events:Vec<Uuid>=sql::scalars(&mut tx,"SELECT id FROM notification_events WHERE projected_at IS NULL AND next_attempt_at<=clock_timestamp() ORDER BY occurred_at,aggregate_version,id LIMIT $1 FOR UPDATE",params![batch.clamp(1,500)]).await?;
        for event in &events {
            let (kind, version): (String, i32) = sql::tuple(
                &mut tx,
                "SELECT event_type,schema_version FROM notification_events WHERE id=$1",
                params![event],
            )
            .await?;
            if !crate::notifications::catalog::is_supported(&kind, version) {
                sql::execute(&mut tx,"UPDATE notification_events SET projected_at=clock_timestamp(),attempts=attempts+1,last_error='unsupported_event_schema' WHERE id=$1",params![event]).await?;
                continue;
            }
            // 已有事项继续更新状态；取消关注的用户不增加提醒，也不进入外发候选。
            let candidates: Vec<(Uuid, bool, bool)> = sql::tuples(&mut tx, r#"
                SELECT u.id, (u.id=ANY(e.candidate_user_ids) AND
                    (e.resource_type='account_safe' OR COALESCE(np.preferences->'in_app'->>e.category,'true')<>'false')),
                    u.id=ANY(e.candidate_user_ids)
                FROM notification_events e
                CROSS JOIN LATERAL (
                    SELECT unnest(e.candidate_user_ids) AS id
                    UNION SELECT i.user_id FROM notification_inbox i WHERE i.aggregate_key=e.aggregate_key
                ) candidate
                JOIN users u ON u.id=candidate.id
                LEFT JOIN notification_preferences np ON np.user_id=u.id
                WHERE e.id=$1 AND notification_user_can_read(u.id,e.tenant_id,e.project_id,e.resource_type,e.resource_id)
                    AND (e.resource_type='account_safe' OR COALESCE(np.preferences->'in_app'->>e.category,'true')<>'false'
                        OR EXISTS(SELECT 1 FROM notification_inbox i WHERE i.user_id=u.id AND i.aggregate_key=e.aggregate_key))
                ORDER BY u.id
            "#, params![event]).await?;
            for (user, attention_enabled, candidate) in candidates {
                sql::execute(
                    &mut tx,
                    "INSERT INTO notification_accounts(user_id) VALUES($1) ON CONFLICT DO NOTHING",
                    params![user],
                )
                .await?;
                let watermark:i64=sql::scalar(&mut tx,"UPDATE notification_accounts SET watermark=watermark+1 WHERE user_id=$1 RETURNING watermark",params![user]).await?;
                sql::execute(&mut tx,r#"
      INSERT INTO notification_inbox(user_id,aggregate_key,event_id,category,event_type,severity,tenant_id,project_id,resource_type,resource_id,payload,occurred_at,updated_at,content_revision,first_received_revision,attention_revision,attention_watermark,needs_action)
      SELECT $2,e.aggregate_key,e.id,e.category,e.event_type,e.severity,e.tenant_id,e.project_id,e.resource_type,e.resource_id,e.payload,e.occurred_at,e.occurred_at,e.aggregate_version,e.aggregate_version,CASE WHEN attention AND $4 THEN aggregate_version ELSE 0 END,CASE WHEN attention AND $4 THEN $3 ELSE 0 END,e.needs_action FROM notification_events e WHERE e.id=$1
      ON CONFLICT(user_id,aggregate_key) DO UPDATE SET
      event_id=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.event_id ELSE notification_inbox.event_id END,
      category=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.category ELSE notification_inbox.category END,
      event_type=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.event_type ELSE notification_inbox.event_type END,
      severity=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.severity ELSE notification_inbox.severity END,
      tenant_id=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.tenant_id ELSE notification_inbox.tenant_id END,
      project_id=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.project_id ELSE notification_inbox.project_id END,
      resource_type=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.resource_type ELSE notification_inbox.resource_type END,
      resource_id=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.resource_id ELSE notification_inbox.resource_id END,
      payload=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.payload ELSE notification_inbox.payload END,
      occurred_at=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.occurred_at ELSE notification_inbox.occurred_at END,
      updated_at=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN GREATEST(excluded.updated_at,notification_inbox.updated_at) ELSE notification_inbox.updated_at END,
      needs_action=CASE WHEN excluded.content_revision>notification_inbox.content_revision THEN excluded.needs_action ELSE notification_inbox.needs_action END,
      content_revision=GREATEST(excluded.content_revision,notification_inbox.content_revision),
      first_received_revision=CASE WHEN $5 THEN LEAST(excluded.first_received_revision,notification_inbox.first_received_revision) ELSE notification_inbox.first_received_revision END,
      attention_watermark=CASE WHEN excluded.attention_revision>notification_inbox.attention_revision THEN excluded.attention_watermark ELSE notification_inbox.attention_watermark END,
      attention_revision=GREATEST(excluded.attention_revision,notification_inbox.attention_revision)
     "#,params![event,user,watermark,attention_enabled,candidate]).await?;
            }
            sql::execute(&mut tx,"UPDATE notification_events SET projected_at=clock_timestamp(),attempts=attempts+1,last_error=NULL WHERE id=$1",params![event]).await?;
        }
        tx.commit().await?;
        Ok(events.len())
    }
}
