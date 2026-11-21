//! [INPUT]: 依赖 Worker 租约、固定模型秘密和有序事件。
//! [OUTPUT]: 原子保存用量、会话和脱敏限长的失败原因。
//! [POS]: Worker 报告的持久边界；失败不能覆盖待接受提案。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

const MAX_ERROR_BYTES: usize = 2000;
const FALLBACK_ERROR: &str = "Pi planning failed";

fn failure_message(payload: &Value, secrets: &[&str]) -> String {
    let mut message = payload["error"].as_str().unwrap_or("").to_owned();
    let mut redactions = Vec::new();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        redactions.push((*secret).to_owned());
        // 供应商错误可嵌套 JSON。先脱敏原值和转义值，再截断。
        let escaped = serde_json::to_string(secret).expect("string serialization cannot fail");
        redactions.push(escaped[1..escaped.len() - 1].to_owned());
    }
    redactions.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    redactions.dedup();
    for secret in redactions {
        message = message.replace(&secret, "[redacted]");
    }
    let clean: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut message = clean.trim().to_owned();
    if message.is_empty() {
        return FALLBACK_ERROR.into();
    }
    let mut end = message.len().min(MAX_ERROR_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message
}

impl PlanningStore {
    pub async fn report_event(&self, v: Value) -> Result<Value> {
        let lease: Lease = serde_json::from_value(v["lease"].clone())?;
        let event = id(&v, "event_id")?;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, lease.run_id).await?;
        let life = authorized(&row, &lease)?;
        let hash = crypto::hash(serde_json::to_vec(&v)?);
        let prior: Option<String> = sql::optional_scalar(
            &mut tx,
            "SELECT input_hash FROM planning_reports WHERE run_id=$1 AND event_id=$2",
            params![lease.run_id, event],
        )
        .await?;
        if let Some(prior) = prior {
            ensure!(prior == hash, "event ID reused");
            return Ok(json!({"ok":true}));
        }
        let kind = v["kind"].as_str().context("event kind required")?;
        ensure!(
            matches!(kind, "usage" | "failed" | "conversation"),
            "unsupported event kind"
        );
        let sequence = v["sequence"].as_i64().context("event sequence required")?;
        if kind == "conversation" {
            conversations::append(&mut tx, &lease, event, &v["payload"]).await?;
        }
        if sequence > row.decode::<i64>("report_sequence")? {
            if kind == "usage" {
                sql::execute(
                    &mut tx,
                    "UPDATE planning_runs SET usage=$2,report_sequence=$3 WHERE id=$1",
                    params![lease.run_id, &v["payload"], sequence],
                )
                .await?;
            }
            if kind == "failed" {
                ensure!(!life.pending, "proposal awaiting acceptance");
                let intent = config(self, &row)?;
                let error = failure_message(&v["payload"], &[&intent.model.api_key, &lease.token]);
                conversations::finish(&mut tx, lease.run_id, lease.generation, "failed").await?;
                sql::execute(
                    &mut tx,
                    "UPDATE planning_runs SET status='failed' WHERE id=$1",
                    params![lease.run_id],
                )
                .await?;
                enqueue(
                    &mut tx,
                    lease.run_id,
                    "failed",
                    json!({"error":error,"calls":life.calls}),
                )
                .await?;
            }
        }
        models::Report::create()
            .run_id(lease.run_id)
            .event_id(event)
            .input_hash(hash)
            .exec(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }
}
