//! [INPUT]: 依赖 Toasty 连接、租户作用域和业务存储。
//! [OUTPUT]: 组合控制平面持久化接口及迁移。
//! [POS]: 业务服务与数据库的统一入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! Persistence operations for the control plane.
use artifactory_database::{params, sql};

mod auth;
pub mod lifecycle;
mod lifecycle_cleanup;
mod tenancy;
pub use tenancy::{NodeAccess, NodeAccessMode};
pub mod dto;
pub mod maintenance;
mod management;
mod models;
mod planning_management;
mod projects;
mod signing;
mod signing_execution;
pub use signing_execution::SigningAccess;

pub use auth::LoginAccount;
pub use projects::{ProjectInput, ProjectSource};

/// Shared control-plane persistence; transport code calls business operations.
#[derive(Clone)]
pub struct ControlStore {
    pool: toasty::Db,
    scope: Option<crate::auth::Auth>,
    pub source: artifactory_source_core::store::SourceStore,
}

impl ControlStore {
    pub fn new(pool: toasty::Db) -> Self {
        Self {
            source: artifactory_source_core::store::SourceStore::embedded(pool.clone()),
            pool,
            scope: None,
        }
    }

    pub async fn audit(
        &self,
        actor: String,
        action: &str,
        resource: String,
        details: serde_json::Value,
    ) -> anyhow::Result<()> {
        models::AuditEvent::create()
            .tenant_id(self.scope.as_ref().map(|s| s.tenant_id))
            .actor(actor)
            .action(action)
            .resource_id(resource)
            .details(toasty::Json((details).clone()))
            .exec(&mut self.pool.clone())
            .await?;
        Ok(())
    }
}

pub use management::{NewToken, NodeUpdate};
pub use planning_management::{ProfileInput, WorkerUpdate};

mod ota_management;
pub use ota_management::Target;

mod builds;
pub mod images;
pub use builds::{ArtifactDownload, BuildInput, LogChunk};

mod scheduling;
pub(crate) use scheduling::ResolvedSource;

mod executions;
pub(crate) use executions::{
    ArtifactDeclaration, EventReceipt, ExecutionEvent, LogCommit, NodeEnrollment,
};

mod lookups;
mod ota;

mod planning;
pub(crate) use planning::{
    PlanningAction, PlanningPrepared, PlanningToolResult, SubmissionContext,
};

mod runtime_bridge;
pub(crate) use runtime_bridge::{RemoteSnapshot, RuntimeEventCommit};

mod node_metadata;
mod node_queries;
mod placements;
mod replication;
mod source_cache;
pub(crate) use executions::SessionUpdate;

impl ControlStore {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        Self::connect_with_capacity(url, 20).await
    }

    /// Connect with the per-process database budget used by this replica.
    ///
    /// # Errors
    /// Returns connection, TimescaleDB or migration errors.
    pub async fn connect_with_capacity(url: &str, capacity: u32) -> anyhow::Result<Self> {
        let pool = artifactory_database::connect(
            url,
            models(),
            artifactory_database::PoolSettings::new(capacity as usize)
                .timeout(std::time::Duration::from_secs(5)),
        )
        .await?;
        artifactory_database::require_timescale(&pool).await?;
        artifactory_database::migrate(&pool, &MIGRATIONS).await?;
        Ok(Self::new(pool))
    }
    pub async fn users_empty(&self) -> anyhow::Result<bool> {
        let count: i64 = sql::scalar(
            &mut self.pool.clone(),
            "SELECT count(*) FROM users",
            params![],
        )
        .await?;
        Ok(count == 0)
    }
    pub async fn bootstrap_admin(&self, username: String, hash: String) -> anyhow::Result<()> {
        models::User::upsert_by_username(username.clone())
            .id(uuid::Uuid::new_v4())
            .password_hash(hash)
            .role("admin")
            .or_ignore()
            .exec(&mut self.pool.clone())
            .await?;
        self.bootstrap_tenant(username).await?;
        Ok(())
    }
}

mod errors;

#[cfg(test)]
mod performance;

pub static MIGRATIONS: toasty::migration::MigrationSet = toasty::embed_migrations!("migrations");

pub fn models() -> toasty::schema::app::ModelSet {
    let mut models = models::registry();
    for model in artifactory_source_core::store::embedded_models() {
        models.add(model);
    }
    models
}
