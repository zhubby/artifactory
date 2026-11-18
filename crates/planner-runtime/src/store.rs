//! [INPUT]: 依赖固定运行意图、数据库事务和执行身份。
//! [OUTPUT]: 持久化预算、提案和 generation 生命周期。
//! [POS]: Runtime 操作状态边界；审计记录不用于恢复会话。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod conversations;
mod model_requests;
mod rows;
use rows::{locked, run_fields};
mod models;
use crate::{lifecycle::Lifecycle, server::id};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_runtime_protocol::{Execution, Lease, RunIntent, crypto};
use chrono::Utc;
use serde_json::{Value, json};
use toasty::stmt::Type;

use std::sync::Arc;
use uuid::Uuid;
#[derive(Clone)]
pub struct RuntimeStore {
    pool: toasty::Db,
    key: Arc<[u8; 32]>,
}
pub struct RuntimeMetrics {
    pub statuses: Vec<(String, i64)>,
    pub calls: i64,
    pub pending: i64,
    pub oldest: f64,
    pub outbox: i64,
}
pub enum ToolDisposition {
    Immediate(Value),
    Forward(Value),
}
pub struct OutboxDelivery {
    pub id: Uuid,
    pub run_id: Uuid,
    pub kind: String,
    pub payload: Value,
}
pub struct ModelRequest<'a> {
    pub run: Uuid,
    pub generation: i64,
    pub lease: &'a Lease,
    pub request_id: Uuid,
    pub path: &'a str,
    pub value: &'a Value,
    pub hash: String,
}
pub enum ModelReservation {
    Duplicate,
    Reserved {
        config: artifactory_runtime_protocol::ModelConfiguration,
        deadline_ms: i64,
        wait_ms: i64,
        endpoint: String,
    },
}

fn config(s: &RuntimeStore, row: &sql::Record) -> Result<RunIntent> {
    let value: Value = serde_json::from_slice(&crypto::open(
        &s.key,
        &row.decode::<String>("intent_ciphertext")?,
    )?)?;
    let tenant: Uuid = row.decode("tenant_id")?;
    let intent: RunIntent = serde_json::from_value(value)?;
    ensure!(
        intent.tenant_id == tenant && Some(intent.project_id) == row.decode("project_id")?,
        "runtime tenant mismatch"
    );
    Ok(intent)
}
fn authorized(row: &sql::Record, lease: &Lease) -> Result<Lifecycle> {
    ensure!(
        row.decode::<Option<String>>("grant_hash")?.as_deref() == Some(&crypto::hash(&lease.token)),
        "invalid run grant"
    );
    let life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
    life.valid(lease.generation, &lease.uid, Utc::now().timestamp_millis())?;
    ensure!(
        !["succeeded", "needs_input", "failed", "canceled"]
            .contains(&row.decode::<String>("status")?.as_str()),
        "run is terminal"
    );
    Ok(life)
}
async fn save(tx: &mut toasty::db::Transaction<'_>, run: Uuid, life: &Lifecycle) -> Result<()> {
    sql::execute(
        tx,
        "UPDATE runtime_runs SET lifecycle=$2,updated_at=now() WHERE id=$1",
        params![run, serde_json::to_value(life)?],
    )
    .await?;
    Ok(())
}
async fn enqueue(
    tx: &mut toasty::db::Transaction<'_>,
    run: Uuid,
    kind: &str,
    payload: Value,
) -> Result<Uuid> {
    let event = Uuid::new_v4();
    models::Outbox::create()
        .id(event)
        .run_id(run)
        .kind(kind)
        .payload(toasty::Json(payload))
        .exec(tx)
        .await?;
    Ok(event)
}
impl RuntimeStore {
    pub async fn purge_project(&self, project: Uuid) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended('runtime-project:'||$1::text,96))",
            params![project],
        )
        .await?;
        let live:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM runtime_runs WHERE project_id=$1 AND status IN ('queued','provisioning','running'))",params![project]).await?;
        ensure!(!live, "runtime still has accepted work");
        sql::execute(
            &mut tx,
            "INSERT INTO runtime_deleted_projects(project_id) VALUES($1) ON CONFLICT DO NOTHING",
            params![project],
        )
        .await?;
        for table in [
            "runtime_messages",
            "runtime_conversations",
            "runtime_model_requests",
            "runtime_reports",
            "runtime_proposals",
            "runtime_outbox",
        ] {
            sql::execute(&mut tx,&format!("DELETE FROM {table} WHERE run_id IN (SELECT id FROM runtime_runs WHERE project_id=$1)"),params![project]).await?;
        }
        sql::execute(
            &mut tx,
            "DELETE FROM runtime_runs WHERE project_id=$1",
            params![project],
        )
        .await?;
        sql::execute(
            &mut tx,
            "DELETE FROM runtime_project_tenants WHERE project_id=$1",
            params![project],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub fn new(pool: toasty::Db, key: Arc<[u8; 32]>) -> Self {
        Self { pool, key }
    }
    pub async fn metrics(&self) -> Result<RuntimeMetrics> {
        let rows = sql::records(
            &mut self.pool.clone(),
            "SELECT status,count(*) AS runs FROM runtime_runs GROUP BY status",
            params![],
            &[("status", Type::String), ("runs", Type::I64)],
        )
        .await?;
        let (calls,pending,oldest):(i64,i64,f64)=sql::tuple(&mut self.pool.clone(), "SELECT coalesce(sum((lifecycle->>'calls')::bigint),0)::bigint,count(*) FILTER(WHERE (lifecycle->>'pending')::bool),coalesce(max(extract(epoch FROM now()-created_at)) FILTER(WHERE status='queued'),0)::float8 FROM runtime_runs", params![]).await?;
        let outbox: i64 = sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM runtime_outbox WHERE NOT delivered",
            params![],
        )
        .await?;
        Ok(RuntimeMetrics {
            statuses: rows
                .into_iter()
                .map(|row| Ok((row.decode("status")?, row.decode("runs")?)))
                .collect::<artifactory_database::Result<_>>()?,
            calls,
            pending,
            oldest,
            outbox,
        })
    }

    pub async fn put_intent(
        &self,
        run: Uuid,
        version: i64,
        hash: String,
        cancel: bool,
        intent: Option<RunIntent>,
        ciphertext: Option<String>,
    ) -> Result<Value> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            params![run.to_string()],
        )
        .await?;
        if let Some(row) = sql::optional_record(
            &mut tx,
            "SELECT version,intent_hash,status FROM runtime_runs WHERE id=$1 FOR UPDATE",
            params![run],
            &[
                ("version", Type::I64),
                ("intent_hash", Type::String),
                ("status", Type::String),
            ],
        )
        .await?
        {
            if row.decode::<i64>("version")? > version {
                return Ok(json!({"ok":true,"stale":true}));
            }
            if row.decode::<i64>("version")? == version {
                ensure!(
                    row.decode::<Option<String>>("intent_hash")?.as_deref() == Some(&hash),
                    "intent ID/version reused with different input"
                );
                return Ok(json!({"ok":true}));
            }
            ensure!(cancel, "running intent is immutable");
            sql::execute(&mut tx,"UPDATE runtime_conversations SET status='canceled',ended_at=coalesce(ended_at,now()) WHERE run_id=$1 AND status='running'",params![run]).await?;
            sql::execute(&mut tx, "UPDATE runtime_runs SET version=$2,intent_hash=$3,status='canceled',lifecycle=jsonb_set(lifecycle,'{canceled}','true'),updated_at=now() WHERE id=$1", params![run, version, hash]).await?;
        } else {
            let mut life = Lifecycle::new(intent.as_ref().map_or(20, |i| i.model.max_calls));
            life.canceled = cancel;
            if let Some(i) = &intent {
                ensure!(!i.tenant_id.is_nil(), "tenant identity is required");
                sql::execute(&mut tx,"INSERT INTO runtime_project_tenants(project_id,tenant_id) VALUES($1,$2) ON CONFLICT(project_id) DO NOTHING",params![i.project_id,i.tenant_id]).await?;
                let same: bool = sql::scalar(
                    &mut tx,
                    "SELECT tenant_id=$2 FROM runtime_project_tenants WHERE project_id=$1",
                    params![i.project_id, i.tenant_id],
                )
                .await?;
                ensure!(same, "project belongs to a different tenant");
            }
            models::Run::create()
                .tenant_id(intent.as_ref().map(|i| i.tenant_id))
                .id(run)
                .version(version)
                .intent_hash(hash)
                .intent_ciphertext(ciphertext)
                .project_id(intent.as_ref().map(|i| i.project_id))
                .pool(intent.as_ref().map(|i| i.pool.clone()))
                .source_manifest_id(intent.as_ref().and_then(|i| i.source_manifest_id))
                .source_digest(intent.as_ref().and_then(|i| i.source_digest.clone()))
                .lifecycle(toasty::Json(serde_json::to_value(life)?))
                .status(if cancel { "canceled" } else { "queued" })
                .exec(&mut tx)
                .await?;
        }
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }

    pub async fn register_execution(&self, execution: Execution) -> Result<Value> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, execution.run_id).await?;
        let life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
        ensure!(
            !life.canceled && life.generation == execution.generation,
            "execution fenced"
        );
        let existing: Option<Value> = row.decode("execution")?;
        if let Some(existing) = existing {
            ensure!(
                existing == serde_json::to_value(&execution)?,
                "execution identity already registered"
            );
        } else {
            sql::execute(
                &mut tx,
                "UPDATE runtime_runs SET execution=$2 WHERE id=$1",
                params![execution.run_id, serde_json::to_value(execution)?],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }

    pub async fn prepare_execution(&self, execution: &Execution) -> Result<()> {
        let row = sql::record(
            &mut self.pool.clone(),
            "SELECT * FROM runtime_runs WHERE id=$1",
            params![execution.run_id],
            &run_fields(),
        )
        .await?;
        let life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
        ensure!(
            !life.canceled && !life.pending && life.generation == execution.generation,
            "execution fenced"
        );
        ensure!(
            row.decode::<String>("status")? == "provisioning",
            "run is not preparing"
        );
        ensure!(
            row.decode::<Option<Value>>("execution")? == Some(serde_json::to_value(execution)?),
            "unregistered Pod UID"
        );
        ensure!(
            life.deadline_ms
                .is_none_or(|deadline| deadline > Utc::now().timestamp_millis()),
            "planning deadline reached"
        );
        Ok(())
    }

    pub async fn heartbeat_run(&self, lease: Lease) -> Result<Value> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, lease.run_id).await?;
        let mut life = authorized(&row, &lease)?;
        life.lease_until_ms =
            Some((Utc::now().timestamp_millis() + 90_000).min(life.deadline_ms.unwrap()));
        save(&mut tx, lease.run_id, &life).await?;
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }

    pub async fn validate_grant(&self, lease: Lease) -> Result<Value> {
        let row = sql::record(
            &mut self.pool.clone(),
            "SELECT * FROM runtime_runs WHERE id=$1",
            params![lease.run_id],
            &run_fields(),
        )
        .await?;
        let life = authorized(&row, &lease)?;
        ensure!(!life.pending, "proposal awaiting acceptance");
        Ok(
            json!({"source_manifest_id":row.decode::<Option<Uuid>>("source_manifest_id")?,"project_id":row.decode::<Uuid>("project_id")?}),
        )
    }

    pub async fn read_run(&self, run: Uuid) -> Result<Value> {
        let row = sql::record(
            &mut self.pool.clone(),
            "SELECT * FROM runtime_runs WHERE id=$1",
            params![run],
            &run_fields(),
        )
        .await?;
        Ok(
            json!({"run_id":run,"status":row.decode::<String>("status")?,"lifecycle":row.decode::<Value>("lifecycle")?,"execution":row.decode::<Option<Value>>("execution")?}),
        )
    }

    pub async fn report_event(&self, v: Value) -> Result<Value> {
        let lease: Lease = serde_json::from_value(v["lease"].clone())?;
        let event = id(&v, "event_id")?;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, lease.run_id).await?;
        authorized(&row, &lease)?;
        let hash = crypto::hash(serde_json::to_vec(&v)?);
        let prior: Option<String> = sql::optional_scalar(
            &mut tx,
            "SELECT input_hash FROM runtime_reports WHERE run_id=$1 AND event_id=$2",
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
                    "UPDATE runtime_runs SET usage=$2,report_sequence=$3 WHERE id=$1",
                    params![lease.run_id, &v["payload"], sequence],
                )
                .await?;
            }
            if kind == "failed" {
                conversations::finish(&mut tx, lease.run_id, lease.generation, "failed").await?;
                sql::execute(
                    &mut tx,
                    "UPDATE runtime_runs SET status='failed' WHERE id=$1",
                    params![lease.run_id],
                )
                .await?;
                enqueue(&mut tx,lease.run_id,"failed",json!({"error":"Pi planning failed","calls":serde_json::from_value::<Lifecycle>(row.decode("lifecycle")?)?.calls})).await?;
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

    pub async fn acquire_execution(&self, execution: Execution, public_url: &str) -> Result<Value> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, execution.run_id).await?;
        ensure!(
            matches!(
                row.decode::<String>("status")?.as_str(),
                "provisioning" | "running"
            ),
            "run is not admitted for execution"
        );
        ensure!(
            row.decode::<Option<Value>>("execution")? == Some(serde_json::to_value(&execution)?),
            "unregistered Pod UID"
        );
        let mut life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
        let intent = config(self, &row)?;
        life.acquire(
            execution.generation,
            &execution.uid,
            Utc::now().timestamp_millis(),
            i64::from(intent.model.timeout_seconds) * 1000,
        )?;
        let token = if let Some(ciphertext) = row.decode::<Option<String>>("grant_ciphertext")? {
            String::from_utf8(crypto::open(&self.key, &ciphertext)?)?
        } else {
            crypto::token()
        };
        sql::execute(&mut tx, "UPDATE runtime_runs SET status='running',lifecycle=$2,grant_hash=$3,grant_ciphertext=$4 WHERE id=$1", params![execution.run_id, serde_json::to_value(&life)?, crypto::hash(&token), crypto::seal(&self.key,token.as_bytes())?]).await?;
        let tenant = intent.tenant_id;
        let mut context = intent.context;
        context["tenant_id"] = json!(tenant);
        context["usage"] = row.decode::<Value>("usage")?;
        conversations::open(
            &mut tx,
            &execution,
            &intent.model,
            row.decode::<Option<String>>("source_digest")?.as_deref(),
        )
        .await?;
        let rejected:Option<Value>=sql::optional_scalar(&mut tx, "SELECT jsonb_build_object('kind',kind,'result',result) FROM runtime_proposals WHERE run_id=$1 AND result IS NOT NULL ORDER BY created_at DESC LIMIT 1", params![execution.run_id]).await?;
        if let Some(value) = rejected {
            context["recovered_proposal"] = value;
        }
        let mut model = intent.model;
        model.api_key = token.clone();
        model.base_url = format!(
            "{}/model/{}/{}/{}/v1",
            public_url, execution.run_id, execution.generation, execution.uid
        );
        let lease = Lease {
            run_id: execution.run_id,
            generation: execution.generation,
            uid: execution.uid,
            token,
        };
        let result = json!({"lease":lease,"task":{"context":context,"model":model,"max_calls":life.max_calls.saturating_sub(life.calls),"timeout_seconds":((life.deadline_ms.unwrap()-Utc::now().timestamp_millis()).max(1)+999)/1000}});
        tx.commit().await?;
        Ok(result)
    }

    pub async fn tool_call(
        &self,
        lease: Lease,
        name: &str,
        proposal: Uuid,
        v: &Value,
    ) -> Result<ToolDisposition> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row = locked(&mut tx, lease.run_id).await?;
        let prior = sql::optional_record(
            &mut tx,
            "SELECT input_hash,result FROM runtime_proposals WHERE id=$1 AND run_id=$2",
            params![proposal, lease.run_id],
            &[("input_hash", Type::String), ("result", Type::String)],
        )
        .await?;
        let hash = crypto::hash(serde_json::to_vec(
            &json!({"name":name,"arguments":v["arguments"]}),
        )?);
        if let Some(prior) = prior {
            ensure!(
                prior.decode::<String>("input_hash")? == hash,
                "proposal ID reused"
            );
            return Ok(ToolDisposition::Immediate(
                prior
                    .decode::<Option<Value>>("result")?
                    .unwrap_or(json!({"pending":true,"proposal_id":proposal})),
            ));
        }
        let mut life = authorized(&row, &lease)?;
        ensure!(!life.pending, "proposal awaiting acceptance");
        if matches!(name, "submit_plan" | "submit_placements" | "request_input") {
            life.pending = true;
            conversations::record(&mut tx,lease.run_id,lease.generation,Uuid::new_v4(),"proposal_submitted",&json!({"proposal_id":proposal,"name":name,"tool_call_id":v["tool_call_id"],"arguments":v["arguments"]})).await?;
            models::Proposal::create()
                .id(proposal)
                .run_id(lease.run_id)
                .generation(lease.generation)
                .input_hash(hash)
                .kind(name)
                .payload(toasty::Json(v["arguments"].clone()))
                .deadline_ms(life.deadline_ms.unwrap())
                .exec(&mut tx)
                .await?;
            save(&mut tx, lease.run_id, &life).await?;
            enqueue(&mut tx,lease.run_id,"proposal",json!({"proposal_id":proposal,"generation":lease.generation,"kind":name,"payload":v["arguments"],"deadline_ms":life.deadline_ms,"source_digest":row.decode::<Option<String>>("source_digest")?})).await?;
            tx.commit().await?;
            Ok(ToolDisposition::Immediate(
                json!({"pending":true,"proposal_id":proposal}),
            ))
        } else {
            ensure!(
                matches!(
                    name,
                    "node_capabilities"
                        | "summarize_nodes"
                        | "query_nodes"
                        | "get_node"
                        | "read_logs"
                ),
                "source tools execute inside the Worker"
            );
            tx.commit().await?;
            Ok(ToolDisposition::Forward(
                json!({"run_id":lease.run_id,"name":name,"arguments":v["arguments"]}),
            ))
        }
    }

    pub async fn proposal_result(&self, proposal: Uuid, lease: Lease) -> Result<Value> {
        let row=sql::record(&mut self.pool.clone(), "SELECT p.result FROM runtime_proposals p JOIN runtime_runs r ON r.id=p.run_id WHERE p.id=$1 AND p.run_id=$2 AND r.grant_hash=$3 AND p.generation=$4", params![proposal, lease.run_id, crypto::hash(lease.token), lease.generation], &[("result", Type::String)]).await?;
        Ok(row
            .decode::<Option<Value>>("result")?
            .unwrap_or(json!({"pending":true})))
    }

    pub async fn queue(&self, pool: &str, limit: i64, per_project: i64) -> Result<Value> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,1))",
            params![pool],
        )
        .await?;
        let active: i64 = sql::scalar(&mut tx, "SELECT count(*) FROM runtime_runs WHERE pool=$1 AND status IN ('provisioning','running')", params![pool])
    .await?;
        let rows=sql::records(&mut tx, "SELECT * FROM runtime_runs r WHERE pool=$1 AND status='queued' AND (SELECT count(*) FROM runtime_runs a WHERE a.project_id=r.project_id AND a.pool=r.pool AND a.status IN ('provisioning','running'))<$3 ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT $2", params![pool, (limit-active).max(0), per_project], &run_fields()).await?;
        let mut admitted = Vec::new();
        for row in rows {
            let project: Uuid = row.decode("project_id")?;
            let count:i64=sql::scalar(&mut tx, "SELECT count(*) FROM runtime_runs WHERE project_id=$1 AND pool=$2 AND status IN ('provisioning','running')", params![project, pool]).await?;
            if count >= per_project {
                continue;
            }
            let run: Uuid = row.decode("id")?;
            let life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
            sql::execute(
                &mut tx,
                "UPDATE runtime_runs SET status='provisioning',dispatched_at=now() WHERE id=$1",
                params![run],
            )
            .await?;
            admitted.push(json!({"run_id":run,"generation":life.generation,"pool":pool,"source_manifest_id":row.decode::<Option<Uuid>>("source_manifest_id")?,"source_digest":row.decode::<Option<String>>("source_digest")?}));
        }
        // Lost create acknowledgements are recovered by returning all admitted descriptors.
        let active = sql::records(
            &mut tx,
            "SELECT * FROM runtime_runs WHERE pool=$1 AND status IN ('provisioning','running')",
            params![pool],
            &run_fields(),
        )
        .await?;
        let mut rows = Vec::new();
        for row in active {
            let intent = config(self, &row)?;
            let lifecycle: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
            rows.push(json!({"run_id":intent.run_id,"generation":lifecycle.generation,"pool":intent.pool,"source_manifest_id":intent.source_manifest_id,"source_digest":intent.source_digest}));
        }
        tx.commit().await?;
        let queued: i64 = sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM runtime_runs WHERE pool=$1 AND status='queued'",
            params![pool],
        )
        .await?;
        Ok(json!({"active":rows.len(),"queued":queued,"runs":rows}))
    }

    pub async fn maintain(&self) -> Result<()> {
        let ids: Vec<Uuid> = sql::scalars(
            &mut self.pool.clone(),
            "SELECT id FROM runtime_runs WHERE status IN ('provisioning','running')",
            params![],
        )
        .await?;
        for id in ids {
            let mut tx_db = self.pool.clone();
            let mut tx = tx_db.transaction().await?;
            let row = locked(&mut tx, id).await?;
            let mut life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
            let now = Utc::now().timestamp_millis();
            if !matches!(
                row.decode::<String>("status")?.as_str(),
                "provisioning" | "running"
            ) {
                continue;
            }
            if life.pending {
                continue;
            }
            let expired = life.deadline_ms.is_some_and(|d| d <= now) || life.generation >= 4;
            let stalled = row.decode::<String>("status")? == "provisioning"
                && row
                    .decode::<Option<chrono::DateTime<Utc>>>("dispatched_at")?
                    .is_some_and(|t| t < Utc::now() - chrono::Duration::seconds(900));
            if expired || (stalled && life.generation >= 3) {
                conversations::finish(&mut tx, id, life.generation, "failed").await?;
                sql::execute(
                    &mut tx,
                    "UPDATE runtime_runs SET status='failed' WHERE id=$1",
                    params![id],
                )
                .await?;
                enqueue(&mut tx,id,"failed",json!({"error":"planning deadline or provisioning retry limit reached","calls":life.calls})).await?;
            } else if stalled || life.lease_until_ms.is_some_and(|until| until <= now) {
                conversations::finish(&mut tx, id, life.generation, "interrupted").await?;
                life.retry(now)?;
                save(&mut tx, id, &life).await?;
                sql::execute(&mut tx, "UPDATE runtime_runs SET status='queued',execution=NULL,grant_hash=NULL,grant_ciphertext=NULL WHERE id=$1", params![id]).await?;
            }
            tx.commit().await?;
        }
        Ok(())
    }

    pub async fn deliveries(&self) -> Result<Vec<OutboxDelivery>> {
        let rows = sql::records(
            &mut self.pool.clone(),
            "SELECT * FROM runtime_outbox WHERE NOT delivered ORDER BY created_at LIMIT 100",
            params![],
            &[
                ("id", Type::Uuid),
                ("run_id", Type::Uuid),
                ("kind", Type::String),
                ("payload", Type::String),
                ("delivered", Type::Bool),
                ("created_at", Type::Timestamp),
            ],
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                Ok(OutboxDelivery {
                    id: row.decode("id")?,
                    run_id: row.decode("run_id")?,
                    kind: row.decode("kind")?,
                    payload: row.decode("payload")?,
                })
            })
            .collect::<artifactory_database::Result<_>>()?)
    }

    pub async fn acknowledge_delivery(
        &self,
        event: Uuid,
        run: Uuid,
        kind: &str,
        payload: &Value,
        result: &Value,
    ) -> Result<()> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let run_row = locked(&mut tx, run).await?;
        let already: bool = sql::scalar(
            &mut tx,
            "SELECT delivered FROM runtime_outbox WHERE id=$1 FOR UPDATE",
            params![event],
        )
        .await?;
        if already {
            return Ok(());
        }
        if kind == "proposal" {
            let mut life: Lifecycle = serde_json::from_value(run_row.decode("lifecycle")?)?;
            life.pending = false;
            conversations::record(&mut tx,run,life.generation,Uuid::new_v4(),"proposal_result",&json!({"proposal_id":payload["proposal_id"],"name":payload["kind"],"result":result})).await?;
            save(&mut tx, run, &life).await?;
            sql::execute(
                &mut tx,
                "UPDATE runtime_proposals SET result=$2 WHERE id=$1 AND result IS NULL",
                params![id(payload, "proposal_id")?, result],
            )
            .await?;
            if result["accepted"] == true {
                conversations::finish(
                    &mut tx,
                    run,
                    life.generation,
                    if payload["kind"] == "request_input" {
                        "needs_input"
                    } else {
                        "succeeded"
                    },
                )
                .await?;
                sql::execute(
                    &mut tx,
                    "UPDATE runtime_runs SET status=$2 WHERE id=$1 AND status<>'canceled'",
                    params![
                        run,
                        if payload["kind"] == "request_input" {
                            "needs_input"
                        } else {
                            "succeeded"
                        }
                    ],
                )
                .await?;
            }
        }
        models::Outbox::filter_by_id(event)
            .update()
            .delivered(true)
            .exec(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

pub fn database_failure_category(error: &anyhow::Error) -> Option<&'static str> {
    let backend = error
        .downcast_ref::<artifactory_database::Error>()
        .and_then(|error| match error {
            artifactory_database::Error::Backend(error) => Some(error),
            _ => None,
        })
        .or_else(|| error.downcast_ref::<toasty::Error>())?;
    Some(if backend.is_connection_pool() {
        "database_pool_timeout"
    } else if backend.is_driver_operation_failed() {
        "database_statement"
    } else {
        "database_transport"
    })
}

impl RuntimeStore {
    pub async fn connect(url: &str, key: Arc<[u8; 32]>) -> Result<Self> {
        let pool = artifactory_database::connect(
            url,
            models(),
            artifactory_database::PoolSettings::new(24),
        )
        .await?;
        artifactory_database::require_timescale(&pool).await?;
        artifactory_database::migrate(&pool, &MIGRATIONS).await?;
        Ok(Self::new(pool, key))
    }
}

pub static MIGRATIONS: toasty::migration::MigrationSet = toasty::embed_migrations!("migrations");

pub fn models() -> toasty::schema::app::ModelSet {
    toasty::models!(crate::*)
}
