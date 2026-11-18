//! [INPUT]: 依赖 Image CRD、Job/Pod UID 归属链和选主租约。
//! [OUTPUT]: 协调镜像准入、逐容器失败和 Finalizer 清理。
//! [POS]: CR 删除前保留 UID 台账与活任务容量。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{BuilderPool, ImageBuild, JobOwnership, OwnedJob},
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
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

const CLEANUP_PENDING: &str = "CleanupPending: waiting for Job/Pod deletion";
const CLEANUP_FINALIZER: &str = "images.artifactory.dev/cleanup";

pub struct Context {
    pub client: Client,
    pub namespace: String,
    pub leader: Leadership,
    pub admission: tokio::sync::Mutex<()>,
}

async fn patch_finalizers(
    ctx: &Context,
    build: &ImageBuild,
    finalizers: Vec<String>,
) -> Result<()> {
    ensure!(
        build.uid().is_some() && build.resource_version().is_some(),
        "ImageBuild identity and version required"
    );
    ctx.leader.ensure().await?;
    Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace).patch(
        &build.name_any(), &PatchParams::default(),
        &Patch::Merge(serde_json::json!({"metadata":{"uid":build.uid(),"resourceVersion":build.resource_version(),"finalizers":finalizers}})),
    ).await?;
    Ok(())
}

fn record_job(build: &ImageBuild, ledger: &mut JobOwnership, job: &Job) -> Result<()> {
    ensure!(resources::owned(build, job), "Job ownership conflict");
    let uid = job
        .uid()
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Job UID required"))?;
    let name = job.name_any();
    let platform = build
        .spec
        .platforms
        .iter()
        .filter(|_| !build.spec.publish_only)
        .find(|p| resources::name(build, Some(p)).is_ok_and(|n| n == name))
        .map(String::as_str)
        .or_else(|| {
            resources::name(build, None)
                .is_ok_and(|n| n == name)
                .then_some("publish")
        })
        .ok_or_else(|| anyhow::anyhow!("unexpected owned Job name"))?;
    if let Some(previous) = ledger.jobs.iter().find(|j| j.name == name || j.uid == uid) {
        ensure!(
            previous.name == name && previous.uid == uid && previous.platform == platform,
            "Job UID ledger conflict"
        );
    } else {
        ensure!(ledger.jobs.len() < 3, "Job ownership ledger exceeds bound");
        ledger.jobs.push(OwnedJob {
            name,
            uid,
            platform: platform.into(),
        });
    }
    Ok(())
}

fn ownership(build: &ImageBuild, jobs: &[Job]) -> Result<JobOwnership> {
    let uid = build
        .uid()
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| anyhow::anyhow!("ImageBuild UID required"))?;
    let mut ledger = build
        .status
        .as_ref()
        .and_then(|s| s.job_ownership.clone())
        .unwrap_or(JobOwnership {
            image_build_uid: uid.clone(),
            jobs: vec![],
        });
    ensure!(
        ledger.image_build_uid == uid && ledger.jobs.len() <= 3,
        "ImageBuild ownership ledger conflict"
    );
    let mut names = BTreeSet::new();
    let mut uids = BTreeSet::new();
    for record in &ledger.jobs {
        let platform = if record.platform == "publish" {
            None
        } else {
            ensure!(
                !build.spec.publish_only && build.spec.platforms.contains(&record.platform),
                "unexpected ledger platform"
            );
            Some(record.platform.as_str())
        };
        ensure!(
            !record.uid.is_empty()
                && record.name == resources::name(build, platform)?
                && names.insert(record.name.clone())
                && uids.insert(record.uid.clone()),
            "invalid Job UID ledger"
        );
    }
    for job in jobs.iter().filter(|j| resources::owned(build, j)) {
        record_job(build, &mut ledger, job)?;
    }
    Ok(ledger)
}

fn pod_owned(pod: &Pod, ledger: &JobOwnership) -> bool {
    pod.owner_references().iter().any(|o| {
        o.api_version == "batch/v1"
            && o.kind == "Job"
            && o.controller == Some(true)
            && ledger
                .jobs
                .iter()
                .any(|j| j.uid == o.uid && j.name == o.name)
    })
}

fn pod_failed(pod: &Pod) -> bool {
    pod.status.as_ref().is_some_and(|status| {
        status.phase.as_deref() == Some("Failed")
            || status
                .init_container_statuses
                .iter()
                .flatten()
                .chain(status.container_statuses.iter().flatten())
                .any(|s| {
                    s.state
                        .as_ref()
                        .and_then(|s| s.terminated.as_ref())
                        .is_some_and(|t| {
                            t.exit_code != 0 || t.reason.as_deref() == Some("OOMKilled")
                        })
                })
    })
}

fn pod_stopped(pod: &Pod) -> bool {
    let (Some(spec), Some(status)) = (&pod.spec, &pod.status) else {
        return false;
    };
    if status.phase.as_deref() == Some("Unknown") {
        return false;
    }
    let stopped =
        |name: &str, statuses: &Option<Vec<k8s_openapi::api::core::v1::ContainerStatus>>| {
            statuses
                .iter()
                .flatten()
                .any(|s| s.name == name && s.state.as_ref().is_some_and(|s| s.terminated.is_some()))
        };
    !spec.containers.is_empty()
        && spec
            .containers
            .iter()
            .all(|c| stopped(&c.name, &status.container_statuses))
        && spec
            .init_containers
            .iter()
            .flatten()
            .all(|c| stopped(&c.name, &status.init_container_statuses))
}

async fn update(
    ctx: &Context,
    build: &ImageBuild,
    ledger: &JobOwnership,
    phase: &str,
    message: Option<String>,
) -> Result<Action> {
    let mut desired = build.status.clone().unwrap_or_default();
    desired.phase = phase.into();
    desired.message = message;
    desired.observed_generation = build.metadata.generation.unwrap_or(1);
    desired.job_ownership = Some(ledger.clone());
    if build.status.as_ref() != Some(&desired) {
        ctx.leader.ensure().await?;
        Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace).patch_status(&build.name_any(), &PatchParams::default(), &Patch::Merge(serde_json::json!({"metadata":{"resourceVersion":build.resource_version()},"status":desired}))).await?;
    }
    Ok(Action::requeue(Duration::from_secs(3)))
}

async fn stop(
    ctx: &Context,
    build: &ImageBuild,
    jobs: &[Job],
    ledger: &JobOwnership,
) -> Result<()> {
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    for job in jobs.iter().filter(|j| resources::owned(build, j)) {
        let uid = job
            .uid()
            .ok_or_else(|| anyhow::anyhow!("Job UID required for deletion"))?;
        ensure!(
            ledger
                .jobs
                .iter()
                .any(|r| r.uid == uid && r.name == job.name_any()),
            "unrecorded Job deletion"
        );
        if job.metadata.deletion_timestamp.is_some() {
            continue;
        }
        ctx.leader.ensure().await?;
        let params = DeleteParams {
            preconditions: Some(Preconditions {
                uid: Some(uid),
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

async fn cleanup(
    ctx: &Context,
    build: &ImageBuild,
    jobs: &[Job],
    pods: &[Pod],
    ledger: &JobOwnership,
    phase: &str,
) -> Result<Action> {
    // 终态与 UID 台账必须先落盘，删除请求随后才能发出。
    if build
        .status
        .as_ref()
        .is_none_or(|s| s.phase != phase || s.job_ownership.as_ref() != Some(ledger))
    {
        return update(ctx, build, ledger, phase, Some(CLEANUP_PENDING.into())).await;
    }
    stop(ctx, build, jobs, ledger).await?;
    let pending = jobs.iter().any(|j| resources::owned(build, j))
        || pods.iter().any(|p| pod_owned(p, ledger));
    let action = update(
        ctx,
        build,
        ledger,
        phase,
        pending.then(|| CLEANUP_PENDING.into()),
    )
    .await?;
    Ok(if pending {
        Action::requeue(Duration::from_secs(3))
    } else if build.metadata.deletion_timestamp.is_some() {
        // 清理状态先确认落盘；下一轮用当前版本移除自身 Finalizer。
        if build.status.as_ref().is_some_and(|s| s.message.is_none()) {
            patch_finalizers(
                ctx,
                build,
                build
                    .finalizers()
                    .iter()
                    .filter(|f| f.as_str() != CLEANUP_FINALIZER)
                    .cloned()
                    .collect(),
            )
            .await?;
            Action::await_change()
        } else {
            action
        }
    } else {
        Action::await_change()
    })
}

async fn capacity(ctx: &Context, build: &ImageBuild, pool: &BuilderPool) -> Result<bool> {
    let jobs = Api::<Job>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let builds = Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let mut slots = BTreeMap::<String, (String, bool)>::new();
    for owner in builds.iter().filter(|b| b.spec.pool == build.spec.pool) {
        let ledger = ownership(owner, &jobs)?;
        for record in &ledger.jobs {
            let job = jobs.iter().find(|j| {
                resources::owned(owner, j) && j.uid().as_deref() == Some(record.uid.as_str())
            });
            let children: Vec<_> = pods
                .iter()
                .filter(|p| {
                    p.owner_references().iter().any(|o| {
                        o.api_version == "batch/v1"
                            && o.kind == "Job"
                            && o.controller == Some(true)
                            && o.name == record.name
                            && o.uid == record.uid
                    })
                })
                .collect();
            // 已终止 Pod 只保留证据；活容器、未知状态和无 Pod 的活动 Job 占槽。
            let active = if children.is_empty() {
                job.is_some_and(|j| resources::phase(j).is_none())
            } else {
                children.iter().any(|p| !pod_stopped(p))
            };
            ensure!(
                slots
                    .insert(record.uid.clone(), (owner.spec.project_id.clone(), active))
                    .is_none(),
                "duplicate Job UID across ImageBuilds"
            );
        }
    }
    let active: Vec<_> = slots.values().filter(|(_, active)| *active).collect();
    Ok(active.len() < pool.spec.max_active as usize
        && active
            .iter()
            .filter(|(project, _)| *project == build.spec.project_id)
            .count()
            < pool.spec.max_per_project as usize)
}

async fn create(
    ctx: &Context,
    build: &ImageBuild,
    pool: &BuilderPool,
    platform: Option<&str>,
    selector: BTreeMap<String, String>,
) -> Result<Option<Job>> {
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let name = resources::name(build, platform)?;
    if let Some(existing) = api.get_opt(&name).await? {
        ensure!(resources::owned(build, &existing), "Job ownership conflict");
        return Ok(Some(existing));
    }
    if !capacity(ctx, build, pool).await? {
        return Ok(None);
    }
    ctx.leader.ensure().await?;
    match api
        .create(
            &PostParams::default(),
            &resources::job(build, pool, platform, selector)?,
        )
        .await
    {
        Ok(job) => Ok(Some(job)),
        Err(kube::Error::Api(e)) if e.code == 409 => {
            let job = api.get(&name).await?;
            ensure!(resources::owned(build, &job), "Job ownership conflict");
            Ok(Some(job))
        }
        Err(e) => Err(e.into()),
    }
}

async fn reconcile_inner(build: Arc<ImageBuild>, ctx: Arc<Context>) -> Result<Action> {
    let _guard = ctx.admission.lock().await;
    ctx.leader.ensure().await?;
    if !build.finalizers().iter().any(|f| f == CLEANUP_FINALIZER) {
        if build.metadata.deletion_timestamp.is_some() {
            return Ok(Action::await_change());
        }
        let mut finalizers = build.finalizers().to_vec();
        finalizers.push(CLEANUP_FINALIZER.into());
        patch_finalizers(&ctx, &build, finalizers).await?;
        return Ok(Action::requeue(Duration::from_secs(3)));
    }
    let jobs = Api::<Job>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let mut ledger = ownership(&build, &jobs)?;
    if build.spec.cancel_requested || build.metadata.deletion_timestamp.is_some() {
        return cleanup(&ctx, &build, &jobs, &pods, &ledger, "Canceled").await;
    }
    if let Some(status) = build
        .status
        .as_ref()
        .filter(|s| ["Succeeded", "Failed", "Canceled"].contains(&s.phase.as_str()))
    {
        return cleanup(&ctx, &build, &jobs, &pods, &ledger, &status.phase).await;
    }
    if jobs
        .iter()
        .any(|j| resources::owned(&build, j) && resources::phase(j) == Some("Failed"))
        || pods.iter().any(|p| pod_owned(p, &ledger) && pod_failed(p))
        || ledger.jobs.iter().any(|r| {
            !jobs
                .iter()
                .any(|j| resources::owned(&build, j) && j.uid().as_deref() == Some(r.uid.as_str()))
        })
    {
        return cleanup(&ctx, &build, &jobs, &pods, &ledger, "Failed").await;
    }
    let pool = Api::<BuilderPool>::namespaced(ctx.client.clone(), &ctx.namespace)
        .get(&build.spec.pool)
        .await?;
    let nodes = Api::<Node>::all(ctx.client.clone())
        .list(&ListParams::default())
        .await?
        .items;
    let mut complete = true;
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
            if let Some(selector) = resources::placement(&build, &pool, platform, &nodes)?
                && let Some(job) = create(&ctx, &build, &pool, Some(platform), selector).await?
            {
                record_job(&build, &mut ledger, &job)?;
                return update(&ctx, &build, &ledger, "Building", None).await;
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
                return cleanup(&ctx, &build, &jobs, &pods, &ledger, "Succeeded").await;
            }
        } else if let Some(job) =
            create(&ctx, &build, &pool, None, pool.spec.node_selector.clone()).await?
        {
            record_job(&build, &mut ledger, &job)?;
        }
        update(&ctx, &build, &ledger, "Publishing", None).await
    } else {
        update(&ctx, &build, &ledger, "Building", None).await
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
pub async fn reconcile(build: Arc<ImageBuild>, ctx: Arc<Context>) -> Result<Action, Error> {
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
