//! [INPUT]: 依赖 SourceTask、AgentPool 与 Kubernetes 任务身份。
//! [OUTPUT]: 创建临时源码 Job，按 UID 回收 Job 和 Pod。
//! [POS]: 源码解析只占用任务临时卷，不创建常驻服务或 PVC。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    api::{AgentPool, SourcePurpose, SourceTask, Status},
    controller::Context,
};
use anyhow::{Result, ensure};
use futures::StreamExt;
use k8s_openapi::api::{batch::v1::Job, core::v1::Pod};
use kube::{
    Api, Resource, ResourceExt,
    api::{
        DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions, PropagationPolicy,
    },
    runtime::{Controller, controller::Action, watcher},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};

pub fn job(task: &SourceTask, pool: &AgentPool) -> Result<Job> {
    let id = uuid::Uuid::parse_str(&task.spec.task_id)?;
    let name = format!("source-{}", id.simple());
    let owner = task
        .controller_owner_ref(&())
        .ok_or_else(|| anyhow::anyhow!("SourceTask UID required"))?;
    let mode = match task.spec.purpose {
        SourcePurpose::Resolve => "resolve",
        SourcePurpose::Refs => "refs",
    };
    let labels = json!({"app.kubernetes.io/name":"artifactory-source-task", "artifactory.dev/source-task":task.name_any(), "artifactory.dev/source-task-pool":task.spec.pool, "artifactory.dev/project":task.spec.project_id});
    let mut value = json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"name":name,"namespace":task.namespace(),"ownerReferences":[owner],"labels":labels},"spec":{"backoffLimit":2,"activeDeadlineSeconds":task.spec.timeout_seconds,"template":{"metadata":{"labels":labels},"spec":{"serviceAccountName":"artifactory-source-preparer","automountServiceAccountToken":false,"restartPolicy":"Never","securityContext":{"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001,"runAsNonRoot":true,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"source","image":pool.spec.source_preparer_image,"imagePullPolicy":pool.spec.image_pull_policy,"command":["artifactory-source-preparer",mode],"env":[
        {"name":"ARTIFACTORY_SOURCE_PREPARE_URL","value":format!("{}/internal/source/prepare",pool.spec.control_url.trim_end_matches('/'))},
        {"name":"ARTIFACTORY_SOURCE_IDENTITY","value":json!({"kind":"source-task","task_id":task.spec.task_id}).to_string()},
        {"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"},
        {"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"},
        {"name":"ARTIFACTORY_SOURCE_AUTH_ROOT","value":"/run/source-auth"},
        {"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/source-identity/token"},
        {"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},
        {"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},
        {"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}}
    ],"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":"2","memory":"2Gi","ephemeral-storage":pool.spec.ephemeral_storage}},"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"volumeMounts":[{"name":"workspace","mountPath":"/workspace"},{"name":"auth","mountPath":"/run/source-auth"},{"name":"identity","mountPath":"/var/run/source-identity","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]}],"volumes":[{"name":"workspace","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}},{"name":"auth","emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}},{"name":"tmp","emptyDir":{"medium":"Memory","sizeLimit":"64Mi"}},{"name":"identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-source-prepare","expirationSeconds":600,"path":"token"}}]}}]}}}});
    if !pool.spec.tls_secret_name.is_empty() {
        let pod = &mut value["spec"]["template"]["spec"];
        pod["containers"][0]["env"]
            .as_array_mut()
            .expect("environment array")
            .push(json!({"name":"ARTIFACTORY_SERVICE_CA","value":"/run/service-ca/ca.crt"}));
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .expect("mount array")
            .push(json!({"name":"ca","mountPath":"/run/service-ca","readOnly":true}));
        pod["volumes"].as_array_mut().expect("volume array").push(json!({"name":"ca","secret":{"secretName":pool.spec.tls_secret_name,"items":[{"key":"ca.crt","path":"ca.crt"}]}}));
    }
    Ok(serde_json::from_value(value)?)
}

async fn status(
    ctx: &Context,
    task: &SourceTask,
    phase: &str,
    message: Option<String>,
    pod_uid: Option<String>,
) -> Result<()> {
    let desired = Status {
        phase: phase.into(),
        message,
        pod_uid,
        observed_generation: task.metadata.generation.unwrap_or(1),
    };
    if task.status.as_ref() != Some(&desired) {
        ctx.leader.ensure().await?;
        Api::<SourceTask>::namespaced(ctx.client.clone(), &ctx.namespace).patch_status(&task.name_any(), &PatchParams::default(), &Patch::Merge(json!({"metadata":{"resourceVersion":task.resource_version()},"status":desired}))).await?;
    }
    Ok(())
}

fn owned(task: &SourceTask, job: &Job) -> bool {
    job.owner_references()
        .iter()
        .any(|owner| Some(&owner.uid) == task.metadata.uid.as_ref() && owner.kind == "SourceTask")
}

async fn reconcile_task(task: Arc<SourceTask>, ctx: Arc<Context>) -> Result<Action> {
    let _guard = ctx.source_admission.lock().await;
    ctx.leader.ensure().await?;
    let jobs: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let name = format!(
        "source-{}",
        uuid::Uuid::parse_str(&task.spec.task_id)?.simple()
    );
    let current = jobs.get_opt(&name).await?;
    if let Some(job) = &current {
        ensure!(owned(&task, job), "source Job owner conflict");
    }
    let completed = current
        .as_ref()
        .and_then(|job| job.status.as_ref())
        .and_then(|s| s.conditions.as_ref())
        .and_then(|conditions| {
            conditions
                .iter()
                .find(|c| c.status == "True" && ["Complete", "Failed"].contains(&c.type_.as_str()))
        })
        .map(|condition| {
            if condition.type_ == "Complete" {
                "Succeeded"
            } else {
                "Failed"
            }
        });
    let terminal = task
        .status
        .as_ref()
        .map(|s| s.phase.as_str())
        .filter(|phase| ["Succeeded", "Failed", "Canceled"].contains(phase));
    let phase = if task.spec.cancel_requested || task.metadata.deletion_timestamp.is_some() {
        Some("Canceled")
    } else {
        completed.or(terminal)
    };
    if let Some(phase) = phase {
        // 先写终态，再删 Job；控制器中断后只重试清理。
        if terminal != Some(phase) {
            status(
                &ctx,
                &task,
                phase,
                Some("CleanupPending: waiting for Job/Pod deletion".into()),
                None,
            )
            .await?;
            return Ok(Action::requeue(Duration::from_secs(3)));
        }
        if let Some(job) = &current
            && job.metadata.deletion_timestamp.is_none()
        {
            ctx.leader.ensure().await?;
            let patched = jobs.patch(&name,&PatchParams::default(),&Patch::Merge(json!({"metadata":{"resourceVersion":job.resource_version()},"spec":{"ttlSecondsAfterFinished":300}}))).await;
            match patched {
                Ok(_) => {
                    let params = DeleteParams {
                        preconditions: Some(Preconditions {
                            uid: job.uid(),
                            resource_version: None,
                        }),
                        propagation_policy: Some(PropagationPolicy::Background),
                        ..Default::default()
                    };
                    match jobs.delete(&name, &params).await {
                        Ok(_) => (),
                        Err(kube::Error::Api(error)) if error.code == 404 => (),
                        Err(error) => return Err(error.into()),
                    }
                }
                Err(kube::Error::Api(error)) if error.code == 404 => (),
                Err(error) => return Err(error.into()),
            }
        }
        let pods = Api::<Pod>::namespaced(ctx.client.clone(), &ctx.namespace)
            .list(&ListParams::default().labels(&format!("job-name={name}")))
            .await?;
        let pod = pods.iter().find(|p| {
            p.owner_references()
                .iter()
                .any(|o| o.kind == "Job" && o.name == name)
        });
        let pending = current.is_some() || pod.is_some();
        status(
            &ctx,
            &task,
            phase,
            pending.then(|| "CleanupPending: waiting for Job/Pod deletion".into()),
            pod.and_then(ResourceExt::uid),
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(if pending {
            3
        } else {
            60
        })));
    }
    if current.is_none() {
        let pool = Api::<AgentPool>::namespaced(ctx.client.clone(), &ctx.namespace)
            .get(&task.spec.pool)
            .await?;
        let active = jobs
            .list(&ListParams::default().labels(&format!(
                "artifactory.dev/source-task-pool={}",
                task.spec.pool
            )))
            .await?;
        let active: Vec<_> = active
            .iter()
            .filter(|j| {
                j.status.as_ref().is_none_or(|s| {
                    s.conditions.as_ref().is_none_or(|c| {
                        !c.iter().any(|c| {
                            c.status == "True" && ["Complete", "Failed"].contains(&c.type_.as_str())
                        })
                    })
                })
            })
            .collect();
        if active.len() >= pool.spec.max_active as usize
            || active
                .iter()
                .filter(|j| {
                    j.labels().get("artifactory.dev/project") == Some(&task.spec.project_id)
                })
                .count()
                >= pool.spec.max_per_project as usize
        {
            status(&ctx, &task, "Queued", None, None).await?;
            return Ok(Action::requeue(Duration::from_secs(3)));
        }
        ctx.leader.ensure().await?;
        match jobs
            .create(&PostParams::default(), &job(&task, &pool)?)
            .await
        {
            Ok(_) => (),
            Err(kube::Error::Api(e)) if e.code == 409 => (),
            Err(e) => return Err(e.into()),
        }
    }
    status(&ctx, &task, "Running", None, None).await?;
    Ok(Action::requeue(Duration::from_secs(3)))
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
    let jobs = Api::<Job>::namespaced(ctx.client.clone(), &ctx.namespace);
    Controller::new(tasks, watcher::Config::default())
        .owns(jobs, watcher::Config::default())
        .run(
            |task, ctx| async { reconcile_task(task, ctx).await.map_err(Error) },
            |_, error, _| {
                tracing::warn!(%error,"source task reconciliation deferred");
                Action::requeue(Duration::from_secs(5))
            },
            ctx,
        )
        .for_each(|_| async {})
        .await;
}
