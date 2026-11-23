//! [INPUT]: 依赖通知事件、渠道绑定、当前权限和投递租约。
//! [OUTPUT]: 提供事务路由、稳定验证令牌、发送前复核与晚结果围栏。
//! [POS]: 渠道持久化边界；网络发送前提交事务并释放行锁。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::db;
use crate::{crypto, notifications::channels::email, state::AppState};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) struct PreparedDelivery {
    pub binding: Value,
    pub event: Option<Value>,
    pub verification_token: Option<String>,
}

pub(crate) async fn cleanup(state: &AppState) -> anyhow::Result<()> {
    sql::execute(
        &mut db(state),
        "DELETE FROM notification_deliveries WHERE id IN (SELECT id FROM notification_deliveries WHERE created_at<now()-interval '90 days' ORDER BY created_at,id LIMIT 500 FOR UPDATE SKIP LOCKED)",
        params![],
    )
    .await?;
    sql::execute(&mut db(state),"UPDATE notification_deliveries SET last_error=NULL,payload='{}' WHERE id IN (SELECT id FROM notification_deliveries WHERE created_at<now()-interval '30 days' AND (last_error IS NOT NULL OR payload<>'{}'::jsonb) ORDER BY created_at,id LIMIT 500 FOR UPDATE SKIP LOCKED)",params![]).await?;

    Ok(())
}

pub(crate) async fn route(state: &AppState) -> anyhow::Result<()> {
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    let ids:Vec<Uuid>=sql::scalars(&mut tx,"SELECT id FROM notification_events WHERE channels_routed_at IS NULL ORDER BY occurred_at,id LIMIT 64 FOR UPDATE SKIP LOCKED",params![]).await?;
    for id in ids {
        let metadata:Value=sql::scalar(&mut tx,"SELECT jsonb_build_object('type',event_type,'version',schema_version) FROM notification_events WHERE id=$1",params![id]).await?;
        if !crate::notifications::catalog::is_supported(
            metadata["type"].as_str().unwrap_or(""),
            metadata["version"].as_i64().unwrap_or(0) as i32,
        ) {
            sql::execute(&mut tx,"UPDATE notification_events SET channels_routed_at=now(),last_error='unsupported_event_schema' WHERE id=$1",params![id]).await?;
            continue;
        }
        sql::execute(&mut tx,"INSERT INTO notification_deliveries(channel,event_id,user_id,generation,event_type) SELECT 'email',e.id,c.user_id,c.generation,e.event_type FROM notification_events e JOIN notification_email_contacts c ON c.user_id=ANY(e.candidate_user_ids) JOIN notification_preferences p ON p.user_id=c.user_id WHERE e.id=$1 AND e.category<>'delivery' AND (e.attention OR e.needs_action OR e.payload->>'status' IN ('online','resolved','certificate_renewed')) AND c.verified_at<=e.occurred_at AND jsonb_typeof(p.preferences->'email_since'->e.category)='string' AND (p.preferences->'email_since'->>e.category)::timestamptz<=e.occurred_at AND p.preferences->'email'->e.category='true'::jsonb AND notification_user_can_read(c.user_id,e.tenant_id,e.project_id,e.resource_type,e.resource_id) ON CONFLICT DO NOTHING",params![id]).await?;
        sql::execute(&mut tx,"INSERT INTO notification_deliveries(channel,event_id,endpoint_id,generation,event_type) SELECT 'webhook',e.id,w.id,w.generation,e.event_type FROM notification_events e JOIN notification_webhooks w ON w.tenant_id=e.tenant_id AND (w.project_id IS NULL OR w.project_id=e.project_id) WHERE e.id=$1 AND e.category<>'delivery' AND w.enabled AND w.binding_at<=e.occurred_at AND e.event_type=ANY(w.event_types) AND notification_user_can_read(w.created_by,e.tenant_id,e.project_id,e.resource_type,e.resource_id) AND EXISTS(SELECT 1 FROM tenant_members m WHERE m.user_id=w.created_by AND m.tenant_id=w.tenant_id AND (m.role IN ('owner','admin') OR (w.project_id IS NOT NULL AND EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id=w.project_id AND pm.user_id=w.created_by AND pm.role='admin')))) ON CONFLICT DO NOTHING",params![id]).await?;
        sql::execute(
            &mut tx,
            "UPDATE notification_events SET channels_routed_at=now() WHERE id=$1",
            params![id],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

fn uuid(value: &Value, key: &str) -> anyhow::Result<Uuid> {
    Ok(Uuid::parse_str(value[key].as_str().ok_or_else(|| {
        anyhow::anyhow!("invalid delivery identity")
    })?)?)
}

pub(crate) async fn claim(
    state: &AppState,
    channel: &str,
    lease: Uuid,
) -> anyhow::Result<Option<Value>> {
    let job:Option<Value>=sql::optional_scalar(&mut db(state),"WITH due AS (SELECT id FROM notification_deliveries WHERE channel=$2 AND ((status='queued' AND next_attempt_at<=now()) OR (status='sending' AND lease_until<now())) AND NOT EXISTS(SELECT 1 FROM notification_webhooks w WHERE w.id=notification_deliveries.endpoint_id AND w.paused_until>now()) ORDER BY next_attempt_at,id LIMIT 1 FOR UPDATE SKIP LOCKED) UPDATE notification_deliveries d SET status='sending',lease_id=$1,lease_until=now()+interval '90 seconds' FROM due WHERE d.id=due.id RETURNING to_jsonb(d)",params![lease,channel]).await?;
    Ok(job)
}

pub(crate) async fn prepare(
    state: &AppState,
    job: &Value,
    lease: Uuid,
) -> anyhow::Result<Option<PreparedDelivery>> {
    let id = uuid(job, "id")?;
    let channel = job["channel"].as_str().unwrap_or("");
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    let binding: Option<Value> = if channel == "webhook" {
        sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(w) FROM notification_webhooks w WHERE id=$1 FOR UPDATE",
            params![uuid(job, "endpoint_id")?],
        )
        .await?
    } else {
        sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(c) FROM notification_email_contacts c WHERE user_id=$1 FOR UPDATE",
            params![uuid(job, "user_id")?],
        )
        .await?
    };
    let current:Option<Value>=sql::optional_scalar(&mut tx,"SELECT to_jsonb(d) FROM notification_deliveries d WHERE id=$1 AND lease_id=$2 AND status='sending' FOR UPDATE",params![id,lease]).await?;
    if current.is_none() {
        tx.commit().await?;
        return Ok(None);
    }
    let mut notification_locale = "zh-CN".to_owned();
    let mut canceled = binding
        .as_ref()
        .is_none_or(|b| b["generation"] != job["generation"]);
    let event: Option<Value> = if let Some(event) = job["event_id"].as_str() {
        sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(e) FROM notification_events e WHERE id=$1",
            params![Uuid::parse_str(event)?],
        )
        .await?
    } else {
        None
    };
    if job["event_id"].is_string() && event.is_none() {
        canceled = true;
    }
    if binding.is_none() {
        sql::execute(&mut tx,"UPDATE notification_deliveries SET status='canceled',completed_at=now(),payload='{}',lease_until=NULL WHERE id=$1",params![id]).await?;
        tx.commit().await?;
        return Ok(None);
    }
    if let Some(event) = &event {
        canceled |= !crate::notifications::catalog::is_supported(
            event["event_type"].as_str().unwrap_or(""),
            event["schema_version"].as_i64().unwrap_or(0) as i32,
        );
        let b = binding.as_ref().unwrap_or(&Value::Null);
        let user = if channel == "webhook" {
            uuid(b, "created_by")?
        } else {
            uuid(job, "user_id")?
        };
        let tenant = event["tenant_id"]
            .as_str()
            .map(Uuid::parse_str)
            .transpose()?;
        let project = event["project_id"]
            .as_str()
            .map(Uuid::parse_str)
            .transpose()?;
        let resource = event["resource_id"]
            .as_str()
            .map(Uuid::parse_str)
            .transpose()?;
        let allowed: bool = sql::scalar(
            &mut tx,
            "SELECT notification_user_can_read($1,$2,$3,$4,$5)",
            params![
                user,
                tenant,
                project,
                event["resource_type"].as_str().unwrap_or(""),
                resource
            ],
        )
        .await?;
        canceled |= !allowed;
        if channel == "email" {
            canceled |= !event["candidate_user_ids"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(user)));
            let preferences: Option<Value> = sql::optional_scalar(
                &mut tx,
                "SELECT preferences FROM notification_preferences WHERE user_id=$1",
                params![user],
            )
            .await?;
            notification_locale = preferences
                .as_ref()
                .and_then(|p| p["locale"].as_str())
                .unwrap_or("zh-CN")
                .to_owned();
            let eligible:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM notification_preferences p JOIN notification_email_contacts c ON c.user_id=p.user_id JOIN notification_events e ON e.id=$2 WHERE p.user_id=$1 AND c.verified_at<=e.occurred_at AND jsonb_typeof(p.preferences->'email_since'->e.category)='string' AND (p.preferences->'email_since'->>e.category)::timestamptz<=e.occurred_at)",params![user,uuid(event,"id")?]).await?;
            canceled |= !eligible;
            let obsolete:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM notification_events old JOIN notification_events newer ON newer.aggregate_key=old.aggregate_key AND newer.aggregate_version>old.aggregate_version WHERE old.id=$1 AND (old.needs_action OR old.payload->>'status' IN ('needs_input','awaiting_input','needs_clarification','offline','certificate_expired','expired','expiring')) AND (newer.payload->>'status' IN ('succeeded','failed','canceled','completed','published','online','resolved','certificate_renewed','revoked','deleted','monitoring_suspended') OR (old.needs_action AND newer.event_type=old.event_type AND NOT newer.needs_action)))",params![uuid(event,"id")?]).await?;
            canceled |= obsolete;

            canceled |= !preferences.as_ref().is_some_and(|p| {
                crate::notifications::channels::subscribed(
                    p,
                    event["category"].as_str().unwrap_or(""),
                )
            });
        } else {
            canceled |= b["tenant_id"] != event["tenant_id"]
                || (b["project_id"].is_string() && b["project_id"] != event["project_id"]);
            let admin:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM tenant_members m WHERE m.user_id=$1 AND m.tenant_id=$2 AND (m.role IN ('owner','admin') OR ($3::uuid IS NOT NULL AND EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id=$3 AND pm.user_id=$1 AND pm.role='admin'))))",params![user,tenant,b["project_id"].as_str().map(Uuid::parse_str).transpose()?]).await?;
            canceled |= !admin
                || !b["event_types"]
                    .as_array()
                    .is_some_and(|types| types.contains(&event["event_type"]));
        }
    }
    if channel == "webhook" {
        let b = binding.as_ref().unwrap();
        let admin:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM tenant_members m JOIN tenants t ON t.id=m.tenant_id WHERE m.user_id=$1 AND m.tenant_id=$2 AND t.status<>'deleting' AND (m.role IN ('owner','admin') OR ($3::uuid IS NOT NULL AND EXISTS(SELECT 1 FROM project_members pm JOIN projects p ON p.id=pm.project_id WHERE pm.project_id=$3 AND pm.user_id=$1 AND pm.role='admin' AND p.tenant_id=m.tenant_id AND p.status<>'deleting'))))",params![uuid(b,"created_by")?,uuid(b,"tenant_id")?,b["project_id"].as_str().map(Uuid::parse_str).transpose()?]).await?;
        canceled |= !admin;
    }
    if let Some(b) = &binding {
        if channel == "webhook" {
            canceled |= b["enabled"] != true && job["event_type"] != "notification.test";
        } else if channel == "email" {
            canceled |= b["verified_at"].is_null();
        } else {
            let valid:bool=sql::scalar(&mut tx,"SELECT token_hash IS NOT NULL AND token_expires_at>now() FROM notification_email_contacts WHERE user_id=$1",params![uuid(job,"user_id")?]).await?;
            canceled |= !valid;
        }
    }
    let expired:bool=sql::scalar(&mut tx,"SELECT attempts>=10 OR created_at<now()-interval '24 hours' FROM notification_deliveries WHERE id=$1",params![id]).await?;
    if canceled || expired {
        sql::execute(&mut tx,"UPDATE notification_deliveries SET status=$2,completed_at=now(),payload='{}',lease_until=NULL WHERE id=$1",params![id,if canceled {"canceled"}else{"failed"}]).await?;
        tx.commit().await?;
        return Ok(None);
    }
    let verification_token = if channel == "verification" {
        let user = uuid(job, "user_id")?;
        let generation = job["generation"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("invalid delivery generation"))?;
        let token = email::verification_token(&state.settings.master_key, user, generation, id);
        // 同一任务重试保留已发链接，不延长绑定请求的验证期限。
        sql::execute(
            &mut tx,
            "UPDATE notification_email_contacts SET token_hash=$2 WHERE user_id=$1",
            params![user, crypto::hash(&token)],
        )
        .await?;
        Some(token)
    } else {
        None
    };
    // 先提交验证摘要和发送围栏；网络等待不得持有业务或绑定行锁。
    tx.commit().await?;
    let mut binding = binding.unwrap();
    binding["notification_locale"] = json!(notification_locale);
    Ok(Some(PreparedDelivery {
        binding,
        event,
        verification_token,
    }))
}

pub(crate) async fn finish(
    state: &AppState,
    job: &Value,
    lease: Uuid,
    outcome: Result<(), bool>,
) -> anyhow::Result<()> {
    let id = uuid(job, "id")?;
    let channel = job["channel"].as_str().unwrap_or("");
    let mut conn = db(state);
    // 与配置变更保持绑定行→投递行的锁顺序。过期租约和旧绑定的晚结果不落库。
    let mut tx = conn.transaction().await?;
    let current_generation: Option<i64> = if channel == "webhook" {
        sql::optional_scalar(
            &mut tx,
            "SELECT generation FROM notification_webhooks WHERE id=$1 FOR UPDATE",
            params![uuid(job, "endpoint_id")?],
        )
        .await?
    } else {
        sql::optional_scalar(
            &mut tx,
            "SELECT generation FROM notification_email_contacts WHERE user_id=$1 FOR UPDATE",
            params![uuid(job, "user_id")?],
        )
        .await?
    };
    let current: Option<Uuid> = sql::optional_scalar(&mut tx,"SELECT id FROM notification_deliveries WHERE id=$1 AND lease_id=$2 AND status='sending' AND generation=$3 FOR UPDATE",params![id,lease,job["generation"].as_i64().unwrap_or(0)]).await?;
    if current.is_none() {
        tx.commit().await?;
        return Ok(());
    }
    if current_generation != job["generation"].as_i64() {
        sql::execute(&mut tx,"UPDATE notification_deliveries SET status='canceled',completed_at=now(),payload='{}',lease_until=NULL WHERE id=$1",params![id]).await?;
        tx.commit().await?;
        return Ok(());
    }
    let attempts = job["attempts"].as_i64().unwrap_or(0) + 1;
    let (status, error) = match outcome {
        Ok(()) => ("succeeded", None),
        Err(retry) if retry && attempts < 10 => ("queued", Some("temporary_delivery_failure")),
        Err(_) => ("failed", Some("delivery_rejected")),
    };
    let delay = retry_seconds(attempts);
    sql::execute(&mut tx,"UPDATE notification_deliveries SET status=$2,attempts=attempts+1,next_attempt_at=now()+make_interval(secs=>$3),last_error=$4,lease_until=NULL,completed_at=CASE WHEN $2 IN ('succeeded','failed') THEN now() ELSE NULL END,payload=CASE WHEN $2 IN ('succeeded','failed') THEN '{}'::jsonb ELSE payload END WHERE id=$1",params![id,status,delay,error]).await?;
    if channel == "webhook" {
        sql::execute(
            &mut tx,
            "UPDATE notification_webhooks SET last_status=CASE WHEN $2<>'succeeded' AND consecutive_failures+1>=5 THEN 'paused' ELSE $2 END,consecutive_failures=CASE WHEN $2='succeeded' THEN 0 ELSE consecutive_failures+1 END,paused_until=CASE WHEN $2='succeeded' THEN NULL WHEN consecutive_failures+1>=5 THEN now()+interval '1 hour' ELSE paused_until END WHERE id=$1",
            params![uuid(job, "endpoint_id")?, status],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) fn retry_seconds(attempts: i64) -> f64 {
    (60_u64.saturating_mul(1_u64 << attempts.clamp(0, 6))).min(3600) as f64
}

#[cfg(test)]
#[path = "channel_delivery_tests.rs"]
mod tests;
