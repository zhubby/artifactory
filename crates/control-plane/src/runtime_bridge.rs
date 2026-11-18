//! [INPUT]: 依赖 Runtime 授权、源码清单和持久业务状态。
//! [OUTPUT]: 传递规划意图、源码材料与幂等提交回执。
//! [POS]: 控制平面和独立规划 Runtime 的业务边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{crypto, execution_plan::ExecutionPlan, state::AppState};
use anyhow::{Context, Result, ensure};
use artifactory_runtime_protocol::{
    ModelConfiguration, RunIntent,
    service::{self, Client},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    routing::post,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn runtime() -> Result<Client> {
    Client::new(
        service::required("ARTIFACTORY_RUNTIME_URL")?,
        service::required("ARTIFACTORY_SERVICE_TOKEN")?,
    )
}
pub fn routes(state: AppState) -> Router {
    let Some(token) = std::env::var("ARTIFACTORY_SERVICE_TOKEN")
        .ok()
        .filter(|v| v.len() >= 32)
    else {
        return Router::new();
    };
    service::protected(
        Router::new()
            .route("/internal/runtime/event", post(event))
            .route("/internal/runtime/tool", post(tool))
            .route("/internal/runtime/source-material", post(source_material))
            .layer(DefaultBodyLimit::max(service::SERVICE_JSON_LIMIT))
            .with_state(state),
        token,
    )
}
async fn source_material(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> artifactory_runtime_protocol::error::Result<Json<Value>> {
    Ok(Json(runtime_source_material(&s, v).await?))
}
async fn runtime_source_material(s: &AppState, v: Value) -> Result<Value> {
    let run = id(&v, "run_id")?;
    ensure!(
        v["service_account"] == "artifactory-pi",
        "wrong planning service account"
    );
    validate_source_execution(&v).await?;
    let build = s.store.runtime_tool_build(run).await?;
    let material = s
        .store
        .build_source_material(&s.settings.master_key, build)
        .await?;
    validate_source_execution(&v).await?;
    Ok(serde_json::to_value(material)?)
}
async fn validate_source_execution(v: &Value) -> Result<()> {
    let current: Value = runtime()?
        .post("/v1/get", &json!({"run_id":id(v,"run_id")?}))
        .await?;
    ensure!(
        current["status"] == "provisioning"
            && current["lifecycle"]["generation"] == v["generation"]
            && current["execution"]["uid"] == v["pod_uid"]
            && current["execution"]["namespace"] == v["namespace"]
            && current["lifecycle"]["canceled"] != true
            && current["lifecycle"]["pending"] != true,
        "planning execution changed"
    );
    Ok(())
}
fn id(v: &Value, key: &str) -> Result<Uuid> {
    Ok(Uuid::parse_str(
        v[key].as_str().context("missing identifier")?,
    )?)
}
async fn tool(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> artifactory_runtime_protocol::error::Result<Json<Value>> {
    let build = s.store.runtime_tool_build(id(&v, "run_id")?).await?;
    Ok(Json(match v["name"].as_str() {
        Some(name @ ("node_capabilities" | "summarize_nodes" | "query_nodes" | "get_node")) => {
            crate::node_query::tool(&s, build, name, &v["arguments"]).await?
        }
        Some("read_logs") => crate::planning::read_logs(&s, build, &v["arguments"]).await?,
        _ => json!({"error":"tool not permitted"}),
    }))
}
pub async fn accept_event(s: &AppState, v: Value) -> Result<Value> {
    let event = id(&v, "event_id")?;
    let run = id(&v, "run_id")?;
    let hash = crypto::hash(serde_json::to_vec(&v)?);
    if let Some(receipt) = s.store.runtime_receipt(event).await? {
        ensure!(receipt.input_hash == hash, "event ID reused");
        return Ok(receipt.result);
    }
    let kind = v["kind"].as_str().context("missing event kind")?;
    let payload = &v["payload"];
    let run_source = s.store.runtime_run_source(run).await?;
    let mut validation = None;
    if kind == "proposal" {
        let remote: Value = runtime()?.post("/v1/get", &json!({"run_id":run})).await?;
        ensure!(
            remote["lifecycle"]["generation"] == payload["generation"],
            "stale runtime generation"
        );
        if payload["kind"] == "submit_plan" {
            let digest = s
                .store
                .source_manifest_digest(run_source.source_manifest_id)
                .await?;
            ensure!(payload["source_digest"] == digest, "source digest changed");
            let plan = payload["payload"]
                .get("plan")
                .unwrap_or(&payload["payload"]);
            if let Err(error) = ExecutionPlan::parse(plan.clone()) {
                validation = Some(error.to_string());
            }
        }
    }
    s.store
        .commit_runtime_event(
            crate::store::RuntimeEventCommit {
                event,
                run,
                build: run_source.build_id,
                hash,
                kind,
                payload,
                validation,
            },
            |context| async move {
                crate::planning::prepare_plan(
                    s,
                    context,
                    payload["payload"]
                        .get("plan")
                        .unwrap_or(&payload["payload"])
                        .clone(),
                )
                .await
            },
        )
        .await
}
async fn event(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> artifactory_runtime_protocol::error::Result<Json<Value>> {
    Ok(Json(accept_event(&s, v).await?))
}
pub async fn deliver_intents(s: &AppState) -> Result<()> {
    let rows = s.store.runtime_deliveries().await?;
    for row in rows {
        let event: i64 = row.id;
        let run: Uuid = row.run_id;
        let value = if row.kind == "cancel" {
            json!({"run_id":run,"version":row.version,"canceled":true})
        } else {
            let ciphertext = if let Some(value) = row.ciphertext {
                value
            } else {
                let intent = make_intent(s, run).await?;
                let sealed = crypto::seal(&s.settings.master_key, &serde_json::to_vec(&intent)?)?;
                s.store.save_runtime_payload(event, sealed).await?
            };
            serde_json::from_slice(&crypto::open(&s.settings.master_key, &ciphertext)?)?
        };
        let _: Value = runtime()?.post("/v1/intent", &value).await?;
        s.store.runtime_delivered(event).await?;
    }
    Ok(())
}
async fn make_intent(s: &AppState, run: Uuid) -> Result<RunIntent> {
    let input = s.store.runtime_intent_context(run).await?;
    let build = input.build_id;
    let mut model = input.model;
    model["api_key"] = json!(String::from_utf8(crypto::open(
        &s.settings.master_key,
        model["key_ciphertext"]
            .as_str()
            .context("model credential missing")?
    )?)?);
    model.as_object_mut().unwrap().remove("key_ciphertext");
    let context = input.context;
    Ok(RunIntent {
        tenant_id: input.tenant_id,
        run_id: run,
        build_id: build,
        project_id: input.project_id,
        version: 1,
        source_manifest_id: input.source_manifest_id,
        source_digest: input.digest,
        context,
        model: ModelConfiguration {
            id: Uuid::parse_str(model["id"].as_str().context("profile ID missing")?)?,
            api: model["api"]
                .as_str()
                .context("model protocol missing")?
                .into(),
            base_url: model["base_url"]
                .as_str()
                .context("model URL missing")?
                .into(),
            model: model["model"].as_str().context("model missing")?.into(),
            api_key: model["api_key"].as_str().unwrap_or_default().into(),
            parameters: model["parameters"].clone(),
            max_calls: model["max_calls"].as_u64().unwrap_or(20) as u32,
            timeout_seconds: model["timeout_seconds"].as_u64().unwrap_or(600) as u32,
        },
        pool: std::env::var("ARTIFACTORY_AGENT_POOL").unwrap_or("default".into()),
    })
}
