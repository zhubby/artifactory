//! [INPUT]: 依赖当前执行、固定根提交、源码清单和跨库回执。
//! [OUTPUT]: 冻结 tags、接纳唯一清单，验证本代源码就绪。
//! [POS]: 源码授权先于模型租约；清单承诺跨 generation 保留。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_source_checkout::{
    SourceDiagnostic, SourceManifest, SourceProgress, valid_commit, validate_repository,
};

pub(crate) fn validate_intent_source(intent: &RunIntent) -> Result<()> {
    let fixed = intent.source_manifest_id.is_some()
        && intent
            .source_digest
            .as_deref()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    ensure!(
        intent.source_manifest_id.is_some() == intent.source_digest.is_some(),
        "incomplete source identity"
    );
    if intent.context["purpose"] == "placement" {
        ensure!(
            !fixed && intent.source_resolution.is_none(),
            "placement cannot prepare source"
        );
        return Ok(());
    }
    if let Some(source) = &intent.source_resolution {
        ensure!(
            !fixed
                && intent.context["purpose"] == "initial"
                && !source.source_id.is_nil()
                && valid_commit(&source.commit_sha),
            "invalid source resolution"
        );
        validate_repository(&source.repository_url)?;
    } else {
        ensure!(fixed, "fixed source manifest required");
        let manifest: SourceManifest =
            serde_json::from_value(intent.context["source_manifest"].clone())?;
        manifest.validate()?;
        ensure!(
            Some(manifest.id) == intent.source_manifest_id
                && Some(&manifest.digest) == intent.source_digest.as_ref(),
            "intent source mismatch"
        );
    }
    Ok(())
}
fn current(row: &sql::Record, ex: &Execution) -> Result<Lifecycle> {
    let life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
    ensure!(
        !life.canceled
            && !life.pending
            && life.generation == ex.generation
            && row.decode::<String>("status")? == "provisioning"
            && row.decode::<Option<Value>>("execution")? == Some(serde_json::to_value(ex)?),
        "source execution fenced"
    );
    ensure!(
        life.deadline_ms
            .is_none_or(|d| d > Utc::now().timestamp_millis()),
        "planning deadline reached"
    );
    ensure!(
        row.decode::<Option<chrono::DateTime<Utc>>>("dispatched_at")?
            .is_none_or(|t| t + chrono::Duration::seconds(900) > Utc::now()),
        "source preparation deadline reached"
    );
    Ok(life)
}
pub(crate) fn acquire_source(
    row: &sql::Record,
    intent: &RunIntent,
    ex: &Execution,
) -> Result<Option<Value>> {
    if intent.context["purpose"] == "placement" {
        return Ok(None);
    }
    ensure!(
        row.decode::<bool>("source_bound")?,
        "source binding pending"
    );
    ensure!(
        row.decode::<Option<Value>>("source_ready_execution")? == Some(serde_json::to_value(ex)?),
        "source workspace not ready"
    );
    let value: Value = row
        .decode::<Option<Value>>("source_manifest")?
        .context("fixed manifest missing")?;
    let manifest: SourceManifest = serde_json::from_value(value.clone())?;
    manifest.validate()?;
    ensure!(
        Some(manifest.id) == row.decode("source_manifest_id")?
            && Some(manifest.digest) == row.decode("source_digest")?,
        "bound source mismatch"
    );
    Ok(Some(value))
}
pub(super) async fn acknowledge_source(
    tx: &mut toasty::db::Transaction<'_>,
    row: &sql::Record,
    result: &Value,
) -> Result<()> {
    let run: Uuid = row.decode("id")?;
    if result["accepted"] == true {
        let manifest: SourceManifest =
            serde_json::from_value(row.decode::<Value>("source_manifest")?)?;
        ensure!(
            result["source_manifest_id"] == json!(manifest.id)
                && result["source_digest"] == manifest.digest,
            "source receipt differs"
        );
        sql::execute(
            tx,
            "UPDATE planning_runs SET source_bound=true WHERE id=$1",
            params![run],
        )
        .await?;
    } else if !["canceled", "failed", "succeeded", "needs_input"]
        .contains(&row.decode::<String>("status")?.as_str())
    {
        sql::execute(
            tx,
            "UPDATE planning_runs SET status='failed' WHERE id=$1",
            params![run],
        )
        .await?;
        enqueue(
            tx,
            run,
            "failed",
            json!({"error":"Source manifest was not accepted"}),
        )
        .await?;
    }
    Ok(())
}
impl PlanningStore {
    pub async fn read_run(&self, run: Uuid) -> Result<Value> {
        let row = sql::record(
            &mut self.pool.clone(),
            "SELECT * FROM planning_runs WHERE id=$1",
            params![run],
            &run_fields(),
        )
        .await?;
        Ok(
            json!({"run_id":run,"status":row.decode::<String>("status")?,"lifecycle":row.decode::<Value>("lifecycle")?,"usage":row.decode::<Value>("usage")?,"execution":row.decode::<Option<Value>>("execution")?,"preparation":row.decode::<Value>("source_preparation")?,"source_bound":row.decode::<bool>("source_bound")?,"source_manifest":row.decode::<Option<Value>>("source_manifest")?,"source_frozen_tags":if row.decode::<Option<Value>>("source_manifest")?.is_some(){json!({})}else{row.decode::<Value>("source_frozen_tags")?}}),
        )
    }

    pub async fn preparation_pin(&self, ex: &Execution, pin: &Value) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, ex.run_id).await?;
        current(&row, ex)?;
        ensure!(pin["kind"] == "tags", "only tags can be pinned");
        let path = pin["path"].as_str().context("repository path required")?;
        artifactory_source_checkout::managed_path(std::path::Path::new("/source"), path)?;
        let tags: std::collections::BTreeMap<String, String> =
            serde_json::from_value(pin["tags"].clone())?;
        ensure!(
            tags.len() <= 100000
                && serde_json::to_vec(&tags)?.len() <= 4 * 1024 * 1024
                && tags.values().all(|v| valid_commit(v)),
            "invalid tags"
        );
        let mut frozen: Value = row.decode("source_frozen_tags")?;
        if frozen.get(path).is_none() {
            frozen[path] = serde_json::to_value(tags)?;
        }
        ensure!(
            serde_json::to_vec(&frozen)?.len() <= 4 * 1024 * 1024,
            "frozen tags too large"
        );
        sql::execute(
            &mut tx,
            "UPDATE planning_runs SET source_frozen_tags=$2 WHERE id=$1",
            params![ex.run_id, &frozen],
        )
        .await?;
        tx.commit().await?;
        Ok(json!({"tags":frozen[path]}))
    }
    pub async fn preparation_commit(&self, ex: &Execution, value: &Value) -> Result<Value> {
        let manifest: SourceManifest = serde_json::from_value(value.clone())?;
        manifest.validate()?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, ex.run_id).await?;
        current(&row, ex)?;
        if let Some(prior) = row.decode::<Option<Value>>("source_manifest")? {
            ensure!(prior == *value, "source manifest changed");
        } else {
            let intent = config(self, &row)?;
            let source = intent
                .source_resolution
                .context("source resolution required")?;
            let root = manifest.root()?;
            ensure!(
                manifest.id == source.source_id
                    && root.commit_sha == source.commit_sha
                    && root.upstream_url == source.repository_url,
                "source root changed"
            );
            let frozen: Value = row.decode("source_frozen_tags")?;
            for repo in &manifest.repositories {
                ensure!(
                    frozen[&repo.path] == serde_json::to_value(&repo.tags)?,
                    "source tags changed"
                );
            }
            let event = enqueue(
                &mut tx,
                ex.run_id,
                "source_ready",
                json!({"manifest":value,"execution":ex}),
            )
            .await?;
            sql::execute(&mut tx,"UPDATE planning_runs SET source_manifest=$2,source_manifest_id=$3,source_digest=$4,source_commit_event=$5 WHERE id=$1",params![ex.run_id,value,manifest.id,&manifest.digest,event]).await?;
        }
        let bound = row.decode::<bool>("source_bound")?;
        tx.commit().await?;
        Ok(
            json!({"accepted":bound,"pending":!bound,"source_manifest_id":manifest.id,"source_digest":manifest.digest}),
        )
    }
    pub async fn preparation_ready(
        &self,
        ex: &Execution,
        manifest: Uuid,
        digest: &str,
    ) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, ex.run_id).await?;
        current(&row, ex)?;
        ensure!(
            row.decode::<bool>("source_bound")?
                && row.decode::<Option<Uuid>>("source_manifest_id")? == Some(manifest)
                && row.decode::<Option<String>>("source_digest")?.as_deref() == Some(digest),
            "source binding pending or changed"
        );
        let value = serde_json::to_value(ex)?;
        if row.decode::<Option<Value>>("source_ready_execution")? != Some(value.clone()) {
            let at = Utc::now();
            let mut summary: Value = row.decode("source_preparation")?;
            summary["execution"] = value.clone();
            summary["current_stage"] = json!("ready");
            summary["ready_at"] = json!(at);
            sql::execute(&mut tx,"UPDATE planning_runs SET source_ready_execution=$2,source_preparation=$3 WHERE id=$1",params![ex.run_id,value,summary]).await?;
            enqueue(&mut tx,ex.run_id,"source_local_ready",json!({"execution":ex,"source_manifest_id":manifest,"source_digest":digest,"ready_at":at})).await?;
        }
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }
    pub async fn preparation_progress(
        &self,
        ex: &Execution,
        progress: &SourceProgress,
    ) -> Result<Value> {
        ensure!(
            (1..=256).contains(&progress.sequence),
            "source progress exceeds limit"
        );
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, ex.run_id).await?;
        current(&row, ex)?;
        let mut summary: Value = row.decode("source_preparation")?;
        let same = summary["execution"] == serde_json::to_value(ex)?;
        let previous = if same {
            summary["sequence"].as_u64().unwrap_or(0)
        } else {
            0
        };
        let history_key = format!("{}:{}", ex.generation, progress.sequence);
        let digest = crypto::hash(history_key);
        let event = Uuid::from_u128(u128::from_str_radix(&digest[..32], 16)?);
        let hash = crypto::hash(serde_json::to_vec(
            &json!({"execution":ex,"progress":progress}),
        )?);
        if let Some(prior) = sql::optional_scalar::<String>(
            &mut tx,
            "SELECT input_hash FROM planning_reports WHERE run_id=$1 AND event_id=$2",
            params![ex.run_id, event],
        )
        .await?
        {
            ensure!(prior == hash, "source progress replay changed");
            return Ok(json!({"ok":true}));
        }
        ensure!(
            u64::from(progress.sequence) > previous,
            "source progress arrived out of order"
        );
        let total: i64 = sql::scalar(
            &mut tx,
            "SELECT count(*) FROM planning_outbox WHERE run_id=$1 AND kind='source_progress'",
            params![ex.run_id],
        )
        .await?;
        ensure!(total < 256, "source progress exceeds run limit");
        models::Report::create()
            .run_id(ex.run_id)
            .event_id(event)
            .input_hash(hash)
            .exec(&mut tx)
            .await?;
        let at = Utc::now();
        if !same {
            summary = json!({});
        }
        summary["execution"] = serde_json::to_value(ex)?;
        summary["sequence"] = json!(progress.sequence);
        summary["progress"] = serde_json::to_value(progress)?;
        if summary["ready_at"].is_null() {
            summary["current_stage"] = json!(progress.stage);
        }
        summary["updated_at"] = json!(at);
        sql::execute(
            &mut tx,
            "UPDATE planning_runs SET source_preparation=$2 WHERE id=$1",
            params![ex.run_id, summary],
        )
        .await?;
        enqueue(
            &mut tx,
            ex.run_id,
            "source_progress",
            json!({"execution":ex,"progress":progress,"created_at":at}),
        )
        .await?;
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }
    pub async fn preparation_fail(
        &self,
        ex: &Execution,
        diagnostic: &SourceDiagnostic,
    ) -> Result<Value> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, ex.run_id).await?;
        current(&row, ex)?;
        let mut summary: Value = row.decode("source_preparation")?;
        summary["execution"] = serde_json::to_value(ex)?;
        summary["diagnostic"] = serde_json::to_value(diagnostic)?;
        sql::execute(
            &mut tx,
            "UPDATE planning_runs SET status='failed',source_preparation=$2 WHERE id=$1",
            params![ex.run_id, summary],
        )
        .await?;
        enqueue(
            &mut tx,
            ex.run_id,
            "failed",
            json!({"error":diagnostic.code.message(),"diagnostic":diagnostic,"execution":ex}),
        )
        .await?;
        tx.commit().await?;
        Ok(json!({"ok":true}))
    }
}
