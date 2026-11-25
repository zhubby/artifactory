//! [INPUT]: 依赖全局账户角色、租户授权锁和签名版本记录。
//! [OUTPUT]: 保存设备公开地址，并原子签发安装凭证和审计。
//! [POS]: 首次安装的持久化边界；全局地址不继承租户管理员权限。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{ControlStore, dto};
use crate::error::{ApiError, Result};
use artifactory_database::{params, sql};
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

impl ControlStore {
    pub async fn system_administrator(&self, user: Uuid) -> Result<bool> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=$1 AND role='admin')",
            params![user],
        )
        .await?)
    }

    pub async fn device_public_endpoint(&self) -> Result<String> {
        Ok(sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT device_endpoint FROM node_installation_settings WHERE singleton=true",
            params![],
        )
        .await?
        .unwrap_or_default())
    }

    pub async fn set_device_public_endpoint(&self, user: Uuid, endpoint: &str) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let role: Option<String> = sql::optional_scalar(
            &mut tx,
            "SELECT role FROM users WHERE id=$1 FOR SHARE",
            params![user],
        )
        .await?;
        if role.as_deref() != Some("admin") {
            return Err(ApiError::Forbidden);
        }
        sql::execute(&mut tx,
            "INSERT INTO node_installation_settings(singleton,device_endpoint,updated_by) VALUES(true,$1,$2) ON CONFLICT(singleton) DO UPDATE SET device_endpoint=excluded.device_endpoint,updated_by=excluded.updated_by,updated_at=clock_timestamp()",
            params![endpoint,user],
        ).await?;
        sql::execute(&mut tx,
            "INSERT INTO audit_events(actor,action,resource_id,details) VALUES($1,'device_public_endpoint_changed','system/node-installation',$2)",
            params![user.to_string(),json!({"device_endpoint":endpoint})],
        ).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn installation_release(&self, id: Uuid) -> Result<dto::EmissaryRelease> {
        sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT to_jsonb(r) FROM emissary_releases r WHERE id=$1 AND tenant_id=$2",
            params![id, self.tenant()?],
        )
        .await?
        .ok_or(ApiError::NotFound)
    }

    pub(crate) async fn installation_pool(&self, id: Uuid) -> Result<()> {
        self.authorize_owned("node_pools", id).await
    }

    pub(crate) async fn installation_credential(
        &self,
        id: Uuid,
        hash: &str,
        pool: Uuid,
        release: Uuid,
        platform: (&str, &str),
        deadline: DateTime<Utc>,
    ) -> Result<DateTime<Utc>> {
        let auth = self.scope.as_ref().ok_or(ApiError::TenantRequired)?;
        auth.session_role("admin")?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        self.authorize_owned_with(&mut tx, "node_pools", pool)
            .await?;
        self.authorize_owned_with(&mut tx, "emissary_releases", release)
            .await?;
        let expires_at = sql::optional_scalar(
            &mut tx,
            "INSERT INTO enrollment_tokens(id,token_hash,pool_id,expires_at) SELECT $1,$2,$3,$4 WHERE $4>clock_timestamp() RETURNING expires_at",
            params![id,hash,pool,deadline],
        ).await?.ok_or_else(|| ApiError::Conflict("installation materials expired while waiting; generate again".into()))?;
        sql::execute(&mut tx,
            "INSERT INTO audit_events(tenant_id,actor,action,resource_id,details) VALUES($1,$2,'node_installation_created',$3,$4)",
            params![self.tenant()?,auth.user_id.to_string(),id.to_string(),json!({"pool_id":pool,"release_id":release,"os":platform.0,"arch":platform.1})],
        ).await?;
        tx.commit().await?;
        Ok(expires_at)
    }
}
