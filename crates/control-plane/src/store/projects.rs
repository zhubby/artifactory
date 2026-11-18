//! [INPUT]: 依赖 Toasty 项目模型、节点授权与源码凭据绑定。
//! [OUTPUT]: 提供项目输入、配置持久化与源码路由投影。
//! [POS]: 项目存储边界；保留未提交的节点授权和签名默认方案。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::dto;
use artifactory_database::{params, sql};

use super::ControlStore;
use super::{NodeAccess, NodeAccessMode};
use crate::error::ApiError;
use crate::{error::Result, source::CredentialBinding};
use serde::Deserialize;
use serde_json::{Value, json};

use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInput {
    pub name: String,
    pub repository_url: String,
    pub credential_id: Option<Uuid>,
    #[serde(default)]
    pub pool_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub node_access: Option<NodeAccess>,
    #[serde(default)]
    pub signing_profiles: Vec<String>,
    #[serde(default)]
    pub model_profile_id: Option<Uuid>,
    #[serde(default)]
    pub build_context: String,
    #[serde(default)]
    pub source_credentials: Vec<CredentialBinding>,
}

/// Source routing information independent of the database representation.
pub struct ProjectSource {
    pub repository_url: String,
    pub credential_id: Option<Uuid>,
}

impl ControlStore {
    pub async fn project_by_slug(&self, slug: &str) -> Result<Uuid> {
        sql::optional_scalar(
            &mut self.pool.clone(),
            "SELECT id FROM projects WHERE tenant_id=$1 AND slug=$2",
            params![self.tenant()?, slug],
        )
        .await?
        .ok_or(ApiError::NotFound)
    }

    pub async fn projects(&self, visible: Option<Vec<Uuid>>) -> Result<Vec<dto::ProjectSummary>> {
        Ok(sql::scalars(&mut self.pool.clone(), "SELECT to_jsonb(p) || jsonb_build_object('node_access',jsonb_build_object('mode',p.node_access_mode,'ids',CASE WHEN p.node_access_mode='pools' THEN p.pool_ids ELSE p.node_ids END),'effective_role',($3::jsonb->>p.id::text),'build_count',(SELECT count(*) FROM builds b WHERE b.project_id=p.id),'last_build',(SELECT jsonb_build_object('id',b.id,'number',b.number,'status',b.status,'created_at',b.created_at) FROM builds b WHERE b.project_id=p.id ORDER BY created_at DESC LIMIT 1)) FROM projects p WHERE ($1::uuid[] IS NULL OR p.id=ANY($1)) AND p.tenant_id=$2 ORDER BY p.created_at DESC", params![visible,self.tenant()?,self.project_roles_json()]).await?)
    }

    pub async fn project(&self, id: Uuid) -> Result<dto::Project> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT to_jsonb(p)||jsonb_build_object('node_access',jsonb_build_object('mode',p.node_access_mode,'ids',CASE WHEN p.node_access_mode='pools' THEN p.pool_ids ELSE p.node_ids END),'effective_role',($2::jsonb->>p.id::text)) FROM projects p WHERE id=$1",
            params![id,self.project_roles_json()],
        )
        .await?)
    }

    pub async fn project_source(&self, id: Uuid) -> Result<ProjectSource> {
        let row = super::models::Project::get_by_id(&mut self.pool.clone(), id).await?;
        Ok(ProjectSource {
            repository_url: row.repository_url,
            credential_id: row.credential_id,
        })
    }

    pub async fn pool_count(&self, ids: &[Uuid]) -> Result<i64> {
        Ok(sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM node_pools WHERE id=ANY($1) AND tenant_id=$2",
            params![ids, self.tenant()?],
        )
        .await?)
    }

    fn project_roles_json(&self) -> Value {
        self.scope
            .as_ref()
            .map_or_else(|| json!({}), |a| json!(a.project_roles))
    }

    async fn project_access(
        &self,
        input: &ProjectInput,
        existing: Option<&dto::Project>,
    ) -> Result<NodeAccess> {
        if let Some(access) = &input.node_access {
            if let Some(pools) = &input.pool_ids
                && (access.mode != NodeAccessMode::Pools || *pools != access.ids)
            {
                return Err(ApiError::BadRequest(
                    "pool_ids conflicts with node_access".into(),
                ));
            }
            return Ok(access.clone());
        }
        if let Some(ids) = &input.pool_ids {
            return Ok(NodeAccess {
                mode: NodeAccessMode::Pools,
                ids: ids.clone(),
            });
        }
        if let Some(current) = existing {
            return Ok(
                serde_json::from_value(current.node_access.clone()).map_err(anyhow::Error::from)?
            );
        }
        let default: Uuid = sql::scalar(
            &mut self.pool.clone(),
            "SELECT id FROM node_pools WHERE tenant_id=$1 AND is_default",
            params![self.tenant()?],
        )
        .await?;
        Ok(NodeAccess {
            mode: NodeAccessMode::Pools,
            ids: vec![default],
        })
    }

    async fn validate_project_resources(
        &self,
        tx: &mut toasty::db::Transaction<'_>,
        input: &ProjectInput,
        access: &NodeAccess,
    ) -> Result<()> {
        if access.ids.len() > 500
            || access
                .ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != access.ids.len()
        {
            return Err(ApiError::BadRequest("invalid device grants".into()));
        }
        let table = if access.mode == NodeAccessMode::Pools {
            "node_pools"
        } else {
            "nodes"
        };
        let count: i64 = sql::scalar(
            tx,
            &format!("SELECT count(*) FROM {table} WHERE tenant_id=$1 AND id=ANY($2)"),
            params![self.tenant()?, &access.ids],
        )
        .await?;
        if usize::try_from(count).ok() != Some(access.ids.len()) {
            return Err(ApiError::BadRequest(
                "device grants must belong to this tenant".into(),
            ));
        }
        if let Some(id) = input.credential_id {
            self.authorize_owned_with(tx, "credentials", id).await?;
        }
        if let Some(id) = input.model_profile_id {
            self.authorize_owned_with(tx, "model_profiles", id).await?;
        }
        for binding in &input.source_credentials {
            self.authorize_owned_with(tx, "credentials", binding.credential_id)
                .await?;
        }
        Ok(())
    }

    pub async fn create_project(&self, id: Uuid, input: &ProjectInput) -> Result<()> {
        let access = self.project_access(input, None).await?;
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        self.validate_project_resources(&mut tx, input, &access)
            .await?;
        let (pools, nodes) = if access.mode == NodeAccessMode::Pools {
            (access.ids.clone(), vec![])
        } else {
            (vec![], access.ids.clone())
        };
        sql::execute(&mut tx,"INSERT INTO projects(id,tenant_id,name,repository_url,credential_id,pool_ids,node_ids,node_access_mode,signing_profiles,model_profile_id,build_context,source_credentials) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",params![id,self.tenant()?,&input.name,&input.repository_url,input.credential_id,pools,nodes,if access.mode==NodeAccessMode::Pools {"pools"} else {"nodes"},&input.signing_profiles,input.model_profile_id,&input.build_context,serde_json::to_value(&input.source_credentials).map_err(anyhow::Error::from)?]).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_project(&self, id: Uuid, input: &ProjectInput) -> Result<()> {
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let tenant_admin = self.lock_project_admin(&mut tx, id).await?;
        // Check protected fields against the same locked version that is updated.
        let current: dto::Project = sql::optional_scalar(
            &mut tx,
            "SELECT to_jsonb(p)||jsonb_build_object('node_access',jsonb_build_object('mode',p.node_access_mode,'ids',CASE WHEN p.node_access_mode='pools' THEN p.pool_ids ELSE p.node_ids END)) FROM projects p WHERE id=$1 AND tenant_id=$2 FOR UPDATE",
            params![id,self.tenant()?],
        ).await?.ok_or(ApiError::NotFound)?;
        let access = self.project_access(input, Some(&current)).await?;
        self.validate_project_resources(&mut tx, input, &access)
            .await?;
        if !tenant_admin
            && (input.credential_id != current.credential_id
                || (input.credential_id.is_some()
                    && input.repository_url != current.repository_url)
                || input.signing_profiles != current.signing_profiles
                || serde_json::to_value(&input.source_credentials).map_err(anyhow::Error::from)?
                    != current.source_credentials
                || json!(access) != current.node_access)
        {
            return Err(ApiError::Forbidden);
        }
        sql::execute(&mut tx,"UPDATE projects SET name=$2,repository_url=$3,credential_id=$4,signing_profiles=$5,model_profile_id=$6,build_context=$7,source_credentials=$8 WHERE id=$1 AND tenant_id=$9",params![id,&input.name,&input.repository_url,input.credential_id,&input.signing_profiles,input.model_profile_id,&input.build_context,serde_json::to_value(&input.source_credentials).map_err(anyhow::Error::from)?,self.tenant()?]).await?;
        if tenant_admin {
            let column = if access.mode == NodeAccessMode::Pools {
                "pool_ids"
            } else {
                "node_ids"
            };
            sql::execute(
                &mut tx,
                &format!("UPDATE projects SET node_access_mode=$2,{column}=$3 WHERE id=$1"),
                params![
                    id,
                    if access.mode == NodeAccessMode::Pools {
                        "pools"
                    } else {
                        "nodes"
                    },
                    &access.ids
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
