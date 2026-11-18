//! [INPUT]: 依赖 Image CRD、节点能力、Job 状态和选主租约。
//! [OUTPUT]: 协调镜像构建、发布、取消与子资源归属。
//! [POS]: Image Operator 的 Kubernetes 生命周期边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{BuilderPool, ImageBuild, Status},
    leadership::Leadership,
    resources,
};
use anyhow::{Result, ensure};
use futures::StreamExt;
use k8s_openapi::api::{
    batch::v1::Job,
    core::v1::{Node, Pod},
};
use kube::{
    Api, Client, ResourceExt,
    api::{DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions},
    runtime::{Controller, controller::Action, watcher},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

pub struct Context {
    pub client: Client,
    pub namespace: String,
    pub leader: Leadership,
    pub admission: tokio::sync::Mutex<()>,
}

async fn update(
    ctx: &Context,
    build: &ImageBuild,
    phase: &str,
    message: Option<String>,
) -> Result<Action> {
    let desired = Status {
        phase: phase.into(),
        message,
        observed_generation: build.metadata.generation.unwrap_or(1),
    };
    if build.status.as_ref() != Some(&desired) {
        ctx.leader.ensure().await?;
        Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace).patch_status(&build.name_any(), &PatchParams::default(), &Patch::Merge(serde_json::json!({"metadata":{"resourceVersion":build.resource_version()},"status":desired}))).await?;
    }
    Ok(Action::requeue(Duration::from_secs(3)))
}

async fn stop(ctx: &Context, build: &ImageBuild, jobs: &[Job]) -> Result<()> {
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    for job in jobs.iter().filter(|j| resources::owned(build, j)) {
        if job.metadata.deletion_timestamp.is_some() {
            continue;
        }
        ctx.leader.ensure().await?;
        match api.patch(&job.name_any(), &PatchParams::default(), &Patch::Merge(serde_json::json!({"metadata":{"resourceVersion":job.resource_version()},"spec":{"ttlSecondsAfterFinished":300}}))).await {
            Ok(_) => (),
            Err(kube::Error::Api(error)) if error.code == 404 => continue,
            Err(error) => return Err(error.into()),
        }
        let params = DeleteParams {
            preconditions: Some(Preconditions {
                uid: job.uid(),
                resource_version: None,
            }),
            propagation_policy: Some(kube::api::PropagationPolicy::Background),
            ..Default::default()
        };
        match api.delete(&job.name_any(), &params).await {
            Ok(_) => (),
            Err(kube::Error::Api(e)) if e.code == 404 => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

async fn cleanup(ctx: &Context, build: &ImageBuild, jobs: &[Job], phase: &str) -> Result<Action> {
    // 先持久化终态。删除后重启不会重建已完成的构建。
    if build.status.as_ref().is_none_or(|s| s.phase != phase) {
        return update(
            ctx,
            build,
            phase,
            Some("CleanupPending: waiting for Job/Pod deletion".into()),
        )
        .await;
    }
    stop(ctx, build, jobs).await?;
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(
            &ListParams::default()
                .labels(&format!("artifactory.dev/image-build={}", build.name_any())),
        )
        .await?;
    let job_uids: Vec<_> = jobs
        .iter()
        .filter(|j| resources::owned(build, j))
        .filter_map(ResourceExt::uid)
        .collect();
    let pending = jobs.iter().any(|j| resources::owned(build, j))
        || pods.iter().any(|pod| {
            pod.owner_references().iter().any(|o| {
                o.kind == "Job"
                    && (job_uids.contains(&o.uid)
                        || o.name.starts_with(&format!(
                            "image-{}-",
                            build.spec.execution_id.replace('-', "")
                        )))
            })
        });
    update(
        ctx,
        build,
        phase,
        pending.then(|| "CleanupPending: waiting for Job/Pod deletion".into()),
    )
    .await?;
    Ok(if pending {
        Action::requeue(Duration::from_secs(3))
    } else {
        Action::await_change()
    })
}

async fn create(
    ctx: &Context,
    build: &ImageBuild,
    pool: &BuilderPool,
    platform: Option<&str>,
    selector: BTreeMap<String, String>,
) -> Result<bool> {
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let name = resources::name(build, platform)?;
    if let Some(existing) = api.get_opt(&name).await? {
        ensure!(resources::owned(build, &existing), "Job ownership conflict");
        return Ok(true);
    }
    let active = api
        .list(
            &ListParams::default()
                .labels(&format!("artifactory.dev/builder-pool={}", build.spec.pool)),
        )
        .await?
        .items;
    let active: Vec<_> = active
        .iter()
        .filter(|j| resources::phase(j).is_none())
        .collect();
    if active.len() >= pool.spec.max_active as usize
        || active
            .iter()
            .filter(|j| j.labels().get("artifactory.dev/project") == Some(&build.spec.project_id))
            .count()
            >= pool.spec.max_per_project as usize
    {
        return Ok(false);
    }
    ctx.leader.ensure().await?;
    match api
        .create(
            &PostParams::default(),
            &resources::job(build, pool, platform, selector)?,
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(kube::Error::Api(e)) if e.code == 409 => {
            ensure!(
                resources::owned(build, &api.get(&name).await?),
                "Job ownership conflict"
            );
            Ok(true)
        }
        Err(e) => Err(e.into()),
    }
}

async fn reconcile_inner(build: Arc<ImageBuild>, ctx: Arc<Context>) -> Result<Action> {
    let _guard = ctx.admission.lock().await;
    ctx.leader.ensure().await?;
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let jobs = api
        .list(
            &ListParams::default()
                .labels(&format!("artifactory.dev/image-build={}", build.name_any())),
        )
        .await?
        .items;
    if build.spec.cancel_requested || build.metadata.deletion_timestamp.is_some() {
        return cleanup(&ctx, &build, &jobs, "Canceled").await;
    }
    if jobs
        .iter()
        .any(|j| resources::owned(&build, j) && resources::phase(j) == Some("Failed"))
    {
        return cleanup(&ctx, &build, &jobs, "Failed").await;
    }
    if build
        .status
        .as_ref()
        .is_some_and(|s| ["Succeeded", "Failed", "Canceled"].contains(&s.phase.as_str()))
    {
        let phase = &build.status.as_ref().expect("terminal status").phase;
        return cleanup(&ctx, &build, &jobs, phase).await;
    }
    let pool = Api::<BuilderPool>::namespaced(ctx.client.clone(), &ctx.namespace)
        .get(&build.spec.pool)
        .await?;
    let mut complete = true;
    let nodes = Api::<Node>::all(ctx.client.clone())
        .list(&ListParams::default())
        .await?
        .items;
    for platform in build
        .spec
        .platforms
        .iter()
        .filter(|_| !build.spec.publish_only)
    {
        let name = resources::name(&build, Some(platform))?;
        if let Some(job) = jobs.iter().find(|j| j.name_any() == name) {
            ensure!(resources::owned(&build, job), "Job ownership conflict");
            complete &= resources::phase(job) == Some("Succeeded");
        } else {
            complete = false;
            if let Some(selector) = resources::placement(&build, &pool, platform, &nodes)? {
                create(&ctx, &build, &pool, Some(platform), selector).await?;
            }
        }
    }
    if complete {
        let name = resources::name(&build, None)?;
        if let Some(job) = jobs.iter().find(|j| j.name_any() == name) {
            ensure!(
                resources::owned(&build, job),
                "publisher ownership conflict"
            );
            if resources::phase(job) == Some("Succeeded") {
                return cleanup(&ctx, &build, &jobs, "Succeeded").await;
            }
        } else {
            create(&ctx, &build, &pool, None, pool.spec.node_selector.clone()).await?;
        }
        update(&ctx, &build, "Publishing", None).await
    } else {
        update(&ctx, &build, "Building", None).await
    }
}

#[derive(Debug)]
pub struct Error(anyhow::Error);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
impl std::error::Error for Error {}
async fn reconcile(build: Arc<ImageBuild>, ctx: Arc<Context>) -> Result<Action, Error> {
    reconcile_inner(build, ctx).await.map_err(Error)
}
fn policy(_: Arc<ImageBuild>, error: &Error, _: Arc<Context>) -> Action {
    tracing::warn!(%error,"image reconciliation deferred");
    Action::requeue(Duration::from_secs(5))
}
pub async fn run(ctx: Arc<Context>) {
    let api = Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace);
    Controller::new(api, watcher::Config::default())
        .run(reconcile, policy, ctx)
        .for_each(|_| async {})
        .await;
}
