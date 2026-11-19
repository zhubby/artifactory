//! [INPUT]: 依赖 CR 版本、直接归属、Pod UID、停机证据与选主租约。
//! [OUTPUT]: 提供持久创建意图、执行记账、归属围栏与安全删除。
//! [POS]: 规划和源码共用的创建围栏与终态保护释放边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{PodExecution, Status},
    controller::Context,
};
use anyhow::{Result, ensure};
use chrono::Utc;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api, Resource, ResourceExt,
    api::{DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions},
};
use serde::de::DeserializeOwned;
use serde_json::json;

pub const CR_FINALIZER: &str = "planning.artifactory.dev/execution-cleanup";
pub const POD_FINALIZER: &str = "planning.artifactory.dev/pod-stop";
pub const CLEANUP_PENDING: &str = "CleanupPending: waiting for confirmed Pod stop and deletion";

pub fn reserve(
    id: &str,
    owner_uid: &str,
    prefix: &str,
    attempt: u32,
    deadline_ms: i64,
) -> Result<PodExecution> {
    let id = uuid::Uuid::parse_str(id)?;
    let hash = artifactory_service_transport::crypto::hash(owner_uid);
    Ok(PodExecution {
        name: format!("{prefix}-{}-a{attempt}-{}", id.simple(), &hash[..8]),
        uid: None,
        node_name: None,
        create_started: false,
        attempt,
        deadline_ms,
        stopped: false,
    })
}

pub fn remaining_seconds(execution: &PodExecution) -> i64 {
    ((execution.deadline_ms - Utc::now().timestamp_millis()).max(1) + 999) / 1000
}

pub fn owned<K: Resource<DynamicType = ()>>(owner: &K, pod: &Pod) -> bool {
    let Some(uid) = owner.uid() else { return false };
    pod.owner_references().iter().any(|reference| {
        reference.uid == uid
            && reference.name == owner.name_any()
            && reference.kind == K::kind(&())
            && reference.api_version == K::api_version(&())
            && reference.controller == Some(true)
    })
}

/// 释放 CR 保护前查询实际归属，不依赖可被改写的 Pod 标签。
pub async fn has_owned<K: Resource<DynamicType = ()>>(ctx: &Context, owner: &K) -> Result<bool> {
    let api = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace);
    Ok(api
        .list(&ListParams::default())
        .await?
        .iter()
        .any(|pod| owned(owner, pod)))
}

/// CR 存在不等于执行仍记账；迟到创建和同名替换都必须额外计费。
pub fn accounted<K: Resource<DynamicType = ()>>(
    owner: &K,
    execution: Option<&PodExecution>,
    pod: &Pod,
) -> bool {
    execution.is_some_and(|record| {
        owned(owner, pod)
            && record.name == pod.name_any()
            && pod.labels().get("artifactory.dev/attempt") == Some(&record.attempt.to_string())
            && record
                .uid
                .as_ref()
                .is_none_or(|uid| pod.metadata.uid.as_ref() == Some(uid))
    })
}

pub fn verify<K: Resource<DynamicType = ()>>(
    owner: &K,
    execution: &PodExecution,
    pod: &Pod,
) -> Result<()> {
    ensure!(
        owned(owner, pod) && pod.name_any() == execution.name,
        "Pod owner conflict"
    );
    ensure!(
        pod.labels().get("artifactory.dev/attempt") == Some(&execution.attempt.to_string()),
        "Pod attempt conflict"
    );
    ensure!(
        execution
            .uid
            .as_ref()
            .is_none_or(|uid| pod.metadata.uid.as_ref() == Some(uid)),
        "Pod UID changed"
    );
    ensure!(
        pod.finalizers().iter().any(|value| value == POD_FINALIZER) || execution.stopped,
        "Pod stop protection missing"
    );
    Ok(())
}

pub fn bind(execution: &PodExecution, pod: &Pod) -> Result<PodExecution> {
    let mut bound = execution.clone();
    bound.uid = Some(
        pod.uid()
            .ok_or_else(|| anyhow::anyhow!("Pod UID required"))?,
    );
    if let Some(node) = pod.spec.as_ref().and_then(|spec| spec.node_name.as_ref()) {
        ensure!(
            bound.node_name.as_ref().is_none_or(|name| name == node),
            "Pod node changed"
        );
        bound.node_name = Some(node.clone());
    }
    Ok(bound)
}

/// 删除中的未调度 Pod 与已报告停止的全部进程能够释放保护。
pub fn stopped(pod: &Pod) -> bool {
    let Some(spec) = &pod.spec else { return false };
    if pod.metadata.deletion_timestamp.is_some() && spec.node_name.is_none() {
        return true;
    }
    let Some(status) = &pod.status else {
        return false;
    };
    if status.phase.as_deref() == Some("Unknown") {
        return false;
    }
    let init_failed = status
        .init_container_statuses
        .iter()
        .flatten()
        .any(|container| {
            container
                .state
                .as_ref()
                .and_then(|state| state.terminated.as_ref())
                .is_some_and(|state| state.exit_code != 0)
        });
    let ended = |name: &str,
                 statuses: &Option<Vec<k8s_openapi::api::core::v1::ContainerStatus>>,
                 blocked: bool| {
        statuses.iter().flatten().any(|container| {
            container.name == name
                && container.state.as_ref().is_some_and(|state| {
                    state.terminated.is_some()
                        || (blocked
                            && container.container_id.is_none()
                            && state.waiting.as_ref().is_some_and(|waiting| {
                                waiting.reason.as_deref() == Some("PodInitializing")
                            }))
                })
        })
    };
    !spec.containers.is_empty()
        && spec
            .containers
            .iter()
            .all(|container| ended(&container.name, &status.container_statuses, init_failed))
        && spec
            .init_containers
            .iter()
            .flatten()
            .all(|container| ended(&container.name, &status.init_container_statuses, false))
}

pub async fn finalizer<K>(
    ctx: &Context,
    api: &Api<K>,
    resource: &K,
    value: &str,
    present: bool,
) -> Result<bool>
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + std::fmt::Debug,
{
    if resource.finalizers().iter().any(|entry| entry == value) == present {
        return Ok(false);
    }
    let mut values = resource.finalizers().to_vec();
    values.retain(|entry| entry != value);
    if present {
        values.push(value.into());
    }
    ctx.leader.ensure().await?;
    api.patch(&resource.name_any(), &PatchParams::default(), &Patch::Merge(json!({"metadata":{"uid":resource.uid(),"resourceVersion":resource.resource_version(),"finalizers":values}}))).await?;
    Ok(true)
}

pub async fn status<K>(
    ctx: &Context,
    api: &Api<K>,
    resource: &K,
    current: Option<&Status>,
    desired: Status,
) -> Result<bool>
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + std::fmt::Debug,
{
    if current == Some(&desired) {
        return Ok(false);
    }
    ctx.leader.ensure().await?;
    api.patch_status(&resource.name_any(), &PatchParams::default(), &Patch::Merge(json!({"metadata":{"uid":resource.uid(),"resourceVersion":resource.resource_version()},"status":desired}))).await?;
    Ok(true)
}

pub async fn create<K>(
    ctx: &Context,
    owner: &K,
    execution: &PodExecution,
    desired: &Pod,
) -> Result<Pod>
where
    K: Resource<DynamicType = (), Scope = kube::core::NamespaceResourceScope>
        + Clone
        + DeserializeOwned
        + std::fmt::Debug,
{
    ensure!(execution.uid.is_none(), "bound Pod cannot be recreated");
    ensure!(
        execution.create_started,
        "Pod creation intent must be persisted"
    );
    let api: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    ctx.leader.ensure().await?;
    // 等待准入锁的旧快照不能在终态清理后重建 Pod。
    let latest = Api::<K>::namespaced(ctx.client.clone(), &ctx.namespace)
        .get(&owner.name_any())
        .await?;
    ensure!(
        latest.uid() == owner.uid()
            && latest.resource_version() == owner.resource_version()
            && latest.meta().deletion_timestamp.is_none()
            && latest
                .finalizers()
                .iter()
                .any(|value| value == CR_FINALIZER),
        "Pod creation owner version fenced"
    );
    ctx.leader.ensure().await?;
    let pod = match api.create(&PostParams::default(), desired).await {
        Ok(pod) => pod,
        Err(error) => match api.get_opt(&execution.name).await? {
            Some(pod) => pod,
            None => return Err(error.into()),
        },
    };
    verify(owner, execution, &pod)?;
    Ok(pod)
}

/// UID 前置条件保护同名替换；停机台账已落盘才移除 Pod finalizer。
pub async fn delete(ctx: &Context, execution: &PodExecution, pod: &Pod) -> Result<()> {
    let api: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    ensure!(
        execution.uid.as_ref() == pod.metadata.uid.as_ref() && execution.uid.is_some(),
        "Pod deletion UID mismatch"
    );
    if pod.metadata.deletion_timestamp.is_none() {
        ctx.leader.ensure().await?;
        match api
            .delete(
                &execution.name,
                &DeleteParams {
                    preconditions: Some(Preconditions {
                        uid: execution.uid.clone(),
                        resource_version: None,
                    }),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => (),
            Err(kube::Error::Api(error)) if error.code == 404 => (),
            Err(error) => return Err(error.into()),
        }
    } else if execution.stopped {
        finalizer(ctx, &api, pod, POD_FINALIZER, false).await?;
    }
    Ok(())
}
