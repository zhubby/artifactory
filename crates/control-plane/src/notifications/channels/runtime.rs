//! [INPUT]: 依赖事务事件、绑定代数、实时权限与外部渠道。
//! [OUTPUT]: 提供幂等事件路由、租约投递和有界指数重试。
//! [POS]: 事务内复核并释放锁，事务外发送，再按绑定代数和租约保存结果。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{email, webhook};
use crate::store::notifications::channel_delivery;
use crate::{crypto, state::AppState};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn tick(state: &AppState) -> anyhow::Result<()> {
    channel_delivery::route(state).await?;
    // Separate bounded workers keep a slow SMTP endpoint from stalling Webhooks.
    let (verification, email, webhook) = tokio::join!(
        work(state, "verification"),
        work(state, "email"),
        work(state, "webhook")
    );
    verification?;
    email?;
    webhook?;
    channel_delivery::cleanup(state).await
}

/// Production runs one independent loop for each known channel.
pub async fn tick_channel(state: &AppState, channel: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(channel, "verification" | "email" | "webhook"),
        "unknown notification channel"
    );
    channel_delivery::route(state).await?;
    work(state, channel).await?;
    if channel == "verification" {
        channel_delivery::cleanup(state).await?;
    }
    Ok(())
}

async fn work(state: &AppState, channel: &str) -> anyhow::Result<()> {
    for _ in 0..4 {
        let lease = Uuid::new_v4();
        let job = channel_delivery::claim(state, channel, lease).await?;
        let Some(job) = job else { break };
        if let Err(error) = deliver(state, &job, lease).await {
            // Do not persist transport errors, addresses, URL paths, or message bodies.
            tracing::warn!(delivery_id=%job["id"],"notification delivery transaction failed");
            let _ = error;
        }
    }
    Ok(())
}

fn uuid(value: &Value, key: &str) -> anyhow::Result<Uuid> {
    Ok(Uuid::parse_str(value[key].as_str().ok_or_else(|| {
        anyhow::anyhow!("invalid delivery identity")
    })?)?)
}

async fn deliver(state: &AppState, job: &Value, lease: Uuid) -> anyhow::Result<()> {
    let Some(prepared) = channel_delivery::prepare(state, job, lease).await? else {
        return Ok(());
    };
    let id = uuid(job, "id")?;
    let outcome = if job["channel"] == "webhook" {
        send_webhook(state, id, job, &prepared.binding, prepared.event.as_ref()).await
    } else {
        send_email(
            state,
            id,
            job,
            &prepared.binding,
            prepared.event.as_ref(),
            prepared.verification_token.as_deref(),
        )
        .await
    };
    channel_delivery::finish(state, job, lease, outcome).await
}

fn safe_data(value: &Value) -> Value {
    let keys = [
        "status",
        "phase",
        "target",
        "build_number",
        "build_status",
        "publish_status",
        "release_version",
        "release_id",
        "tenant_name",
        "tenant_slug",
        "project_name",
        "project_slug",
        "resource_name",
        "reason_code",
        "role",
        "subject_user_id",
        "action",
        "version",
    ];
    let data = value
        .as_object()
        .into_iter()
        .flat_map(|map| map.iter())
        .filter(|(key, value)| {
            keys.contains(&key.as_str())
                && (value.is_boolean()
                    || value.is_number()
                    || value.as_str().is_some_and(|s| {
                        s.chars().count() <= 256 && !s.chars().any(char::is_control)
                    }))
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<serde_json::Map<_, _>>();
    Value::Object(data)
}
fn cloud_event(
    public_url: &str,
    id: Uuid,
    job: &Value,
    b: &Value,
    event: Option<&Value>,
) -> Result<Value, bool> {
    let data = if let Some(e) = event {
        safe_data(&e["payload"])
    } else {
        json!({"message":"Artifactory webhook test"})
    };
    Ok(
        json!({"specversion":"1.0","id":event.map(|e|e["id"].clone()).unwrap_or(json!(id)),"source":format!("{public_url}/tenants/{}",b["tenant_id"].as_str().ok_or(false)?),"type":job["event_type"],"time":event.map(|e|e["occurred_at"].clone()).unwrap_or(job["created_at"].clone()),"datacontenttype":"application/json","artifactoryschemaversion":event.map(|e|e["schema_version"].clone()).unwrap_or(json!(1)),"aggregatekey":event.map(|e|e["aggregate_key"].clone()).unwrap_or(json!(id)),"aggregateversion":event.map(|e|e["aggregate_version"].clone()).unwrap_or(json!(1)),"data":data}),
    )
}

async fn send_webhook(
    state: &AppState,
    id: Uuid,
    job: &Value,
    b: &Value,
    event: Option<&Value>,
) -> Result<(), bool> {
    let url = webhook::validate_url(b["url"].as_str().ok_or(false)?).map_err(|_| false)?;
    let client = webhook::client(state, &url).await.map_err(|error| {
        matches!(error,crate::error::ApiError::Internal(_)) || matches!(&error,crate::error::ApiError::BadRequest(message) if message.starts_with("webhook DNS"))
    })?;
    let secret = crypto::open_context(
        &state.settings.master_key,
        b["secret_encrypted"].as_str().ok_or(false)?,
        uuid(b, "id").map_err(|_| false)?.as_bytes(),
    )
    .map_err(|_| false)?;
    let secret = std::str::from_utf8(&secret).map_err(|_| false)?;
    let cloud_event = cloud_event(&state.settings.public_url, id, job, b, event)?;
    let body = serde_json::to_vec(&cloud_event).map_err(|_| false)?;
    let timestamp = chrono::Utc::now().timestamp();
    let response = client
        .post(url)
        .header("Content-Type", "application/cloudevents+json")
        .header("X-Artifactory-Delivery", id.to_string())
        .header("X-Artifactory-Timestamp", timestamp.to_string())
        .header(
            "X-Artifactory-Signature",
            webhook::signature(secret, timestamp, id, &body),
        )
        .body(body)
        .send()
        .await
        .map_err(|_| true)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response.status().as_u16() == 429 || response.status().is_server_error())
    }
}

async fn send_email(
    state: &AppState,
    id: Uuid,
    job: &Value,
    b: &Value,
    event: Option<&Value>,
    verification_token: Option<&str>,
) -> Result<(), bool> {
    let address = b["address"].as_str().ok_or(false)?;
    let (subject, body) = if job["channel"] == "verification" {
        let token = verification_token.ok_or(false)?;
        (
            "Artifactory — Verify your email / 验证邮箱".to_owned(),
            format!(
                "Verify your email within 30 minutes / 请在 30 分钟内验证邮箱：\n{}/notifications/verify?token={}\n\nIf you did not request this, ignore this email. / 如果不是你发起的请求，请忽略此邮件。",
                state.settings.public_url, token
            ),
        )
    } else {
        let e = event.ok_or(false)?;
        let user = uuid(job, "user_id").map_err(|_| false)?;
        let chinese = b["notification_locale"].as_str().unwrap_or("zh-CN") == "zh-CN";
        let subject = if chinese {
            "Artifactory 通知"
        } else {
            "Artifactory notification"
        }
        .to_owned();
        let (title, summary) = crate::notifications::catalog::render(
            e["event_type"].as_str().unwrap_or(""),
            &e["payload"],
        );
        let (title, summary) = if chinese {
            (title.zh, summary.zh)
        } else {
            (title.en, summary.en)
        };
        let token = email::unsubscribe_token(
            &state.settings.master_key,
            user,
            b["generation"].as_i64().ok_or(false)?,
        );
        (
            subject,
            format!(
                "{title}\n{summary}\n\n{}/notifications\n\nUnsubscribe / 取消邮件订阅：\n{}/notifications/unsubscribe?token={token}",
                state.settings.public_url, state.settings.public_url
            ),
        )
    };
    let unsubscribe_url = if job["channel"] == "email" {
        Some(format!(
            "{}/api/v1/notifications/email/one-click?token={}",
            state.settings.public_url,
            email::unsubscribe_token(
                &state.settings.master_key,
                uuid(job, "user_id").map_err(|_| false)?,
                b["generation"].as_i64().ok_or(false)?
            )
        ))
    } else {
        None
    };
    email::send(
        state,
        id,
        address,
        &subject,
        body,
        unsubscribe_url.as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloud_events_are_versioned_and_drop_diagnostics_and_nested_secrets() {
        let event = json!({"id":Uuid::nil(),"occurred_at":"2026-11-23T00:00:00Z","schema_version":1,"aggregate_key":"build:safe","aggregate_version":4,"payload":{"status":"failed","error":"private-token","logs":["secret"],"resource_name":{"secret":"no"}}});
        let body = cloud_event(
            "https://example.org",
            Uuid::nil(),
            &json!({"event_type":"build.status.changed"}),
            &json!({"tenant_id":Uuid::nil()}),
            Some(&event),
        )
        .unwrap();
        assert_eq!(body["specversion"], "1.0");
        assert_eq!(body["aggregateversion"], 4);
        assert_eq!(body["data"], json!({"status":"failed"}));
        assert!(!body.to_string().contains("private-token"));
    }
    #[test]
    fn retries_are_bounded_and_increase() {
        assert_eq!(channel_delivery::retry_seconds(1), 120.);
        assert_eq!(channel_delivery::retry_seconds(9), 3600.);
        assert_eq!(channel_delivery::retry_seconds(100), 3600.);
    }
}
