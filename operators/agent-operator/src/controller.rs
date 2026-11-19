//! [INPUT]: 依赖 Agent CR、Runtime、Pod 执行台账与选主租约。
//! [OUTPUT]: 按池和项目配额创建规划 Pod，上报故障并确认停机后回收。
//! [POS]: 未知执行保留容量；终态无台账且无归属 Pod 后释放保护。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{api::*, leadership::Leadership, pods, resources};
use anyhow::Result;
use artifactory_runtime_protocol::{
    Execution, ExecutionFailure, ExecutionFailureReason, service::Client as ServiceClient,
};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api, Client, ResourceExt,
    api::{ListParams, Patch, PatchParams, PostParams},
    runtime::{Controller, controller::Action, watcher},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
const HISTORY_SECONDS: i64 = 86400;
pub struct Context {
    pub client: Client,
    pub namespace: String,
    pub runtime: ServiceClient,
    pub leader: Leadership,
    pub source_admission: tokio::sync::Mutex<()>,
}
fn retry() -> Action {
    Action::requeue(Duration::from_secs(3))
}
fn execution(run: &AgentRun, saved: &PodExecution, namespace: &str) -> Result<Execution> {
    Ok(Execution {
        run_id: run.spec.run_id.parse()?,
        generation: run.spec.generation,
        uid: saved
            .uid
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Pod UID not yet bound"))?,
        name: saved.name.clone(),
        namespace: namespace.into(),
    })
}
async fn update(
    ctx: &Context,
    run: &AgentRun,
    phase: &str,
    message: Option<String>,
    execution: Option<PodExecution>,
) -> Result<bool> {
    let desired = Status {
        phase: phase.into(),
        message,
        execution,
        observed_generation: run.metadata.generation.unwrap_or(1),
    };
    pods::status(
        ctx,
        &Api::<AgentRun>::namespaced(ctx.client.clone(), &ctx.namespace),
        run,
        run.status.as_ref(),
        desired,
    )
    .await
}
async fn cleanup(ctx: &Context, run: &AgentRun, phase: &str, pod: Option<&Pod>) -> Result<Action> {
    let mut saved = run
        .status
        .as_ref()
        .and_then(|status| status.execution.clone());
    if let Some(current) = pod {
        let record = saved
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pod execution ledger missing"))?;
        pods::verify(run, record, current)?;
        let mut record = pods::bind(record, current)?;
        record.stopped |= pods::stopped(current);
        saved = Some(record);
    } else if saved
        .as_ref()
        .is_some_and(|record| !record.create_started || record.stopped)
    {
        saved = None;
    }
    let pending = saved.is_some() || pods::has_owned(ctx, run).await?;
    if pending
        && run.metadata.deletion_timestamp.is_none()
        && pods::finalizer(
            ctx,
            &Api::<AgentRun>::namespaced(ctx.client.clone(), &ctx.namespace),
            run,
            pods::CR_FINALIZER,
            true,
        )
        .await?
    {
        return Ok(retry());
    }
    let message = pending.then(|| pods::CLEANUP_PENDING.into());
    if update(ctx, run, phase, message, saved.clone()).await? {
        return Ok(retry());
    }
    if let (Some(record), Some(pod)) = (&saved, pod) {
        pods::delete(ctx, record, pod).await?;
    }
    if !pending
        && [
            "Superseded",
            "canceled",
            "failed",
            "succeeded",
            "needs_input",
        ]
        .contains(&phase)
    {
        pods::finalizer(
            ctx,
            &Api::<AgentRun>::namespaced(ctx.client.clone(), &ctx.namespace),
            run,
            pods::CR_FINALIZER,
            false,
        )
        .await?;
    }
    Ok(if pending {
        retry()
    } else {
        Action::requeue(Duration::from_secs(60))
    })
}
async fn lost(
    ctx: &Context,
    run: &AgentRun,
    saved: Option<&PodExecution>,
    reason: &str,
) -> Result<Value> {
    let identity = saved
        .filter(|record| record.uid.is_some())
        .map(|record| execution(run, record, &ctx.namespace))
        .transpose()?;
    ctx.leader.ensure().await?;
    ctx.runtime.post("/v1/execution-lost", &json!({"run_id":run.spec.run_id,"generation":run.spec.generation,"execution":identity,"reason":reason})).await
}
pub async fn run_reconcile(run: Arc<AgentRun>, ctx: Arc<Context>) -> Result<Action, Error> {
    reconcile(run, ctx).await.map_err(Error)
}
async fn reconcile(run: Arc<AgentRun>, ctx: Arc<Context>) -> Result<Action> {
    let _guard = ctx.source_admission.lock().await;
    ctx.leader.ensure().await?;
    let runs = Api::<AgentRun>::namespaced(ctx.client.clone(), &ctx.namespace);
    let completed = run.status.as_ref().filter(|status| {
        status.execution.is_none()
            && [
                "Superseded",
                "canceled",
                "failed",
                "succeeded",
                "needs_input",
            ]
            .contains(&status.phase.as_str())
    });
    if run.metadata.deletion_timestamp.is_none()
        && completed.is_none()
        && pods::finalizer(&ctx, &runs, &*run, pods::CR_FINALIZER, true).await?
    {
        return Ok(retry());
    }
    if let Some(status) = completed {
        return cleanup(&ctx, &run, &status.phase, None).await;
    }
    let runtime: Value = ctx
        .runtime
        .post("/v1/get", &json!({"run_id":run.spec.run_id}))
        .await?;
    let generation = runtime["lifecycle"]["generation"]
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("Runtime generation missing"))?;
    let state = runtime["status"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Runtime status missing"))?;
    let saved = run
        .status
        .as_ref()
        .and_then(|status| status.execution.as_ref());
    let api = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    let current = match saved {
        Some(record) => api.get_opt(&record.name).await?,
        None => None,
    };
    if let (Some(record), Some(pod)) = (saved, &current) {
        pods::verify(&*run, record, pod)?;
    }
    let terminal = ["canceled", "failed", "succeeded", "needs_input"].contains(&state);
    if run.metadata.deletion_timestamp.is_some() && generation == run.spec.generation && !terminal {
        // 删除预留阶段也必须终止 Runtime，防止该代永久占用逻辑槽位。
        if let Some(record) = saved.filter(|record| record.uid.is_some()) {
            ctx.leader.ensure().await?;
            let _: Value = ctx
                .runtime
                .post("/v1/register", &execution(&run, record, &ctx.namespace)?)
                .await?;
        }
        let result = lost(&ctx, &run, saved, "resource_deleted").await?;
        let phase = result["status"].as_str().unwrap_or("failed");
        return cleanup(&ctx, &run, phase, current.as_ref()).await;
    }
    if generation != run.spec.generation || terminal {
        return cleanup(
            &ctx,
            &run,
            if generation != run.spec.generation {
                "Superseded"
            } else {
                state
            },
            current.as_ref(),
        )
        .await;
    }
    if let Some(record) = saved {
        if let Some(pod) = &current {
            let bound = pods::bind(record, pod)?;
            if &bound != record {
                update(&ctx, &run, "Provisioning", None, Some(bound)).await?;
                return Ok(retry());
            }
            let identity = execution(&run, record, &ctx.namespace)?;
            ctx.leader.ensure().await?;
            let _: Value = ctx.runtime.post("/v1/register", &identity).await?;
            if pod.metadata.deletion_timestamp.is_some() {
                let result = lost(&ctx, &run, Some(record), "pod_missing").await?;
                return cleanup(
                    &ctx,
                    &run,
                    result["status"].as_str().unwrap_or("failed"),
                    Some(pod),
                )
                .await;
            }
            if let Some(reason) = pod_failure(pod) {
                ctx.leader.ensure().await?;
                let result: Value = ctx
                    .runtime
                    .post(
                        "/v1/execution-failed",
                        &ExecutionFailure {
                            execution: identity,
                            reason,
                        },
                    )
                    .await?;
                if let Some(terminal) = result["status"].as_str().filter(|value| {
                    ["canceled", "failed", "succeeded", "needs_input"].contains(value)
                }) {
                    return cleanup(&ctx, &run, terminal, Some(pod)).await;
                }
            }
            update(
                &ctx,
                &run,
                if state == "running" {
                    "Running"
                } else {
                    "Provisioning"
                },
                pod_message(pod),
                Some(record.clone()),
            )
            .await?;
            return Ok(retry());
        }
        if record.uid.is_some() {
            ctx.leader.ensure().await?;
            let _: Value = ctx
                .runtime
                .post("/v1/register", &execution(&run, record, &ctx.namespace)?)
                .await?;
            lost(&ctx, &run, Some(record), "pod_missing").await?;
            update(
                &ctx,
                &run,
                "Lost",
                Some("PodMissing: execution remains charged until stop is confirmed".into()),
                Some(record.clone()),
            )
            .await?;
            return Ok(retry());
        }
        if record.deadline_ms <= chrono::Utc::now().timestamp_millis() {
            lost(&ctx, &run, None, "resource_deleted").await?;
            return Ok(retry());
        }
        if !record.create_started {
            let mut started = record.clone();
            started.create_started = true;
            update(&ctx, &run, "Provisioning", None, Some(started)).await?;
            return Ok(retry());
        }
        let pool = Api::<AgentPool>::namespaced(ctx.client.clone(), &ctx.namespace)
            .get(&run.spec.pool)
            .await?;
        let desired = resources::planning_pod(&run, &pool, record)?;
        let created = pods::create(&ctx, &*run, record, &desired).await?;
        update(
            &ctx,
            &run,
            "Provisioning",
            None,
            Some(pods::bind(record, &created)?),
        )
        .await?;
        return Ok(retry());
    }
    // 旧代未知执行未停止时，新代只保留逻辑排队，不创建第二个 Pod。
    let previous = runs.list(&ListParams::default()).await?;
    if previous.iter().any(|other| {
        other.spec.run_id == run.spec.run_id
            && other.spec.generation < run.spec.generation
            && other
                .status
                .as_ref()
                .is_some_and(|status| status.execution.is_some())
    }) {
        update(
            &ctx,
            &run,
            "Queued",
            Some("PreviousExecutionPending: waiting for previous generation cleanup".into()),
            None,
        )
        .await?;
        return Ok(retry());
    }
    let uid = run
        .uid()
        .ok_or_else(|| anyhow::anyhow!("AgentRun UID required"))?;
    let pool = Api::<AgentPool>::namespaced(ctx.client.clone(), &ctx.namespace)
        .get(&run.spec.pool)
        .await?;
    let charged = previous
        .iter()
        .filter(|other| {
            other.spec.pool == run.spec.pool
                && other
                    .status
                    .as_ref()
                    .is_some_and(|status| status.execution.is_some())
        })
        .collect::<Vec<_>>();
    let orphaned = api
        .list(
            &ListParams::default().labels(&format!("artifactory.dev/agent-pool={}", run.spec.pool)),
        )
        .await?
        .items
        .into_iter()
        .filter(|pod| {
            !previous.iter().any(|owner| {
                pods::accounted(
                    owner,
                    owner.status.as_ref().and_then(|s| s.execution.as_ref()),
                    pod,
                )
            })
        })
        .collect::<Vec<_>>();
    let project_count = charged
        .iter()
        .filter(|owner| owner.spec.project_id == run.spec.project_id)
        .count()
        + orphaned
            .iter()
            .filter(|pod| pod.labels().get("artifactory.dev/project") == Some(&run.spec.project_id))
            .count();
    if charged.len() + orphaned.len() >= pool.spec.max_active as usize
        || project_count >= pool.spec.max_per_project as usize
    {
        update(
            &ctx,
            &run,
            "Queued",
            Some("PoolCapacityPending: waiting for confirmed execution cleanup".into()),
            None,
        )
        .await?;
        return Ok(retry());
    }
    let mut reserved = pods::reserve(
        &run.spec.run_id,
        &uid,
        "pi",
        1,
        chrono::Utc::now().timestamp_millis() + 4_500_000,
    )?;
    reserved.name = format!(
        "pi-{}-g{}-{}",
        uuid::Uuid::parse_str(&run.spec.run_id)?.simple(),
        run.spec.generation,
        &artifactory_runtime_protocol::crypto::hash(&uid)[..8]
    );
    update(&ctx, &run, "Provisioning", None, Some(reserved)).await?;
    Ok(retry())
}

fn pod_failure(pod: &Pod) -> Option<ExecutionFailureReason> {
    let status = pod.status.as_ref()?;
    if let Some(container) = status
        .init_container_statuses
        .iter()
        .flatten()
        .find(|container| {
            container
                .state
                .as_ref()
                .and_then(|state| state.terminated.as_ref())
                .is_some_and(|terminated| terminated.exit_code != 0)
        })
    {
        let exit_code = container.state.as_ref()?.terminated.as_ref()?.exit_code;
        return Some(if container.name == "source" {
            ExecutionFailureReason::SourcePreparation { exit_code }
        } else {
            ExecutionFailureReason::PodFailed
        });
    }
    if let Some(terminated) = status
        .container_statuses
        .iter()
        .flatten()
        .find(|container| container.name == "pi")
        .and_then(|container| container.state.as_ref()?.terminated.as_ref())
    {
        return Some(ExecutionFailureReason::WorkerExit {
            exit_code: terminated.exit_code,
        });
    }
    match status.phase.as_deref() {
        Some("Failed") => Some(ExecutionFailureReason::PodFailed),
        Some("Succeeded") => Some(ExecutionFailureReason::WorkerExit { exit_code: 0 }),
        _ => None,
    }
}
fn pod_message(pod: &Pod) -> Option<String> {
    let status = pod.status.as_ref()?;
    status
        .init_container_statuses
        .iter()
        .flatten()
        .chain(status.container_statuses.iter().flatten())
        .find_map(|container| container.state.as_ref()?.waiting.as_ref()?.reason.clone())
        .or_else(|| {
            status
                .conditions
                .iter()
                .flatten()
                .find(|condition| condition.type_ == "PodScheduled" && condition.status == "False")
                .and_then(|condition| condition.reason.clone())
        })
}
#[derive(Debug)]
pub struct Error(anyhow::Error);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
fn policy(_: Arc<AgentRun>, error: &Error, _: Arc<Context>) -> Action {
    tracing::warn!(%error, "planning Pod reconciliation deferred");
    retry()
}
pub async fn controller(ctx: Arc<Context>) {
    let runs = Api::<AgentRun>::namespaced(ctx.client.clone(), &ctx.namespace);
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    Controller::new(runs, watcher::Config::default())
        .owns(pods, watcher::Config::default())
        .with_config(kube::runtime::controller::Config::default().concurrency(8))
        .run(run_reconcile, policy, ctx)
        .for_each(|_| async {})
        .await;
}
async fn orphaned_capacity(ctx: &Context, pool: &str) -> Result<u32> {
    let runs = Api::<AgentRun>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?;
    let mut occupied = 0;
    for run in runs.iter().filter(|run| {
        run.spec.pool == pool
            && run
                .status
                .as_ref()
                .is_some_and(|status| status.execution.is_some())
    }) {
        let state: Value = ctx
            .runtime
            .post("/v1/get", &json!({"run_id":run.spec.run_id}))
            .await?;
        if state["lifecycle"]["generation"] != run.spec.generation
            || ["canceled", "failed", "succeeded", "needs_input"]
                .contains(&state["status"].as_str().unwrap_or("unknown"))
        {
            occupied += 1;
        }
    }
    let api = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    for pod in api
        .list(&ListParams::default().labels(&format!("artifactory.dev/agent-pool={pool}")))
        .await?
    {
        if !runs.iter().any(|run| {
            pods::accounted(
                run,
                run.status.as_ref().and_then(|s| s.execution.as_ref()),
                &pod,
            )
        }) {
            occupied += 1;
        }
    }
    Ok(occupied)
}

pub async fn dispatch(ctx: &Context) -> Result<()> {
    ctx.leader.ensure().await?;
    let pools: Api<AgentPool> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let runs: Api<AgentRun> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    for pool in pools.list(&ListParams::default()).await? {
        let occupied = orphaned_capacity(ctx, &pool.name_any()).await?;
        let limit = pool.spec.max_active.saturating_sub(occupied);
        ctx.leader.ensure().await?;
        let value:Value=ctx.runtime.post("/v1/queue",&json!({"pool":pool.name_any(),"limit":limit,"per_project":pool.spec.max_per_project})).await?;
        let status = Status {
            phase: if pool.spec.max_active == 0 {
                "Paused"
            } else {
                "Ready"
            }
            .into(),
            message: Some(format!(
                "active={}, queued={}",
                value["active"], value["queued"]
            )),
            execution: None,
            observed_generation: pool.metadata.generation.unwrap_or(1),
        };
        if pool.status.as_ref() != Some(&status) {
            ctx.leader.ensure().await?;
            pools.patch_status(&pool.name_any(),&PatchParams::default(),&Patch::Merge(json!({"metadata":{"resourceVersion":pool.resource_version()},"status":status}))).await?;
        }
        for descriptor in value["runs"].as_array().into_iter().flatten() {
            let run = descriptor["run_id"].as_str().unwrap_or_default();
            let generation = descriptor["generation"].as_i64().unwrap_or(1);
            let name = format!("run-{run}-g{generation}");
            if runs.get_opt(&name).await?.is_some() {
                continue;
            }
            let resource = AgentRun::new(
                &name,
                AgentRunSpec {
                    run_id: run.into(),
                    project_id: descriptor["project_id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Runtime project ID missing"))?
                        .into(),
                    generation,
                    pool: pool.name_any(),
                    source_manifest_id: descriptor["source_manifest_id"]
                        .as_str()
                        .map(str::to_owned),
                    source_digest: descriptor["source_digest"].as_str().map(str::to_owned),
                },
            );
            ctx.leader.ensure().await?;
            match runs.create(&PostParams::default(), &resource).await {
                Ok(_) => {}
                Err(kube::Error::Api(e)) if e.code == 409 => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    let known_runs = runs.list(&ListParams::default()).await?;
    for run in known_runs {
        if run_reclaimable(&run, chrono::Utc::now().timestamp()) {
            ctx.leader.ensure().await?;
            runs.delete(
                &run.name_any(),
                &kube::api::DeleteParams {
                    preconditions: Some(kube::api::Preconditions {
                        uid: run.uid(),
                        resource_version: None,
                    }),
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    Ok(())
}
fn run_reclaimable(run: &AgentRun, now: i64) -> bool {
    run.status.as_ref().is_some_and(|s| {
        s.message.is_none()
            && s.execution.is_none()
            && matches!(
                s.phase.as_str(),
                "Superseded" | "canceled" | "failed" | "succeeded" | "needs_input"
            )
    }) && run
        .metadata
        .creation_timestamp
        .as_ref()
        .is_some_and(|t| t.0.as_second() < now - HISTORY_SECONDS)
}
