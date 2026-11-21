//! [INPUT]: 依赖 Coordinator 授权、源码清单和持久业务状态。
//! [OUTPUT]: 传递规划意图与回执；缺池可修正，查池故障保留事件重试。
//! [POS]: Coordinator 的业务授权边界；集群等待不占构建行锁。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{crypto, state::AppState};
use anyhow::{Context, Result, ensure};
use artifactory_planning_protocol::{ModelConfiguration, RunIntent};
use artifactory_service_transport::service::{self, Client};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    routing::post,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn coordinator() -> Result<Client> {
    Client::new(
        service::required("ARTIFACTORY_PLANNING_URL")?,
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
            .route("/internal/planning/event", post(event))
            .route("/internal/planning/tool", post(tool))
            .route("/internal/planning/source-material", post(source_material))
            .layer(DefaultBodyLimit::max(service::SERVICE_JSON_LIMIT))
            .with_state(state),
        token,
    )
}
async fn source_material(
    State(s): State<AppState>,
    Json(v): Json<Value>,
) -> artifactory_service_transport::error::Result<Json<Value>> {
    Ok(Json(planning_source_material(&s, v).await?))
}
async fn planning_source_material(s: &AppState, v: Value) -> Result<Value> {
    let run = id(&v, "run_id")?;
    ensure!(
        v["service_account"] == "artifactory-planning-worker",
        "wrong planning service account"
    );
    validate_source_execution(&v).await?;
    let build = s.store.planning_tool_build(run).await?;
    let material = s
        .store
        .build_source_material(&s.settings.master_key, build)
        .await?;
    validate_source_execution(&v).await?;
    Ok(serde_json::to_value(material)?)
}
async fn validate_source_execution(v: &Value) -> Result<()> {
    let current: Value = coordinator()?
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
) -> artifactory_service_transport::error::Result<Json<Value>> {
    let build = s.store.planning_tool_build(id(&v, "run_id")?).await?;
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
    if let Some(receipt) = s.store.planning_receipt(event).await? {
        ensure!(receipt.input_hash == hash, "event ID reused");
        return Ok(receipt.result);
    }
    let kind = v["kind"].as_str().context("missing event kind")?;
    let payload = &v["payload"];
    let run_source = s.store.planning_run_source(run).await?;
    let mut validation = None;
    if kind == "proposal" {
        let remote: Value = coordinator()?
            .post("/v1/get", &json!({"run_id":run}))
            .await?;
        ensure!(
            remote["lifecycle"]["generation"] == payload["generation"],
            "stale coordinator generation"
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
            let prepared = (|| -> Result<_> {
                let plan = crate::planning::parse_plan(&run_source.planning_context, plan.clone())?;
                crate::images::ensure_targets_enabled(
                    plan.recipe.targets.values(),
                    crate::images::enabled(),
                )?;
                Ok(plan)
            })();
            match prepared {
                Err(error) => validation = Some(error.to_string()),
                Ok(plan) => match crate::images::validate_plan_pools(&plan).await {
                    Ok(()) => (),
                    Err(error @ crate::images::PoolValidationError::MissingPool(_)) => {
                        validation = Some(error.to_string());
                    }
                    Err(error) => return Err(error.into()),
                },
            }
        }
    }
    s.store
        .commit_planning_event(
            crate::store::PlanningEventCommit {
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
) -> artifactory_service_transport::error::Result<Json<Value>> {
    Ok(Json(accept_event(&s, v).await?))
}
pub async fn deliver_intents(s: &AppState) -> Result<()> {
    let rows = s.store.planning_deliveries().await?;
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
                s.store.save_planning_payload(event, sealed).await?
            };
            serde_json::from_slice(&crypto::open(&s.settings.master_key, &ciphertext)?)?
        };
        let _: Value = coordinator()?.post("/v1/intent", &value).await?;
        s.store.planning_delivered(event).await?;
    }
    Ok(())
}
async fn make_intent(s: &AppState, run: Uuid) -> Result<RunIntent> {
    let input = s.store.planning_intent_context(run).await?;
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
        pool: std::env::var("ARTIFACTORY_PLANNING_POOL").unwrap_or("default".into()),
    })
}
