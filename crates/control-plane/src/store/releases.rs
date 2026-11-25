//! [INPUT]: 依赖版本元数据、项目设备授权与本次构建的源码和目标输入。
//! [OUTPUT]: 创建版本、更新说明，创建固定镜像默认池与独立源码的构建快照。
//! [POS]: 版本元数据与构建创建的事务边界；正式交付由 release_publication 封存。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, dto};
use crate::error::{ApiError, Result};
use artifactory_database::{params, sql};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseInput {
    pub version: String,
    #[serde(default)]
    pub notes: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseUpdate {
    pub notes: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseBuildInput {
    #[serde(rename = "ref")]
    pub reference: String,
    pub goal: String,
    pub target_requirements: Value,
    pub targets: Vec<String>,
    #[serde(default = "object")]
    pub parameters: Value,
}
fn object() -> Value {
    json!({})
}
fn bad(message: &str) -> ApiError {
    ApiError::BadRequest(message.into())
}
pub(crate) fn validate_build_contract(
    goal: &str,
    requirements: &Value,
    targets: &[String],
) -> Result<()> {
    if goal.trim().is_empty() || goal.len() > 16000 {
        return Err(bad("goal must contain 1..16000 bytes"));
    }
    let list = requirements
        .as_array()
        .ok_or_else(|| bad("target_requirements must be an array"))?;
    if list.is_empty() || list.len() > 32 || targets.is_empty() || targets.len() > 32 {
        return Err(bad("build requires 1..32 targets"));
    }
    let mut keys = std::collections::BTreeSet::new();
    for item in list {
        let kind = item["kind"].as_str().unwrap_or("");
        let key = item["key"].as_str().unwrap_or("");
        if !crate::build::safe_key(key) || !keys.insert(key) {
            return Err(bad("target keys must be unique lowercase logical keys"));
        }
        let map = item.as_object().ok_or_else(|| bad("invalid target"))?;
        let allowed = match kind {
            "native" => &[
                "kind",
                "key",
                "os",
                "arch",
                "application_platform",
                "artifact_key",
                "format_suffix",
            ][..],
            "image" => &["kind", "key", "platforms"][..],
            _ => return Err(bad("target kind must be native or image")),
        };
        if map.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(bad("unknown target contract field"));
        }
        for optional in ["application_platform", "artifact_key", "format_suffix"] {
            if item
                .get(optional)
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err(bad("native optional fields must be strings or null"));
            }
        }
        if kind == "image" {
            let platforms = item["platforms"]
                .as_array()
                .ok_or_else(|| bad("image platforms are required"))?;
            if platforms.is_empty()
                || platforms.len() > 2
                || platforms
                    .iter()
                    .any(|p| !matches!(p.as_str(), Some("linux/amd64" | "linux/arm64")))
                || platforms
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != platforms.len()
            {
                return Err(bad("invalid image platforms"));
            }
            if !crate::images::enabled() {
                return Err(ApiError::Conflict(crate::images::DISABLED_MESSAGE.into()));
            }
        } else {
            let os = item["os"].as_str().unwrap_or("");
            let arch = item["arch"].as_str().unwrap_or("");
            if !["linux", "darwin", "windows"].contains(&os) || !["amd64", "arm64"].contains(&arch)
            {
                return Err(bad("invalid native platform"));
            }
            if let Some(platform) = item["application_platform"].as_str() {
                let valid = match platform {
                    "ios" | "macos" => os == "darwin",
                    "linux" => os == "linux",
                    "windows" => os == "windows",
                    "android" => os == "linux" || os == "darwin",
                    _ => false,
                };
                if !valid {
                    return Err(bad("application platform does not match host"));
                }
            }
            let artifact = item["artifact_key"].as_str().unwrap_or(key);
            let suffix = item["format_suffix"].as_str().unwrap_or(".bin");
            crate::build::delivery_filename(
                "project",
                artifact,
                "0.0.0",
                item["application_platform"].as_str().unwrap_or(os),
                arch,
                suffix,
            )?;
        }
    }
    let selected: std::collections::BTreeSet<_> = targets.iter().map(String::as_str).collect();
    if selected.len() != targets.len() || selected != keys {
        return Err(bad(
            "targets must contain every target contract key exactly once",
        ));
    }
    Ok(())
}
fn validate_delivery_contract(project: &str, version: &str, requirements: &Value) -> Result<()> {
    let mut names = std::collections::BTreeSet::new();
    for target in requirements
        .as_array()
        .ok_or_else(|| bad("target contract missing"))?
    {
        if target["kind"] != "native" {
            continue;
        }
        let name = crate::build::delivery_filename(
            project,
            target["artifact_key"]
                .as_str()
                .or_else(|| target["key"].as_str())
                .ok_or_else(|| bad("artifact key missing"))?,
            version,
            target["application_platform"]
                .as_str()
                .or_else(|| target["os"].as_str())
                .ok_or_else(|| bad("native platform missing"))?,
            target["arch"]
                .as_str()
                .ok_or_else(|| bad("native arch missing"))?,
            target["format_suffix"].as_str().unwrap_or(".bin"),
        )?;
        if !names.insert(name) {
            return Err(bad("native delivery filenames must be unique"));
        }
    }
    Ok(())
}
const PROJECTION: &str = "SELECT to_jsonb(r)||jsonb_build_object('project_name',p.name,'project_slug',p.slug,'build_count',(SELECT count(*) FROM builds b WHERE b.release_id=r.id),'selected_build_id',d.build_id,'delivery_manifest',d.manifest,'artifact_count',(SELECT count(*) FROM release_artifacts WHERE release_id=r.id)+(SELECT count(*) FROM release_images WHERE release_id=r.id)) FROM releases r JOIN projects p ON p.id=r.project_id LEFT JOIN release_deliveries d ON d.release_id=r.id";
impl ControlStore {
    pub async fn create_release(&self, project: Uuid, input: &ReleaseInput) -> Result<Uuid> {
        crate::build::validate_release_version(&input.version)?;
        if input.notes.len() > 16000 {
            return Err(bad("notes must contain at most 16000 bytes"));
        }
        let id = Uuid::new_v4();
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT require_active_project($1)",
            params![project],
        )
        .await?;
        let n = sql::execute(&mut tx,"INSERT INTO releases(id,tenant_id,project_id,version,notes) SELECT $1,tenant_id,id,$3,$4 FROM projects WHERE id=$2 AND tenant_id=$5",params![id,project,&input.version,&input.notes,self.tenant()?]).await?;
        if n != 1 {
            return Err(ApiError::NotFound);
        }
        tx.commit().await?;
        Ok(id)
    }
    pub async fn update_release(&self, id: Uuid, input: &ReleaseUpdate) -> Result<()> {
        if input.notes.len() > 16000 {
            return Err(bad("notes must contain at most 16000 bytes"));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let project: Uuid = sql::scalar(
            &mut tx,
            "SELECT project_id FROM releases WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
            params![id, self.tenant()?],
        )
        .await?;
        sql::execute(
            &mut tx,
            "SELECT require_active_project($1)",
            params![project],
        )
        .await?;
        sql::execute(
            &mut tx,
            "UPDATE releases SET notes=$2 WHERE id=$1",
            params![id, &input.notes],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn release_project(&self, id: Uuid) -> Result<Uuid> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT project_id FROM releases WHERE id=$1 AND tenant_id=$2",
            params![id, self.tenant()?],
        )
        .await?)
    }
    pub async fn release(&self, id: Uuid) -> Result<dto::Release> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            &format!("{PROJECTION} WHERE r.id=$1 AND r.tenant_id=$2"),
            params![id, self.tenant()?],
        )
        .await?)
    }
    pub async fn releases(
        &self,
        project: Option<Uuid>,
        visible: Option<Vec<Uuid>>,
    ) -> Result<Vec<dto::ReleaseSummary>> {
        let mut rows:Vec<dto::ReleaseSummary>=sql::scalars(&mut self.pool.clone(),&format!("{PROJECTION} WHERE r.tenant_id=$1 AND ($2::uuid IS NULL OR r.project_id=$2) AND ($3::uuid[] IS NULL OR r.project_id=ANY($3))"),params![self.tenant()?,project,visible]).await?;
        rows.sort_by(|a, b| {
            crate::build::compare_versions(&b.version, &a.version).then_with(|| a.id.cmp(&b.id))
        });
        Ok(rows)
    }
    pub async fn release_builds(&self, id: Uuid) -> Result<Vec<dto::BuildSummary>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT (to_jsonb(b)-'config_yaml'-'config_snapshot'-'model_config_snapshot')||jsonb_build_object('project_name',p.name,'project_slug',p.slug,'targets',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',j.id,'target',j.target,'status',j.status)),'[]') FROM jobs j WHERE j.build_id=b.id)) FROM builds b JOIN projects p ON p.id=b.project_id WHERE b.release_id=$1 AND b.tenant_id=$2 ORDER BY b.created_at DESC",params![id,self.tenant()?]).await?)
    }
    pub async fn create_release_build(
        &self,
        release: Uuid,
        actor: Uuid,
        input: &ReleaseBuildInput,
    ) -> Result<Uuid> {
        if input.reference.is_empty()
            || input.reference.trim() != input.reference
            || input.reference.len() > 255
            || input.reference.starts_with('-')
            || input.reference.contains(['\0', '\n', '\r'])
        {
            return Err(bad("invalid source ref"));
        }
        validate_build_contract(&input.goal, &input.target_requirements, &input.targets)?;
        if !input.parameters.is_object() || input.parameters.to_string().len() > 16384 {
            return Err(bad("invalid build parameters"));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let r:Value=sql::scalar(&mut tx,"SELECT to_jsonb(r)||jsonb_build_object('project',to_jsonb(p)) FROM releases r JOIN projects p ON p.id=r.project_id WHERE r.id=$1 AND r.tenant_id=$2 FOR UPDATE OF r",params![release,self.tenant()?]).await?;
        if r["status"] != "draft" {
            return Err(ApiError::Conflict("release is sealed".into()));
        }
        let project = r["project_id"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .map_err(anyhow::Error::from)?;
        sql::execute(
            &mut tx,
            "SELECT require_active_project($1)",
            params![project],
        )
        .await?;
        let p = &r["project"];
        for target in input.target_requirements.as_array().unwrap() {
            if target["kind"] != "native" {
                continue;
            }
            let os = target["os"].as_str().unwrap();
            let arch = target["arch"].as_str().unwrap();
            // 保留离线与忙碌设备的排队语义，锁定授权设备直到构建事务提交。
            let device: Option<Uuid> = sql::optional_scalar(
                &mut tx,
                "SELECT n.id FROM nodes n WHERE project_node_allowed($1,n.id) AND NOT n.revoked AND n.os=$2 AND n.arch=$3 ORDER BY n.id LIMIT 1 FOR SHARE OF n",
                params![project, os, arch],
            ).await?;
            if device.is_none() {
                return Err(bad(&format!(
                    "native target '{}' requires an authorized project device for {os}/{arch}",
                    target["key"].as_str().unwrap()
                )));
            }
        }
        validate_delivery_contract(
            p["slug"].as_str().unwrap(),
            r["version"].as_str().unwrap(),
            &input.target_requirements,
        )?;
        let model = self
            .model_snapshot(
                serde_json::from_value(p["model_profile_id"].clone())
                    .map_err(anyhow::Error::from)?,
                self.tenant()?,
            )
            .await?
            .ok_or_else(|| {
                ApiError::BadRequest("configure an enabled model profile before building".into())
            })?;
        let context = json!({"release_id":release,"release_version":r["version"],"image_builds_enabled":crate::images::enabled(),"image_builder_pool":crate::images::configured_pool()?,"image_pool_policy":crate::images::IMAGE_POOL_POLICY,"placement_version":if std::env::var("ARTIFACTORY_AI_PLACEMENT").is_ok_and(|v|v=="1"||v=="true"){1}else{0},"pool_ids":p["pool_ids"],"signing_profiles":p["signing_profiles"],"build_context":p["build_context"],"source_credentials":p["source_credentials"]});
        let id = Uuid::new_v4();
        let credential: Option<Uuid> =
            serde_json::from_value(p["credential_id"].clone()).map_err(anyhow::Error::from)?;
        sql::execute(&mut tx,"INSERT INTO builds(id,project_id,release_id,release_version,requested_ref,parameters,selected_targets,requested_by,source_repository_url,source_credential_id,goal,target_requirements,planning_context,model_config_snapshot) SELECT $1,r.project_id,r.id,r.version,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12 FROM releases r WHERE r.id=$2",params![id,release,&input.reference,&input.parameters,&input.targets,actor,p["repository_url"].as_str().unwrap(),credential,&input.goal,&input.target_requirements,context,&model]).await?;
        sql::execute(
            &mut tx,
            "UPDATE builds SET source_preparation_version=2 WHERE id=$1",
            params![id],
        )
        .await?;
        sql::execute(&mut tx,"INSERT INTO source_tasks(id,tenant_id,project_id,build_id,purpose,repository_url,reference,credential_id,bindings,deadline_at) SELECT b.id,b.tenant_id,b.project_id,b.id,'resolve',b.source_repository_url,b.requested_ref,b.source_credential_id,coalesce(b.planning_context->'source_credentials','[]'),now()+interval '15 minutes' FROM builds b WHERE b.id=$1",params![id]).await?;
        tx.commit().await?;
        Ok(id)
    }
}
