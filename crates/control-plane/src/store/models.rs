//! [INPUT]: 依赖 Toasty 模型宏和新初始化基线。
//! [OUTPUT]: 提供业务记录的私有 CRUD 投影。
//! [POS]: SQL 持有默认值、约束和索引，模型只描述读写列。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! Server-generated columns omitted here retain their PostgreSQL defaults.
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "users"]
pub(super) struct User {
    #[key]
    pub id: Uuid,
    #[unique]
    pub username: String,
    #[default(String::new())]
    pub display_name: String,
    pub password_hash: String,
    pub role: String,
}

#[derive(Debug, toasty::Model)]
#[table = "sessions"]
pub(super) struct Session {
    #[key]
    pub token_hash: String,
    pub user_id: Uuid,
    pub expires_at: jiff::Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "api_tokens"]
pub(super) struct ApiToken {
    pub tenant_id: Uuid,
    #[key]
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub token_hash: String,
    pub project_ids: Vec<Uuid>,
    pub scopes: Vec<String>,
    pub expires_at: jiff::Timestamp,
    #[default(false)]
    pub revoked: bool,
}

#[derive(Debug, toasty::Model)]
#[table = "projects"]
pub(super) struct Project {
    pub tenant_id: Uuid,
    #[key]
    pub id: Uuid,
    pub name: String,
    #[default("active".to_owned())]
    pub status: String,
    pub archived_at: Option<jiff::Timestamp>,
    pub repository_url: String,
    pub credential_id: Option<Uuid>,
    pub pool_ids: Vec<Uuid>,
    pub signing_profiles: Vec<String>,
    pub model_profile_id: Option<Uuid>,
    pub build_context: String,
    #[column(type = jsonb)]
    pub source_credentials: toasty::Json<serde_json::Value>,
}

#[derive(Debug, toasty::Model)]
#[table = "node_pools"]
pub(super) struct NodePool {
    pub tenant_id: Uuid,
    #[key]
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, toasty::Model)]
#[table = "enrollment_tokens"]
pub(super) struct EnrollmentToken {
    #[key]
    pub id: Uuid,
    pub token_hash: String,
    pub pool_id: Uuid,
    pub expires_at: jiff::Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "model_profiles"]
pub(super) struct ModelProfile {
    pub tenant_id: Uuid,
    #[key]
    pub id: Uuid,
    pub name: String,
    pub api: String,
    pub base_url: String,
    pub model: String,
    pub key_ciphertext: String,
    #[column(type = jsonb)]
    pub parameters: toasty::Json<serde_json::Value>,
    #[default(1)]
    pub revision: i32,
    pub enabled: bool,
    pub is_default: bool,
    pub max_calls: i32,
    pub timeout_seconds: i32,
}

pub(super) fn registry() -> toasty::schema::app::ModelSet {
    toasty::models!(crate::*)
}

#[derive(Debug, toasty::Model)]
#[table = "nodes"]
#[key(id)]
pub(super) struct Node {
    pub id: Uuid,
    pub name: String,
    pub os: String,
    pub arch: String,
    pub pool_id: Uuid,
    pub certificate_hash: String,
    pub certificate_expires_at: jiff::Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "artifacts"]
#[key(id)]
pub(super) struct Artifact {
    pub id: Uuid,
    pub attempt_id: Uuid,
    pub path: String,
    pub object_key: String,
    pub size: i64,
    pub sha256: String,
}

#[derive(Debug, toasty::Model)]
#[table = "log_chunks"]
#[key(id)]
pub(super) struct LogChunk {
    #[auto]
    pub id: i64,
    pub attempt_id: Uuid,
    pub sequence: i64,
    pub step: String,
    pub object_key: String,
    pub sha256: String,
    pub size: i64,
}

#[derive(Debug, toasty::Model)]
#[table = "step_runs"]
#[key(attempt_id, step_index)]
pub(super) struct StepRun {
    pub attempt_id: Uuid,
    pub step_index: i32,
    pub name: String,
    pub phase: String,
    pub status: String,
}

#[derive(Debug, toasty::Model)]
#[table = "attempt_events"]
#[key(attempt_id, sequence)]
pub(super) struct AttemptEvent {
    pub attempt_id: Uuid,
    pub sequence: i64,
    pub kind: String,
    #[column(type = jsonb)]
    pub payload: toasty::Json<serde_json::Value>,
}

#[derive(Debug, toasty::Model)]
#[table = "planning_events"]
#[key(id)]
pub(super) struct PlanningEvent {
    #[auto]
    pub id: i64,
    pub run_id: Uuid,
    pub kind: String,
    #[column(type = jsonb)]
    pub payload: toasty::Json<serde_json::Value>,
}

#[derive(Debug, toasty::Model)]
#[table = "planning_runs"]
#[key(id)]
pub(super) struct PlanningRun {
    pub id: Uuid,
    pub build_id: Uuid,
    pub purpose: String,
    #[column(type = jsonb)]
    pub model_config: toasty::Json<serde_json::Value>,
    #[default(Vec::<String>::new())]
    pub failed_targets: Vec<String>,
}

#[derive(Debug, toasty::Model)]
#[table = "plan_revisions"]
#[key(id)]
pub(super) struct PlanRevision {
    pub id: Uuid,
    pub build_id: Uuid,
    pub run_id: Uuid,
    pub revision: i32,
    pub source_manifest_id: Uuid,
    #[column(type = jsonb)]
    pub contents: toasty::Json<serde_json::Value>,
    pub sha256: String,
}

#[derive(Debug, toasty::Model)]
#[table = "jobs"]
#[key(id)]
#[unique(build_id, target)]
pub(super) struct Job {
    pub id: Uuid,
    pub build_id: Uuid,
    pub target: String,
    #[column(type = jsonb)]
    pub target_config: toasty::Json<serde_json::Value>,
    pub plan_revision_id: Option<Uuid>,
}

#[derive(Debug, toasty::Model)]
#[table = "releases"]
#[key(id)]
pub(super) struct Release {
    pub id: Uuid,
    pub project_id: Uuid,
    pub build_id: Uuid,
    pub version: String,
    pub idempotency_key: String,
    pub commit_sha: String,
}

#[derive(Debug, toasty::Model)]
#[table = "release_artifacts"]
#[key(release_id, artifact_id)]
pub(super) struct ReleaseArtifact {
    pub release_id: Uuid,
    pub artifact_id: Uuid,
}

#[derive(Debug, toasty::Model)]
#[table = "publish_attempts"]
#[key(id)]
pub(super) struct PublishAttempt {
    pub id: Uuid,
    pub build_id: Uuid,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, toasty::Model)]
#[table = "emissary_updates"]
#[key(id)]
pub(super) struct EmissaryUpdate {
    pub id: Uuid,
    pub node_id: Uuid,
    pub pool_id: Uuid,
    pub release_id: Uuid,
    pub previous_version: Option<String>,
    pub retry_of: Option<Uuid>,
}

#[derive(Debug, toasty::Model)]
#[table = "emissary_releases"]
#[key(id)]
pub(super) struct EmissaryRelease {
    pub tenant_id: Uuid,
    pub id: Uuid,
    #[unique]
    pub version: String,
    pub manifest: String,
    pub signature: String,
    pub manifest_hash: String,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_receipts"]
#[key(event_id)]
pub(super) struct RuntimeReceipt {
    pub event_id: Uuid,
    pub input_hash: String,
    #[column(type = jsonb)]
    pub result: toasty::Json<serde_json::Value>,
    pub run_id: Uuid,
    pub generation: Option<i64>,
}

#[derive(Debug, toasty::Model)]
#[table = "audit_events"]
#[key(id)]
pub(super) struct AuditEvent {
    pub tenant_id: Option<Uuid>,
    #[auto]
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub resource_id: String,
    #[column(type = jsonb)]
    #[default(toasty::Json(serde_json::json!({})))]
    pub details: toasty::Json<serde_json::Value>,
}

#[derive(Debug, toasty::Model)]
#[table = "tenants"]
pub(super) struct Tenant {
    #[key]
    pub id: Uuid,
    pub name: String,
    #[default("active".to_owned())]
    pub status: String,
    pub archived_at: Option<jiff::Timestamp>,
    pub owner_id: Uuid,
    pub created_by: Uuid,
    pub creation_key: Uuid,
}
#[derive(Debug, toasty::Model)]
#[table = "tenant_members"]
#[key(tenant_id, user_id)]
pub(super) struct TenantMember {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
}
#[derive(Debug, toasty::Model)]
#[table = "project_members"]
#[key(project_id, user_id)]
pub(super) struct ProjectMember {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
}
#[derive(Debug, toasty::Model)]
#[table = "tenant_invitations"]
pub(super) struct TenantInvitation {
    #[key]
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub token_hash: String,
    pub role: String,
    pub created_by: Uuid,
    pub expires_at: jiff::Timestamp,
    pub revoked_at: Option<jiff::Timestamp>,
    pub accepted_at: Option<jiff::Timestamp>,
    pub accepted_by: Option<Uuid>,
}
#[derive(Debug, toasty::Model)]
#[table = "project_node_grants"]
#[key(project_id, node_id)]
pub(super) struct ProjectNodeGrant {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub node_id: Uuid,
}
#[derive(Debug, toasty::Model)]
#[table = "project_pool_grants"]
#[key(project_id, pool_id)]
pub(super) struct ProjectPoolGrant {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub pool_id: Uuid,
}
