//! [INPUT]: 依赖固定源码、执行器准入和项目授权。
//! [OUTPUT]: 校验并提交 Poll 与 Runtime 共用执行计划。
//! [POS]: 原生 LFS 能力与镜像目标在持久化前分别校验。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    crypto, execution_plan::ExecutionPlan, pki, protocol::*, recipe::Recipe, source_cache,
    state::AppState,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tonic::{
    Request, Response, Status,
    transport::server::{TcpConnectInfo, TlsConnectInfo},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct PlanningService(pub AppState);
pub(crate) fn internal(error: impl std::fmt::Display) -> Status {
    tracing::error!(%error,"planning operation failed");
    Status::internal("planning operation failed")
}
async fn identity<T>(state: &AppState, request: &Request<T>) -> std::result::Result<Uuid, Status> {
    let certificate = request
        .extensions()
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .and_then(|info| info.peer_certs())
        .and_then(|certs| certs.first().cloned())
        .ok_or_else(|| Status::unauthenticated("Agent worker certificate required"))?;
    state
        .store
        .planner_identity(crypto::hash(certificate.as_ref()))
        .await
}

#[tonic::async_trait]
impl planner_enrollment_server::PlannerEnrollment for PlanningService {
    async fn enroll(
        &self,
        request: Request<PlannerEnrollRequest>,
    ) -> std::result::Result<Response<PlannerEnrollReply>, Status> {
        let input = request.into_inner();
        if input.name.trim().is_empty() || input.name.len() > 100 || input.token.len() > 256 {
            return Err(Status::invalid_argument("invalid worker enrollment"));
        }
        let state = &self.0;
        let enrolled = state
            .store
            .enroll_planner(&input.token, &input.name, |id| async move {
                pki::sign(&state.settings, id, &input.csr_pem)
                    .await
                    .map_err(internal)
            })
            .await?;
        let id = enrolled.id;
        let certificate = enrolled.certificate;
        Ok(Response::new(PlannerEnrollReply {
            worker_id: id.to_string(),
            certificate_pem: certificate,
            ca_pem: tokio::fs::read_to_string(state.settings.pki_dir.join("ca.pem"))
                .await
                .map_err(internal)?,
            heartbeat_seconds: 10,
            lease_seconds: 90,
        }))
    }
}

pub async fn nodes(state: &AppState, build: Uuid) -> Result<Vec<Value>> {
    state.store.planning_nodes(build).await
}

#[tonic::async_trait]
impl planning_server::Planning for PlanningService {
    async fn poll(
        &self,
        request: Request<PlannerPollRequest>,
    ) -> std::result::Result<Response<PlannerTask>, Status> {
        let state = &self.0;
        let worker = identity(state, &request).await?;
        let Some(claim) = state.store.claim_planning(worker).await? else {
            return Ok(Response::new(PlannerTask::default()));
        };
        let config = claim.config;
        let mut public_config = config.clone();
        let key = crypto::open(
            &state.settings.master_key,
            config["key_ciphertext"]
                .as_str()
                .ok_or_else(|| Status::internal("model key unavailable"))?,
        )
        .map_err(internal)?;
        public_config
            .as_object_mut()
            .unwrap()
            .remove("key_ciphertext");
        public_config["api_key"] = json!(String::from_utf8(key).map_err(internal)?);
        Ok(Response::new(PlannerTask {
            run_id: claim.id.to_string(),
            lease_token: claim.token,
            context_json: claim.context.to_string(),
            model_config_json: public_config.to_string(),
            timeout_seconds: claim.timeout_seconds,
            max_calls: claim.max_calls,
        }))
    }
    async fn heartbeat(
        &self,
        request: Request<PlannerLeaseRequest>,
    ) -> std::result::Result<Response<PlannerHeartbeat>, Status> {
        let state = &self.0;
        let worker = identity(state, &request).await?;
        let input = request.into_inner();
        let canceled = state
            .store
            .renew_planning(worker, &input.run_id, &input.lease_token)
            .await?;
        Ok(Response::new(PlannerHeartbeat { canceled }))
    }
    async fn tool(
        &self,
        request: Request<PlannerToolRequest>,
    ) -> std::result::Result<Response<PlannerToolReply>, Status> {
        let state = &self.0;
        let worker = identity(state, &request).await?;
        let input = request.into_inner();
        if input.arguments_json.len() > 384 * 1024 {
            return Err(Status::invalid_argument("tool input exceeds limit"));
        }
        let args: Value = serde_json::from_str(&input.arguments_json)
            .map_err(|_| Status::invalid_argument("invalid tool arguments"))?;
        let outcome = state
            .store
            .planning_tool(
                worker,
                &input.run_id,
                &input.lease_token,
                &input.name,
                &args,
                |action| async {
                    match action {
                        crate::store::PlanningAction::Submit(context) => {
                            Ok(crate::store::PlanningPrepared::Plan(
                                prepare_plan(
                                    state,
                                    context,
                                    args.get("plan").cloned().unwrap_or(args.clone()),
                                    false,
                                )
                                .await?,
                            ))
                        }
                        crate::store::PlanningAction::ReadLogs(build) => {
                            Ok(crate::store::PlanningPrepared::Value(
                                read_logs(state, build, &args).await?,
                            ))
                        }
                    }
                },
            )
            .await?;
        let value = match outcome {
            crate::store::PlanningToolResult::Value(value) => value,
            crate::store::PlanningToolResult::Nodes(build) => {
                crate::node_query::tool(state, build, &input.name, &args)
                    .await
                    .unwrap_or_else(|e| json!({"error":e.to_string()}))
            }
            crate::store::PlanningToolResult::Inspect(snapshot) => {
                match source_cache::inspect_tool(state, snapshot, &input.name, &args).await {
                    Ok(value) => value,
                    Err(error) => json!({"error":error.to_string()}),
                }
            }
        };
        Ok(Response::new(PlannerToolReply {
            result_json: value.to_string(),
        }))
    }
    async fn report(
        &self,
        request: Request<PlannerReportRequest>,
    ) -> std::result::Result<Response<PlannerToolReply>, Status> {
        let state = &self.0;
        let worker = identity(state, &request).await?;
        let input = request.into_inner();
        if input.payload_json.len() > 512 * 1024
            || !["model_call", "usage", "summary", "checkpoint", "failed"]
                .contains(&input.kind.as_str())
        {
            return Err(Status::invalid_argument("invalid planning event"));
        }
        let value = state
            .store
            .planning_report(
                worker,
                &input.run_id,
                &input.lease_token,
                &input.kind,
                &input.payload_json,
            )
            .await?;
        Ok(Response::new(PlannerToolReply {
            result_json: value.to_string(),
        }))
    }
}

pub(crate) async fn read_logs(state: &AppState, build: Uuid, args: &Value) -> Result<Value> {
    let target = args.get("target").and_then(Value::as_str).unwrap_or("");
    let rows = state.store.planning_logs(build, target).await?;
    let mut result = Vec::new();
    let mut size = 0;
    for row in rows.into_iter().rev() {
        let bytes = state.storage.get_log(&row.object_key).await?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text
            .chars()
            .take((128 * 1024usize).saturating_sub(size))
            .collect::<String>();
        size += text.len();
        result.push(json!({"step":row.step,"content":text}));
        if size >= 128 * 1024 {
            break;
        }
    }
    Ok(json!({"logs":result}))
}

fn target_requirements(recipe: &Recipe, requirements: &Value) -> Result<()> {
    let requirements = requirements
        .as_array()
        .context("target requirements must be an array")?;
    let platforms: Vec<_> = recipe
        .targets
        .values()
        .flat_map(|target| {
            if let Some(image) = &target.image {
                image.platforms.clone()
            } else {
                vec![format!("{}/{}", target.os, target.arch)]
            }
        })
        .collect();
    for requirement in requirements {
        let os = requirement["os"]
            .as_str()
            .context("target OS is required")?;
        let arch = requirement["arch"]
            .as_str()
            .context("target architecture is required")?;
        ensure!(
            platforms.contains(&format!("{os}/{arch}")),
            "plan does not cover requested platform {os}/{arch}"
        );
    }
    if !requirements.is_empty() {
        ensure!(
            platforms
                .iter()
                .all(|platform| requirements.iter().any(|r| platform
                    == &format!(
                        "{}/{}",
                        r["os"].as_str().unwrap_or(""),
                        r["arch"].as_str().unwrap_or("")
                    ))),
            "plan adds an unrequested platform"
        );
    }
    Ok(())
}

pub(crate) async fn prepare_plan(
    state: &AppState,
    context: crate::store::SubmissionContext,
    value: Value,
    evidence_verified: bool,
) -> Result<ExecutionPlan> {
    let mut plan = ExecutionPlan::parse(value)?;
    crate::images::ensure_targets_enabled(plan.recipe.targets.values(), crate::images::enabled())?;
    let snapshot: Uuid = context.snapshot_id;
    let source = source_cache::load(state, snapshot).await?;
    if !evidence_verified {
        source_cache::validate_evidence(
            state,
            &source,
            &plan
                .evidence
                .iter()
                .map(|item| item.path.clone())
                .collect::<Vec<_>>(),
        )
        .await?;
    }
    plan.recipe.environment(&context.parameters)?;
    target_requirements(&plan.recipe, &context.target_requirements)?;
    let planning_context = context.planning_context;
    if planning_context["placement_version"] == 1 {
        ensure!(
            plan.recipe
                .targets
                .iter()
                .filter(|(_, target)| target.executor == "native")
                .map(|(key, _)| key)
                .eq(plan.placements.keys()),
            "AI placement is required for every native target"
        );
    } else {
        // A newer worker must preserve the scheduler selected when this build began.
        plan.placements.clear();
    }
    let permitted: Vec<String> = serde_json::from_value(
        planning_context
            .get("signing_profiles")
            .cloned()
            .unwrap_or(json!([])),
    )?;
    let selected: Vec<String> = context.selected_targets;
    ensure!(
        selected.is_empty()
            || plan.recipe.targets.keys().all(|key| selected.contains(key))
                && selected
                    .iter()
                    .all(|key| plan.recipe.targets.contains_key(key)),
        "plan target names differ from requested targets"
    );
    let signing_defaults: Vec<crate::signing::Binding> = serde_json::from_value(
        planning_context
            .get("signing_defaults")
            .cloned()
            .unwrap_or(json!([])),
    )?;
    for target in plan.recipe.targets.values_mut() {
        crate::signing::policy::validate(target, &signing_defaults)?;
        if !target.signing_outputs.is_empty() {
            let binding = signing_defaults
                .iter()
                .find(|b| Some(b.platform.as_str()) == target.application_platform.as_deref())
                .context("missing signing binding")?;
            let mut tools = vec!["managed-signing-v1"];
            if binding.platform == "macos" {
                tools.extend(["codesign", "security"]);
                if target
                    .signing_outputs
                    .iter()
                    .any(|o| o.path.ends_with(".zip"))
                {
                    tools.push("ditto");
                }
                if target
                    .signing_outputs
                    .iter()
                    .any(|o| o.path.ends_with(".dmg"))
                {
                    tools.push("hdiutil");
                }
                if binding.notarize {
                    tools.push("xcrun");
                }
            } else if binding.metadata.purpose == Some(crate::signing::AndroidPurpose::Upload) {
                tools.extend(["jarsigner", "keytool"]);
            } else {
                tools.push("apksigner");
            }
            for tool in tools {
                if !target.capabilities.iter().any(|c| c == tool) {
                    target.capabilities.push(tool.into());
                }
            }
        }
        ensure!(
            target
                .signing_profiles
                .iter()
                .all(|name| permitted.contains(name)),
            "plan exceeds project signing authorization"
        );
    }
    source_capabilities(&mut plan.recipe, &source);
    let current: Option<Uuid> = context.current_plan_id;
    let failed: Vec<String> = context.failed_targets;
    if current.is_some() {
        let previous = context.previous.context("previous plan missing")?;
        let previous = ExecutionPlan::parse(previous)?;
        ensure!(
            plan.recipe
                .targets
                .keys()
                .eq(previous.recipe.targets.keys()),
            "revision cannot change target set"
        );
        for (name, target) in &plan.recipe.targets {
            let old = &previous.recipe.targets[name];
            ensure!(
                target.os == old.os
                    && target.arch == old.arch
                    && target.application_platform == old.application_platform
                    && target
                        .signing_profiles
                        .iter()
                        .all(|p| old.signing_profiles.contains(p)),
                "revision cannot change platform or expand signing authorization"
            );
            if !failed.contains(name) {
                ensure!(
                    serde_json::to_value(target)? == serde_json::to_value(old)?,
                    "revision cannot change a successful target"
                );
            }
        }
    }
    // Publication is controlled by explicit API requests and release authorization.
    plan.recipe.release.enabled = true;
    ExecutionPlan::parse(serde_json::to_value(plan)?)
}

fn source_capabilities(recipe: &mut Recipe, source: &source_cache::Snapshot) {
    if source.repositories.iter().any(|repo| !repo.lfs.is_empty()) {
        for target in recipe
            .targets
            .values_mut()
            .filter(|target| target.executor == "native")
        {
            if !target.capabilities.iter().any(|c| c == "git-lfs") {
                target.capabilities.push("git-lfs".into());
            }
        }
    }
}

pub async fn reap(state: &AppState) -> Result<()> {
    state.store.reap_planning().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lfs_capabilities_keep_mixed_plans_valid_after_persistence() {
        let value = json!({"summary":"mixed plan","evidence":[{"path":"Dockerfile","reason":"build context"}],"recipe":{"version":1,"targets":{"image":{"os":"linux","arch":"amd64","executor":"buildkit","image":{"name":"app"}},"native":{"os":"linux","arch":"amd64","artifacts":["dist/*"],"steps":[{"name":"build","run":"echo ok"}]}}}});
        let mut plan = ExecutionPlan::parse(value).unwrap();
        let source:source_cache::Snapshot=serde_json::from_value(json!({"id":Uuid::new_v4(),"project_id":Uuid::new_v4(),"commit":"a".repeat(40),"repositories":[{"id":Uuid::new_v4(),"repository_id":Uuid::new_v4(),"path":"","commit":"a".repeat(40),"tags":{},"lfs":[{"path":"data.bin","oid":"b".repeat(64),"size":42}]}]})).unwrap();
        source_capabilities(&mut plan.recipe, &source);
        let persisted = serde_json::to_value(&plan).unwrap();
        let mut revision = ExecutionPlan::parse(persisted.clone())
            .expect("saved plans must reparse for assignment and revisions");
        assert_eq!(plan.recipe.targets["native"].capabilities, vec!["git-lfs"]);
        assert!(plan.recipe.targets["image"].capabilities.is_empty());
        source_capabilities(&mut revision.recipe, &source);
        assert_eq!(serde_json::to_value(revision).unwrap(), persisted);
    }
    #[test]
    fn image_platform_requirements_cover_the_index_platforms() {
        let recipe=Recipe::parse("version: 1\ntargets:\n  image:\n    os: linux\n    arch: amd64\n    executor: buildkit\n    image:\n      name: app\n      platforms: [linux/amd64, linux/arm64]\n").unwrap();
        assert!(
            target_requirements(
                &recipe,
                &json!([{"os":"linux","arch":"amd64"},{"os":"linux","arch":"arm64"}])
            )
            .is_ok()
        );
        assert!(target_requirements(&recipe, &json!([{"os":"linux","arch":"amd64"}])).is_err());
        assert!(target_requirements(&recipe,&json!([{"os":"linux","arch":"amd64"},{"os":"linux","arch":"arm64"},{"os":"darwin","arch":"arm64"}])).is_err());
    }
}
