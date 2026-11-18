//! [INPUT]: 依赖已锁定的运行身份和完成消息。
//! [OUTPUT]: 持久化 generation 会话并分页读取审计记录。
//! [POS]: 审计数据独立于预算、租约和恢复状态。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{RuntimeStore, models};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_runtime_protocol::{Execution, Lease, ModelConfiguration, crypto};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) async fn open(
    tx: &mut toasty::db::Transaction<'_>,
    execution: &Execution,
    model: &ModelConfiguration,
    digest: Option<&str>,
) -> Result<()> {
    sql::execute(tx, "INSERT INTO runtime_conversations(run_id,generation,pod_uid,status,model,source_digest) VALUES($1,$2,$3,'running',$4,$5) ON CONFLICT(run_id,generation) DO NOTHING", params![execution.run_id, execution.generation, &execution.uid, &model.model, digest]).await?;
    Ok(())
}
pub(super) async fn finish(
    tx: &mut toasty::db::Transaction<'_>,
    run: Uuid,
    generation: i64,
    status: &str,
) -> Result<()> {
    sql::execute(tx, "UPDATE runtime_conversations SET status=$3,ended_at=coalesce(ended_at,now()) WHERE run_id=$1 AND generation=$2 AND status='running'", params![run,generation,status]).await?;
    Ok(())
}
pub(super) async fn record(
    tx: &mut toasty::db::Transaction<'_>,
    run: Uuid,
    generation: i64,
    event: Uuid,
    kind: &str,
    payload: &Value,
) -> Result<()> {
    let hash = crypto::hash(serde_json::to_vec(&json!({"kind":kind,"payload":payload}))?);
    let prior: Option<String> = sql::optional_scalar(
        tx,
        "SELECT input_hash FROM runtime_messages WHERE run_id=$1 AND generation=$2 AND event_id=$3",
        params![run, generation, event],
    )
    .await?;
    if let Some(prior) = prior {
        ensure!(prior == hash, "conversation event identity reused");
        return Ok(());
    }
    let sequence:i64=sql::scalar(tx,"SELECT coalesce(max(sequence),0)+1 FROM runtime_messages WHERE run_id=$1 AND generation=$2",params![run,generation]).await?;
    models::Message::create()
        .run_id(run)
        .generation(generation)
        .sequence(sequence)
        .event_id(event)
        .kind(kind)
        .input_hash(hash)
        .payload(toasty::Json(payload.clone()))
        .exec(tx)
        .await?;
    Ok(())
}
pub(super) async fn append(
    tx: &mut toasty::db::Transaction<'_>,
    lease: &Lease,
    event: Uuid,
    value: &Value,
) -> Result<()> {
    let kind = value["kind"]
        .as_str()
        .context("conversation kind required")?;
    ensure!(
        matches!(kind, "system" | "message" | "message_chunk"),
        "unsupported conversation kind"
    );
    let content = value
        .get("content")
        .context("conversation content required")?;
    ensure!(
        serde_json::to_vec(content)?.len() <= 512 * 1024,
        "conversation event exceeds limit"
    );
    record(tx, lease.run_id, lease.generation, event, kind, content).await
}
impl RuntimeStore {
    async fn conversation_scope(&self, value: &Value) -> Result<Uuid> {
        let run = super::id(value, "run_id")?;
        let tenant = super::id(value, "tenant_id")?;
        let project = super::id(value, "project_id")?;
        let valid:bool=sql::scalar(&mut self.pool.clone(),"SELECT EXISTS(SELECT 1 FROM runtime_runs WHERE id=$1 AND tenant_id=$2 AND project_id=$3)",params![run,tenant,project]).await?;
        ensure!(valid, "conversation scope rejected");
        Ok(run)
    }
    pub async fn conversations(&self, value: Value) -> Result<Value> {
        let run = self.conversation_scope(&value).await?;
        let rows: Vec<Value> = sql::scalars(
            &mut self.pool.clone(),
            "SELECT to_jsonb(c) FROM runtime_conversations c WHERE run_id=$1 ORDER BY generation",
            params![run],
        )
        .await?;
        Ok(json!({"conversations":rows}))
    }
    pub async fn conversation_messages(&self, value: Value) -> Result<Value> {
        let run = self.conversation_scope(&value).await?;
        let generation = value["generation"]
            .as_i64()
            .filter(|g| *g > 0)
            .context("generation required")?;
        let after = value["after"].as_i64().unwrap_or(0);
        let limit = value["limit"].as_i64().unwrap_or(50).clamp(1, 100);
        let rows:Vec<Value>=sql::scalars(&mut self.pool.clone(),"SELECT jsonb_build_object('sequence',sequence,'kind',kind,'payload',payload,'created_at',created_at) FROM runtime_messages WHERE run_id=$1 AND generation=$2 AND sequence>$3 ORDER BY sequence LIMIT $4",params![run,generation,after,limit+1]).await?;
        // 对话分页保持独立的 1 MiB 响应预算。
        let mut messages = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        for row in rows {
            let size = serde_json::to_vec(&row)?.len();
            if messages.len() >= limit as usize || bytes + size > 1024 * 1024 {
                more = true;
                break;
            }
            bytes += size;
            messages.push(row);
        }
        let next = if more {
            messages.last().and_then(|row| row["sequence"].as_i64())
        } else {
            None
        };
        Ok(json!({"messages":messages,"next_cursor":next}))
    }
}
