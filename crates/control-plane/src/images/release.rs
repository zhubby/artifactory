//! [INPUT]: 依赖冻结交付、各 CR 终态协议、停机证据和 OCI 客户端。
//! [OUTPUT]: 复核执行停止，再推广唯一版本标签并读回完整镜像图。
//! [POS]: Release 冻结与 published 提交之间的外部副作用边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Config;
use crate::{crypto, state::AppState};
use anyhow::{Context, Result, ensure};
use artifactory_oci_registry_client::Registry;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api, ResourceExt,
    api::{ApiResource, DynamicObject, ListParams},
};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

fn pod_stopped(pod: &Pod) -> bool {
    let Some(spec) = &pod.spec else { return false };
    let Some(status) = &pod.status else {
        return false;
    };
    let terminated =
        |name: &str, states: &Option<Vec<k8s_openapi::api::core::v1::ContainerStatus>>| {
            states.as_ref().is_some_and(|states| {
                states.iter().any(|state| {
                    state.name == name
                        && state.state.as_ref().is_some_and(|s| s.terminated.is_some())
                })
            })
        };
    spec.containers
        .iter()
        .all(|c| terminated(&c.name, &status.container_statuses))
        && spec.init_containers.as_ref().is_none_or(|containers| {
            containers
                .iter()
                .all(|c| terminated(&c.name, &status.init_container_statuses))
        })
}

pub(crate) async fn stopped(state: &AppState, release: Uuid) -> Result<()> {
    let workloads = state.store.release_workloads(release).await?;
    if ["sources", "planning", "images"]
        .iter()
        .all(|key| workloads[key].as_array().is_some_and(Vec::is_empty))
    {
        return Ok(());
    }
    let client = kube::Client::try_default().await?;
    let default = std::env::var("ARTIFACTORY_KUBERNETES_NAMESPACE")
        .context("Kubernetes namespace required for publication")?;
    let image_namespace =
        std::env::var("ARTIFACTORY_IMAGE_NAMESPACE").unwrap_or_else(|_| default.clone());
    confirm_workloads(&client, &default, &image_namespace, &workloads).await
}

async fn confirm_workloads(
    client: &kube::Client,
    default: &str,
    image_namespace: &str,
    workloads: &Value,
) -> Result<()> {
    for (key, group, kind, plural, field, namespace) in [
        (
            "sources",
            "planning.artifactory.dev",
            "SourceTask",
            "sourcetasks",
            "taskId",
            default,
        ),
        (
            "planning",
            "planning.artifactory.dev",
            "PlanningRun",
            "planningruns",
            "runId",
            default,
        ),
        (
            "images",
            "images.artifactory.dev",
            "ImageBuild",
            "imagebuilds",
            "executionId",
            image_namespace,
        ),
    ] {
        let ids: BTreeSet<String> = serde_json::from_value(workloads[key].clone())?;
        if ids.is_empty() {
            continue;
        }
        let resource = ApiResource {
            group: group.into(),
            version: "v1alpha1".into(),
            api_version: format!("{group}/v1alpha1"),
            kind: kind.into(),
            plural: plural.into(),
        };
        let objects: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &resource);
        let resources = objects.list(&ListParams::default()).await?;
        let mut owners = BTreeSet::new();
        let mut names = BTreeSet::new();
        for object in resources.iter().filter(|o| {
            o.data["spec"][field]
                .as_str()
                .is_some_and(|id| ids.contains(id))
        }) {
            if let Some(uid) = object.uid() {
                owners.insert(uid);
            }
            names.insert(object.name_any());
            let status = &object.data["status"];
            let phase = status["phase"].as_str().unwrap_or("");
            let terminal = match kind {
                "PlanningRun" => matches!(
                    phase,
                    "succeeded" | "failed" | "canceled" | "needs_input" | "Superseded"
                ),
                "SourceTask" | "ImageBuild" => {
                    matches!(phase, "Succeeded" | "Failed" | "Canceled")
                }
                _ => false,
            };
            ensure!(terminal, "Release execution controller is still active");
            if let Some(execution) = status.get("execution").filter(|v| v.is_object()) {
                ensure!(
                    execution["createStarted"] != true || execution["stopped"] == true,
                    "Release execution has no confirmed Pod stop"
                );
            }
            if let Some(pods) = status
                .pointer("/podOwnership/pods")
                .and_then(Value::as_array)
            {
                ensure!(
                    pods.iter().all(|pod| pod["stopped"] == true),
                    "image Pod stop is not confirmed"
                );
            }
            ensure!(
                status
                    .pointer("/podOwnership/creating")
                    .is_none_or(Value::is_null),
                "image Pod creation is still unconfirmed"
            );
        }
        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
        for pod in pods.list(&ListParams::default()).await?.iter() {
            let owned = pod.owner_references().iter().any(|owner| {
                owners.contains(&owner.uid) || (owner.kind == kind && names.contains(&owner.name))
            });
            let labeled = match key {
                "sources" => pod
                    .labels()
                    .get("artifactory.dev/source-task")
                    .is_some_and(|name| {
                        names.contains(name)
                            || ids
                                .iter()
                                .any(|id| name == &format!("source-{}", id.replace('-', "")))
                    }),
                "planning" => pod
                    .labels()
                    .get("artifactory.dev/run")
                    .is_some_and(|id| ids.contains(id)),
                _ => pod
                    .labels()
                    .get("artifactory.dev/image-build")
                    .is_some_and(|name| {
                        names.contains(name)
                            || ids
                                .iter()
                                .any(|id| name == &format!("image-{}", id.replace('-', "")))
                    }),
            };
            ensure!(
                !(owned || labeled) || pod_stopped(pod),
                "Release still has an execution Pod without confirmed stop"
            );
        }
    }
    Ok(())
}

pub(crate) async fn publish(state: &AppState, release: Uuid) -> Result<bool> {
    if state.store.release_is_published(release).await? {
        return Ok(false);
    }
    let images = state.store.delivery_images(release).await?;
    let token = crypto::token();
    let Some(generation) = state
        .store
        .acquire_release_publisher(release, &crypto::hash(&token))
        .await?
    else {
        return Ok(false);
    };
    let store = state.store.clone();
    let renewal = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            if !store
                .renew_release_publisher(release, generation)
                .await
                .unwrap_or(false)
            {
                break;
            }
        }
    });
    let result = async {
        stopped(state, release).await?;
        if images.is_empty() {
            return Ok(true);
        }
        let config = Config::load()?;
        let registry = Registry::new(config.login(release, &token), state.settings.insecure_dev)?;
        let temporary = tempfile::tempdir()?;
        for image in images {
            let source = image["candidate_repository"]
                .as_str()
                .context("candidate repository missing")?;
            let destination = image["repository"]
                .as_str()
                .context("delivery repository missing")?;
            let version = image["version"]
                .as_str()
                .context("delivery version missing")?;
            let root = image["root_digest"]
                .as_str()
                .context("delivery digest missing")?;
            let expected: BTreeSet<String> =
                serde_json::from_value(image["manifest_digests"].clone())?;
            ensure!(
                registry.manifest_graph(source, root).await? == expected,
                "candidate image graph differs from frozen delivery"
            );
            registry
                .ensure_tag_available(destination, version, root)
                .await?;
            registry
                .copy(
                    &registry,
                    source,
                    destination,
                    root,
                    &[version.to_owned()],
                    temporary.path(),
                )
                .await?;
            ensure!(
                artifactory_image_protocol::sha256(&registry.manifest(destination, version).await?)
                    == root,
                "formal version tag differs from frozen delivery"
            );
            ensure!(
                registry.manifest_graph(destination, root).await? == expected,
                "published graph differs from frozen delivery"
            );
        }
        Ok(true)
    }
    .await;
    let result = match result {
        Ok(done) => state
            .store
            .complete_release_publication(release, generation)
            .await
            .map(|()| done),
        Err(error) => {
            match state
                .store
                .fail_release_publication(release, generation, &error.to_string())
                .await
            {
                Ok(()) => Err(error),
                Err(fence) => Err(fence.context("publication owner lost its lease")),
            }
        }
    };
    renewal.abort();
    let _ = state
        .store
        .finish_release_publisher(release, generation)
        .await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactory_image_operator::api::Status as ImageStatus;
    use artifactory_planning_operator::api::{PodExecution, Status as PlanningStatus};
    use axum::{Router, http::Uri, routing::get};
    use serde_json::json;

    fn pod(init_stopped: bool) -> Pod {
        serde_json::from_value(json!({
            "metadata":{"name":"worker"},
            "spec":{"containers":[{"name":"build","image":"fixture"}],"initContainers":[{"name":"source","image":"fixture"}]},
            "status":{"phase":"Succeeded","containerStatuses":[{"name":"build","image":"fixture","imageID":"fixture","ready":false,"restartCount":0,"state":{"terminated":{"exitCode":0}}}],
                "initContainerStatuses":[{"name":"source","image":"fixture","imageID":"fixture","ready":false,"restartCount":0,"state":if init_stopped{json!({"terminated":{"exitCode":0}})}else{json!({"running":{}})}}]}
        })).unwrap()
    }

    #[test]
    fn pod_stop_requires_every_container_and_init_container() {
        assert!(pod_stopped(&pod(true)));
        assert!(!pod_stopped(&pod(false)));
        let mut missing = pod(true);
        missing.status.as_mut().unwrap().container_statuses = None;
        assert!(!pod_stopped(&missing));
        missing.status.as_mut().unwrap().phase = Some("Failed".into());
        assert!(
            !pod_stopped(&missing),
            "terminal phase alone is not stop proof"
        );
    }

    async fn proof_for(key: &str, objects: Vec<Value>, pods: Vec<Pod>, id: Uuid) -> bool {
        let (group, kind, plural) = match key {
            "sources" => ("planning.artifactory.dev", "SourceTaskList", "sourcetasks"),
            "planning" => (
                "planning.artifactory.dev",
                "PlanningRunList",
                "planningruns",
            ),
            "images" => ("images.artifactory.dev", "ImageBuildList", "imagebuilds"),
            _ => panic!("unknown workload test key {key}"),
        };
        let app = Router::new().fallback(get(move |uri: Uri| {
            let objects = objects.clone();
            let pods = pods.clone();
            async move {
                if uri.path().ends_with(plural) {
                    axum::Json(json!({"apiVersion":format!("{group}/v1alpha1"),"kind":kind,"metadata":{},"items":objects}))
                } else {
                    assert!(uri.path().ends_with("/pods"));
                    axum::Json(json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":pods}))
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = kube::Client::try_from(kube::Config::new(
            format!("http://{}", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        ))
        .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut workloads = json!({"sources":[],"planning":[],"images":[]});
        workloads[key] = json!([id]);
        let result = confirm_workloads(&client, "test", "test", &workloads)
            .await
            .is_ok();
        server.abort();
        result
    }

    async fn proof(objects: Vec<Value>, pods: Vec<Pod>, id: Uuid) -> bool {
        proof_for("images", objects, pods, id).await
    }

    fn controller(kind: &str, id: Uuid, phase: &str) -> Value {
        let (group, field) = match kind {
            "PlanningRun" => ("planning.artifactory.dev", "runId"),
            "SourceTask" => ("planning.artifactory.dev", "taskId"),
            "ImageBuild" => ("images.artifactory.dev", "executionId"),
            _ => panic!("unknown test controller {kind}"),
        };
        let status = if kind == "ImageBuild" {
            serde_json::to_value(ImageStatus {
                phase: phase.into(),
                observed_generation: 1,
                ..Default::default()
            })
            .unwrap()
        } else {
            serde_json::to_value(PlanningStatus {
                phase: phase.into(),
                observed_generation: 1,
                ..Default::default()
            })
            .unwrap()
        };
        let mut object = json!({"apiVersion":format!("{group}/v1alpha1"),"kind":kind,"metadata":{"name":"controller","uid":"controller","generation":1},"spec":{},"status":status});
        object["spec"][field] = json!(id);
        object
    }

    #[tokio::test]
    async fn publication_uses_each_operators_serialized_terminal_protocol() {
        let id = Uuid::new_v4();
        for phase in [
            "succeeded",
            "failed",
            "canceled",
            "needs_input",
            "Superseded",
        ] {
            let object = controller("PlanningRun", id, phase);
            assert_eq!(object["status"]["phase"], phase);
            assert!(
                proof_for("planning", vec![object], vec![], id).await,
                "PlanningRun {phase} is terminal"
            );
        }
        for phase in [
            "Succeeded",
            "Failed",
            "Canceled",
            "superseded",
            "Running",
            "Provisioning",
            "unknown",
            "",
        ] {
            assert!(
                !proof_for(
                    "planning",
                    vec![controller("PlanningRun", id, phase)],
                    vec![],
                    id
                )
                .await,
                "PlanningRun {phase} is not its terminal protocol"
            );
        }
        for (key, kind) in [("sources", "SourceTask"), ("images", "ImageBuild")] {
            for phase in ["Succeeded", "Failed", "Canceled"] {
                assert!(
                    proof_for(key, vec![controller(kind, id, phase)], vec![], id).await,
                    "{kind} {phase} is terminal"
                );
            }
            for phase in [
                "succeeded",
                "failed",
                "canceled",
                "needs_input",
                "Superseded",
                "Running",
                "",
            ] {
                assert!(
                    !proof_for(key, vec![controller(kind, id, phase)], vec![], id).await,
                    "{kind} {phase} is not its terminal protocol"
                );
            }
        }
    }

    #[tokio::test]
    async fn terminal_planning_and_source_controllers_still_require_physical_stop() {
        let id = Uuid::new_v4();
        for (key, kind, phase, label, value) in [
            (
                "planning",
                "PlanningRun",
                "succeeded",
                "artifactory.dev/run",
                id.to_string(),
            ),
            (
                "sources",
                "SourceTask",
                "Succeeded",
                "artifactory.dev/source-task",
                format!("source-{}", id.simple()),
            ),
        ] {
            let mut object = controller(kind, id, phase);
            object["status"]["execution"] = serde_json::to_value(PodExecution {
                name: "worker".into(),
                uid: Some("worker-uid".into()),
                node_name: Some("node".into()),
                create_started: true,
                attempt: 1,
                deadline_ms: 1,
                stopped: false,
            })
            .unwrap();
            assert!(
                !proof_for(key, vec![object.clone()], vec![], id).await,
                "{kind} terminal phase cannot replace its Pod ledger"
            );
            object["status"]["execution"]["stopped"] = json!(true);
            assert!(proof_for(key, vec![object.clone()], vec![], id).await);
            let mut physical = pod(false);
            physical.metadata.owner_references = Some(serde_json::from_value(json!([{
                "apiVersion":"planning.artifactory.dev/v1alpha1","kind":kind,"name":"controller","uid":"controller","controller":true
            }])).unwrap());
            assert!(
                !proof_for(key, vec![object.clone()], vec![physical.clone()], id).await,
                "{kind} saved stop cannot hide an active owned Pod"
            );
            physical.metadata.owner_references = None;
            physical.metadata.labels = Some(std::collections::BTreeMap::from([(
                label.into(),
                value.clone(),
            )]));
            assert!(
                !proof_for(key, vec![], vec![physical], id).await,
                "deleted {kind} cannot hide an active labeled Pod"
            );
            let mut physical = pod(true);
            physical.metadata.labels =
                Some(std::collections::BTreeMap::from([(label.into(), value)]));
            assert!(proof_for(key, vec![object], vec![physical], id).await);
        }
    }

    #[tokio::test]
    async fn publication_requires_controller_and_orphan_pod_stop_proof() {
        let id = Uuid::new_v4();
        let name = format!("image-{}", id.simple());
        let mut object = json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":name,"uid":"controller"},"spec":{"executionId":id},"status":{"phase":"Succeeded","podOwnership":{"pods":[{"stopped":false}]}}});
        assert!(!proof(vec![object.clone()], vec![], id).await);
        object["status"]["podOwnership"]["pods"][0]["stopped"] = json!(true);
        assert!(proof(vec![object.clone()], vec![], id).await);
        object["status"]["podOwnership"]["creating"] = json!("late-worker");
        assert!(
            !proof(vec![object.clone()], vec![], id).await,
            "terminal controller cannot hide unconfirmed Pod creation"
        );
        object["status"]["podOwnership"]["creating"] = Value::Null;
        object["status"]["phase"] = json!("Running");
        assert!(!proof(vec![object], vec![], id).await);
        let mut orphan = pod(false);
        orphan.metadata.labels = Some(std::collections::BTreeMap::from([(
            "artifactory.dev/image-build".into(),
            name,
        )]));
        assert!(
            !proof(vec![], vec![orphan.clone()], id).await,
            "deleted CR must not hide a live labeled Pod"
        );
        orphan = pod(true);
        orphan.metadata.labels = Some(std::collections::BTreeMap::from([(
            "artifactory.dev/image-build".into(),
            format!("image-{}", id.simple()),
        )]));
        assert!(proof(vec![], vec![orphan], id).await);
    }
}
