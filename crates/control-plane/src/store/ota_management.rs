//! [INPUT]: 依赖产品统一版本、租户发行目录和节点池锁。
//! [OUTPUT]: 读取唯一同版本发行，原子接受手动升级、重试和取消。
//! [POS]: 手动升级授权边界；版本差异和池恢复不创建更新。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use super::dto;
use crate::error::{ApiError, Result};
use artifactory_database::{params, sql};
use serde::Deserialize;
use serde_json::json;
use toasty::stmt::Type;

use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub(crate) release_id: Option<Uuid>,
    pub(crate) concurrency: Option<i32>,
    pub(crate) paused: Option<bool>,
}
impl ControlStore {
    pub(crate) async fn current_emissary_releases(&self) -> Result<Vec<dto::EmissaryRelease>> {
        Ok(self
            .emissary_releases()
            .await?
            .into_iter()
            .filter(|release| crate::ota::release_matches_product_version(&release.version))
            .collect())
    }

    pub(crate) async fn current_emissary_release(&self) -> Result<Option<dto::EmissaryRelease>> {
        let mut releases = self.current_emissary_releases().await?;
        Ok((releases.len() == 1).then(|| releases.remove(0)))
    }

    async fn validate_product_target(&self, id: Option<Uuid>) -> Result<()> {
        if let Some(id) = id
            && self.current_emissary_release().await?.map(|r| r.id) != Some(id)
        {
            return Err(ApiError::Conflict(
                "target must be the unique signed release for the Artifactory version".into(),
            ));
        }
        Ok(())
    }

    pub async fn set_node_target(&self, id: Uuid, input: &Target, actor: Uuid) -> Result<()> {
        self.validate_product_target(input.release_id).await?;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let revoked: bool = sql::scalar(
            &mut tx,
            "SELECT revoked FROM nodes WHERE id=$1 FOR UPDATE",
            params![id],
        )
        .await?;
        if revoked {
            return Err(ApiError::Conflict("revoked nodes cannot upgrade".into()));
        }
        let busy: bool = sql::scalar(&mut tx, "SELECT EXISTS(SELECT 1 FROM emissary_updates WHERE node_id=$1 AND finished_at IS NULL)", params![id])
    .await?;
        if busy {
            return Err(ApiError::Conflict(
                "an update is already active; cancel or wait for it first".into(),
            ));
        }
        sql::execute(
            &mut tx,
            "UPDATE nodes SET emissary_target_release=$2 WHERE id=$1",
            params![id, input.release_id],
        )
        .await?;
        super::models::AuditEvent::create()
            .tenant_id(self.scope.as_ref().map(|s| s.tenant_id))
            .actor(actor.to_string())
            .action("emissary_node_target_changed")
            .resource_id(id.to_string())
            .details(toasty::Json(
                (json!({"release_id":input.release_id})).clone(),
            ))
            .exec(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_pool_target(&self, id: Uuid, input: &Target, actor: Uuid) -> Result<()> {
        self.validate_product_target(input.release_id).await?;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let changed=sql::execute(&mut tx, "UPDATE node_pools SET emissary_target_release=$2,emissary_update_concurrency=coalesce($3,emissary_update_concurrency),emissary_updates_paused=coalesce($4,emissary_updates_paused) WHERE id=$1", params![id, input.release_id, input.concurrency, input.paused]).await?;
        if changed == 0 {
            return Err(ApiError::NotFound);
        }
        super::models::AuditEvent::create().tenant_id(self.scope.as_ref().map(|s|s.tenant_id)).actor(actor.to_string()).action("emissary_pool_policy_changed").resource_id(id.to_string()).details(toasty::Json((json!({"release_id":input.release_id,"concurrency":input.concurrency,"paused":input.paused})).clone())).exec(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn request_emissary_update(
        &self,
        node: Uuid,
        actor: Uuid,
        product_version: &str,
        release: Uuid,
    ) -> Result<Uuid> {
        if product_version != crate::VERSION {
            return Err(ApiError::Conflict(
                "Artifactory version changed; confirm the upgrade again".into(),
            ));
        }
        self.accept_emissary_update(node, actor, Some(release), None)
            .await
    }

    pub async fn retry_update(&self, id: Uuid, actor: Uuid) -> Result<Uuid> {
        let prior = sql::optional_record(
            &mut self.pool.clone(),
            "SELECT node_id,release_id,status FROM emissary_updates WHERE id=$1 AND tenant_id=$2",
            params![id, self.tenant()?],
            &[
                ("node_id", Type::Uuid),
                ("release_id", Type::Uuid),
                ("status", Type::String),
            ],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        if !["failed", "rolled_back", "canceled"]
            .contains(&prior.decode::<String>("status")?.as_str())
        {
            return Err(ApiError::Conflict(
                "only a failed, rolled back or canceled update can be retried".into(),
            ));
        }
        self.accept_emissary_update(
            prior.decode("node_id")?,
            actor,
            None,
            Some((id, prior.decode("release_id")?)),
        )
        .await
    }

    async fn accept_emissary_update(
        &self,
        node: Uuid,
        actor: Uuid,
        confirmed_release: Option<Uuid>,
        retry: Option<(Uuid, Uuid)>,
    ) -> Result<Uuid> {
        let auth = self.scope.as_ref().ok_or(ApiError::TenantRequired)?;
        auth.session_role("admin")?;
        if actor != auth.user_id {
            return Err(ApiError::Forbidden);
        }
        let tenant = self.tenant()?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // 发行目录使用独立租户互斥锁；共享租户行锁不反转节点写入触发器锁序。
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,105))",
            params![tenant.to_string()],
        )
        .await?;
        let active: Option<Uuid> = sql::optional_scalar(
            &mut tx,
            "SELECT id FROM tenants WHERE id=$1 AND status='active' FOR SHARE",
            params![tenant],
        )
        .await?;
        if active.is_none() {
            return Err(ApiError::Conflict("tenant is not active".into()));
        }
        let member: Option<Uuid> = sql::optional_scalar(&mut tx,
            "SELECT user_id FROM tenant_members WHERE tenant_id=$1 AND user_id=$2 AND role IN ('owner','admin') FOR SHARE", params![tenant,actor]).await?;
        if member.is_none() {
            return Err(ApiError::Forbidden);
        }
        let session: Option<String> = sql::optional_scalar(&mut tx,
            "SELECT token_hash FROM sessions WHERE token_hash=$1 AND user_id=$2 AND expires_at>clock_timestamp() FOR SHARE",
            params![&auth.credential_hash, actor]).await?;
        if session.is_none() {
            return Err(ApiError::Unauthorized);
        }
        let pool: Uuid = sql::optional_scalar(
            &mut tx,
            "SELECT pool_id FROM nodes WHERE id=$1 AND tenant_id=$2",
            params![node, tenant],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,57))",
            params![pool.to_string()],
        )
        .await?;
        let policy = sql::optional_record(&mut tx,
            "SELECT emissary_updates_paused,emissary_update_concurrency FROM node_pools WHERE id=$1 AND tenant_id=$2 FOR SHARE",
            params![pool,tenant], &[("emissary_updates_paused",Type::Bool),("emissary_update_concurrency",Type::I32)]).await?.ok_or(ApiError::NotFound)?;
        let n = sql::optional_record(&mut tx,
            "SELECT pool_id,emissary_current_version FROM nodes WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
            params![node,tenant], &[("pool_id",Type::Uuid),("emissary_current_version",Type::String)]).await?.ok_or(ApiError::NotFound)?;
        if n.decode::<Uuid>("pool_id")? != pool {
            return Err(ApiError::Conflict(
                "node changed pools; retry the request".into(),
            ));
        }
        let releases: Vec<Uuid> = sql::scalars(
            &mut tx,
            "SELECT id FROM emissary_releases WHERE tenant_id=$1 AND version=ANY($2) FOR SHARE",
            params![tenant, crate::ota::current_release_versions()],
        )
        .await?;
        let authorized: bool = sql::scalar(&mut tx,
            "SELECT EXISTS(SELECT 1 FROM sessions s JOIN tenant_members m ON m.user_id=s.user_id JOIN tenants t ON t.id=m.tenant_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.expires_at>clock_timestamp() AND m.tenant_id=$3 AND m.role IN ('owner','admin') AND t.status='active')",
            params![&auth.credential_hash,actor,tenant]).await?;
        if !authorized {
            return Err(ApiError::Forbidden);
        }
        if releases.len() != 1
            || confirmed_release.is_some_and(|release| releases[0] != release)
            || retry.is_some_and(|(_, release)| releases[0] != release)
        {
            return Err(ApiError::Conflict(
                "upgrade requires the unique signed release matching the Artifactory version"
                    .into(),
            ));
        }
        let target = releases[0];
        if let Some(active) = sql::optional_record(
            &mut tx,
            "SELECT id,release_id FROM emissary_updates WHERE node_id=$1 AND finished_at IS NULL",
            params![node],
            &[("id", Type::Uuid), ("release_id", Type::Uuid)],
        )
        .await?
        {
            if active.decode::<Uuid>("release_id")? != target {
                return Err(ApiError::Conflict(
                    "an update with a different frozen target is already active".into(),
                ));
            }
            let id = active.decode("id")?;
            tx.commit().await?;
            return Ok(id);
        }
        let eligible: bool = sql::scalar(&mut tx,
            "SELECT NOT revoked AND certificate_expires_at>clock_timestamp() AND active_session IS NOT NULL AND connection_owner IS NOT NULL AND last_seen>clock_timestamp()-interval '30 seconds' AND capabilities @> '[\"emissary-ota-v1\"]'::jsonb FROM nodes WHERE id=$1",
            params![node]).await?;
        if !eligible {
            return Err(ApiError::Conflict(
                "node must be online with a valid certificate and OTA launcher".into(),
            ));
        }
        if n.decode::<Option<String>>("emissary_current_version")?
            .as_deref()
            .is_some_and(crate::ota::matches_product_version)
        {
            return Err(ApiError::Conflict(
                "node already matches the Artifactory version".into(),
            ));
        }
        if policy.decode::<bool>("emissary_updates_paused")? {
            return Err(ApiError::Conflict("pool upgrades are paused".into()));
        }
        let count: i64 = sql::scalar(
            &mut tx,
            "SELECT count(*) FROM emissary_updates WHERE pool_id=$1 AND finished_at IS NULL",
            params![pool],
        )
        .await?;
        if count >= i64::from(policy.decode::<i32>("emissary_update_concurrency")?) {
            return Err(ApiError::Conflict("pool upgrade capacity is in use".into()));
        }
        let id = Uuid::new_v4();
        super::models::EmissaryUpdate::create()
            .id(id)
            .node_id(node)
            .pool_id(pool)
            .release_id(target)
            .previous_version(n.decode::<Option<String>>("emissary_current_version")?)
            .retry_of(retry.map(|(id, _)| id))
            .exec(&mut tx)
            .await?;
        sql::execute(
            &mut tx,
            "UPDATE nodes SET ota_draining=true WHERE id=$1",
            params![node],
        )
        .await?;
        super::models::AuditEvent::create()
            .tenant_id(tenant)
            .actor(actor.to_string())
            .action(if retry.is_some() {
                "emissary_update_retried"
            } else {
                "emissary_update_requested"
            })
            .resource_id(id.to_string())
            .details(toasty::Json(
                json!({"node_id":node,"release_id":target,"retry_of":retry.map(|(id,_)|id)}),
            ))
            .exec(&mut tx)
            .await?;
        // 写入和审计触发器也能等待；提交前再次校验原会话和设备时钟。
        let still_authorized: bool = sql::scalar(&mut tx,
            "SELECT EXISTS(SELECT 1 FROM sessions s JOIN tenant_members m ON m.user_id=s.user_id JOIN tenants t ON t.id=m.tenant_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.expires_at>clock_timestamp() AND m.tenant_id=$3 AND m.role IN ('owner','admin') AND t.status='active')",
            params![&auth.credential_hash,actor,tenant]).await?;
        if !still_authorized {
            return Err(ApiError::Forbidden);
        }
        let still_eligible: bool = sql::scalar(&mut tx,
            "SELECT NOT revoked AND certificate_expires_at>clock_timestamp() AND active_session IS NOT NULL AND connection_owner IS NOT NULL AND last_seen>clock_timestamp()-interval '30 seconds' FROM nodes WHERE id=$1", params![node]).await?;
        if !still_eligible {
            return Err(ApiError::Conflict(
                "node became unavailable before the upgrade was accepted".into(),
            ));
        }
        tx.commit().await?;
        Ok(id)
    }

    pub async fn cancel_update(&self, id: Uuid, actor: Uuid) -> Result<()> {
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let node: Uuid = sql::scalar(
            &mut tx,
            "SELECT node_id FROM emissary_updates WHERE id=$1",
            params![id],
        )
        .await?;
        sql::execute(
            &mut tx,
            "SELECT id FROM nodes WHERE id=$1 FOR UPDATE",
            params![node],
        )
        .await?;
        let row=sql::optional_record(&mut tx, "UPDATE emissary_updates SET cancel_requested=true,last_command_at=NULL,updated_at=now() WHERE id=$1 AND finished_at IS NULL AND status IN ('queued','offered','downloading','draining') RETURNING status", params![id], &[("status", Type::String)]).await?;
        let Some(row) = row else {
            return Err(ApiError::Conflict(
                "update is activating or already finished".into(),
            ));
        };
        if row.decode::<String>("status")? == "queued" {
            sql::execute(
                &mut tx,
                "UPDATE emissary_updates SET status='canceled',finished_at=now() WHERE id=$1",
                params![id],
            )
            .await?;
            sql::execute(
                &mut tx,
                "UPDATE nodes SET ota_draining=false WHERE id=$1",
                params![node],
            )
            .await?;
        }
        super::models::AuditEvent::create()
            .tenant_id(self.scope.as_ref().map(|s| s.tenant_id))
            .actor(actor.to_string())
            .action("emissary_update_cancel_requested")
            .resource_id(id.to_string())
            .details(toasty::Json(serde_json::json!({})))
            .exec(&mut tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn emissary_releases(&self) -> Result<Vec<dto::EmissaryRelease>> {
        Ok(sql::scalars(
            &mut self.pool.clone(),
            "SELECT to_jsonb(r) FROM emissary_releases r WHERE r.tenant_id=$1 ORDER BY created_at DESC",
            params![self.tenant()?],
        )
        .await?)
    }

    pub async fn emissary_updates(&self) -> Result<Vec<dto::EmissaryUpdate>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(u)||jsonb_build_object('version',r.version,'node_name',n.name) FROM emissary_updates u JOIN emissary_releases r ON r.id=u.release_id JOIN nodes n ON n.id=u.node_id WHERE u.tenant_id=$1 AND ($2 OR EXISTS(SELECT 1 FROM projects p WHERE p.id=ANY($3) AND project_node_allowed(p.id,n.id))) ORDER BY u.created_at DESC LIMIT 500", params![self.tenant()?,self.scoped_admin(),self.scoped_projects().unwrap_or_default()]).await?)
    }
}
