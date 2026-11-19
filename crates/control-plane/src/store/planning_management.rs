//! [INPUT]: 依赖模型配置、规划记录和项目读取权限。
//! [OUTPUT]: 保存模型方案、澄清回复和对话查询上下文。
//! [POS]: 控制台规划管理与 Coordinator 审计查询的业务边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use super::dto;
use super::models::ModelProfile;
use crate::{
    auth::Auth,
    crypto,
    error::{ApiError, Result},
};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use toasty::stmt::Type;

use uuid::Uuid;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub(crate) name: String,
    pub(crate) api: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) api_key: String,
    #[serde(default = "object")]
    pub(crate) parameters: Value,
    #[serde(default = "yes")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) is_default: bool,
    #[serde(default = "calls")]
    pub(crate) max_calls: i32,
    #[serde(default = "timeout")]
    pub(crate) timeout_seconds: i32,
}

fn object() -> Value {
    json!({})
}

fn yes() -> bool {
    true
}

fn calls() -> i32 {
    20
}

fn timeout() -> i32 {
    600
}

impl ControlStore {
    pub(crate) async fn conversation_context(&self, build: Uuid) -> anyhow::Result<Value> {
        let project: Uuid = sql::scalar(
            &mut self.pool.clone(),
            "SELECT project_id FROM builds WHERE id=$1 AND tenant_id=$2",
            params![build, self.tenant()?],
        )
        .await?;
        let runs: Vec<Value> = sql::scalars(&mut self.pool.clone(), "SELECT jsonb_build_object('run_id',id,'purpose',purpose) FROM planning_runs WHERE build_id=$1 ORDER BY created_at,id", params![build]).await?;
        Ok(json!({"project_id":project,"runs":runs}))
    }

    pub async fn model_profiles(&self) -> Result<Vec<dto::ModelProfile>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT (to_jsonb(m)-'key_ciphertext') || jsonb_build_object('has_key',key_ciphertext<>'','api_key','********') FROM model_profiles m WHERE tenant_id=$1 ORDER BY created_at", params![self.tenant()?]).await?)
    }

    pub async fn model_options(&self) -> Result<Vec<dto::ModelOption>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT jsonb_build_object('id',id,'name',name,'enabled',enabled) FROM model_profiles WHERE enabled AND tenant_id=$1 ORDER BY name", params![self.tenant()?]).await?)
    }

    pub async fn save_model_profile(
        &self,
        master_key: &[u8; 32],
        id: Uuid,
        input: ProfileInput,
        update: bool,
    ) -> Result<()> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,92))",
            params![self.tenant()?.to_string()],
        )
        .await?;
        if input.is_default {
            sql::execute(
                &mut tx,
                "UPDATE model_profiles SET is_default=false WHERE tenant_id=$1 AND is_default",
                params![self.tenant()?],
            )
            .await?;
        }
        let mut existing = if update {
            Some(ModelProfile::get_by_id(&mut tx, id).await?)
        } else {
            None
        };
        let ciphertext = if input.api_key.is_empty() || input.api_key == "********" {
            if !update {
                return Err(ApiError::BadRequest("model API key is required".into()));
            }
            existing
                .as_ref()
                .ok_or(ApiError::NotFound)?
                .key_ciphertext
                .clone()
        } else {
            crypto::seal(master_key, input.api_key.as_bytes())?
        };
        if let Some(profile) = existing.as_mut() {
            profile
                .update()
                .name(input.name)
                .api(input.api)
                .base_url(input.base_url.trim_end_matches('/'))
                .model(input.model)
                .key_ciphertext(ciphertext)
                .parameters(toasty::Json(input.parameters))
                .enabled(input.enabled)
                .is_default(input.is_default)
                .max_calls(input.max_calls)
                .timeout_seconds(input.timeout_seconds)
                .revision(toasty::stmt::increment())
                .exec(&mut tx)
                .await?;
        } else {
            ModelProfile::create()
                .tenant_id(self.tenant()?)
                .id(id)
                .name(input.name)
                .api(input.api)
                .base_url(input.base_url.trim_end_matches('/'))
                .model(input.model)
                .key_ciphertext(ciphertext)
                .parameters(toasty::Json(input.parameters))
                .enabled(input.enabled)
                .is_default(input.is_default)
                .max_calls(input.max_calls)
                .timeout_seconds(input.timeout_seconds)
                .exec(&mut tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_model_profile(&self, id: Uuid) -> Result<()> {
        super::models::ModelProfile::filter(super::models::ModelProfile::fields().id().eq(id))
            .delete()
            .exec(&mut self.pool.clone())
            .await?;
        Ok(())
    }

    pub async fn model_snapshot(
        &self,
        id: Option<Uuid>,
        tenant: Uuid,
    ) -> anyhow::Result<Option<Value>> {
        Ok(sql::optional_scalar(&mut self.pool.clone(), "SELECT to_jsonb(m) FROM model_profiles m WHERE tenant_id=$2 AND enabled AND (($1::uuid IS NOT NULL AND id=$1) OR ($1::uuid IS NULL AND is_default)) LIMIT 1", params![id,tenant]).await?)
    }

    pub async fn clarify_build(&self, id: Uuid, answer: &str, auth: &Auth) -> Result<()> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let row=sql::record(&mut tx, "SELECT project_id,status,cancel_requested,planning_context,clarification FROM builds WHERE id=$1 FOR UPDATE", params![id], &[("project_id", Type::Uuid),("status", Type::String),("cancel_requested", Type::Bool),("planning_context", Type::String),("clarification", Type::String)]).await?;
        auth.project(row.decode("project_id")?, "build")?;
        if row.decode::<String>("status")? != "needs_input"
            || row.decode::<bool>("cancel_requested")?
        {
            return Err(ApiError::Conflict("build is not waiting for input".into()));
        }
        let previous=sql::record(&mut tx, "SELECT model_config,failed_targets FROM planning_runs WHERE build_id=$1 ORDER BY created_at DESC LIMIT 1", params![id], &[("model_config", Type::String),("failed_targets", Type::List(Box::new(Type::String)))]).await?;
        let mut context: Value = row.decode("planning_context")?;
        if !context["clarifications"].is_array() {
            context["clarifications"] = json!([]);
        }
        context["clarifications"].as_array_mut().unwrap().push(
            json!({"question":row.decode::<Option<Value>>("clarification")?,"answer":answer}),
        );
        super::models::PlanningRun::create()
            .id(Uuid::new_v4())
            .build_id(id)
            .purpose("clarification")
            .model_config(toasty::Json(
                (previous.decode::<Value>("model_config")?).clone(),
            ))
            .failed_targets(previous.decode::<Vec<String>>("failed_targets")?)
            .exec(&mut tx)
            .await?;
        sql::execute(&mut tx, "UPDATE builds SET planning_context=$2,clarification=NULL,status=CASE WHEN current_plan_id IS NULL THEN 'planning' ELSE 'replanning' END WHERE id=$1", params![id, context]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn plans(&self, id: Uuid, auth: &Auth) -> Result<dto::BuildPlans> {
        let project: Uuid = sql::scalar(
            &mut self.pool.clone(),
            "SELECT project_id FROM builds WHERE id=$1",
            params![id],
        )
        .await?;
        auth.project(project, "read")?;
        let plans: Vec<dto::PlanRevision> = sql::scalars(
            &mut self.pool.clone(),
            "SELECT to_jsonb(p) FROM plan_revisions p WHERE build_id=$1 ORDER BY revision",
            params![id],
        )
        .await?;
        let runs:Vec<dto::PlanningRun>=sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(r)-'model_config'-'lease_hash' FROM planning_runs r WHERE build_id=$1 ORDER BY created_at", params![id]).await?;
        let events:Vec<dto::PlanningEvent>=sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(e) FROM planning_events e JOIN planning_runs r ON r.id=e.run_id WHERE r.build_id=$1 ORDER BY e.id DESC LIMIT 300", params![id]).await?;
        Ok(dto::BuildPlans {
            plans,
            runs,
            events,
        })
    }
}
