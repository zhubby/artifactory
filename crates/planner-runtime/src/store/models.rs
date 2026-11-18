//! [INPUT]: 依赖固定意图、运行预算与审计消息。
//! [OUTPUT]: 声明 Runtime 私有数据库模型。
//! [POS]: SQL 投影与全新初始化基线的模型地图。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "runtime_runs"]
pub(super) struct Run {
    pub tenant_id: Option<Uuid>,
    #[key]
    pub id: Uuid,
    pub version: i64,
    pub intent_hash: Option<String>,
    pub intent_ciphertext: Option<String>,
    pub project_id: Option<Uuid>,
    pub pool: Option<String>,
    pub source_manifest_id: Option<Uuid>,
    pub source_digest: Option<String>,
    #[column(type = jsonb)]
    pub lifecycle: toasty::Json<serde_json::Value>,
    pub status: String,
    #[column(type = jsonb)]
    pub execution: Option<toasty::Json<serde_json::Value>>,
    #[column(type = jsonb)]
    #[default(toasty::Json(serde_json::json!({})))]
    pub usage: toasty::Json<serde_json::Value>,
    #[default(0)]
    pub report_sequence: i64,
    pub grant_hash: Option<String>,
    pub grant_ciphertext: Option<String>,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_profiles"]
pub(super) struct Profile {
    #[key]
    pub id: Uuid,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_model_requests"]
#[key(run_id, request_id)]
pub(super) struct ModelRequest {
    pub run_id: Uuid,
    pub request_id: Uuid,
    pub input_hash: String,
    pub generation: i64,
    #[default("reserved".to_owned())]
    pub state: String,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_outbox"]
pub(super) struct Outbox {
    #[key]
    pub id: Uuid,
    pub run_id: Uuid,
    pub kind: String,
    #[column(type = jsonb)]
    pub payload: toasty::Json<serde_json::Value>,
    #[default(false)]
    pub delivered: bool,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_reports"]
#[key(run_id, event_id)]
pub(super) struct Report {
    pub run_id: Uuid,
    pub event_id: Uuid,
    pub input_hash: String,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_proposals"]
pub(super) struct Proposal {
    #[key]
    pub id: Uuid,
    pub run_id: Uuid,
    pub generation: i64,
    pub input_hash: String,
    pub kind: String,
    #[column(type = jsonb)]
    pub payload: toasty::Json<serde_json::Value>,
    pub deadline_ms: i64,
    #[column(type = jsonb)]
    pub result: Option<toasty::Json<serde_json::Value>>,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_conversations"]
#[key(run_id, generation)]
pub(super) struct Conversation {
    pub run_id: Uuid,
    pub generation: i64,
    pub pod_uid: String,
    pub status: String,
    pub model: String,
    pub source_digest: Option<String>,
}

#[derive(Debug, toasty::Model)]
#[table = "runtime_messages"]
#[key(run_id, generation, sequence)]
pub(super) struct Message {
    pub run_id: Uuid,
    pub generation: i64,
    pub sequence: i64,
    pub event_id: Uuid,
    pub kind: String,
    pub input_hash: String,
    #[column(type = jsonb)]
    pub payload: toasty::Json<serde_json::Value>,
}
