//! [INPUT]: 依赖 Coordinator 持久事件、业务构建锁与固定根提交。
//! [OUTPUT]: 接纳不可变清单，单调更新摘要并补齐迟到历史。
//! [POS]: 清单受业务状态围栏；观测写入不改变业务终态。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use anyhow::{Context, Result, ensure};
use artifactory_source_checkout::{SourceManifest, SourceProgress};
use serde_json::{Value, json};
use uuid::Uuid;
impl ControlStore {
    pub(crate) async fn preparation_build(&self, run: Uuid) -> Result<Uuid> {
        Ok(sql::scalar(&mut self.pool.clone(),"SELECT b.id FROM builds b JOIN planning_runs r ON r.build_id=b.id WHERE r.id=$1 AND NOT b.cancel_requested AND b.status IN ('resolving','planning','replanning') AND r.state IN ('queued','leased')",params![run]).await?)
    }
    pub(crate) async fn commit_preparation_event(
        &self,
        event: Uuid,
        run: Uuid,
        hash: &str,
        kind: &str,
        payload: &Value,
    ) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let build: Uuid = sql::scalar(
            &mut tx,
            "SELECT build_id FROM planning_runs WHERE id=$1",
            params![run],
        )
        .await?;
        let scope:Value=sql::scalar(&mut tx,"SELECT jsonb_build_object('tenant',tenant_id,'project',project_id,'release',release_id) FROM builds WHERE id=$1",params![build]).await?;
        for (table, key, mode) in [
            ("tenants", "tenant", "SHARE"),
            ("projects", "project", "SHARE"),
            ("releases", "release", "UPDATE"),
            ("builds", "id", "UPDATE"),
        ] {
            let id = if key == "id" {
                build
            } else {
                Uuid::parse_str(scope[key].as_str().context("build scope missing")?)?
            };
            sql::execute(
                &mut tx,
                &format!("SELECT id FROM {table} WHERE id=$1 FOR {mode}"),
                params![id],
            )
            .await?;
        }
        let row:Value=sql::scalar(&mut tx,"SELECT to_jsonb(r)||jsonb_build_object('build',to_jsonb(b),'project_status',p.status,'tenant_status',t.status) FROM planning_runs r JOIN builds b ON b.id=r.build_id JOIN projects p ON p.id=b.project_id JOIN tenants t ON t.id=b.tenant_id WHERE r.id=$1 FOR UPDATE OF r",params![run]).await?;
        if let Some(prior)=sql::optional_scalar::<Value>(&mut tx,"SELECT jsonb_build_object('hash',input_hash,'result',result) FROM planning_receipts WHERE event_id=$1",params![event]).await?{
   ensure!(prior["hash"]==hash,"event ID reused");return Ok(prior["result"].clone());
  }
        let b = &row["build"];
        let mut summary = row["source_preparation"].clone();
        let ex: artifactory_planning_protocol::Execution =
            serde_json::from_value(payload["execution"].clone())?;
        ensure!(
            ex.run_id == run
                && ex.generation > 0
                && !ex.uid.is_empty()
                && !ex.name.is_empty()
                && !ex.namespace.is_empty(),
            "source event identity mismatch"
        );
        let live = b["cancel_requested"] != true
            && matches!(
                b["status"].as_str(),
                Some("resolving" | "planning" | "replanning")
            )
            && matches!(row["state"].as_str(), Some("queued" | "leased"))
            && matches!(row["project_status"].as_str(), Some("active" | "draining"))
            && matches!(row["tenant_status"].as_str(), Some("active" | "draining"));
        let latest: i64 = sql::scalar(
            &mut tx,
            "SELECT coalesce(max(generation),0) FROM planning_receipts WHERE run_id=$1",
            params![run],
        )
        .await?;
        let generation = summary["generation"].as_i64().unwrap_or(0).max(latest);
        let result = if kind == "source_ready" && !live {
            json!({"accepted":false,"error":"build canceled or planning superseded"})
        } else if kind == "source_ready" {
            let manifest: SourceManifest = serde_json::from_value(payload["manifest"].clone())?;
            manifest.validate()?;
            let root = manifest.root()?;
            ensure!(
                json!(manifest.id) == b["id"]
                    && json!(root.commit_sha) == b["commit_sha"]
                    && json!(root.upstream_url) == b["source_repository_url"],
                "source manifest differs from fixed root"
            );
            ensure!(
                row["purpose"] == "initial",
                "only initial run can bind source"
            );
            if let Some(id) = b["source_manifest_id"].as_str() {
                ensure!(id == manifest.id.to_string(), "source identity changed");
                let prior: Value = sql::scalar(
                    &mut tx,
                    "SELECT manifest FROM source_manifests WHERE id=$1",
                    params![manifest.id],
                )
                .await?;
                ensure!(
                    prior == serde_json::to_value(&manifest)?,
                    "source manifest changed"
                );
            } else {
                ensure!(b["status"] == "resolving", "source no longer resolving");
                sql::execute(&mut tx,"INSERT INTO source_manifests(id,tenant_id,project_id,commit_sha,manifest,digest) VALUES($1,$2,$3,$4,$5,$6)",params![manifest.id,Uuid::parse_str(b["tenant_id"].as_str().context("tenant missing")?)?,Uuid::parse_str(b["project_id"].as_str().context("project missing")?)?,&root.commit_sha,serde_json::to_value(&manifest)?,&manifest.digest]).await?;
                sql::execute(
                    &mut tx,
                    "UPDATE builds SET source_manifest_id=$2,status='planning' WHERE id=$1",
                    params![build, manifest.id],
                )
                .await?;
            }
            json!({"accepted":true,"source_manifest_id":manifest.id,"source_digest":manifest.digest})
        } else {
            ensure!(
                matches!(
                    kind,
                    "source_progress" | "source_local_ready" | "planning_started"
                ),
                "unknown source preparation event"
            );
            let at = payload["created_at"]
                .as_str()
                .or_else(|| payload["ready_at"].as_str())
                .or_else(|| payload["started_at"].as_str())
                .context("source event timestamp missing")?;
            let occurred = chrono::DateTime::parse_from_rfc3339(at)?;
            // 终态之后只补齐终态前产生的事实，不能新建执行证据。
            for finished in [&row["finished_at"], &b["finished_at"]] {
                if let Some(finished) = finished.as_str() {
                    ensure!(
                        occurred <= chrono::DateTime::parse_from_rfc3339(finished)?,
                        "source observation occurred after completion"
                    );
                }
            }
            if kind == "source_local_ready" {
                ensure!(
                    payload["source_manifest_id"] == b["source_manifest_id"],
                    "source ready identity changed"
                );
                let digest: String = sql::scalar(
                    &mut tx,
                    "SELECT digest FROM source_manifests WHERE id=$1",
                    params![Uuid::parse_str(
                        b["source_manifest_id"].as_str().context("source missing")?
                    )?],
                )
                .await?;
                ensure!(payload["source_digest"] == digest, "source digest changed");
            }
            let mut events = row["source_preparation_events"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            record_observation(
                &mut summary,
                &mut events,
                &ex,
                generation,
                kind,
                payload,
                at,
            )?;
            sql::execute(&mut tx,"UPDATE planning_runs SET source_preparation=$2,source_preparation_events=$3 WHERE id=$1",params![run,summary,Value::Array(events)]).await?;
            json!({"accepted":true,"historical":ex.generation<generation})
        };
        models::PlanningReceipt::create()
            .event_id(event)
            .input_hash(hash)
            .result(toasty::Json(result.clone()))
            .run_id(run)
            .generation(Some(ex.generation))
            .exec(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn record_observation(
    summary: &mut Value,
    events: &mut Vec<Value>,
    ex: &artifactory_planning_protocol::Execution,
    latest_generation: i64,
    kind: &str,
    payload: &Value,
    at: &str,
) -> Result<()> {
    let execution = serde_json::to_value(ex)?;
    for previous in events
        .iter()
        .filter(|event| event["execution"]["generation"] == ex.generation)
    {
        ensure!(
            previous["execution"] == execution,
            "source Pod changed within generation"
        );
    }
    if summary["generation"] == ex.generation {
        for (key, value) in [
            ("pod_uid", &ex.uid),
            ("pod_name", &ex.name),
            ("namespace", &ex.namespace),
        ] {
            ensure!(
                summary[key].is_null() || summary[key] == *value,
                "source Pod changed within generation"
            );
        }
    }
    let progress = if kind == "source_progress" {
        let progress: SourceProgress = serde_json::from_value(payload["progress"].clone())?;
        ensure!(
            (1..=256).contains(&progress.sequence),
            "source progress exceeds limit"
        );
        Some(progress)
    } else {
        None
    };
    let entry = if let Some(progress) = &progress {
        json!({"execution":ex,"progress":progress,"created_at":at})
    } else {
        json!({"kind":kind,"execution":ex,"created_at":at})
    };
    let prior = events.iter().find(|event| {
        event["execution"]["generation"] == ex.generation
            && if let Some(progress) = &progress {
                event["progress"]["sequence"] == progress.sequence
            } else {
                event["kind"] == kind
            }
    });
    if let Some(prior) = prior {
        ensure!(*prior == entry, "source observation replay changed");
    } else {
        ensure!(
            events.len() < 256,
            "source observation history exceeds limit"
        );
        events.push(entry);
        events.sort_by_key(|event| {
            (
                event["execution"]["generation"].as_i64().unwrap_or(0),
                event["created_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| at.timestamp_micros())
                    .unwrap_or(0),
                event["progress"]["sequence"].as_u64().unwrap_or(0),
            )
        });
    }
    if ex.generation < latest_generation {
        return Ok(());
    }
    if summary["generation"] != ex.generation {
        *summary = json!({"run_id":ex.run_id,"generation":ex.generation,"pod_uid":ex.uid,"pod_name":ex.name,"namespace":ex.namespace});
    } else {
        summary["namespace"] = json!(ex.namespace);
    }
    if let Some(progress) = progress {
        if u64::from(progress.sequence) > summary["sequence"].as_u64().unwrap_or(0) {
            if summary["ready_at"].is_null() {
                summary["current_stage"] = json!(progress.stage);
            }
            summary["state"] = json!(progress.state);
            summary["sequence"] = json!(progress.sequence);
            summary["updated_at"] = json!(at);
        }
    } else if kind == "source_local_ready" {
        summary["ready_at"] = json!(at);
        summary["current_stage"] = json!("ready");
    } else {
        summary["started_at"] = json!(at);
    }
    Ok(())
}
