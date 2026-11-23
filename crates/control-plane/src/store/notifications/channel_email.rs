//! [INPUT]: 依赖个人偏好、地址绑定与纯邮件校验器。
//! [OUTPUT]: 提供地址绑定、单次验证、订阅更新和退订事务。
//! [POS]: 个人邮件身份的持久边界；原子递增绑定代数并取消旧任务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::db;
use crate::{
    crypto,
    error::{ApiError, Result},
    notifications::channels::{
        CATEGORIES,
        email::{normalize_address, unsubscribe_token},
    },
    state::AppState,
};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use uuid::Uuid;
fn bad(message: &str) -> ApiError {
    ApiError::BadRequest(message.into())
}

pub async fn preferences(state: &AppState, user: Uuid) -> Result<Value> {
    let preferences: Option<Value> = sql::optional_scalar(
        &mut db(state),
        "SELECT preferences FROM notification_preferences WHERE user_id=$1",
        params![user],
    )
    .await?;
    let contact: Option<Value> = sql::optional_scalar(&mut db(state), "SELECT jsonb_build_object('email_address',NULLIF(address,''),'email_verified',verified_at IS NOT NULL) FROM notification_email_contacts WHERE user_id=$1",params![user]).await?;
    let p = preferences.unwrap_or_else(|| json!({}));
    let mut in_app = serde_json::Map::new();
    let mut email = serde_json::Map::new();
    for category in CATEGORIES {
        in_app.insert(
            (*category).into(),
            json!(p["in_app"][category].as_bool().unwrap_or(true)),
        );
        email.insert(
            (*category).into(),
            json!(p["email"][category].as_bool().unwrap_or(false)),
        );
    }
    Ok(
        json!({"in_app":in_app,"email":email,"locale":p["locale"].as_str().unwrap_or("zh-CN"),"email_address":contact.as_ref().and_then(|v|v.get("email_address")),"email_verified":contact.as_ref().and_then(|v|v["email_verified"].as_bool()).unwrap_or(false),"email_available":state.settings.notifications.smtp.is_some()}),
    )
}

pub async fn put_preferences(state: &AppState, user: Uuid, input: Value) -> Result<Value> {
    let obj = input
        .as_object()
        .ok_or_else(|| bad("invalid notification preferences"))?;
    if obj
        .keys()
        .any(|key| !matches!(key.as_str(), "in_app" | "email" | "locale"))
    {
        return Err(bad("unknown preference field"));
    }
    for name in ["in_app", "email"] {
        if let Some(values) = obj.get(name) {
            let values = values
                .as_object()
                .ok_or_else(|| bad("invalid category preferences"))?;
            if values
                .iter()
                .any(|(key, value)| !CATEGORIES.contains(&key.as_str()) || !value.is_boolean())
            {
                return Err(bad("invalid category preference"));
            }
        }
    }
    if obj
        .get("locale")
        .is_some_and(|v| !matches!(v.as_str(), Some("zh-CN" | "en")))
    {
        return Err(bad("invalid notification locale"));
    }
    if obj.get("email").is_some_and(|v| v["delivery"] == true) {
        return Err(bad("delivery alerts are in-app only"));
    }
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    sql::execute(
        &mut tx,
        "INSERT INTO notification_preferences(user_id) VALUES($1) ON CONFLICT DO NOTHING",
        params![user],
    )
    .await?;
    let current: Value = sql::scalar(
        &mut tx,
        "SELECT preferences FROM notification_preferences WHERE user_id=$1 FOR UPDATE",
        params![user],
    )
    .await?;
    let mut merged = current.as_object().cloned().unwrap_or_default();
    for (key, value) in obj {
        if key == "locale" {
            merged.insert(key.clone(), value.clone());
        } else {
            let mut categories = merged
                .get(key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            categories.extend(value.as_object().unwrap().clone());
            merged.insert(key.clone(), Value::Object(categories));
        }
    }
    let enabled_at: chrono::DateTime<chrono::Utc> =
        sql::scalar(&mut tx, "SELECT clock_timestamp()", params![]).await?;
    let mut since = merged
        .get("email_since")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let previous = current.get("email").and_then(Value::as_object);
    let mut enabling_email = false;
    if let Some(next) = merged.get("email").and_then(Value::as_object) {
        for (category, value) in next {
            if value == &json!(true)
                && !previous
                    .and_then(|p| p.get(category))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            {
                enabling_email = true;
                since.insert(category.clone(), json!(enabled_at.to_rfc3339()));
            }
            if value != &json!(true) {
                since.remove(category);
            }
        }
    }
    merged.insert("email_since".into(), Value::Object(since));
    if enabling_email {
        let verified:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM notification_email_contacts WHERE user_id=$1 AND verified_at IS NOT NULL)",params![user]).await?;
        if !verified || state.settings.notifications.smtp.is_none() {
            return Err(bad(
                "verify an email address and configure SMTP before subscribing",
            ));
        }
    }
    sql::execute(
        &mut tx,
        "UPDATE notification_preferences SET preferences=$2 WHERE user_id=$1",
        params![user, Value::Object(merged)],
    )
    .await?;
    tx.commit().await?;
    preferences(state, user).await
}

pub async fn bind(state: &AppState, user: Uuid, address: Option<&str>) -> Result<Value> {
    if state.settings.notifications.smtp.is_none() {
        return Err(bad("email delivery is not configured"));
    }
    let address = address.map(normalize_address).transpose()?;
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    // Serialize first binding, preferences, resend and deletion across replicas.
    sql::execute(
        &mut tx,
        "SELECT id FROM users WHERE id=$1 FOR UPDATE",
        params![user],
    )
    .await?;
    let old:Option<Value>=sql::optional_scalar(&mut tx,"SELECT jsonb_build_object('address',address,'generation',generation,'rate_limited',last_requested_at>now()-interval '60 seconds' OR (request_window_at>now()-interval '1 hour' AND request_count>=5)) FROM notification_email_contacts WHERE user_id=$1 FOR UPDATE",params![user]).await?;
    if old.as_ref().is_some_and(|v| v["rate_limited"] == true) {
        return Err(ApiError::TooManyRequests(60));
    }
    let address = address
        .or_else(|| {
            old.as_ref()
                .and_then(|v| v["address"].as_str().map(str::to_owned))
        })
        .ok_or_else(|| bad("bind an email address first"))?;
    if address.is_empty() {
        return Err(bad("bind an email address first"));
    }
    let token = crypto::token();
    let unsubscribe = crypto::token();
    let generation: i64=sql::scalar(&mut tx,"INSERT INTO notification_email_contacts(user_id,address,token_hash,token_expires_at,unsubscribe_hash) VALUES($1,$2,$3,now()+interval '30 minutes',$4) ON CONFLICT(user_id) DO UPDATE SET address=EXCLUDED.address,generation=notification_email_contacts.generation+1,verified_at=NULL,token_hash=EXCLUDED.token_hash,token_expires_at=EXCLUDED.token_expires_at,unsubscribe_hash=EXCLUDED.unsubscribe_hash,last_requested_at=now(),request_count=CASE WHEN notification_email_contacts.request_window_at<now()-interval '1 hour' THEN 1 ELSE notification_email_contacts.request_count+1 END,request_window_at=CASE WHEN notification_email_contacts.request_window_at<now()-interval '1 hour' THEN now() ELSE notification_email_contacts.request_window_at END RETURNING generation",params![user,address,crypto::hash(&token),crypto::hash(&unsubscribe)]).await?;
    let unsubscribe = unsubscribe_token(&state.settings.master_key, user, generation);
    sql::execute(
        &mut tx,
        "UPDATE notification_email_contacts SET unsubscribe_hash=$2 WHERE user_id=$1",
        params![user, crypto::hash(&unsubscribe)],
    )
    .await?;
    sql::execute(&mut tx,"INSERT INTO notification_preferences(user_id,preferences) VALUES($1,'{\"email\":{}}') ON CONFLICT(user_id) DO UPDATE SET preferences=notification_preferences.preferences||'{\"email\":{},\"email_since\":{}}'::jsonb",params![user]).await?;
    sql::execute(&mut tx,"UPDATE notification_deliveries SET status='canceled',completed_at=now(),payload='{}' WHERE user_id=$1 AND status IN ('queued','sending')",params![user]).await?;
    // Verification tokens exist only in memory during the SMTP attempt.
    let payload = json!({});
    sql::execute(&mut tx,"INSERT INTO notification_deliveries(channel,user_id,generation,event_type,payload) VALUES('verification',$1,$2,'email.verification',$3)",params![user,generation,payload]).await?;
    tx.commit().await?;
    preferences(state, user).await
}

pub async fn verify(state: &AppState, user: Uuid, token: &str) -> Result<Value> {
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad("invalid or expired verification token"));
    }
    let changed:i64=sql::scalar(&mut db(state),"WITH verified AS (UPDATE notification_email_contacts SET verified_at=clock_timestamp(),token_hash=NULL,token_expires_at=NULL WHERE user_id=$1 AND token_hash=$2 AND token_expires_at>now() RETURNING 1) SELECT count(*) FROM verified",params![user,crypto::hash(token)]).await?;
    if changed != 1 {
        return Err(bad("invalid or expired verification token"));
    }
    preferences(state, user).await
}

pub async fn retire(state: &AppState, user: Uuid) -> Result<Value> {
    let mut conn = db(state);
    let mut tx = conn.transaction().await?;
    sql::execute(
        &mut tx,
        "SELECT id FROM users WHERE id=$1 FOR UPDATE",
        params![user],
    )
    .await?;
    // Preserve monotonically increasing generations even after a binding is retired.
    sql::execute(&mut tx,"UPDATE notification_email_contacts SET generation=generation+1,address='',verified_at=NULL,token_hash=NULL,token_expires_at=NULL,unsubscribe_hash=$2 WHERE user_id=$1",params![user,crypto::hash(crypto::token())]).await?;
    sql::execute(&mut tx,"UPDATE notification_preferences SET preferences=preferences||'{\"email\":{},\"email_since\":{}}'::jsonb WHERE user_id=$1",params![user]).await?;
    sql::execute(&mut tx,"UPDATE notification_deliveries SET status='canceled',completed_at=now(),payload='{}' WHERE user_id=$1 AND status IN ('queued','sending')",params![user]).await?;
    tx.commit().await?;
    preferences(state, user).await
}

pub async fn unsubscribe(state: &AppState, token: &str) -> Result<Value> {
    if token.len() != 64 {
        return Err(bad("invalid unsubscribe token"));
    }
    sql::execute(&mut db(state),"UPDATE notification_preferences p SET preferences=preferences||'{\"email\":{},\"email_since\":{}}'::jsonb FROM notification_email_contacts c WHERE p.user_id=c.user_id AND c.unsubscribe_hash=$1",params![crypto::hash(token)]).await?;
    Ok(json!({"ok":true}))
}
