//! Tenant authority and membership. No request may choose a default tenant implicitly.
use super::ControlStore;
use crate::{
    auth::Auth,
    crypto,
    error::{ApiError, Result},
};
use artifactory_database::{params, sql};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAccessMode {
    Pools,
    Nodes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAccess {
    pub mode: NodeAccessMode,
    pub ids: Vec<Uuid>,
}

impl ControlStore {
    pub fn for_auth(&self, auth: &Auth) -> Self {
        let mut store = self.clone();
        store.scope = Some(auth.clone());
        store
    }

    pub(crate) fn tenant(&self) -> Result<Uuid> {
        self.scope
            .as_ref()
            .map(|s| s.tenant_id)
            .ok_or(ApiError::TenantRequired)
    }

    pub(crate) fn scoped_projects(&self) -> Option<Vec<Uuid>> {
        self.scope.as_ref().and_then(Auth::visible_projects)
    }

    pub(crate) fn scoped_admin(&self) -> bool {
        self.scope.as_ref().is_some_and(Auth::tenant_admin)
    }

    pub async fn authorize_tenant(&self, auth: &mut Auth, tenant: Uuid) -> Result<()> {
        let role: String = sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT role FROM tenant_members WHERE tenant_id=$1 AND user_id=$2",
            params![tenant, auth.user_id],
        )
        .await?
        .ok_or(ApiError::Forbidden)?;
        if auth.token_projects.is_some() && auth.tenant_id != tenant {
            return Err(ApiError::Forbidden);
        }
        auth.tenant_id = tenant;
        auth.role = role;
        let roles:Vec<(Uuid,String)>=sql::tuples(&mut self.pool.clone(),"SELECT p.id,CASE WHEN $3 IN ('owner','admin') THEN 'admin' ELSE m.role END FROM projects p LEFT JOIN project_members m ON m.project_id=p.id AND m.user_id=$2 WHERE p.tenant_id=$1 AND ($3 IN ('owner','admin') OR m.user_id IS NOT NULL)",params![tenant,auth.user_id,&auth.role]).await?;
        auth.project_roles = roles.into_iter().collect();
        Ok(())
    }

    /// Protect direct-ID endpoints before loading their contents. Lists additionally filter in SQL.
    pub async fn authorize_resource(&self, kind: &str, id: Uuid, auth: &Auth) -> Result<()> {
        let project_sql = match kind {
            "projects" => Some("SELECT id FROM projects WHERE id=$1"),
            "builds" => Some("SELECT project_id FROM builds WHERE id=$1"),
            "jobs" => Some(
                "SELECT b.project_id FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=$1",
            ),
            "attempts" => Some(
                "SELECT b.project_id FROM attempts a JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE a.id=$1",
            ),
            "artifacts" => Some(
                "SELECT b.project_id FROM artifacts ar JOIN attempts a ON a.id=ar.attempt_id JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id WHERE ar.id=$1",
            ),
            "releases" => Some("SELECT project_id FROM releases WHERE id=$1"),
            _ => None,
        };
        if let Some(query) = project_sql {
            let project: Uuid = sql::optional_scalar(&mut self.pool.clone(), query, params![id])
                .await?
                .ok_or(ApiError::NotFound)?;
            // A write-only API token can reach build operations; the handler checks its operation.
            if !auth
                .visible_projects()
                .unwrap_or_default()
                .contains(&project)
            {
                return Err(ApiError::Forbidden);
            }
            return Ok(());
        }
        let table = match kind {
            "nodes" => "nodes",
            "pools" => "node_pools",
            "model-profiles" => "model_profiles",
            "ai-workers" => "ai_workers",
            "emissary-updates" => "emissary_updates",
            "emissary-releases" => "emissary_releases",
            "credentials" => "credentials",
            // Token mutation also checks its user ID.
            "tokens" => "api_tokens",
            _ => return Err(ApiError::Forbidden),
        };
        let owned: bool = sql::scalar(
            &mut self.pool.clone(),
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=$1 AND tenant_id=$2)"),
            params![id, auth.tenant_id],
        )
        .await?;
        if !owned {
            return Err(ApiError::NotFound);
        }
        if kind == "nodes" && !auth.tenant_admin() {
            let visible:bool=sql::scalar(&mut self.pool.clone(),"SELECT EXISTS(SELECT 1 FROM projects p WHERE p.id=ANY($2) AND project_node_allowed(p.id,$1))",params![id,auth.visible_projects().unwrap_or_default()]).await?;
            if !visible {
                return Err(ApiError::Forbidden);
            }
        } else if !matches!(kind, "nodes" | "tokens") && !auth.tenant_admin() {
            return Err(ApiError::Forbidden);
        }
        Ok(())
    }

    pub(crate) async fn authorize_owned(&self, table: &str, id: Uuid) -> Result<()> {
        self.authorize_owned_with(&mut self.pool.clone(), table, id)
            .await
    }

    pub(super) async fn authorize_owned_with(
        &self,
        executor: &mut dyn toasty::db::Executor,
        table: &str,
        id: Uuid,
    ) -> Result<()> {
        // Table is selected by store code, never an HTTP field.
        let owned: bool = sql::scalar(
            executor,
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=$1 AND tenant_id=$2)"),
            params![id, self.tenant()?],
        )
        .await?;
        if !owned {
            return Err(ApiError::BadRequest(
                "resource must belong to current tenant".into(),
            ));
        }
        Ok(())
    }

    pub async fn tenants(&self, user: Uuid) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT jsonb_build_object('id',t.id,'slug',t.slug,'name',t.name,'role',m.role,'owner_id',t.owner_id,'created_at',t.created_at) FROM tenants t JOIN tenant_members m ON m.tenant_id=t.id WHERE m.user_id=$1 ORDER BY t.created_at,t.id",params![user]).await?)
    }

    /// Resolve only a current member's tenant. Unknown and inaccessible slugs are indistinguishable.
    pub async fn tenant_by_slug(&self, user: Uuid, slug: &str) -> Result<Value> {
        sql::optional_scalar(&mut self.pool.clone(), "SELECT jsonb_build_object('id',t.id,'slug',t.slug,'name',t.name,'role',m.role,'owner_id',t.owner_id,'created_at',t.created_at) FROM tenants t JOIN tenant_members m ON m.tenant_id=t.id WHERE m.user_id=$1 AND t.slug=$2", params![user,slug]).await?.ok_or(ApiError::NotFound)
    }

    pub async fn tenant_slug(&self, id: Uuid) -> Result<String> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT slug FROM tenants WHERE id=$1",
            params![id],
        )
        .await?)
    }

    pub async fn create_tenant(&self, user: Uuid, name: &str, key: Uuid) -> Result<Uuid> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // Serialize retries before creating the default pool, including simultaneous requests.
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91))",
            params![format!("{user}:{key}")],
        )
        .await?;
        if let Some(id) = sql::optional_scalar::<Uuid>(
            &mut tx,
            "SELECT id FROM tenants WHERE created_by=$1 AND creation_key=$2",
            params![user, key],
        )
        .await?
        {
            tx.commit().await?;
            return Ok(id);
        }
        let id = Uuid::new_v4();
        sql::execute(
            &mut tx,
            "INSERT INTO tenants(id,name,owner_id,created_by,creation_key) VALUES($1,$2,$3,$3,$4)",
            params![id, name, user, key],
        )
        .await?;
        sql::execute(
            &mut tx,
            "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'owner')",
            params![id, user],
        )
        .await?;
        sql::execute(
            &mut tx,
            "INSERT INTO node_pools(id,tenant_id,name,is_default) VALUES($1,$2,'Default',true)",
            params![Uuid::new_v4(), id],
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub(crate) async fn bootstrap_tenant(&self, username: String) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(724319912)",
            params![],
        )
        .await?;
        let user: Uuid = sql::scalar(
            &mut tx,
            "SELECT id FROM users WHERE username=$1",
            params![username],
        )
        .await?;
        sql::execute(&mut tx,"INSERT INTO tenants(id,name,owner_id,created_by,creation_key) VALUES('00000000-0000-0000-0000-000000000001','Default',$1,$1,'00000000-0000-0000-0000-000000000001') ON CONFLICT(id) DO NOTHING",params![user]).await?;
        // Only the initial bootstrap account is added. Subsequent startup cannot restore revoked authority.
        sql::execute(&mut tx,"INSERT INTO tenant_members(tenant_id,user_id,role) SELECT id,owner_id,'owner' FROM tenants t WHERE t.owner_id=$1 AND NOT EXISTS(SELECT 1 FROM tenant_members m WHERE m.tenant_id=t.id) ON CONFLICT DO NOTHING",params![user]).await?;
        sql::execute(&mut tx,"INSERT INTO node_pools(id,tenant_id,name,is_default) VALUES('00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-000000000001','Default',true) ON CONFLICT(id) DO NOTHING",params![]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn tenant_members(&self) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT jsonb_build_object('id',u.id,'user_id',u.id,'username',u.username,'role',m.role,'created_at',m.created_at) FROM tenant_members m JOIN users u ON u.id=m.user_id WHERE m.tenant_id=$1 ORDER BY m.created_at,u.id",params![self.tenant()?]).await?)
    }

    pub async fn add_member(&self, username: &str, role: &str) -> Result<Uuid> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let tenant = self.tenant()?;
        self.lock_admin(&mut tx).await?;
        let user: Uuid = sql::optional_scalar(
            &mut tx,
            "SELECT id FROM users WHERE username=$1",
            params![username],
        )
        .await?
        .ok_or(ApiError::NotFound)?;
        sql::execute(&mut tx,"INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",params![tenant,user,role]).await?;
        tx.commit().await?;
        Ok(user)
    }

    async fn lock_admin(&self, tx: &mut toasty::db::Transaction<'_>) -> Result<()> {
        let user = self.scope.as_ref().ok_or(ApiError::TenantRequired)?.user_id;
        sql::execute(
            tx,
            // Serialize membership changes without blocking task inserts' foreign-key locks.
            "SELECT id FROM tenants WHERE id=$1 FOR NO KEY UPDATE",
            params![self.tenant()?],
        )
        .await?;
        let allowed:bool=sql::scalar(tx,"SELECT EXISTS(SELECT 1 FROM tenant_members WHERE tenant_id=$1 AND user_id=$2 AND role IN ('owner','admin'))",params![self.tenant()?,user]).await?;
        if !allowed {
            return Err(ApiError::Forbidden);
        }
        Ok(())
    }

    /// Serialize project administration with tenant revocation and other member changes.
    /// Lock the tenant first to keep the same ordering as tenant administration.
    pub(super) async fn lock_project_admin(
        &self,
        tx: &mut toasty::db::Transaction<'_>,
        project: Uuid,
    ) -> Result<bool> {
        let actor = self.scope.as_ref().ok_or(ApiError::TenantRequired)?;
        actor.project(project, "admin")?;
        sql::execute(
            tx,
            "SELECT id FROM tenants WHERE id=$1 FOR SHARE",
            params![self.tenant()?],
        )
        .await?;
        let found: Option<Uuid> = sql::optional_scalar(
            tx,
            "SELECT id FROM projects WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
            params![project, self.tenant()?],
        )
        .await?;
        if found.is_none() {
            return Err(ApiError::NotFound);
        }
        let role: Option<String> = sql::optional_scalar(
            tx,
            "SELECT CASE WHEN t.role IN ('owner','admin') THEN 'tenant_admin' ELSE m.role END FROM tenant_members t LEFT JOIN project_members m ON m.tenant_id=t.tenant_id AND m.user_id=t.user_id AND m.project_id=$3 WHERE t.tenant_id=$1 AND t.user_id=$2 AND (t.role IN ('owner','admin') OR m.role='admin')",
            params![self.tenant()?, actor.user_id, project],
        )
        .await?;
        match role.as_deref() {
            Some("tenant_admin") => Ok(true),
            Some("admin") => Ok(false),
            _ => Err(ApiError::Forbidden),
        }
    }

    pub async fn change_member(&self, user: Uuid, role: Option<&str>) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        let owner: Uuid = sql::scalar(
            &mut tx,
            "SELECT owner_id FROM tenants WHERE id=$1",
            params![self.tenant()?],
        )
        .await?;
        if owner == user {
            return Err(ApiError::Conflict(
                "transfer ownership before changing the owner".into(),
            ));
        }
        if let Some(role) = role {
            sql::execute(
                &mut tx,
                "UPDATE tenant_members SET role=$3 WHERE tenant_id=$1 AND user_id=$2",
                params![self.tenant()?, user, role],
            )
            .await?;
        } else {
            sql::execute(
                &mut tx,
                "DELETE FROM tenant_members WHERE tenant_id=$1 AND user_id=$2",
                params![self.tenant()?, user],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn transfer_owner(&self, user: Uuid) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        let actor = self.scope.as_ref().ok_or(ApiError::TenantRequired)?.user_id;
        let owner: Uuid = sql::scalar(
            &mut tx,
            "SELECT owner_id FROM tenants WHERE id=$1",
            params![self.tenant()?],
        )
        .await?;
        if owner != actor {
            return Err(ApiError::Forbidden);
        }
        let exists: bool = sql::scalar(
            &mut tx,
            "SELECT EXISTS(SELECT 1 FROM tenant_members WHERE tenant_id=$1 AND user_id=$2)",
            params![self.tenant()?, user],
        )
        .await?;
        if !exists {
            return Err(ApiError::BadRequest(
                "new owner must be a tenant member".into(),
            ));
        }
        sql::execute(
            &mut tx,
            "UPDATE tenant_members SET role='admin' WHERE tenant_id=$1 AND role='owner'",
            params![self.tenant()?],
        )
        .await?;
        sql::execute(
            &mut tx,
            "UPDATE tenant_members SET role='owner' WHERE tenant_id=$1 AND user_id=$2",
            params![self.tenant()?, user],
        )
        .await?;
        sql::execute(
            &mut tx,
            "UPDATE tenants SET owner_id=$2 WHERE id=$1",
            params![self.tenant()?, user],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn invitations(&self) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT to_jsonb(i)-'token_hash' FROM tenant_invitations i WHERE tenant_id=$1 ORDER BY created_at DESC LIMIT 100",params![self.tenant()?]).await?)
    }

    pub async fn invite(&self, role: &str) -> Result<Value> {
        let id = Uuid::new_v4();
        let token = crypto::token();
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        sql::execute(&mut tx,"INSERT INTO tenant_invitations(id,tenant_id,token_hash,role,created_by) VALUES($1,$2,$3,$4,$5)",params![id,self.tenant()?,crypto::hash(&token),role,self.scope.as_ref().ok_or(ApiError::TenantRequired)?.user_id]).await?;
        tx.commit().await?;
        Ok(json!({"id":id,"token":token,"expires_seconds":259200}))
    }

    pub async fn revoke_invitation(&self, id: Uuid) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        sql::execute(&mut tx,"UPDATE tenant_invitations SET revoked_at=coalesce(revoked_at,now()) WHERE id=$1 AND tenant_id=$2",params![id,self.tenant()?]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn invitation_preview(&self, token: &str) -> Result<Value> {
        sql::optional_scalar(&mut self.pool.clone(),"SELECT jsonb_build_object('tenant_id',t.id,'tenant_name',t.name,'role',i.role,'expires_at',i.expires_at,'available',i.revoked_at IS NULL AND i.accepted_at IS NULL AND i.expires_at>now()) FROM tenant_invitations i JOIN tenants t ON t.id=i.tenant_id WHERE i.token_hash=$1",params![crypto::hash(token)]).await?.ok_or(ApiError::NotFound)
    }

    pub async fn accept_invitation(&self, token: &str, user: Uuid) -> Result<Uuid> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row:Value=sql::optional_scalar(&mut tx,"SELECT to_jsonb(i) FROM tenant_invitations i WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>now() AND accepted_at IS NULL FOR UPDATE",params![crypto::hash(token)]).await?.ok_or(ApiError::Conflict("invitation expired, revoked or already used".into()))?;
        let tenant = Uuid::parse_str(row["tenant_id"].as_str().ok_or(ApiError::NotFound)?)
            .map_err(anyhow::Error::from)?;
        sql::execute(&mut tx,"INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",params![tenant,user,row["role"].as_str().ok_or(ApiError::NotFound)?]).await?;
        sql::execute(
            &mut tx,
            "UPDATE tenant_invitations SET accepted_at=now(),accepted_by=$2 WHERE token_hash=$1",
            params![crypto::hash(token), user],
        )
        .await?;
        tx.commit().await?;
        Ok(tenant)
    }

    pub async fn project_members(&self, project: Uuid) -> Result<Vec<Value>> {
        Ok(sql::scalars(&mut self.pool.clone(),"SELECT jsonb_build_object('user_id',u.id,'username',u.username,'role',m.role,'created_at',m.created_at) FROM project_members m JOIN users u ON u.id=m.user_id WHERE m.project_id=$1 AND m.tenant_id=$2 ORDER BY m.created_at",params![project,self.tenant()?]).await?)
    }

    pub async fn set_project_member(
        &self,
        project: Uuid,
        user: Uuid,
        role: Option<&str>,
    ) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_project_admin(&mut tx, project).await?;
        if let Some(role) = role {
            let member: bool = sql::scalar(
                &mut tx,
                "SELECT EXISTS(SELECT 1 FROM tenant_members WHERE tenant_id=$1 AND user_id=$2)",
                params![self.tenant()?, user],
            )
            .await?;
            if !member {
                return Err(ApiError::BadRequest(
                    "account must already belong to this tenant".into(),
                ));
            }
            sql::execute(&mut tx,"INSERT INTO project_members(tenant_id,project_id,user_id,role) VALUES($1,$2,$3,$4) ON CONFLICT(project_id,user_id) DO UPDATE SET role=excluded.role",params![self.tenant()?,project,user,role]).await?;
        } else {
            sql::execute(
                &mut tx,
                "DELETE FROM project_members WHERE project_id=$1 AND user_id=$2 AND tenant_id=$3",
                params![project, user, self.tenant()?],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_node_access(&self, project: Uuid, access: &NodeAccess) -> Result<()> {
        if access.ids.len() > 500
            || access
                .ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != access.ids.len()
        {
            return Err(ApiError::BadRequest(
                "invalid or duplicate device grants".into(),
            ));
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.lock_admin(&mut tx).await?;
        let (mode, column, table) = match access.mode {
            NodeAccessMode::Pools => ("pools", "pool_ids", "node_pools"),
            NodeAccessMode::Nodes => ("nodes", "node_ids", "nodes"),
        };
        let count: i64 = sql::scalar(
            &mut tx,
            &format!("SELECT count(*) FROM {table} WHERE tenant_id=$1 AND id=ANY($2)"),
            params![self.tenant()?, &access.ids],
        )
        .await?;
        if count as usize != access.ids.len() {
            return Err(ApiError::BadRequest(
                "device grants must belong to this tenant".into(),
            ));
        }
        sql::execute(
            &mut tx,
            &format!(
                "UPDATE projects SET node_access_mode=$3,{column}=$4 WHERE id=$1 AND tenant_id=$2"
            ),
            params![project, self.tenant()?, mode, &access.ids],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
