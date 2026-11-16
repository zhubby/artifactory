//! [INPUT]: 依赖租户权限、托管签名契约及不可变凭据版本。
//! [OUTPUT]: 提供凭据、方案、项目默认设置及当前可用性。
//! [POS]: 签名管理的数据库边界；秘密只以信封密文写入。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::{
    error::{ApiError, Result},
    signing::{Binding, Metadata},
};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

const PROFILE_AVAILABLE_SQL: &str = "s.enabled AND c.enabled AND v.enabled AND (v.metadata->>'not_before')::bigint<=extract(epoch FROM clock_timestamp()) AND (v.metadata->>'not_after')::bigint>extract(epoch FROM clock_timestamp()) AND (NOT s.notarize OR (v.metadata->>'notarization_available')::boolean)";

impl ControlStore {
    pub async fn signing_credentials(&self) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT jsonb_build_object('id',c.id,'name',c.name,'platform',c.platform,'enabled',c.enabled,'created_at',c.created_at,'versions',(SELECT coalesce(jsonb_agg(jsonb_build_object('id',v.id,'metadata',v.metadata,'enabled',v.enabled,'created_at',v.created_at) ORDER BY v.created_at DESC,v.id DESC),'[]') FROM signing_versions v WHERE v.credential_id=c.id)) FROM signing_credentials c WHERE c.tenant_id=$1 ORDER BY c.created_at DESC",params![self.tenant()?]).await?)
    }

    pub async fn save_signing_version(
        &self,
        credential: Uuid,
        version: Uuid,
        name: Option<&str>,
        platform: &str,
        metadata: &Metadata,
        envelope: &Value,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        if let Some(name) = name {
            sql::execute(
                &mut tx,
                "INSERT INTO signing_credentials(id,tenant_id,name,platform) VALUES($1,$2,$3,$4)",
                params![credential, self.tenant()?, name, platform],
            )
            .await?;
        }
        let found:Option<Uuid>=sql::optional_scalar(&mut tx,"SELECT id FROM signing_credentials WHERE id=$1 AND tenant_id=$2 AND platform=$3 FOR UPDATE",params![credential,self.tenant()?,platform]).await?;
        if found.is_none() {
            return Err(ApiError::NotFound);
        }
        if platform == "android" {
            let previous:Option<Value>=sql::optional_scalar(&mut tx,"SELECT metadata FROM signing_versions WHERE credential_id=$1 ORDER BY created_at DESC,id DESC LIMIT 1",params![credential]).await?;
            if previous.is_some_and(|old| {
                old["fingerprint_sha256"] != json!(metadata.fingerprint_sha256)
                    || old["purpose"] != json!(metadata.purpose)
            }) {
                return Err(ApiError::BadRequest("Android signing identity or purpose changed; create a separate credential and explicitly update project defaults".into()));
            }
        }
        sql::execute(&mut tx,"INSERT INTO signing_versions(id,tenant_id,credential_id,metadata,envelope) VALUES($1,$2,$3,$4,$5)",params![version,self.tenant()?,credential,json!(metadata),envelope]).await?;
        self.signing_audit(
            &mut tx,
            "signing_version_created",
            credential.to_string(),
            json!({"version_id":version}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_signing_enabled(&self, resource: &str, id: &str, enabled: bool) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        let (table, column) = match resource {
            "credentials" => ("signing_credentials", "id::text"),
            "versions" => ("signing_versions", "id::text"),
            "profiles" => ("signing_profiles", "name"),
            _ => return Err(ApiError::NotFound),
        };
        let changed = sql::execute(
            &mut tx,
            &format!("UPDATE {table} SET enabled=$3 WHERE tenant_id=$1 AND {column}=$2"),
            params![self.tenant()?, id, enabled],
        )
        .await?;
        if changed != 1 {
            return Err(ApiError::NotFound);
        }
        self.signing_audit(
            &mut tx,
            "signing_enabled_changed",
            id.to_owned(),
            json!({"resource":resource,"enabled":enabled}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn signing_profiles(&self) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),&format!("SELECT jsonb_build_object('profile',s.name,'credential_id',s.credential_id,'platform',c.platform,'notarize',s.notarize,'enabled',s.enabled,'credential_enabled',c.enabled,'available',({PROFILE_AVAILABLE_SQL}),'metadata',v.metadata,'version_id',v.id) FROM signing_profiles s JOIN signing_credentials c ON c.id=s.credential_id JOIN LATERAL (SELECT id,metadata,enabled FROM signing_versions WHERE credential_id=c.id ORDER BY created_at DESC,id DESC LIMIT 1) v ON true WHERE s.tenant_id=$1 ORDER BY s.name"),params![self.tenant()?]).await?)
    }

    pub async fn create_signing_profile(
        &self,
        name: &str,
        credential: Uuid,
        notarize: bool,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        let valid:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM signing_credentials c JOIN LATERAL (SELECT metadata,enabled FROM signing_versions WHERE credential_id=c.id ORDER BY created_at DESC,id DESC LIMIT 1) v ON true WHERE c.tenant_id=$1 AND c.id=$2 AND c.enabled AND v.enabled AND (NOT $3 OR (c.platform='macos' AND (v.metadata->>'notarization_available')::boolean)))",params![self.tenant()?,credential,notarize]).await?;
        if !valid {
            return Err(ApiError::BadRequest(
                "signing credential unavailable or notarization is not configured".into(),
            ));
        }
        sql::execute(&mut tx,"INSERT INTO signing_profiles(tenant_id,name,credential_id,notarize) VALUES($1,$2,$3,$4)",params![self.tenant()?,name,credential,notarize]).await?;
        self.signing_audit(
            &mut tx,
            "signing_profile_created",
            name.to_owned(),
            json!({"credential_id":credential,"notarize":notarize}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn project_signing_defaults(&self, project: Uuid) -> Result<Value> {
        self.authorize_owned("projects", project).await?;
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT signing_defaults FROM projects WHERE id=$1 AND tenant_id=$2",
            params![project, self.tenant()?],
        )
        .await?)
    }

    pub async fn project_signing_preview(&self, project: Uuid) -> Result<Vec<Value>> {
        self.authorize_owned("projects", project).await?;
        Ok(sql::scalars(&mut self.pool.clone(),&format!("SELECT jsonb_build_object('profile',s.name,'platform',c.platform,'credential_id',c.id,'version_id',v.id,'metadata',v.metadata,'notarize',s.notarize,'enabled',s.enabled AND c.enabled AND v.enabled,'available',({PROFILE_AVAILABLE_SQL})) FROM projects p,LATERAL jsonb_each_text(p.signing_defaults) d JOIN signing_profiles s ON s.name=d.value JOIN signing_credentials c ON c.id=s.credential_id JOIN LATERAL (SELECT * FROM signing_versions WHERE credential_id=c.id ORDER BY created_at DESC,id DESC LIMIT 1) v ON true WHERE p.id=$1 AND p.tenant_id=$2 AND s.tenant_id=p.tenant_id ORDER BY c.platform"),params![project,self.tenant()?]).await?)
    }

    pub async fn set_project_signing_defaults(
        &self,
        project: Uuid,
        defaults: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        if !self.lock_project_admin(&mut tx, project).await? {
            return Err(ApiError::Forbidden);
        }
        for (platform, name) in defaults {
            let valid:bool=sql::scalar(&mut tx,"SELECT EXISTS(SELECT 1 FROM signing_profiles s JOIN signing_credentials c ON c.id=s.credential_id JOIN LATERAL (SELECT enabled,metadata FROM signing_versions WHERE credential_id=c.id ORDER BY created_at DESC,id DESC LIMIT 1) v ON true WHERE s.tenant_id=$1 AND s.name=$2 AND c.platform=$3 AND s.enabled AND c.enabled AND v.enabled AND (v.metadata->>'not_after')::bigint>extract(epoch FROM clock_timestamp()) AND (NOT s.notarize OR (v.metadata->>'notarization_available')::boolean))",params![self.tenant()?,name,platform]).await?;
            if !valid {
                return Err(ApiError::BadRequest(
                    "default signing profile unavailable".into(),
                ));
            }
        }
        let names: Vec<_> = defaults.values().cloned().collect();
        sql::execute(&mut tx,"UPDATE projects SET signing_defaults=$2,signing_profiles=ARRAY(SELECT DISTINCT unnest(signing_profiles||$3::text[])) WHERE id=$1",params![project,json!(defaults),names]).await?;
        self.signing_audit(
            &mut tx,
            "project_signing_defaults_changed",
            project.to_string(),
            json!(defaults),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn signing_assignment(&self, attempt: Uuid) -> anyhow::Result<Value> {
        let (bindings,session):(Value,Uuid)=sql::tuple(&mut self.pool.clone(),"SELECT coalesce(b.planning_context->'signing_defaults','[]'),n.active_session FROM attempts a JOIN nodes n ON n.id=a.node_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE a.id=$1",params![attempt]).await?;
        let bindings: Vec<Binding> = serde_json::from_value(bindings)?;
        Ok(json!({"bindings":bindings,"session_id":session}))
    }

    async fn signing_audit(
        &self,
        tx: &mut toasty::db::Transaction<'_>,
        action: &str,
        resource: String,
        details: Value,
    ) -> Result<()> {
        let actor = self.scope.as_ref().ok_or(ApiError::TenantRequired)?.user_id;
        sql::execute(tx,"INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,$3,$4,$5)",params![self.tenant()?,actor.to_string(),action,resource,details]).await?;
        Ok(())
    }
}
