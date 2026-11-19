//! [INPUT]: 依赖 SourceTask、直接 Pod 台账、PlanningPool 配额与选主租约。
//! [OUTPUT]: 固定总截止时间，最多执行三次源码 Pod，确认停机后重试或清理。
//! [POS]: 未知执行保留配额；终态无台账且无归属 Pod 后释放保护。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{PlanningPool, PodExecution, SourceTask, Status},
    controller::Context,
    pods, resources,
};
use anyhow::{Result, ensure};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api, ResourceExt,
    api::ListParams,
    runtime::{Controller, controller::Action, watcher},
};
use std::{sync::Arc, time::Duration};
const MAX_ATTEMPTS: u32 = 3;
fn retry() -> Action {
    Action::requeue(Duration::from_secs(3))
}
async fn status(
    ctx: &Context,
    task: &SourceTask,
    phase: &str,
    message: Option<String>,
    execution: Option<PodExecution>,
) -> Result<bool> {
    pods::status(
        ctx,
        &Api::<SourceTask>::namespaced(ctx.client.clone(), &ctx.namespace),
        task,
        task.status.as_ref(),
        Status {
            phase: phase.into(),
            message,
            execution,
            observed_generation: task.metadata.generation.unwrap_or(1),
        },
    )
    .await
}
async fn cleanup(
    ctx: &Context,
    task: &SourceTask,
    phase: &str,
    current: Option<&Pod>,
) -> Result<Action> {
    let mut saved = task
        .status
        .as_ref()
        .and_then(|status| status.execution.clone());
    if let Some(pod) = current {
        let record = saved
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("source Pod ledger missing"))?;
        pods::verify(task, record, pod)?;
        let mut bound = pods::bind(record, pod)?;
        bound.stopped |= pods::stopped(pod);
        saved = Some(bound);
    } else if saved
        .as_ref()
        .is_some_and(|record| record.stopped || !record.create_started)
    {
        if phase == "Retrying" {
            let old = saved.as_ref().expect("retry requires execution ledger");
            let next = pods::reserve(
                &task.spec.task_id,
                &task
                    .uid()
                    .ok_or_else(|| anyhow::anyhow!("SourceTask UID required"))?,
                "source",
                old.attempt + 1,
                old.deadline_ms,
            )?;
            status(ctx, task, "Provisioning", None, Some(next)).await?;
            return Ok(retry());
        }
        saved = None;
    }
    let pending = saved.is_some() || pods::has_owned(ctx, task).await?;
    if pending
        && task.metadata.deletion_timestamp.is_none()
        && pods::finalizer(
            ctx,
            &Api::<SourceTask>::namespaced(ctx.client.clone(), &ctx.namespace),
            task,
            pods::CR_FINALIZER,
            true,
        )
        .await?
    {
        return Ok(retry());
    }
    if status(
        ctx,
        task,
        phase,
        pending.then(|| pods::CLEANUP_PENDING.into()),
        saved.clone(),
    )
    .await?
    {
        return Ok(retry());
    }
    if let (Some(record), Some(pod)) = (&saved, current) {
        pods::delete(ctx, record, pod).await?;
    }
    if !pending && ["Succeeded", "Failed", "Canceled"].contains(&phase) {
        pods::finalizer(
            ctx,
            &Api::<SourceTask>::namespaced(ctx.client.clone(), &ctx.namespace),
            task,
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
pub async fn reconcile_task(task: Arc<SourceTask>, ctx: Arc<Context>) -> Result<Action> {
    // 所有本进程预留先串行，CR 状态再用 resourceVersion 防止跨主副本重放。
    let _guard = ctx.source_admission.lock().await;
    ctx.leader.ensure().await?;
    let tasks = Api::<SourceTask>::namespaced(ctx.client.clone(), &ctx.namespace);
    let completed = task.status.as_ref().filter(|status| {
        status.execution.is_none()
            && ["Succeeded", "Failed", "Canceled"].contains(&status.phase.as_str())
    });
    if task.metadata.deletion_timestamp.is_none()
        && completed.is_none()
        && pods::finalizer(&ctx, &tasks, &*task, pods::CR_FINALIZER, true).await?
    {
        return Ok(retry());
    }
    if let Some(status) = completed {
        return cleanup(&ctx, &task, &status.phase, None).await;
    }
    let saved = task
        .status
        .as_ref()
        .and_then(|status| status.execution.as_ref());
    let api = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    let current = match saved {
        Some(record) => api.get_opt(&record.name).await?,
        None => None,
    };
    if let (Some(record), Some(pod)) = (saved, &current) {
        pods::verify(&*task, record, pod)?;
    }
    let terminal = task
        .status
        .as_ref()
        .map(|status| status.phase.as_str())
        .filter(|phase| ["Succeeded", "Failed", "Canceled"].contains(phase));
    if let Some(phase) = terminal {
        return cleanup(&ctx, &task, phase, current.as_ref()).await;
    }
    if task.spec.cancel_requested || task.metadata.deletion_timestamp.is_some() {
        return cleanup(&ctx, &task, "Canceled", current.as_ref()).await;
    }
    if saved.is_some_and(|record| record.deadline_ms <= chrono::Utc::now().timestamp_millis()) {
        return cleanup(&ctx, &task, "Failed", current.as_ref()).await;
    }
    if task
        .status
        .as_ref()
        .is_some_and(|status| status.phase == "Retrying")
    {
        return cleanup(&ctx, &task, "Retrying", current.as_ref()).await;
    }
    if let Some(record) = saved {
        if let Some(pod) = &current {
            let bound = pods::bind(record, pod)?;
            if &bound != record {
                status(&ctx, &task, "Provisioning", None, Some(bound)).await?;
                return Ok(retry());
            }
            if pod.metadata.deletion_timestamp.is_some() {
                return cleanup(&ctx, &task, "Failed", Some(pod)).await;
            }
            let failed = pod.status.as_ref().is_some_and(|status| {
                status.phase.as_deref() == Some("Failed")
                    || status.container_statuses.iter().flatten().any(|container| {
                        container
                            .state
                            .as_ref()
                            .and_then(|state| state.terminated.as_ref())
                            .is_some_and(|ended| ended.exit_code != 0)
                    })
            });
            if failed {
                return cleanup(
                    &ctx,
                    &task,
                    if record.attempt < MAX_ATTEMPTS {
                        "Retrying"
                    } else {
                        "Failed"
                    },
                    Some(pod),
                )
                .await;
            }
            if pod
                .status
                .as_ref()
                .is_some_and(|status| status.phase.as_deref() == Some("Succeeded"))
            {
                return cleanup(&ctx, &task, "Succeeded", Some(pod)).await;
            }
            status(
                &ctx,
                &task,
                "Running",
                pod.status.as_ref().and_then(|status| {
                    status
                        .container_statuses
                        .iter()
                        .flatten()
                        .find_map(|container| {
                            container.state.as_ref()?.waiting.as_ref()?.reason.clone()
                        })
                }),
                Some(record.clone()),
            )
            .await?;
            return Ok(retry());
        }
        if record.uid.is_some() {
            // 缺失对象不证明进程停止；保留原 UID，不创建下一次 attempt。
            status(
                &ctx,
                &task,
                "Failed",
                Some("PodMissing: execution remains charged until stop is confirmed".into()),
                Some(record.clone()),
            )
            .await?;
            return Ok(retry());
        }
        if !record.create_started {
            let mut started = record.clone();
            started.create_started = true;
            status(&ctx, &task, "Provisioning", None, Some(started)).await?;
            return Ok(retry());
        }
        let pool = Api::<PlanningPool>::namespaced(ctx.client.clone(), &ctx.namespace)
            .get(&task.spec.pool)
            .await?;
        let desired = resources::source_pod(&task, &pool, record)?;
        let pod = pods::create(&ctx, &*task, record, &desired).await?;
        status(
            &ctx,
            &task,
            "Provisioning",
            None,
            Some(pods::bind(record, &pod)?),
        )
        .await?;
        return Ok(retry());
    }
    let pool = Api::<PlanningPool>::namespaced(ctx.client.clone(), &ctx.namespace)
        .get(&task.spec.pool)
        .await?;
    let all = tasks.list(&ListParams::default()).await?;
    let charged: Vec<_> = all
        .iter()
        .filter(|other| {
            other.spec.pool == task.spec.pool
                && other
                    .status
                    .as_ref()
                    .is_some_and(|status| status.execution.is_some())
        })
        .collect();
    // 即使 CR 被强制移除，受保护的孤儿 Pod 仍占容量。
    let orphaned: Vec<_> = api
        .list(&ListParams::default().labels(&format!(
            "artifactory.dev/source-task-pool={}",
            task.spec.pool
        )))
        .await?
        .items
        .into_iter()
        .filter(|pod| {
            !all.iter().any(|owner| {
                pods::accounted(
                    owner,
                    owner.status.as_ref().and_then(|s| s.execution.as_ref()),
                    pod,
                )
            })
        })
        .collect();
    let project_count = charged
        .iter()
        .filter(|other| other.spec.project_id == task.spec.project_id)
        .count()
        + orphaned
            .iter()
            .filter(|pod| {
                pod.labels().get("artifactory.dev/project") == Some(&task.spec.project_id)
            })
            .count();
    if charged.len() + orphaned.len() >= pool.spec.max_active as usize
        || project_count >= pool.spec.max_per_project as usize
    {
        status(&ctx, &task, "Queued", None, None).await?;
        return Ok(retry());
    }
    let uid = task
        .uid()
        .ok_or_else(|| anyhow::anyhow!("SourceTask UID required"))?;
    let record = pods::reserve(
        &task.spec.task_id,
        &uid,
        "source",
        1,
        chrono::Utc::now().timestamp_millis() + i64::from(task.spec.timeout_seconds) * 1000,
    )?;
    ensure!(
        record.attempt <= MAX_ATTEMPTS,
        "source retry budget exhausted"
    );
    status(&ctx, &task, "Provisioning", None, Some(record)).await?;
    Ok(retry())
}
#[derive(Debug)]
struct Error(anyhow::Error);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
pub async fn controller(ctx: Arc<Context>) {
    let tasks = Api::<SourceTask>::namespaced(ctx.client.clone(), &ctx.namespace);
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    Controller::new(tasks, watcher::Config::default())
        .owns(pods, watcher::Config::default())
        .run(
            |task, ctx| async { reconcile_task(task, ctx).await.map_err(Error) },
            |_, error, _| {
                tracing::warn!(%error, "source Pod reconciliation deferred");
                retry()
            },
            ctx,
        )
        .for_each(|_| async {})
        .await;
}
