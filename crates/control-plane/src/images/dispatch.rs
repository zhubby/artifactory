//! [INPUT]: 依赖镜像持久化认领、固定源码引用、加密材料与 Kubernetes API。
//! [OUTPUT]: 固定 CR UID，核对当前 spec 与 Operator 成功后提交候选。
//! [POS]: 多副本可重放的控制平面到 Operator 交付边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Config, tasks};
use crate::{
    crypto,
    state::AppState,
    store::images::{ImageExecution, ImageStop},
};
use anyhow::{Result, ensure};
use artifactory_image_protocol::{BuildTask, PublishTask};
use k8s_openapi::api::core::v1::Secret;
use kube::{
    Api, Client, ResourceExt,
    api::{ApiResource, DeleteParams, DynamicObject, Patch, PatchParams, PostParams},
};
use serde_json::json;
use uuid::Uuid;

pub(super) fn serialize_materials(build: &BuildTask, publish: &PublishTask) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&json!({
        "build.json": serde_json::to_string(build)?,
        "publish.json": serde_json::to_string(publish)?,
    }))?;
    ensure!(
        bytes.len() <= 900 * 1024,
        "task Secret exceeds Kubernetes limit"
    );
    Ok(bytes)
}

fn api(client: &Client, config: &Config) -> Api<DynamicObject> {
    let resource = ApiResource {
        group: "images.artifactory.dev".into(),
        version: "v1alpha1".into(),
        api_version: "images.artifactory.dev/v1alpha1".into(),
        kind: "ImageBuild".into(),
        plural: "imagebuilds".into(),
    };
    Api::namespaced_with(client.clone(), &config.namespace, &resource)
}
async fn deliver(
    state: &AppState,
    config: &Config,
    client: &Client,
    e: &ImageExecution,
) -> Result<()> {
    let api = api(client, config);
    let name = format!("image-{}", e.id.simple());
    let secret_name = format!("{name}-task");
    if let Some(stop) = state.store.image_stop_status(e.id).await? {
        if let ImageStop::Terminate(status) = stop {
            if let Some(existing) = api.get_opt(&name).await? {
                ensure!(
                    existing.labels().get("artifactory.dev/execution") == Some(&e.id.to_string()),
                    "ImageBuild identity conflict"
                );
                api.patch(
                    &name,
                    &PatchParams::default(),
                    &Patch::Merge(json!({"spec":{"cancelRequested":true}})),
                )
                .await?;
            }
            state.store.finish_image(e.id, status).await?;
        }
        let secrets: Api<Secret> = Api::namespaced(client.clone(), &config.namespace);
        if let Some(secret) = secrets.get_opt(&secret_name).await? {
            ensure!(
                secret.labels().get("artifactory.dev/execution") == Some(&e.id.to_string()),
                "task Secret identity conflict"
            );
            secrets
                .delete(
                    &secret_name,
                    &DeleteParams {
                        preconditions: Some(kube::api::Preconditions {
                            uid: secret.uid(),
                            resource_version: None,
                        }),
                        ..Default::default()
                    },
                )
                .await?;
        }
        state.store.clear_image_task(e.id).await?;
        return Ok(());
    }
    let Some(envelope) = &e.task_envelope else {
        state.store.finish_image(e.id, "failed").await?;
        return Ok(());
    };
    let bytes = crypto::open_context(&state.settings.master_key, envelope, e.id.as_bytes())?;
    let materials: std::collections::BTreeMap<String, String> = serde_json::from_slice(&bytes)?;
    let digest = state
        .store
        .source_manifest_digest(e.source_manifest_id)
        .await?;
    let desired_build: DynamicObject = serde_json::from_value(
        json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":name,"namespace":config.namespace,"labels":{"artifactory.dev/execution":e.id.to_string()}},"spec":{"executionId":e.id,"tenantId":e.tenant_id,"projectId":e.project_id,"pool":e.spec.pool,"sourceManifestId":e.source_manifest_id,"sourceDigest":digest,"taskSecret":secret_name,"platforms":e.spec.platforms,"allowEmulation":e.spec.allow_emulation,"publishOnly":e.operation=="publish","timeoutSeconds":e.task_timeout_seconds,"cancelRequested":false}}),
    )?;
    let build = match api.get_opt(&name).await? {
        Some(v) => v,
        None => {
            ensure!(e.image_build_uid.is_none(), "bound ImageBuild disappeared");
            match api.create(&PostParams::default(), &desired_build).await {
                Ok(v) => v,
                Err(kube::Error::Api(e)) if e.code == 409 => api.get(&name).await?,
                Err(e) => return Err(e.into()),
            }
        }
    };
    verify_build(&build, &desired_build)?;
    let uid = build
        .uid()
        .ok_or_else(|| anyhow::anyhow!("ImageBuild UID required"))?;
    state.store.bind_image_build(e.id, &uid).await?;
    let secrets: Api<Secret> = Api::namespaced(client.clone(), &config.namespace);
    let desired: Secret = serde_json::from_value(
        json!({"apiVersion":"v1","kind":"Secret","metadata":{"name":secret_name,"namespace":config.namespace,"labels":{"artifactory.dev/execution":e.id.to_string()},"ownerReferences":[{"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","name":name,"uid":build.uid(),"controller":true,"blockOwnerDeletion":false}]},"immutable":true,"stringData":materials}),
    )?;
    match secrets.create(&PostParams::default(), &desired).await {
        Ok(_) => (),
        Err(kube::Error::Api(error)) if error.code == 409 => {
            let existing = secrets.get(&secret_name).await?;
            ensure!(
                existing
                    .owner_references()
                    .iter()
                    .any(|r| Some(&r.uid) == build.metadata.uid.as_ref()),
                "task Secret owner conflict"
            );
            for (key, value) in &materials {
                ensure!(
                    existing
                        .data
                        .as_ref()
                        .and_then(|d| d.get(key))
                        .is_some_and(|v| v.0 == value.as_bytes()),
                    "task Secret content changed"
                );
            }
        }
        Err(e) => return Err(e.into()),
    }
    if build.data["status"]["phase"] == "Failed" {
        state.store.finish_image(e.id, "failed").await?;
    } else if build.data["status"]["phase"] == "Succeeded" {
        let current = api.get(&name).await?;
        verify_build(&current, &desired_build)?;
        ensure!(
            current.uid().as_deref() == Some(&uid),
            "ImageBuild UID changed"
        );
        ensure!(
            current.data["status"]["phase"] == "Succeeded"
                && current
                    .metadata
                    .generation
                    .is_some_and(|generation| generation > 0
                        && current.data["status"]["observedGeneration"].as_i64()
                            == Some(generation)),
            "ImageBuild success observation is stale"
        );
        state.store.complete_image(e.id, &uid).await?;
    }
    Ok(())
}
fn verify_build(build: &DynamicObject, desired: &DynamicObject) -> Result<()> {
    ensure!(
        build.labels().get("artifactory.dev/execution")
            == desired.labels().get("artifactory.dev/execution")
            && build.data["spec"] == desired.data["spec"],
        "ImageBuild execution context conflict"
    );
    Ok(())
}
pub async fn tick(state: &AppState, config: &Config, client: &Client) -> Result<()> {
    for id in state.store.image_pending().await? {
        let e = state.store.image_execution(id).await?;
        if let Err(error) = deliver(state, config, client, &e).await {
            tracing::warn!(%id,%error,"image delivery deferred");
        }
    }
    for job in state.store.image_candidates().await? {
        let id = Uuid::new_v4();
        let build_token = crypto::token();
        let publish_token = crypto::token();
        if !state
            .store
            .claim_image(
                job,
                id,
                &crypto::hash(&build_token),
                &crypto::hash(&publish_token),
            )
            .await?
        {
            continue;
        }
        let result = async {
            let e = state.store.image_execution(id).await?;
            let build = tasks::build(state, config, &e, &build_token).await?;
            let publish = tasks::publication(state, config, &e, &publish_token).await?;
            let bytes = serialize_materials(&build, &publish)?;
            let envelope = crypto::seal_context(&state.settings.master_key, &bytes, id.as_bytes())?;
            state.store.save_image_task(id, &envelope).await?;
            let execution = state.store.image_execution(id).await?;
            if let Err(error) = deliver(state, config, client, &execution).await {
                tracing::warn!(%id,%error,"image delivery deferred");
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(%id,%error,"image preparation failed");
            state.store.finish_image(id, "failed").await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reused_crd_must_keep_the_complete_execution_context() {
        let desired: DynamicObject = serde_json::from_value(json!({"apiVersion":"images.artifactory.dev/v1alpha1","kind":"ImageBuild","metadata":{"name":"test","labels":{"artifactory.dev/execution":"execution"}},"spec":{"executionId":"execution","taskSecret":"task","platforms":["linux/amd64"],"timeoutSeconds":1200,"cancelRequested":false}})).unwrap();
        assert!(verify_build(&desired, &desired).is_ok());
        for (field, value) in [
            ("taskSecret", json!("other")),
            ("platforms", json!(["linux/arm64"])),
            ("timeoutSeconds", json!(1)),
            ("cancelRequested", json!(true)),
        ] {
            let mut changed = desired.clone();
            changed.data["spec"][field] = value;
            assert!(verify_build(&changed, &desired).is_err(), "{field}");
        }
    }
}
