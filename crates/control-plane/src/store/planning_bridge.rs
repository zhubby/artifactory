//! [INPUT]: 依赖规划运行时回执、源码清单和任务状态。
//! [OUTPUT]: 读取冻结镜像默认池，持久化回执与统计，投影两类失败并保护终态。
//! [POS]: 业务状态与独立 Coordinator 之间的事务边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use anyhow::Result;
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use toasty::stmt::Type;

use uuid::Uuid;
pub(crate) struct PlanningReceipt {
    pub input_hash: String,
    pub result: Value,
}
pub(crate) struct PlanningRunSource {
    pub build_id: Uuid,
    pub source_manifest_id: Uuid,
    pub planning_context: Value,
}
pub(crate) struct PlanningDelivery {
    pub id: i64,
    pub run_id: Uuid,
    pub kind: String,
    pub version: i64,
    pub ciphertext: Option<String>,
}
pub(crate) struct PlanningIntentContext {
    pub tenant_id: Uuid,
    pub build_id: Uuid,
    pub project_id: Uuid,
    pub source_manifest_id: Option<Uuid>,
    pub digest: Option<String>,
    pub model: Value,
    pub context: Value,
}
pub(crate) struct PlanningEventCommit<'a> {
    pub event: Uuid,
    pub run: Uuid,
    pub build: Uuid,
    pub hash: String,
    pub kind: &'a str,
    pub payload: &'a Value,
    pub validation: Option<String>,
}

fn cumulative_usage(current: Value, incoming: &Value) -> Value {
    let mut usage = current.as_object().cloned().unwrap_or_default();
    for field in ["input", "output", "cacheRead", "cacheWrite", "totalTokens"] {
        if let Some(value) = incoming[field].as_u64()
            && value >= usage.get(field).and_then(Value::as_u64).unwrap_or(0)
        {
            usage.insert(field.into(), json!(value));
        }
    }
    if let Some(cost) = incoming["cost"].as_f64()
        && cost.is_finite()
        && cost >= usage.get("cost").and_then(Value::as_f64).unwrap_or(0.0)
    {
        usage.insert("cost".into(), json!(cost));
    }
    Value::Object(usage)
}

impl ControlStore {
    pub(crate) async fn planning_tool_build(&self, run: Uuid) -> Result<Uuid> {
        let build:Uuid=sql::scalar(&mut self.pool.clone(), "SELECT b.id FROM builds b JOIN planning_runs r ON r.build_id=b.id WHERE r.id=$1 AND NOT b.cancel_requested AND (b.status IN ('planning','replanning') OR (r.purpose='placement' AND b.status IN ('queued','running'))) AND r.state IN ('queued','leased') AND (r.deadline_at IS NULL OR r.deadline_at>clock_timestamp())", params![run]).await?;
        Ok(build)
    }

    pub(crate) async fn planning_receipt(&self, event: Uuid) -> Result<Option<PlanningReceipt>> {
        let row = sql::optional_record(
            &mut self.pool.clone(),
            "SELECT input_hash,result FROM planning_receipts WHERE event_id=$1",
            params![event],
            &[("input_hash", Type::String), ("result", Type::String)],
        )
        .await?;
        Ok(row
            .map(|row| {
                Ok::<_, artifactory_database::Error>(PlanningReceipt {
                    input_hash: row.decode("input_hash")?,
                    result: row.decode("result")?,
                })
            })
            .transpose()?)
    }

    pub(crate) async fn planning_run_source(&self, run: Uuid) -> Result<PlanningRunSource> {
        let row=sql::record(&mut self.pool.clone(), "SELECT r.build_id,b.source_manifest_id,b.planning_context FROM planning_runs r JOIN builds b ON b.id=r.build_id WHERE r.id=$1", params![run], &[("build_id", Type::Uuid),("source_manifest_id", Type::Uuid),("planning_context", Type::String)]).await?;
        Ok(PlanningRunSource {
            build_id: row.decode("build_id")?,
            source_manifest_id: row.decode("source_manifest_id")?,
            planning_context: row.decode("planning_context")?,
        })
    }

    pub(crate) async fn source_manifest_digest(&self, snapshot: Uuid) -> Result<String> {
        let digest: String = sql::scalar(
            &mut self.pool.clone(),
            "SELECT digest FROM source_manifests WHERE id=$1",
            params![snapshot],
        )
        .await?;
        Ok(digest)
    }

    pub(crate) async fn commit_planning_event<F, Fut>(
        &self,
        input: PlanningEventCommit<'_>,
        prepare: F,
    ) -> Result<Value>
    where
        F: FnOnce(super::planning::SubmissionContext) -> Fut,
        Fut: std::future::Future<Output = Result<crate::execution_plan::ExecutionPlan>>,
    {
        let PlanningEventCommit {
            event,
            run,
            build,
            hash,
            kind,
            payload,
            validation,
        } = input;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;

        sql::execute(
            &mut tx,
            "SELECT id FROM builds WHERE id=$1 FOR UPDATE",
            params![build],
        )
        .await?;
        let row=sql::record(&mut tx, "SELECT r.id,r.build_id,r.purpose,r.state,r.failed_targets,r.model_config,r.lease_hash,r.lease_expires_at,r.deadline_at,r.calls,r.usage,r.error,r.created_at,r.finished_at,b.project_id,b.source_manifest_id,b.goal,b.target_requirements,b.planning_context,b.parameters,b.selected_targets,b.current_plan_id,b.revision_count,b.release_version,b.cancel_requested,b.status AS build_status FROM planning_runs r JOIN builds b ON b.id=r.build_id WHERE r.id=$1 FOR UPDATE OF r", params![run], &[("id", Type::Uuid),("build_id", Type::Uuid),("purpose", Type::String),("state", Type::String),("failed_targets", Type::List(Box::new(Type::String))),("model_config", Type::String),("lease_hash", Type::String),("lease_expires_at", Type::Timestamp),("deadline_at", Type::Timestamp),("calls", Type::I32),("usage", Type::String),("error", Type::String),("created_at", Type::Timestamp),("finished_at", Type::Timestamp),("project_id", Type::Uuid),("source_manifest_id", Type::Uuid),("goal", Type::String),("target_requirements", Type::String),("planning_context", Type::String),("parameters", Type::String),("selected_targets", Type::List(Box::new(Type::String))),("current_plan_id", Type::Uuid),("revision_count", Type::I32),("release_version", Type::String),("cancel_requested", Type::Bool),("build_status", Type::String)]).await?;
        if let Some(prior) = sql::optional_scalar::<Value>(
            &mut tx,
            "SELECT result FROM planning_receipts WHERE event_id=$1",
            params![event],
        )
        .await?
        {
            return Ok(prior);
        }
        let existing: Option<Uuid> = sql::optional_scalar(
            &mut tx,
            "SELECT id FROM plan_revisions WHERE run_id=$1",
            params![run],
        )
        .await?;
        let latest_generation: i64 = sql::scalar(
            &mut tx,
            "SELECT coalesce(max(generation),0) FROM planning_receipts WHERE run_id=$1",
            params![run],
        )
        .await?;
        let stale_generation = payload["generation"]
            .as_i64()
            .is_some_and(|generation| generation < latest_generation);
        let placement = row.decode::<String>("purpose")? == "placement";
        let active = !row.decode::<bool>("cancel_requested")?
            && ["queued", "leased"].contains(&row.decode::<String>("state")?.as_str())
            && (["planning", "replanning"]
                .contains(&row.decode::<String>("build_status")?.as_str())
                || placement
                    && ["queued", "running"]
                        .contains(&row.decode::<String>("build_status")?.as_str()))
            && (!placement
                || row
                    .decode::<Option<chrono::DateTime<chrono::Utc>>>("deadline_at")?
                    .is_some_and(|deadline| deadline > chrono::Utc::now()));
        if active
            && !stale_generation
            && matches!(kind, "failed" | "proposal")
            && let Some(calls) = payload["calls"].as_i64()
            && (0..=100).contains(&calls)
            && calls >= i64::from(row.decode::<i32>("calls")?)
        {
            let usage = cumulative_usage(row.decode("usage")?, &payload["usage"]);
            sql::execute(
                &mut tx,
                "UPDATE planning_runs SET calls=$2,usage=$3 WHERE id=$1",
                params![run, i32::try_from(calls)?, usage],
            )
            .await?;
        }
        let result = if let Some(plan) = existing {
            json!({"accepted":true,"plan_revision_id":plan})
        } else if stale_generation {
            json!({"error":"stale coordinator generation"})
        } else if !active {
            json!({"error":"build canceled or planning superseded"})
        } else if kind == "failed" {
            sql::execute(
                &mut tx,
                "UPDATE planning_runs SET state='failed',error=$2,finished_at=now() WHERE id=$1",
                params![
                    run,
                    payload["error"].as_str().unwrap_or("Pi planning failed")
                ],
            )
            .await?;
            sql::execute(
                &mut tx,
                "UPDATE builds SET status='failed',error=$2,finished_at=now() WHERE id=$1 AND NOT EXISTS(SELECT 1 FROM planning_runs WHERE id=$3 AND purpose='placement')",
                params![
                    build,
                    payload["error"].as_str().unwrap_or("Pi planning failed"),run
                ],
            )
            .await?;
            json!({"ok":true})
        } else if kind != "proposal" {
            json!({"error":"unknown coordinator event"})
        } else if payload["deadline_ms"]
            .as_i64()
            .is_none_or(|d| d <= chrono::Utc::now().timestamp_millis())
        {
            json!({"error":"planning deadline reached"})
        } else if let Some(error) = validation {
            json!({"error":error})
        } else if payload["kind"] == "submit_placements" {
            let mut savepoint = tx.transaction().await?;
            let result =
                super::placements::persist_placements(&mut savepoint, &row, &payload["payload"])
                    .await;
            match result {
                Ok(value) => {
                    savepoint.commit().await?;
                    value
                }
                Err(error) => {
                    savepoint.rollback().await?;
                    json!({"error":error.to_string()})
                }
            }
        } else if payload["kind"] == "request_input"
            && row.decode::<String>("purpose")? != "placement"
        {
            let question = payload["payload"]["question"].as_str().unwrap_or("");
            if question.trim().is_empty() || question.len() > 8000 {
                json!({"error":"a concise question is required"})
            } else {
                sql::execute(
                    &mut tx,
                    "UPDATE builds SET status='needs_input',clarification=$2 WHERE id=$1",
                    params![build, json!({"question":question})],
                )
                .await?;
                sql::execute(
                    &mut tx,
                    "UPDATE planning_runs SET state='needs_input',finished_at=now() WHERE id=$1",
                    params![run],
                )
                .await?;
                json!({"accepted":true,"needs_input":true})
            }
        } else if payload["kind"] == "submit_plan" {
            let mut savepoint = tx.transaction().await?;
            let result = async {
                let context = super::planning::submission_context(&mut savepoint, &row).await?;
                let plan = prepare(context).await?;
                super::planning::persist_plan(&mut savepoint, &row, plan).await
            }
            .await;
            match result {
                Ok(value) => {
                    savepoint.commit().await?;
                    value
                }
                Err(error) => {
                    savepoint.rollback().await?;
                    json!({"error":error.to_string()})
                }
            }
        } else {
            json!({"error":"invalid proposal kind"})
        };
        super::models::PlanningReceipt::create()
            .event_id(event)
            .input_hash(hash)
            .result(toasty::Json(result.clone()))
            .run_id(run)
            .generation(payload["generation"].as_i64())
            .exec(&mut tx)
            .await?;
        super::models::PlanningEvent::create()
            .run_id(run)
            .kind("planning_result")
            .payload(toasty::Json(result.clone()))
            .exec(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub(crate) async fn planning_deliveries(&self) -> Result<Vec<PlanningDelivery>> {
        let rows = sql::records(
            &mut self.pool.clone(),
            "SELECT * FROM planning_intents WHERE delivered_at IS NULL ORDER BY id LIMIT 64",
            params![],
            &[
                ("id", Type::I64),
                ("run_id", Type::Uuid),
                ("version", Type::I64),
                ("kind", Type::String),
                ("payload_ciphertext", Type::String),
                ("delivered_at", Type::Timestamp),
                ("created_at", Type::Timestamp),
            ],
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                Ok::<_, artifactory_database::Error>(PlanningDelivery {
                    id: row.decode("id")?,
                    run_id: row.decode("run_id")?,
                    kind: row.decode("kind")?,
                    version: row.decode("version")?,
                    ciphertext: row.decode("payload_ciphertext")?,
                })
            })
            .collect::<artifactory_database::Result<_>>()?)
    }

    pub(crate) async fn save_planning_payload(&self, event: i64, sealed: String) -> Result<String> {
        Ok(sql::scalar(&mut self.pool.clone(), "UPDATE planning_intents SET payload_ciphertext=coalesce(payload_ciphertext,$2) WHERE id=$1 RETURNING payload_ciphertext", params![event, sealed]).await?)
    }

    pub(crate) async fn planning_delivered(&self, event: i64) -> Result<()> {
        sql::execute(
            &mut self.pool.clone(),
            "UPDATE planning_intents SET delivered_at=now() WHERE id=$1",
            params![event],
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn planning_intent_context(&self, run: Uuid) -> Result<PlanningIntentContext> {
        let row=sql::record(&mut self.pool.clone(), "SELECT r.id,r.build_id,r.purpose,r.state,r.failed_targets,r.model_config,r.lease_hash,r.lease_expires_at,r.deadline_at,r.calls,r.usage,r.error,r.created_at,r.finished_at,r.tenant_id,b.project_id,b.source_manifest_id,b.goal,b.requested_ref,b.commit_sha,b.parameters,b.target_requirements,b.planning_context,b.revision_count,ss.digest,ss.manifest FROM planning_runs r JOIN builds b ON b.id=r.build_id JOIN source_manifests ss ON ss.id=b.source_manifest_id WHERE r.id=$1", params![run], &[("id", Type::Uuid),("build_id", Type::Uuid),("purpose", Type::String),("state", Type::String),("failed_targets", Type::List(Box::new(Type::String))),("model_config", Type::String),("lease_hash", Type::String),("lease_expires_at", Type::Timestamp),("deadline_at", Type::Timestamp),("calls", Type::I32),("usage", Type::String),("error", Type::String),("created_at", Type::Timestamp),("finished_at", Type::Timestamp),("tenant_id",Type::Uuid),("project_id", Type::Uuid),("source_manifest_id", Type::Uuid),("goal", Type::String),("requested_ref", Type::String),("commit_sha", Type::String),("parameters", Type::String),("target_requirements", Type::String),("planning_context", Type::String),("revision_count", Type::I32),("digest", Type::String),("manifest", Type::String)]).await?;
        let build: Uuid = row.decode("build_id")?;
        let current:Option<Value>=sql::optional_scalar(&mut self.pool.clone(), "SELECT contents FROM plan_revisions WHERE id=(SELECT current_plan_id FROM builds WHERE id=$1)", params![build]).await?;
        let failed:Vec<Value>=sql::scalars(&mut self.pool.clone(), &format!("WITH failures AS ({}) SELECT jsonb_strip_nulls(jsonb_build_object('target',target,'error',error,'failure_kind',failure_kind,'attempt_id',attempt_id,'image_execution_id',image_execution_id)) FROM failures WHERE build_id=$1 ORDER BY target", super::failures::LATEST_FAILURES), params![build]).await?;
        let placement = row.decode::<String>("purpose")? == "placement";
        let source_manifest_id: Uuid = row.decode("source_manifest_id")?;
        let context = json!({"purpose":row.decode::<String>("purpose")?,"placement_task":self.placement_context(run).await?,"build_id":build,"goal":row.decode::<Option<String>>("goal")?,"ref":row.decode::<String>("requested_ref")?,"commit":row.decode::<Option<String>>("commit_sha")?,"parameters":row.decode::<Value>("parameters")?,"target_requirements":row.decode::<Value>("target_requirements")?,"project_context":row.decode::<Value>("planning_context")?,"source_manifest":if placement{None}else{Some(row.decode::<Value>("manifest")?)},"revision":row.decode::<i32>("revision_count")?,"current_plan":current,"failed_targets":failed,"nodes":self.planning_nodes(build).await?});
        Ok(PlanningIntentContext {
            tenant_id: row.decode("tenant_id")?,
            build_id: build,
            project_id: row.decode("project_id")?,
            source_manifest_id: if placement {
                None
            } else {
                Some(source_manifest_id)
            },
            digest: if placement {
                None
            } else {
                Some(row.decode("digest")?)
            },
            model: row.decode("model_config")?,
            context,
        })
    }
}

#[cfg(test)]
#[path = "planning_bridge_tests.rs"]
mod tests;
