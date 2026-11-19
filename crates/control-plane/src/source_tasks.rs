//! [INPUT]: 依赖 Kubernetes 绑定身份、临时源码任务和业务存储。
//! [OUTPUT]: 固定 CR UID，授权直接归属当前 CR 的 Pod。
//! [POS]: TokenReview、Pod UID 与不可变任务契约的凭据边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{state::AppState, store::sources::SourceTaskExecution};
use anyhow::{Context, Result, ensure};
use artifactory_source_checkout::PrepareResponse;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use k8s_openapi::{
    api::{authentication::v1::TokenReview, core::v1::Pod},
    apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Api, Client, ResourceExt,
    api::{ApiResource, DynamicObject, Patch, PatchParams, PostParams},
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum SourceError {
    Forbidden,
    Pending,
    Unavailable,
    Internal,
}
impl SourceError {
    fn authorization(error: anyhow::Error) -> Self {
        if error.downcast_ref::<IdentityPending>().is_some() {
            return Self::Pending;
        }
        match error.downcast_ref::<kube::Error>() {
            Some(kube::Error::Api(error))
                if [404, 409, 429].contains(&error.code) || error.code >= 500 =>
            {
                Self::Unavailable
            }
            Some(kube::Error::Api(_)) | None => Self::Forbidden,
            Some(_) => Self::Unavailable,
        }
    }
}
impl<E: Into<anyhow::Error>> From<E> for SourceError {
    fn from(_: E) -> Self {
        Self::Internal
    }
}
impl IntoResponse for SourceError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "source execution is stale or unauthorized",
            ),
            Self::Pending => (
                StatusCode::CONFLICT,
                "source Pod identity registration pending",
            ),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "source identity service temporarily unavailable",
            ),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "source operation failed"),
        };
        (status, Json(json!({"error":message}))).into_response()
    }
}
type SourceResult<T> = std::result::Result<T, SourceError>;

#[derive(Debug, thiserror::Error)]
#[error("Pod identity registration pending")]
struct IdentityPending;

fn namespace() -> Result<String> {
    std::env::var("ARTIFACTORY_KUBERNETES_NAMESPACE").context("Kubernetes namespace required")
}
fn tasks(client: Client, namespace: &str) -> Api<DynamicObject> {
    Api::namespaced_with(
        client,
        namespace,
        &ApiResource {
            group: "agents.artifactory.dev".into(),
            version: "v1alpha1".into(),
            api_version: "agents.artifactory.dev/v1alpha1".into(),
            kind: "SourceTask".into(),
            plural: "sourcetasks".into(),
        },
    )
}
fn id(value: &Value, key: &str) -> Result<Uuid> {
    Ok(value[key].as_str().context("identifier missing")?.parse()?)
}

pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/internal/source/prepare", post(prepare))
        .route("/internal/source/prepare/pin", post(pin))
        .route("/internal/source/prepare/commit", post(commit))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state)
}

pub async fn reconcile(state: &AppState) -> Result<()> {
    state.store.enqueue_source_resolutions().await?;
    let pending = state.store.source_tasks_pending().await?;
    if pending.is_empty() {
        return Ok(());
    }
    let client = Client::try_default().await?;
    let namespace = namespace()?;
    let api = tasks(client, &namespace);
    for task in pending {
        let task_id = id(&task, "id")?;
        let name = format!("source-{}", task_id.simple());
        let stopped = matches!(task["status"].as_str(), Some("failed" | "canceled"));
        let desired: DynamicObject = serde_json::from_value(
            json!({"apiVersion":"agents.artifactory.dev/v1alpha1","kind":"SourceTask","metadata":{"name":name,"namespace":namespace,"labels":{"artifactory.dev/source-task-id":task_id.to_string()}},"spec":{"taskId":task_id,"tenantId":task["tenant_id"],"projectId":task["project_id"],"purpose":task["purpose"],"pool":std::env::var("ARTIFACTORY_AGENT_POOL").unwrap_or_else(|_|"default".into()),"timeoutSeconds":if task["purpose"]=="refs"{30}else{900},"cancelRequested":false}}),
        )?;
        let existing = match api.get_opt(&name).await? {
            Some(value) => value,
            None if stopped => {
                state.store.source_task_dispatched(task_id, true).await?;
                continue;
            }
            None => {
                ensure!(
                    task["source_task_uid"].is_null(),
                    "bound SourceTask disappeared"
                );
                match api.create(&PostParams::default(), &desired).await {
                    Ok(value) => value,
                    Err(kube::Error::Api(error)) if error.code == 409 => api.get(&name).await?,
                    Err(error) => return Err(error.into()),
                }
            }
        };
        for field in [
            "taskId",
            "tenantId",
            "projectId",
            "purpose",
            "pool",
            "timeoutSeconds",
        ] {
            ensure!(
                existing.data["spec"][field] == desired.data["spec"][field],
                "SourceTask identity conflict"
            );
        }
        state
            .store
            .bind_source_task(task_id, &existing.uid().context("SourceTask UID required")?)
            .await?;
        if stopped && existing.data["spec"]["cancelRequested"] != true {
            api.patch(
                &name,
                &PatchParams::default(),
                &Patch::Merge(json!({"spec":{"cancelRequested":true}})),
            )
            .await?;
        }
        if existing.data["status"]["phase"] == "Failed" {
            state
                .store
                .source_task_failed(
                    task_id,
                    existing.data["status"]["message"]
                        .as_str()
                        .unwrap_or("source preparation failed"),
                )
                .await?;
        }
        state.store.source_task_dispatched(task_id, stopped).await?;
    }
    Ok(())
}

pub async fn refs(state: &AppState, project: Uuid) -> Result<Value> {
    let task = state.store.create_refs_task(project).await?;
    reconcile(state).await?;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let value = state.store.source_task_result(task).await?;
            match value["status"].as_str() {
                Some("succeeded") => return Ok(value["result"]["refs"].clone()),
                Some("failed" | "canceled") => anyhow::bail!("repository reference query failed"),
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    })
    .await
    .context("repository reference query timed out")?
}

struct PodIdentity {
    client: Client,
    namespace: String,
    pod: Pod,
}
async fn pod_identity(request: &Value, account: &str, namespace: &str) -> Result<PodIdentity> {
    let namespace = namespace.to_owned();
    ensure!(
        request["pod"]["namespace"] == namespace,
        "wrong Pod namespace"
    );
    let name = request["pod"]["name"]
        .as_str()
        .context("Pod name missing")?;
    let uid = request["pod"]["uid"].as_str().context("Pod UID missing")?;
    let token = request["bootstrap_token"]
        .as_str()
        .context("Pod token missing")?;
    ensure!(token.len() <= 16 * 1024, "oversized Pod token");
    let client = Client::try_default().await?;
    let review: TokenReview = serde_json::from_value(
        json!({"apiVersion":"authentication.k8s.io/v1","kind":"TokenReview","spec":{"token":token,"audiences":["artifactory-source-prepare"]}}),
    )?;
    let reviewed = Api::<TokenReview>::all(client.clone())
        .create(&PostParams::default(), &review)
        .await?;
    validate_review(
        &serde_json::to_value(reviewed)?,
        namespace.as_str(),
        account,
        name,
        uid,
    )?;
    let pod = Api::<Pod>::namespaced(client.clone(), &namespace)
        .get(name)
        .await?;
    validate_pod(&pod, &namespace, account, name, uid)?;
    Ok(PodIdentity {
        client,
        namespace,
        pod,
    })
}

fn validate_review(
    reviewed: &Value,
    namespace: &str,
    account: &str,
    name: &str,
    uid: &str,
) -> Result<()> {
    let status = &reviewed["status"];
    ensure!(
        status["authenticated"] == true
            && status["audiences"]
                .as_array()
                .is_some_and(|items| items.iter().any(|v| v == "artifactory-source-prepare")),
        "Pod token rejected"
    );
    ensure!(
        status["user"]["username"] == format!("system:serviceaccount:{namespace}:{account}"),
        "wrong preparer account"
    );
    ensure!(
        status["user"]["extra"]["authentication.kubernetes.io/pod-uid"][0] == uid
            && status["user"]["extra"]["authentication.kubernetes.io/pod-name"][0] == name,
        "Pod token identity mismatch"
    );
    Ok(())
}

fn validate_pod(pod: &Pod, namespace: &str, account: &str, name: &str, uid: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && !uid.is_empty()
            && pod.name_any() == name
            && pod.namespace().as_deref() == Some(namespace)
            && pod.uid().as_deref() == Some(uid)
            && pod.metadata.deletion_timestamp.is_none()
            && pod
                .spec
                .as_ref()
                .and_then(|s| s.service_account_name.as_deref())
                == Some(account),
        "Pod no longer active"
    );
    ensure!(
        !pod.status
            .as_ref()
            .is_some_and(|s| matches!(s.phase.as_deref(), Some("Succeeded" | "Failed"))),
        "Pod execution is terminal"
    );
    Ok(())
}

fn direct_owner<'a>(
    pod: &'a Pod,
    version: &str,
    kind: &str,
    name: &str,
) -> Result<&'a OwnerReference> {
    let owner = pod
        .owner_references()
        .iter()
        .find(|o| o.controller == Some(true))
        .context("Pod controller owner missing")?;
    ensure!(
        owner.api_version == version
            && owner.kind == kind
            && owner.name == name
            && !owner.uid.is_empty(),
        "Pod direct owner does not match task"
    );
    Ok(owner)
}

fn validate_resource(
    resource: &DynamicObject,
    owner: &OwnerReference,
    namespace: &str,
    bound_uid: Option<&str>,
) -> Result<()> {
    let uid = resource.uid().context("CR UID missing")?;
    ensure!(
        resource
            .types
            .as_ref()
            .is_some_and(|t| t.api_version == owner.api_version && t.kind == owner.kind)
            && resource.name_any() == owner.name
            && resource.namespace().as_deref() == Some(namespace)
            && uid == owner.uid
            && resource.metadata.deletion_timestamp.is_none()
            && resource.metadata.generation.is_some_and(|g| g > 0)
            && resource.data["spec"]["cancelRequested"] == false,
        "task resource identity changed"
    );
    let bound = bound_uid.ok_or(IdentityPending)?;
    ensure!(uid == bound, "bound task resource UID changed");
    Ok(())
}

async fn authorize_task(state: &AppState, request: &Value) -> Result<(Value, SourceTaskExecution)> {
    ensure!(
        request["identity"]["kind"] == "source-task",
        "wrong source identity"
    );
    let task_id = id(&request["identity"], "task_id")?;
    let identity = pod_identity(request, "artifactory-source-preparer", &namespace()?).await?;
    let name = format!("source-{}", task_id.simple());
    let owner = direct_owner(
        &identity.pod,
        "agents.artifactory.dev/v1alpha1",
        "SourceTask",
        &name,
    )?;
    let resource = tasks(identity.client.clone(), &identity.namespace)
        .get(&owner.name)
        .await?;
    let task = state.store.source_task_context(task_id).await?;
    validate_resource(
        &resource,
        owner,
        &identity.namespace,
        task["source_task_uid"].as_str(),
    )?;
    validate_source_contract(&identity.pod, &resource, &task)?;
    let execution = SourceTaskExecution {
        cr_uid: owner.uid.clone(),
        name: identity.pod.name_any(),
        uid: identity.pod.uid().context("Pod UID missing")?,
        attempt: resource.data["status"]["execution"]["attempt"]
            .as_u64()
            .context("source attempt missing")?
            .try_into()?,
    };
    Ok((task, execution))
}

fn validate_source_contract(pod: &Pod, resource: &DynamicObject, task: &Value) -> Result<()> {
    let task_id = id(task, "id")?;
    let pool = std::env::var("ARTIFACTORY_AGENT_POOL").unwrap_or_else(|_| "default".into());
    ensure!(
        resource.data["spec"]["taskId"] == task_id.to_string()
            && resource.data["spec"]["projectId"] == task["project_id"]
            && resource.data["spec"]["tenantId"] == task["tenant_id"]
            && resource.data["spec"]["purpose"] == task["purpose"]
            && resource.data["spec"]["pool"] == pool
            && resource.data["spec"]["timeoutSeconds"]
                == if task["purpose"] == "refs" { 30 } else { 900 },
        "source scope changed"
    );
    let execution = &resource.data["status"]["execution"];
    let attempt = execution["attempt"].as_u64().ok_or(IdentityPending)?;
    ensure!((1..=3).contains(&attempt), "invalid source attempt");
    let uid = resource.uid().context("SourceTask UID missing")?;
    let expected = format!(
        "source-{}-a{attempt}-{}",
        task_id.simple(),
        &crate::crypto::hash(uid.as_bytes())[..8]
    );
    ensure!(
        pod.name_any() == expected
            && execution["name"] == expected
            && pod.labels().get("artifactory.dev/attempt") == Some(&attempt.to_string()),
        "source Pod attempt changed"
    );
    let pod_uid = pod.uid().context("Pod UID missing")?;
    let registered = execution["uid"].as_str().ok_or(IdentityPending)?;
    ensure!(registered == pod_uid, "source Pod UID changed");
    ensure!(
        resource.data["status"]["observedGeneration"].as_i64() == resource.metadata.generation,
        "source observation is stale"
    );
    ensure!(
        execution["deadlineMs"]
            .as_i64()
            .is_some_and(|deadline| deadline > chrono::Utc::now().timestamp_millis())
            && execution["stopped"] != true
            && !matches!(
                resource.data["status"]["phase"].as_str(),
                Some("Succeeded" | "Failed" | "Canceled")
            ),
        "source execution stopped"
    );
    validate_source_identity(
        pod,
        &json!({"kind":"source-task","task_id":task_id.to_string()}),
    )?;
    Ok(())
}

fn validate_source_identity(pod: &Pod, expected: &Value) -> Result<()> {
    let container = pod
        .spec
        .as_ref()
        .context("Pod spec missing")?
        .init_containers
        .iter()
        .flatten()
        .chain(
            pod.spec
                .as_ref()
                .context("Pod spec missing")?
                .containers
                .iter(),
        )
        .find(|c| c.name == "source")
        .context("source container missing")?;
    let raw = container
        .env
        .iter()
        .flatten()
        .find(|e| e.name == "ARTIFACTORY_SOURCE_IDENTITY")
        .and_then(|e| e.value.as_deref())
        .context("source identity environment missing")?;
    ensure!(
        serde_json::from_str::<Value>(raw)? == *expected,
        "source identity environment changed"
    );
    Ok(())
}

async fn authorize_image(
    state: &AppState,
    request: &Value,
) -> Result<(Uuid, String, DynamicObject)> {
    let execution = id(&request["identity"], "execution_id")?;
    let platform = request["identity"]["platform"]
        .as_str()
        .context("platform missing")?;
    ensure!(
        ["linux/amd64", "linux/arm64"].contains(&platform),
        "unsupported source platform"
    );
    let identity = pod_identity(
        request,
        "artifactory-image-worker",
        &crate::images::Config::load()?.namespace,
    )
    .await?;
    let name = format!("image-{}", execution.simple());
    ensure!(
        identity.pod.name_any()
            == format!(
                "{name}-{}",
                platform
                    .strip_prefix("linux/")
                    .context("platform missing")?
            ),
        "wrong platform Pod"
    );
    let owner = direct_owner(
        &identity.pod,
        "images.artifactory.dev/v1alpha1",
        "ImageBuild",
        &name,
    )?;
    let resource = ApiResource {
        group: "images.artifactory.dev".into(),
        version: "v1alpha1".into(),
        api_version: "images.artifactory.dev/v1alpha1".into(),
        kind: "ImageBuild".into(),
        plural: "imagebuilds".into(),
    };
    let build = Api::<DynamicObject>::namespaced_with(
        identity.client.clone(),
        &identity.namespace,
        &resource,
    )
    .get(&owner.name)
    .await?;
    let record = state.store.image_execution(execution).await?;
    validate_resource(
        &build,
        owner,
        &identity.namespace,
        record.image_build_uid.as_deref(),
    )?;
    let digest = state
        .store
        .source_manifest_digest(record.source_manifest_id)
        .await?;
    let expected = json!({"executionId":record.id,"tenantId":record.tenant_id,"projectId":record.project_id,"pool":record.spec.pool,"sourceManifestId":record.source_manifest_id,"sourceDigest":digest,"taskSecret":format!("image-{}-task",record.id.simple()),"platforms":record.spec.platforms,"allowEmulation":record.spec.allow_emulation,"publishOnly":record.operation=="publish","timeoutSeconds":record.task_timeout_seconds,"cancelRequested":false});
    validate_image_contract(&identity.pod, &build, &expected, platform)?;
    Ok((execution, platform.to_owned(), build))
}

fn validate_image_contract(
    pod: &Pod,
    build: &DynamicObject,
    expected: &Value,
    platform: &str,
) -> Result<()> {
    ensure!(
        build.data["spec"] == *expected,
        "image execution spec changed"
    );
    ensure!(
        expected["publishOnly"] == false
            && expected["platforms"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item == platform)),
        "image platform not assigned"
    );
    let ownership = &build.data["status"]["podOwnership"];
    let uid = build.uid().context("ImageBuild UID missing")?;
    let owner_uid = ownership["imageBuildUid"].as_str().ok_or(IdentityPending)?;
    ensure!(owner_uid == uid, "image Pod ownership changed");
    let pods = ownership["pods"].as_array().ok_or(IdentityPending)?;
    let assigned = pods
        .iter()
        .find(|p| p["platform"] == platform)
        .ok_or(IdentityPending)?;
    ensure!(
        assigned["name"] == pod.name_any() && assigned["uid"].as_str() == pod.uid().as_deref(),
        "image Pod UID changed"
    );
    ensure!(assigned["stopped"] != true, "image Pod execution stopped");
    ensure!(
        build.data["status"]["observedGeneration"].as_i64() == build.metadata.generation,
        "image observation is stale"
    );
    ensure!(
        pod.labels()
            .get("artifactory.dev/platform")
            .map(String::as_str)
            == platform.strip_prefix("linux/"),
        "image platform label changed"
    );
    validate_source_identity(
        pod,
        &json!({"kind":"image-build","execution_id":expected["executionId"],"platform":platform}),
    )?;
    Ok(())
}
async fn image_material(state: &AppState, request: &Value) -> SourceResult<PrepareResponse> {
    let (execution, platform, build) = authorize_image(state, request)
        .await
        .map_err(SourceError::authorization)?;
    let material = state
        .store
        .image_source_material(&state.settings.master_key, execution, &platform)
        .await?;
    if !(build.data["spec"]["sourceManifestId"] == material.source_id.to_string()
        && build.data["spec"]["sourceDigest"]
            == material
                .manifest
                .as_ref()
                .context("manifest missing")?
                .digest)
    {
        return Err(SourceError::Forbidden);
    }
    Ok(material)
}

async fn prepare(
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> SourceResult<Json<Value>> {
    let material = match request["identity"]["kind"].as_str() {
        Some("source-task") => {
            let (task, execution) = authorize_task(&state, &request)
                .await
                .map_err(SourceError::authorization)?;
            state
                .store
                .source_task_material(&state.settings.master_key, id(&task, "id")?, &execution)
                .await?
        }
        Some("image-build") => image_material(&state, &request).await?,
        _ => return Err(SourceError::Forbidden),
    };
    Ok(Json(serde_json::to_value(material)?))
}
async fn pin(
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> SourceResult<Json<Value>> {
    let (task, execution) = authorize_task(&state, &request)
        .await
        .map_err(SourceError::authorization)?;
    Ok(Json(
        state
            .store
            .pin_source(id(&task, "id")?, &execution, &request["pin"])
            .await?,
    ))
}
async fn commit(
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> SourceResult<Json<Value>> {
    let (task, execution) = authorize_task(&state, &request)
        .await
        .map_err(SourceError::authorization)?;
    state
        .store
        .commit_source_task(id(&task, "id")?, &execution, &request["result"])
        .await?;
    Ok(Json(json!({"accepted":true})))
}

#[cfg(test)]
#[path = "source_tasks_tests.rs"]
mod tests;
