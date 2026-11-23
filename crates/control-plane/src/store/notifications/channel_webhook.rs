//! [INPUT]: 依赖管理员权限、端点代数和纯 Webhook 校验器。
//! [OUTPUT]: 提供端点增删改、密钥轮换、测试排队及安全诊断。
//! [POS]: Webhook 配置持久边界；修改绑定时原子取消旧代任务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::db;
use crate::{
    auth::Auth,
    crypto,
    error::{ApiError, Result},
    notifications::channels::webhook::{EVENTS, Input, authorize, client, validate_url},
    state::AppState,
};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use uuid::Uuid;
fn bad(message: &str) -> ApiError {
    ApiError::BadRequest(message.into())
}
const VIEW: &str = "jsonb_build_object('id',id,'name',name,'url_display',split_part(split_part(url,'?',1),'/',1)||'//'||split_part(split_part(url,'?',1),'/',3),'enabled',enabled,'event_types',event_types,'generation',generation,'last_status',last_status,'created_at',created_at)";

pub async fn list(state: &AppState, auth: &Auth, project: Option<Uuid>) -> Result<Value> {
    authorize(auth, project)?;
    let rows:Vec<Value>=sql::scalars(&mut db(state),&format!("SELECT {VIEW} FROM notification_webhooks WHERE tenant_id=$1 AND project_id IS NOT DISTINCT FROM $2 ORDER BY created_at,id"),params![auth.tenant_id,project]).await?;
    Ok(json!(rows))
}

pub async fn write(
    state: &AppState,
    auth: &Auth,
    project: Option<Uuid>,
    id: Option<Uuid>,
    input: Input,
) -> Result<Value> {
    authorize(auth, project)?;
    if input.name.as_ref().is_some_and(|n| {
        n.trim().is_empty() || n.chars().count() > 100 || n.chars().any(char::is_control)
    }) {
        return Err(bad("invalid webhook name"));
    }
    if input
        .event_types
        .as_ref()
        .is_some_and(|e| e.len() > EVENTS.len() || e.iter().any(|e| !EVENTS.contains(&e.as_str())))
    {
        return Err(bad("unknown webhook event type"));
    }
    if let Some(url) = &input.url {
        let parsed = validate_url(url)?;
        client(state, &parsed).await?;
    }
    let secret = crypto::token();
    let endpoint = id.unwrap_or_else(Uuid::new_v4);
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    sql::execute(
        &mut tx,
        "SELECT id FROM tenants WHERE id=$1 FOR UPDATE",
        params![auth.tenant_id],
    )
    .await?;
    if id.is_some() {
        let exists:Option<Uuid>=sql::optional_scalar(&mut tx,"SELECT id FROM notification_webhooks WHERE id=$1 AND tenant_id=$2 AND project_id IS NOT DISTINCT FROM $3 FOR UPDATE",params![endpoint,auth.tenant_id,project]).await?;
        if exists.is_none() {
            return Err(ApiError::NotFound);
        }
        // Any edit creates a new binding generation; old queued requests cannot see new secrets or URLs.
        sql::execute(&mut tx,"UPDATE notification_webhooks SET name=coalesce($2,name),url=coalesce($3,url),enabled=coalesce($4,enabled),event_types=coalesce($5,event_types),generation=generation+1,binding_at=clock_timestamp(),consecutive_failures=0,paused_until=NULL WHERE id=$1",params![endpoint,input.name,input.url,input.enabled,input.event_types]).await?;
        sql::execute(&mut tx,"UPDATE notification_deliveries SET status='canceled',completed_at=now() WHERE endpoint_id=$1 AND status IN ('queued','sending')",params![endpoint]).await?;
    } else {
        let name = input.name.ok_or_else(|| bad("webhook name is required"))?;
        let url = input.url.ok_or_else(|| bad("webhook URL is required"))?;
        let count: i64 = sql::scalar(
            &mut tx,
            "SELECT count(*) FROM notification_webhooks WHERE tenant_id=$1",
            params![auth.tenant_id],
        )
        .await?;
        if count >= 100 {
            return Err(bad("tenant webhook limit reached"));
        }
        sql::execute(&mut tx,"INSERT INTO notification_webhooks(id,tenant_id,project_id,created_by,name,url,enabled,event_types,secret_encrypted) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",params![endpoint,auth.tenant_id,project,auth.user_id,name,url,input.enabled.unwrap_or(false),input.event_types.unwrap_or_default(),crypto::seal_context(&state.settings.master_key,secret.as_bytes(),endpoint.as_bytes())?]).await?;
    }
    let mut result: Value = sql::scalar(
        &mut tx,
        &format!("SELECT {VIEW} FROM notification_webhooks WHERE id=$1"),
        params![endpoint],
    )
    .await?;
    tx.commit().await?;
    if id.is_none() {
        result["secret"] = json!(secret);
    }
    Ok(result)
}

pub async fn mutate(
    state: &AppState,
    auth: &Auth,
    project: Option<Uuid>,
    endpoint: Uuid,
    action: &str,
    delivery: Option<Uuid>,
) -> Result<Value> {
    authorize(auth, project)?;
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    let generation:Option<i64>=sql::optional_scalar(&mut tx,"SELECT generation FROM notification_webhooks WHERE id=$1 AND tenant_id=$2 AND project_id IS NOT DISTINCT FROM $3 FOR UPDATE",params![endpoint,auth.tenant_id,project]).await?;
    let generation = generation.ok_or(ApiError::NotFound)?;
    let mut result = json!({"ok":true});
    match action {
        "delete" => {
            sql::execute(
                &mut tx,
                "DELETE FROM notification_webhooks WHERE id=$1",
                params![endpoint],
            )
            .await?;
        }
        "rotate" => {
            let secret = crypto::token();
            sql::execute(&mut tx,"UPDATE notification_webhooks SET generation=generation+1,secret_encrypted=$2,binding_at=clock_timestamp(),consecutive_failures=0,paused_until=NULL WHERE id=$1",params![endpoint,crypto::seal_context(&state.settings.master_key,secret.as_bytes(),endpoint.as_bytes())?]).await?;
            sql::execute(&mut tx,"UPDATE notification_deliveries SET status='canceled',completed_at=now() WHERE endpoint_id=$1 AND status IN ('queued','sending')",params![endpoint]).await?;
            result = json!({"secret":secret,"generation":generation+1});
        }
        "test" => {
            let count:i64=sql::scalar(&mut tx,"SELECT count(*) FROM notification_deliveries WHERE endpoint_id=$1 AND event_type='notification.test' AND created_at>now()-interval '1 minute'",params![endpoint]).await?;
            if count >= 3 {
                return Err(ApiError::TooManyRequests(60));
            }
            let id:Uuid=sql::scalar(&mut tx,"INSERT INTO notification_deliveries(channel,endpoint_id,generation,event_type,payload) VALUES('webhook',$1,$2,'notification.test','{\"message\":\"Artifactory webhook test\"}') RETURNING id",params![endpoint,generation]).await?;
            result = json!({"id":id,"status":"queued"});
        }
        "retry" => {
            let count:i64=sql::scalar(&mut tx,"WITH updated AS (UPDATE notification_deliveries SET status='queued',next_attempt_at=now(),last_error=NULL WHERE id=$1 AND endpoint_id=$2 AND generation=$3 AND status='failed' AND attempts<10 AND created_at>now()-interval '24 hours' RETURNING id) SELECT count(*) FROM updated",params![delivery,endpoint,generation]).await?;
            if count != 1 {
                return Err(ApiError::Conflict(
                    "delivery expired, changed binding, or cannot be retried".into(),
                ));
            }
        }
        _ => return Err(ApiError::NotFound),
    }
    tx.commit().await?;
    Ok(result)
}

pub async fn deliveries(
    state: &AppState,
    auth: &Auth,
    project: Option<Uuid>,
    endpoint: Uuid,
) -> Result<Value> {
    authorize(auth, project)?;
    let exists:bool=sql::scalar(&mut db(state),"SELECT EXISTS(SELECT 1 FROM notification_webhooks WHERE id=$1 AND tenant_id=$2 AND project_id IS NOT DISTINCT FROM $3)",params![endpoint,auth.tenant_id,project]).await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    let items:Vec<Value>=sql::scalars(&mut db(state),"SELECT jsonb_build_object('id',id,'event_type',event_type,'status',status,'attempts',attempts,'next_attempt_at',next_attempt_at,'last_error',last_error,'created_at',created_at) FROM notification_deliveries WHERE endpoint_id=$1 AND created_at>now()-interval '30 days' ORDER BY created_at DESC,id DESC LIMIT 100",params![endpoint]).await?;
    Ok(json!(items))
}
