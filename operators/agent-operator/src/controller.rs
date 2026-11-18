//! [INPUT]: 依赖Agent CRD、Kubernetes API 与选主租约。
//! [OUTPUT]: 协调临时规划 Job 与已清理运行元数据。
//! [POS]: Agent 工作负载的资源生命周期。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{api::*, leadership::Leadership};
use anyhow::{Result, ensure};
use artifactory_runtime_protocol::{Execution, service::Client as ServiceClient};
use futures::StreamExt;
use k8s_openapi::api::{batch::v1::Job, core::v1::Pod};
use kube::{
    Api, Client, Resource, ResourceExt,
    api::{
        DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions, PropagationPolicy,
    },
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
pub fn job(run: &AgentRun, pool: &AgentPool) -> Result<Job> {
    let owner = run
        .controller_owner_ref(&())
        .ok_or_else(|| anyhow::anyhow!("run UID required"))?;
    let name = format!("pi-{}-g{}", run.spec.run_id, run.spec.generation);
    let mut value = json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"name":name,"namespace":run.namespace(),"ownerReferences":[owner],"labels":{"artifactory.dev/run":run.spec.run_id}},"spec":{"parallelism":1,"completions":1,"backoffLimit":0,"activeDeadlineSeconds":4500,"ttlSecondsAfterFinished":300,"template":{"metadata":{"labels":{"app.kubernetes.io/name":"artifactory-pi","artifactory.dev/run":run.spec.run_id}},"spec":{"serviceAccountName":"artifactory-pi","automountServiceAccountToken":false,"restartPolicy":"Never","securityContext":{"runAsNonRoot":true,"runAsUser":10001,"runAsGroup":10001,"fsGroup":10001,"seccompProfile":{"type":"RuntimeDefault"}},"containers":[{"name":"pi","image":pool.spec.image,"imagePullPolicy":pool.spec.image_pull_policy,"command":["node","dist/job-main.js"],"securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},"env":[{"name":"ARTIFACTORY_RUN_ID","value":run.spec.run_id},{"name":"ARTIFACTORY_RUN_GENERATION","value":run.spec.generation.to_string()},{"name":"ARTIFACTORY_RUNTIME_URL","value":pool.spec.runtime_url},{"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},{"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},{"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}},{"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/artifactory/token"}],"resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory},"limits":{"cpu":"2","memory":"2Gi"}},"volumeMounts":[{"name":"identity","mountPath":"/var/run/artifactory","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]}],"volumes":[{"name":"identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-runtime","expirationSeconds":600,"path":"token"}}]}},{"name":"tmp","emptyDir":{}}]}}}});
    if run.spec.source_manifest_id.is_some() {
        let pod = &mut value["spec"]["template"]["spec"];
        let mut init = json!({
            "name":"source", "image":pool.spec.source_preparer_image,
            "imagePullPolicy":pool.spec.image_pull_policy,
            "command":["artifactory-source-preparer", "checkout"],
            "env":[
                {"name":"ARTIFACTORY_SOURCE_PREPARE_URL","value":format!("{}/v1/prepare",pool.spec.runtime_url.trim_end_matches('/'))},
                {"name":"ARTIFACTORY_SOURCE_IDENTITY","value":json!({"run_id":run.spec.run_id,"generation":run.spec.generation}).to_string()},
                {"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"},
                {"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"},
                {"name":"ARTIFACTORY_SOURCE_AUTH_ROOT","value":"/run/source-auth"},
                {"name":"ARTIFACTORY_POD_TOKEN","value":"/var/run/source-identity/token"},
                {"name":"POD_NAME","valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}},
                {"name":"POD_UID","valueFrom":{"fieldRef":{"fieldPath":"metadata.uid"}}},
                {"name":"POD_NAMESPACE","valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}}
            ],
            "securityContext":{"allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,"capabilities":{"drop":["ALL"]}},
            "resources":{"requests":{"cpu":pool.spec.cpu,"memory":pool.spec.memory,"ephemeral-storage":pool.spec.ephemeral_storage},"limits":{"cpu":"2","memory":"2Gi","ephemeral-storage":pool.spec.ephemeral_storage}},
            "volumeMounts":[{"name":"source","mountPath":"/workspace"},{"name":"source-auth","mountPath":"/run/source-auth"},{"name":"source-identity","mountPath":"/var/run/source-identity","readOnly":true},{"name":"tmp","mountPath":"/tmp"}]
        });
        if !pool.spec.tls_secret_name.is_empty() {
            init["env"]
                .as_array_mut()
                .expect("environment array")
                .push(json!({"name":"ARTIFACTORY_SERVICE_CA","value":"/run/service-ca/ca.crt"}));
            init["volumeMounts"]
                .as_array_mut()
                .expect("mount array")
                .push(json!({"name":"ca","mountPath":"/run/service-ca","readOnly":true}));
        }
        pod["initContainers"] = json!([init]);
        pod["volumes"].as_array_mut().expect("volume array").extend([
            json!({"name":"source","emptyDir":{"sizeLimit":pool.spec.ephemeral_storage}}),
            json!({"name":"source-auth","emptyDir":{"medium":"Memory","sizeLimit":"16Mi"}}),
            json!({"name":"source-identity","projected":{"sources":[{"serviceAccountToken":{"audience":"artifactory-runtime","expirationSeconds":600,"path":"token"}}]}})
        ]);
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .expect("mount array")
            .push(json!({"name":"source","mountPath":"/workspace","readOnly":true}));
        pod["containers"][0]["env"].as_array_mut().expect("environment array").extend([
            json!({"name":"ARTIFACTORY_SOURCE_ROOT","value":"/workspace/source"}),
            json!({"name":"ARTIFACTORY_SOURCE_METADATA","value":"/workspace/source-manifest.json"})
        ]);
    }
    if !pool.spec.tls_secret_name.is_empty() {
        let pod = &mut value["spec"]["template"]["spec"];
        pod["containers"][0]["env"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"NODE_EXTRA_CA_CERTS","value":"/run/service-ca/ca.crt"}));
        pod["containers"][0]["volumeMounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"ca","mountPath":"/run/service-ca","readOnly":true}));
        pod["volumes"].as_array_mut().unwrap().push(json!({"name":"ca","secret":{"secretName":pool.spec.tls_secret_name,"items":[{"key":"ca.crt","path":"ca.crt"}]}}));
    }
    Ok(serde_json::from_value(value)?)
}
async fn create_job(ctx: &Context, run: &AgentRun, pool: &AgentPool) -> Result<Job> {
    let desired = job(run, pool)?;
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let name = desired.name_any();
    if let Some(existing) = api.get_opt(&name).await? {
        ensure!(
            existing
                .owner_references()
                .iter()
                .any(|o| Some(&o.uid) == run.metadata.uid.as_ref()),
            "Job owned by another run UID"
        );
        return Ok(existing);
    }
    ctx.leader.ensure().await?;
    match api.create(&PostParams::default(), &desired).await {
        Ok(job) => Ok(job),
        Err(error) => {
            if let Some(job) = api.get_opt(&name).await? {
                ensure!(
                    job.owner_references()
                        .iter()
                        .any(|o| Some(&o.uid) == run.metadata.uid.as_ref()),
                    "Job ownership mismatch"
                );
                Ok(job)
            } else {
                Err(error.into())
            }
        }
    }
}
fn job_delete_params(run: &AgentRun, job: &Job) -> Result<DeleteParams> {
    ensure!(
        job.owner_references()
            .iter()
            .any(|owner| Some(&owner.uid) == run.metadata.uid.as_ref()),
        "Job owned by another run UID"
    );
    let uid = job
        .uid()
        .ok_or_else(|| anyhow::anyhow!("Job UID required"))?;
    Ok(DeleteParams {
        propagation_policy: Some(PropagationPolicy::Background),
        preconditions: Some(Preconditions {
            uid: Some(uid),
            resource_version: None,
        }),
        ..Default::default()
    })
}
async fn cleanup_run(ctx: &Context, run: &AgentRun, phase: &str) -> Result<Action> {
    let jobs: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let name = format!("pi-{}-g{}", run.spec.run_id, run.spec.generation);
    let job = jobs.get_opt(&name).await?;
    if let Some(job) = &job {
        let deletion = job_delete_params(run, job)?;
        if job.metadata.deletion_timestamp.is_none() {
            ctx.leader.ensure().await?;
            match jobs.delete(&name, &deletion).await {
                Ok(_) => {}
                Err(kube::Error::Api(error)) if error.code == 404 => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    // Background deletion can acknowledge the Job before its Pods disappear.
    // Observe by Job name even after GC has removed its UID; never delete Pods by label.
    let pods: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let pod = pods
        .list(&ListParams::default().labels(&format!("job-name={name}")))
        .await?
        .into_iter()
        .find(|pod| {
            pod.owner_references()
                .iter()
                .any(|owner| owner.kind == "Job" && owner.name == name)
        });
    let pending = job.is_some() || pod.is_some();
    let pod_uid = if pending {
        pod.and_then(|p| p.uid())
            .or_else(|| run.status.as_ref().and_then(|s| s.pod_uid.clone()))
    } else {
        None
    };
    update(
        ctx,
        run,
        Status {
            phase: phase.into(),
            message: pending.then(|| "CleanupPending: waiting for Job/Pod deletion".into()),
            pod_uid,
            observed_generation: run.metadata.generation.unwrap_or(1),
        },
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(if pending {
        5
    } else {
        60
    })))
}
pub async fn run_reconcile(run: Arc<AgentRun>, ctx: Arc<Context>) -> Result<Action, Error> {
    ctx.leader.ensure().await?;
    let status: Value = crate::client::call("get", &json!({"run_id":run.spec.run_id})).await?;
    let generation = status["lifecycle"]["generation"].as_i64().unwrap_or(0);
    let state = status["status"].as_str().unwrap_or("unknown");
    let name = format!("pi-{}-g{}", run.spec.run_id, run.spec.generation);
    if generation != run.spec.generation
        || ["canceled", "failed", "succeeded", "needs_input"].contains(&state)
        || run.metadata.deletion_timestamp.is_some()
    {
        let phase = if generation != run.spec.generation {
            "Superseded"
        } else {
            state
        };
        return Ok(cleanup_run(&ctx, &run, phase).await?);
    }
    let pools: Api<AgentPool> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let pool = pools.get(&run.spec.pool).await?;
    let job = create_job(&ctx, &run, &pool).await?;
    let pods: Api<Pod> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let pods = pods
        .list(&ListParams::default().labels(&format!("job-name={name}")))
        .await?;
    let mut uid = None;
    let mut message = None;
    if let Some(pod) = pods.items.iter().find(|p| {
        p.owner_references()
            .iter()
            .any(|owner| Some(&owner.uid) == job.metadata.uid.as_ref())
    }) {
        let pod_uid = pod
            .uid()
            .ok_or_else(|| anyhow::anyhow!("Pod UID missing"))?;
        let execution = Execution {
            run_id: run.spec.run_id.parse()?,
            generation: run.spec.generation,
            uid: pod_uid.clone(),
            name: pod.name_any(),
            namespace: ctx.namespace.clone(),
        };
        ctx.leader.ensure().await?;
        let _: Value = crate::client::call(
            "register",
            &serde_json::to_value(execution).map_err(anyhow::Error::from)?,
        )
        .await?;
        uid = Some(pod_uid);
        message = pod.status.as_ref().and_then(|status| {
            status
                .container_statuses
                .as_ref()
                .and_then(|containers| {
                    containers.iter().find_map(|c| {
                        c.state.as_ref().and_then(|state| {
                            state
                                .waiting
                                .as_ref()
                                .map(|w| w.reason.clone().unwrap_or_default())
                                .or_else(|| {
                                    state
                                        .terminated
                                        .as_ref()
                                        .filter(|t| t.exit_code != 0)
                                        .map(|t| format!("Pi exited with code {}", t.exit_code))
                                })
                        })
                    })
                })
                .or_else(|| {
                    status.conditions.as_ref().and_then(|conditions| {
                        conditions
                            .iter()
                            .find(|c| c.type_ == "PodScheduled" && c.status == "False")
                            .and_then(|c| c.reason.clone())
                    })
                })
        });
    }
    update(
        &ctx,
        &run,
        Status {
            phase: if uid.is_some() {
                "Running"
            } else {
                "Provisioning"
            }
            .into(),
            message,
            pod_uid: uid,
            observed_generation: run.metadata.generation.unwrap_or(1),
        },
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(5)))
}
async fn update(ctx: &Context, run: &AgentRun, status: Status) -> Result<()> {
    if run.status.as_ref() == Some(&status) {
        return Ok(());
    }
    ctx.leader.ensure().await?;
    let api: Api<AgentRun> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    api.patch_status(
        &run.name_any(),
        &PatchParams::default(),
        &Patch::Merge(
            json!({"metadata":{"resourceVersion":run.resource_version()},"status":status}),
        ),
    )
    .await?;
    Ok(())
}
#[derive(Debug)]
pub struct Error(anyhow::Error);
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl From<kube::Error> for Error {
    fn from(e: kube::Error) -> Self {
        Self(e.into())
    }
}
impl From<uuid::Error> for Error {
    fn from(e: uuid::Error) -> Self {
        Self(e.into())
    }
}
impl std::error::Error for Error {}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
fn policy(_object: Arc<AgentRun>, error: &Error, _ctx: Arc<Context>) -> Action {
    tracing::warn!(%error,"run reconcile failed");
    Action::requeue(Duration::from_secs(5))
}
pub async fn controller(ctx: Arc<Context>) {
    let runs: Api<AgentRun> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let jobs: Api<Job> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    Controller::new(runs, watcher::Config::default())
        .owns(jobs, watcher::Config::default())
        .with_config(kube::runtime::controller::Config::default().concurrency(8))
        .run(run_reconcile, policy, ctx)
        .for_each(|_| async {})
        .await;
}
pub async fn dispatch(ctx: &Context) -> Result<()> {
    ctx.leader.ensure().await?;
    let pools: Api<AgentPool> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    let runs: Api<AgentRun> = Api::namespaced(ctx.client.clone(), &ctx.namespace);
    for pool in pools.list(&ListParams::default()).await? {
        let value:Value=ctx.runtime.post("/v1/queue",&json!({"pool":pool.name_any(),"limit":pool.spec.max_active,"per_project":pool.spec.max_per_project})).await?;
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
            pod_uid: None,
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
            runs.delete(&run.name_any(), &DeleteParams::background())
                .await?;
        }
    }
    Ok(())
}
fn run_reclaimable(run: &AgentRun, now: i64) -> bool {
    run.status.as_ref().is_some_and(|s| {
        s.message.is_none()
            && s.pod_uid.is_none()
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
#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
