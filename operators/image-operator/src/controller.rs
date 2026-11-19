//! [INPUT]: 依赖 Image CRD、Pod UID 归属链和选主租约。
//! [OUTPUT]: 先持久化 Pod 创建意图，确认停机收尾后释放任务 Finalizer。
//! [POS]: ImageBuild 是工作 Pod 的唯一 owner；同一执行不重建丢失 Pod。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{BuilderPool, ImageBuild, OwnedPod, PodOwnership},
    leadership::Leadership,
    resources,
};
use anyhow::{Result, ensure};
use futures::StreamExt;
use k8s_openapi::api::core::v1::{Node, Pod};
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

const CLEANUP_PENDING: &str = "CleanupPending: waiting for Pod deletion";
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

fn platform(build: &ImageBuild, name: &str) -> Result<String> {
    build
        .spec
        .platforms
        .iter()
        .filter(|_| !build.spec.publish_only)
        .find(|p| resources::name(build, Some(p)).is_ok_and(|expected| expected == name))
        .cloned()
        .or_else(|| {
            resources::name(build, None)
                .is_ok_and(|expected| expected == name)
                .then(|| "publish".into())
        })
        .ok_or_else(|| anyhow::anyhow!("unexpected owned Pod name"))
}
fn record_pod(build: &ImageBuild, ledger: &mut PodOwnership, pod: &Pod) -> Result<()> {
    ensure!(resources::owned(build, pod), "Pod ownership conflict");
    let uid = pod
        .uid()
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Pod UID required"))?;
    let name = pod.name_any();
    let platform = platform(build, &name)?;
    if let Some(previous) = ledger
        .pods
        .iter_mut()
        .find(|p| p.name == name || p.uid == uid)
    {
        ensure!(
            previous.name == name && previous.uid == uid && previous.platform == platform,
            "Pod UID ledger conflict"
        );
        previous.stopped |= pod_stopped(pod);
    } else {
        ensure!(ledger.pods.len() < 3, "Pod ownership ledger exceeds bound");
        ledger.pods.push(OwnedPod {
            name,
            uid,
            platform,
            stopped: pod_stopped(pod),
        });
    }
    if ledger.creating.as_deref() == Some(pod.name_any().as_str()) {
        ledger.creating = None;
    }
    Ok(())
}
fn ownership(build: &ImageBuild, pods: &[Pod]) -> Result<PodOwnership> {
    let uid = build
        .uid()
        .filter(|uid| !uid.is_empty())
        .ok_or_else(|| anyhow::anyhow!("ImageBuild UID required"))?;
    let mut ledger = build
        .status
        .as_ref()
        .and_then(|s| s.pod_ownership.clone())
        .unwrap_or(PodOwnership {
            image_build_uid: uid.clone(),
            creating: None,
            pods: vec![],
        });
    ensure!(
        ledger.image_build_uid == uid && ledger.pods.len() <= 3,
        "ImageBuild ownership ledger conflict"
    );
    if let Some(name) = &ledger.creating {
        platform(build, name)?;
        ensure!(
            !ledger.pods.iter().any(|record| record.name == *name),
            "bound Pod cannot retain a creation intent"
        );
    }
    let mut names = BTreeSet::new();
    let mut uids = BTreeSet::new();
    for record in &ledger.pods {
        ensure!(
            !record.uid.is_empty()
                && platform(build, &record.name)? == record.platform
                && names.insert(record.name.clone())
                && uids.insert(record.uid.clone()),
            "invalid Pod UID ledger"
        );
    }
    // create 成功而 status 写入失败时，只接管同名且直接归属当前 CR 的 Pod。
    // 已记录的名字绝不接管其他 UID，由 failure_reason 转为业务失败。
    for pod in pods.iter().filter(|pod| resources::owned(build, pod)) {
        if !ledger.pods.iter().any(|record| {
            record.name == pod.name_any() && pod.uid().as_deref() != Some(&record.uid)
        }) {
            record_pod(build, &mut ledger, pod)?;
        }
    }
    Ok(ledger)
}
fn recorded(pod: &Pod, ledger: &PodOwnership) -> bool {
    ledger
        .pods
        .iter()
        .any(|record| record.name == pod.name_any() && pod.uid().as_deref() == Some(&record.uid))
}
fn pod_failed(pod: &Pod) -> bool {
    pod.status.as_ref().is_some_and(|status| {
        status.phase.as_deref() == Some("Failed")
            || status
                .init_container_statuses
                .iter()
                .flatten()
                .chain(status.container_statuses.iter().flatten())
                .any(|container| {
                    container
                        .state
                        .as_ref()
                        .and_then(|state| state.terminated.as_ref())
                        .is_some_and(|ended| {
                            ended.exit_code != 0 || ended.reason.as_deref() == Some("OOMKilled")
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
    // 未分配节点的删除中 Pod 没有运行位置；调度器不再绑定待删除 Pod。
    if pod.metadata.deletion_timestamp.is_some()
        && spec.node_name.is_none()
        && status.container_statuses.iter().flatten().all(|container| {
            container.container_id.is_none()
                && container
                    .state
                    .as_ref()
                    .is_none_or(|state| state.running.is_none())
        })
        && status
            .init_container_statuses
            .iter()
            .flatten()
            .all(|container| {
                container.container_id.is_none()
                    && container
                        .state
                        .as_ref()
                        .is_none_or(|state| state.running.is_none())
            })
    {
        return true;
    }
    // 普通 init 失败后，未创建过的主容器不会启动；不能要求不存在的退出状态。
    let init_failed = status
        .init_container_statuses
        .iter()
        .flatten()
        .any(|container| {
            container
                .state
                .as_ref()
                .and_then(|state| state.terminated.as_ref())
                .is_some_and(|ended| ended.exit_code != 0)
        });
    let main_never_started = spec.containers.iter().all(|container| {
        status
            .container_statuses
            .iter()
            .flatten()
            .find(|state| state.name == container.name)
            .is_none_or(|state| {
                state.container_id.is_none()
                    && state
                        .state
                        .as_ref()
                        .is_none_or(|state| state.waiting.is_some())
            })
    });
    let stopped =
        |name: &str, statuses: &Option<Vec<k8s_openapi::api::core::v1::ContainerStatus>>| {
            statuses.iter().flatten().any(|container| {
                container.name == name
                    && container
                        .state
                        .as_ref()
                        .is_some_and(|state| state.terminated.is_some())
            })
        };
    !spec.containers.is_empty()
        && (init_failed && main_never_started
            || spec
                .containers
                .iter()
                .all(|container| stopped(&container.name, &status.container_statuses)))
        && spec
            .init_containers
            .iter()
            .flatten()
            .all(|container| stopped(&container.name, &status.init_container_statuses))
}
fn succeeded(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .is_some_and(|s| s.phase.as_deref() == Some("Succeeded"))
        && pod_stopped(pod)
        && !pod_failed(pod)
}
fn failure_reason(build: &ImageBuild, pods: &[Pod], ledger: &PodOwnership) -> Option<String> {
    let reason = |value: Option<&str>| {
        [
            "Evicted",
            "OOMKilled",
            "DeadlineExceeded",
            "NodeLost",
            "Shutdown",
            "Error",
            "ContainerCannotRun",
            "StartError",
        ]
        .into_iter()
        .find(|candidate| Some(*candidate) == value)
        .unwrap_or("Failed")
    };
    for record in &ledger.pods {
        let Some(pod) = pods.iter().find(|pod| pod.name_any() == record.name) else {
            return Some(format!(
                "Image failure: platform={} pod=Missing",
                record.platform
            ));
        };
        if pod.uid().as_deref() != Some(&record.uid) || !resources::owned(build, pod) {
            return Some(format!(
                "Image failure: platform={} pod=IdentityChanged",
                record.platform
            ));
        }
        if pod_failed(pod) {
            let Some(status) = &pod.status else { continue };
            if matches!(
                status.reason.as_deref(),
                Some("Evicted" | "DeadlineExceeded" | "NodeLost" | "Shutdown")
            ) {
                return Some(format!(
                    "Image failure: platform={} pod={}",
                    record.platform,
                    reason(status.reason.as_deref())
                ));
            }
            for container in status
                .init_container_statuses
                .iter()
                .flatten()
                .chain(status.container_statuses.iter().flatten())
            {
                if let Some(ended) = container
                    .state
                    .as_ref()
                    .and_then(|state| state.terminated.as_ref())
                    && (ended.exit_code != 0 || ended.reason.as_deref() == Some("OOMKilled"))
                {
                    let name = match container.name.as_str() {
                        name @ ("source" | "worker" | "buildkit") => name,
                        _ => "unknown",
                    };
                    return Some(format!(
                        "Image failure: platform={} container={name} reason={} exit={}",
                        record.platform,
                        reason(ended.reason.as_deref()),
                        ended.exit_code
                    ));
                }
            }
            return Some(format!(
                "Image failure: platform={} pod={}",
                record.platform,
                reason(status.reason.as_deref())
            ));
        }
        if pod.metadata.deletion_timestamp.is_some() && !succeeded(pod) {
            return Some(format!(
                "Image failure: platform={} pod=Deleting",
                record.platform
            ));
        }
    }
    None
}
fn cleanup_message(failure: Option<&str>, pending: bool) -> Option<String> {
    match (pending, failure) {
        (true, Some(failure)) => Some(format!("{CLEANUP_PENDING}\n{failure}")),
        (true, None) => Some(CLEANUP_PENDING.into()),
        (false, Some(failure)) => Some(failure.into()),
        (false, None) => None,
    }
}
async fn update(
    ctx: &Context,
    build: &ImageBuild,
    ledger: &PodOwnership,
    phase: &str,
    message: Option<String>,
) -> Result<Action> {
    let mut desired = build.status.clone().unwrap_or_default();
    desired.phase = phase.into();
    desired.message = message;
    desired.observed_generation = build.metadata.generation.unwrap_or(1);
    desired.pod_ownership = Some(ledger.clone());
    if build.status.as_ref() != Some(&desired) {
        ctx.leader.ensure().await?;
        Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace).patch_status(&build.name_any(), &PatchParams::default(), &Patch::Merge(serde_json::json!({"metadata":{"uid":build.uid(),"resourceVersion":build.resource_version()},"status":desired}))).await?;
    }
    Ok(Action::requeue(Duration::from_secs(3)))
}
async fn stop(
    ctx: &Context,
    build: &ImageBuild,
    pods: &[Pod],
    ledger: &PodOwnership,
) -> Result<()> {
    let api: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    for pod in pods
        .iter()
        .filter(|pod| resources::owned(build, pod) && recorded(pod, ledger))
    {
        if pod_stopped(pod)
            && ledger
                .pods
                .iter()
                .any(|record| record.uid == pod.uid().unwrap_or_default() && record.stopped)
            && pod
                .finalizers()
                .iter()
                .any(|finalizer| finalizer == resources::POD_FINALIZER)
        {
            ctx.leader.ensure().await?;
            let finalizers: Vec<_> = pod
                .finalizers()
                .iter()
                .filter(|finalizer| finalizer.as_str() != resources::POD_FINALIZER)
                .cloned()
                .collect();
            api.patch(&pod.name_any(), &PatchParams::default(), &Patch::Merge(serde_json::json!({"metadata":{"uid":pod.uid(),"resourceVersion":pod.resource_version(),"finalizers":finalizers}}))).await?;
        }
        if pod.metadata.deletion_timestamp.is_some() {
            continue;
        }
        ctx.leader.ensure().await?;
        let params = DeleteParams {
            preconditions: Some(Preconditions {
                uid: pod.uid(),
                resource_version: None,
            }),
            ..Default::default()
        };
        match api.delete(&pod.name_any(), &params).await {
            Ok(_) => (),
            Err(kube::Error::Api(error)) if error.code == 404 => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
async fn cleanup(
    ctx: &Context,
    build: &ImageBuild,
    pods: &[Pod],
    ledger: &PodOwnership,
    phase: &str,
    failure: Option<String>,
) -> Result<Action> {
    let saved = build
        .status
        .as_ref()
        .and_then(|s| s.message.as_deref())
        .and_then(|message| {
            message
                .lines()
                .find(|line| line.starts_with("Image failure: "))
        });
    let failure = saved.map(str::to_owned).or(failure);
    // 先保存终态与 UID，随后只删除台账中的 Pod。
    if build
        .status
        .as_ref()
        .is_none_or(|s| s.phase != phase || s.pod_ownership.as_ref() != Some(ledger))
    {
        return update(
            ctx,
            build,
            ledger,
            phase,
            cleanup_message(failure.as_deref(), true),
        )
        .await;
    }
    stop(ctx, build, pods, ledger).await?;
    // DELETE 被接受不是收尾证明；Unknown 和仍存在的删除中 Pod 保留 Finalizer。
    let pending = ledger.creating.is_some()
        || ledger.pods.iter().any(|record| !record.stopped)
        || pods
            .iter()
            .any(|pod| resources::owned(build, pod) || recorded(pod, ledger));
    let message = cleanup_message(failure.as_deref(), pending);
    let action = update(ctx, build, ledger, phase, message.clone()).await?;
    if pending {
        return Ok(action);
    }
    if build.finalizers().iter().any(|f| f == CLEANUP_FINALIZER)
        && build.status.as_ref().is_some_and(|s| s.message == message)
    {
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
        return Ok(Action::await_change());
    }
    Ok(if build.metadata.deletion_timestamp.is_some() {
        action
    } else {
        Action::await_change()
    })
}
async fn capacity(ctx: &Context, build: &ImageBuild, pool: &BuilderPool) -> Result<bool> {
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let builds = Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let mut slots = BTreeMap::<String, String>::new();
    for owner in builds
        .iter()
        .filter(|owner| owner.spec.pool == build.spec.pool)
    {
        // UID 台账即使不完整，也按直接 owner 统计已创建 Pod。
        let ledger = ownership(owner, &pods)?;
        if let Some(name) = &ledger.creating {
            slots.insert(
                format!("creating:{}:{name}", ledger.image_build_uid),
                owner.spec.project_id.clone(),
            );
        }
        for record in ledger.pods.iter().filter(|record| {
            !record.stopped
                && !pods.iter().any(|pod| {
                    resources::owned(owner, pod) && pod.uid().as_deref() == Some(&record.uid)
                })
        }) {
            ensure!(
                slots
                    .insert(record.uid.clone(), owner.spec.project_id.clone())
                    .is_none(),
                "duplicate missing Pod UID across ImageBuilds"
            );
        }
        for pod in pods
            .iter()
            .filter(|pod| resources::owned(owner, pod) && !pod_stopped(pod))
        {
            let uid = pod
                .uid()
                .ok_or_else(|| anyhow::anyhow!("Pod UID required for capacity"))?;
            ensure!(
                slots.insert(uid, owner.spec.project_id.clone()).is_none(),
                "duplicate Pod UID across ImageBuilds"
            );
        }
    }
    // CR 被人工绕过 Finalizer 删除时，仍存在的受保护 Pod 继续占池容量。
    for pod in pods.iter().filter(|pod| {
        !pod_stopped(pod)
            && pod.labels().get("artifactory.dev/builder-pool") == Some(&build.spec.pool)
            && pod
                .finalizers()
                .iter()
                .any(|value| value == resources::POD_FINALIZER)
            && pod.owner_references().iter().any(|owner| {
                owner.kind == "ImageBuild"
                    && owner.api_version == "images.artifactory.dev/v1alpha1"
                    && owner.controller == Some(true)
            })
    }) {
        let uid = pod
            .uid()
            .ok_or_else(|| anyhow::anyhow!("orphan Pod UID required for capacity"))?;
        slots.entry(uid).or_insert_with(|| {
            pod.labels()
                .get("artifactory.dev/project")
                .cloned()
                .unwrap_or_default()
        });
    }
    Ok(slots.len() < pool.spec.max_active as usize
        && slots
            .values()
            .filter(|project| **project == build.spec.project_id)
            .count()
            < pool.spec.max_per_project as usize)
}
async fn create(
    ctx: &Context,
    build: &ImageBuild,
    pool: &BuilderPool,
    platform: Option<&str>,
    selector: BTreeMap<String, String>,
    ledger: &mut PodOwnership,
) -> Result<Option<Pod>> {
    let api: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let name = resources::name(build, platform)?;
    if let Some(existing) = api.get_opt(&name).await? {
        ensure!(resources::owned(build, &existing), "Pod ownership conflict");
        return Ok(Some(existing));
    }
    match ledger.creating.as_deref() {
        Some(pending) if pending != name => return Ok(None),
        Some(_) => (),
        None => {
            if capacity(ctx, build, pool).await? {
                ledger.creating = Some(name);
            }
            return Ok(None);
        }
    }
    ctx.leader.ensure().await?;
    // 旧 Arc 的创建意图不能越过取消、终态或其他协调写入。
    let current = Api::<ImageBuild>::namespaced(ctx.client.clone(), &ctx.namespace)
        .get(&build.name_any())
        .await?;
    ensure!(
        current.uid() == build.uid()
            && current.resource_version() == build.resource_version()
            && current.metadata.deletion_timestamp.is_none()
            && !current.spec.cancel_requested
            && current.status.as_ref().is_some_and(|status| {
                !["Succeeded", "Failed", "Canceled"].contains(&status.phase.as_str())
                    && status
                        .pod_ownership
                        .as_ref()
                        .is_some_and(|ownership| ownership.creating.as_deref() == Some(&name))
            }),
        "Pod creation intent changed"
    );
    match api
        .create(
            &PostParams::default(),
            &resources::pod(build, pool, platform, selector)?,
        )
        .await
    {
        Ok(pod) => Ok(Some(pod)),
        Err(kube::Error::Api(error)) if error.code == 409 => {
            let pod = api.get(&name).await?;
            ensure!(resources::owned(build, &pod), "Pod ownership conflict");
            Ok(Some(pod))
        }
        Err(error) => Err(error.into()),
    }
}
async fn reconcile_inner(build: Arc<ImageBuild>, ctx: Arc<Context>) -> Result<Action> {
    let _guard = ctx.admission.lock().await;
    ctx.leader.ensure().await?;
    let terminal = build
        .status
        .as_ref()
        .is_some_and(|s| ["Succeeded", "Failed", "Canceled"].contains(&s.phase.as_str()));
    if !terminal && !build.finalizers().iter().any(|f| f == CLEANUP_FINALIZER) {
        if build.metadata.deletion_timestamp.is_some() {
            return Ok(Action::await_change());
        }
        let mut finalizers = build.finalizers().to_vec();
        finalizers.push(CLEANUP_FINALIZER.into());
        patch_finalizers(&ctx, &build, finalizers).await?;
        return Ok(Action::requeue(Duration::from_secs(3)));
    }
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace)
        .list(&ListParams::default())
        .await?
        .items;
    let mut ledger = ownership(&build, &pods)?;
    if let Some(status) = build
        .status
        .as_ref()
        .filter(|s| ["Succeeded", "Failed", "Canceled"].contains(&s.phase.as_str()))
    {
        return cleanup(&ctx, &build, &pods, &ledger, &status.phase, None).await;
    }
    if build.spec.cancel_requested || build.metadata.deletion_timestamp.is_some() {
        return cleanup(&ctx, &build, &pods, &ledger, "Canceled", None).await;
    }
    if let Some(reason) = failure_reason(&build, &pods, &ledger) {
        return cleanup(&ctx, &build, &pods, &ledger, "Failed", Some(reason)).await;
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
        if let Some(pod) = pods.iter().find(|pod| pod.name_any() == name) {
            ensure!(resources::owned(&build, pod), "Pod ownership conflict");
            complete &= succeeded(pod);
        } else {
            complete = false;
            if let Some(selector) = resources::placement(&build, &pool, platform, &nodes)?
                && let Some(pod) =
                    create(&ctx, &build, &pool, Some(platform), selector, &mut ledger).await?
            {
                record_pod(&build, &mut ledger, &pod)?;
                return update(&ctx, &build, &ledger, "Building", None).await;
            }
        }
    }
    if complete {
        let name = resources::name(&build, None)?;
        if let Some(pod) = pods.iter().find(|pod| pod.name_any() == name) {
            ensure!(
                resources::owned(&build, pod),
                "publisher ownership conflict"
            );
            if succeeded(pod) {
                return cleanup(&ctx, &build, &pods, &ledger, "Succeeded", None).await;
            }
        } else if let Some(pod) = create(
            &ctx,
            &build,
            &pool,
            None,
            pool.spec.node_selector.clone(),
            &mut ledger,
        )
        .await?
        {
            record_pod(&build, &mut ledger, &pod)?;
        }
        update(&ctx, &build, &ledger, "Publishing", None).await
    } else {
        update(&ctx, &build, &ledger, "Building", None).await
    }
}
#[derive(Debug)]
pub struct Error(anyhow::Error);
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
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
    let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    Controller::new(api, watcher::Config::default())
        .owns(pods, watcher::Config::default())
        .run(reconcile, policy, ctx)
        .for_each(|_| async {})
        .await;
}
