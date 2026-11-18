//! [INPUT]: 依赖 Kubernetes 绑定身份、临时源码任务和业务存储。
//! [OUTPUT]: 派发源码任务，区分身份拒绝和集群暂时故障。
//! [POS]: 只有已归属当前任务的 Pod 能领取源码凭据。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::state::AppState;
use anyhow::{Context, Result, ensure};
use artifactory_source_checkout::PrepareResponse;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use k8s_openapi::api::{authentication::v1::TokenReview, batch::v1::Job, core::v1::Pod};
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
    Unavailable,
    Internal,
}
impl SourceError {
    fn authorization(error: anyhow::Error) -> Self {
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
            None => match api.create(&PostParams::default(), &desired).await {
                Ok(value) => value,
                Err(kube::Error::Api(error)) if error.code == 409 => api.get(&name).await?,
                Err(error) => return Err(error.into()),
            },
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
    job: Job,
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
    let reviewed = serde_json::to_value(reviewed)?;
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
    let pod = Api::<Pod>::namespaced(client.clone(), &namespace)
        .get(name)
        .await?;
    ensure!(
        pod.uid().as_deref() == Some(uid)
            && pod.metadata.deletion_timestamp.is_none()
            && pod
                .spec
                .as_ref()
                .and_then(|s| s.service_account_name.as_deref())
                == Some(account),
        "Pod no longer active"
    );
    let owner = pod
        .owner_references()
        .iter()
        .find(|o| o.controller == Some(true) && o.kind == "Job" && o.api_version == "batch/v1")
        .context("Pod Job owner missing")?;
    let job = Api::<Job>::namespaced(client.clone(), &namespace)
        .get(&owner.name)
        .await?;
    ensure!(
        job.uid().as_deref() == Some(owner.uid.as_str())
            && job.metadata.deletion_timestamp.is_none(),
        "Job owner changed"
    );
    Ok(PodIdentity {
        client,
        namespace,
        job,
    })
}

async fn authorize_task(state: &AppState, request: &Value) -> Result<Value> {
    ensure!(
        request["identity"]["kind"] == "source-task",
        "wrong source identity"
    );
    let task_id = id(&request["identity"], "task_id")?;
    let identity = pod_identity(request, "artifactory-source-preparer", &namespace()?).await?;
    let name = format!("source-{}", task_id.simple());
    ensure!(identity.job.name_any() == name, "wrong source Job");
    let owner = identity
        .job
        .owner_references()
        .iter()
        .find(|o| {
            o.controller == Some(true)
                && o.kind == "SourceTask"
                && o.api_version == "agents.artifactory.dev/v1alpha1"
        })
        .context("SourceTask owner missing")?;
    let resource = tasks(identity.client.clone(), &identity.namespace)
        .get(&owner.name)
        .await?;
    ensure!(
        resource.uid().as_deref() == Some(owner.uid.as_str())
            && resource.data["spec"]["taskId"] == task_id.to_string()
            && resource.data["spec"]["cancelRequested"] != true
            && resource.metadata.deletion_timestamp.is_none(),
        "source execution stopped"
    );
    let task = state.store.source_task_context(task_id).await?;
    ensure!(
        resource.data["spec"]["projectId"] == task["project_id"]
            && resource.data["spec"]["tenantId"] == task["tenant_id"]
            && resource.data["spec"]["purpose"] == task["purpose"],
        "source scope changed"
    );
    Ok(task)
}

async fn authorize_image(request: &Value) -> Result<(Uuid, String, DynamicObject)> {
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
        identity.job.name_any()
            == format!(
                "{name}-{}",
                platform
                    .strip_prefix("linux/")
                    .context("platform missing")?
            ),
        "wrong platform Job"
    );
    let owner = identity
        .job
        .owner_references()
        .iter()
        .find(|o| {
            o.controller == Some(true)
                && o.kind == "ImageBuild"
                && o.api_version == "images.artifactory.dev/v1alpha1"
        })
        .context("ImageBuild owner missing")?;
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
    ensure!(
        build.uid().as_deref() == Some(owner.uid.as_str())
            && build.data["spec"]["executionId"] == execution.to_string()
            && build.data["spec"]["cancelRequested"] != true
            && build.metadata.deletion_timestamp.is_none(),
        "image execution stopped"
    );
    Ok((execution, platform.to_owned(), build))
}
async fn image_material(state: &AppState, request: &Value) -> SourceResult<PrepareResponse> {
    let (execution, platform, build) = authorize_image(request)
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
            let task = authorize_task(&state, &request)
                .await
                .map_err(SourceError::authorization)?;
            state
                .store
                .source_task_material(&state.settings.master_key, id(&task, "id")?)
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
    let task = authorize_task(&state, &request)
        .await
        .map_err(SourceError::authorization)?;
    Ok(Json(
        state
            .store
            .pin_source(id(&task, "id")?, &request["pin"])
            .await?,
    ))
}
async fn commit(
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> SourceResult<Json<Value>> {
    let task = authorize_task(&state, &request)
        .await
        .map_err(SourceError::authorization)?;
    state
        .store
        .commit_source_task(id(&task, "id")?, &request["result"])
        .await?;
    Ok(Json(json!({"accepted":true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    async fn failure(error: anyhow::Error) -> Response {
        let error = SourceError::authorization(error);
        let router = Router::new().route(
            "/internal/source/prepare",
            post(move || async move { Err::<Json<Value>, _>(error) }),
        );
        router
            .oneshot(
                Request::post("/internal/source/prepare")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn bootstrap_http_preserves_transient_cluster_failures() {
        for code in [404, 409, 429, 500, 502, 503, 504] {
            let error = kube::Error::Api(Box::new(kube::core::Status {
                message: "private cluster diagnostic".into(),
                reason: "temporary".into(),
                code,
                ..Default::default()
            }));
            let response = failure(anyhow::Error::new(error).context("reading current Pod")).await;
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "Kubernetes {code} must allow the init container to retry"
            );
            let body = to_bytes(response.into_body(), 1024).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("private cluster diagnostic"));
        }
    }

    #[tokio::test]
    async fn bootstrap_http_rejects_identity_failures_without_retry() {
        for code in [401, 403] {
            let error = kube::Error::Api(Box::new(kube::core::Status {
                message: "private token diagnostic".into(),
                reason: "denied".into(),
                code,
                ..Default::default()
            }));
            assert_eq!(failure(error.into()).await.status(), StatusCode::FORBIDDEN);
        }
        for reason in [
            "Pod token identity mismatch",
            "source execution stopped",
            "source scope changed",
        ] {
            assert_eq!(
                failure(anyhow::anyhow!(reason)).await.status(),
                StatusCode::FORBIDDEN
            );
        }
    }
}
