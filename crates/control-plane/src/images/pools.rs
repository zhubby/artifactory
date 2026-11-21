//! [INPUT]: 依赖部署默认池、构建冻结上下文和 Kubernetes BuilderPool。
//! [OUTPUT]: 补全缺省镜像池，区分可修正缺池与可重试查询故障。
//! [POS]: 镜像计划的准入边界；原生设备池不参与镜像调度。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::execution_plan::ExecutionPlan;
use anyhow::{Context, Result, ensure};
use artifactory_image_protocol::{DEFAULT_BUILDER_POOL, dns_label};
use kube::{
    Api, Client,
    api::{ApiResource, DynamicObject},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::Instant;

pub(crate) const IMAGE_POOL_POLICY: &str = "image_builder_pool is the default image.pool and names a Kubernetes BuilderPool. Use it unless the user explicitly requests another existing BuilderPool. nodes[].pool_id and project_context.pool_ids authorize native devices only; never copy those UUIDs into image.pool. Image platforms are independent of native devices.";

#[derive(Debug, thiserror::Error)]
pub(crate) enum PoolValidationError {
    #[error(
        "image builder pool '{0}' does not exist; choose an existing Kubernetes BuilderPool, not a native node pool"
    )]
    MissingPool(String),
    #[error(
        "image builder pool lookup failed (Kubernetes HTTP {0}); check Control access to BuilderPool"
    )]
    Lookup(u16),
    #[error("image builder pool validation unavailable; retry after Kubernetes recovers")]
    Unavailable,
    #[error("image builder pool validation timed out; retry after Kubernetes recovers")]
    TimedOut,
}

fn pool_name(value: Option<&str>) -> Result<String> {
    let value = value.unwrap_or(DEFAULT_BUILDER_POOL);
    ensure!(dns_label(value), "invalid image builder pool name");
    Ok(value.into())
}

pub(crate) fn configured_pool() -> Result<String> {
    match std::env::var("ARTIFACTORY_IMAGE_POOL") {
        Ok(value) => pool_name(Some(&value)),
        Err(std::env::VarError::NotPresent) => pool_name(None),
        Err(error) => Err(error).context("invalid ARTIFACTORY_IMAGE_POOL"),
    }
}

pub(crate) fn normalize_plan_pools(value: &mut Value, context: &Value) -> Result<()> {
    let Some(targets) = value
        .pointer_mut("/recipe/targets")
        .and_then(Value::as_object_mut)
    else {
        return Ok(());
    };
    for target in targets
        .values_mut()
        .filter(|target| target["executor"] == "buildkit")
    {
        let Some(image) = target.get_mut("image").and_then(Value::as_object_mut) else {
            continue;
        };
        if !image.contains_key("pool") {
            let frozen = match context.get("image_builder_pool") {
                None => None,
                Some(value) => Some(
                    value
                        .as_str()
                        .context("invalid frozen image builder pool")?,
                ),
            };
            image.insert("pool".into(), json!(pool_name(frozen)?));
        }
    }
    Ok(())
}

fn plan_pools(plan: &ExecutionPlan) -> BTreeSet<&str> {
    plan.recipe
        .targets
        .values()
        .filter_map(|target| target.image.as_ref().map(|image| image.pool.as_str()))
        .collect()
}

pub(crate) async fn validate_plan_pools(
    plan: &ExecutionPlan,
) -> std::result::Result<(), PoolValidationError> {
    if plan_pools(plan).is_empty() {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let namespace = std::env::var("ARTIFACTORY_IMAGE_NAMESPACE")
        .ok()
        .filter(|namespace| dns_label(namespace))
        .ok_or(PoolValidationError::Unavailable)?;
    let client = tokio::time::timeout_at(deadline, Client::try_default())
        .await
        .map_err(|_| PoolValidationError::TimedOut)?
        .map_err(|_| PoolValidationError::Unavailable)?;
    validate_with(&client, &namespace, plan, deadline).await
}

async fn validate_with(
    client: &Client,
    namespace: &str,
    plan: &ExecutionPlan,
    deadline: Instant,
) -> std::result::Result<(), PoolValidationError> {
    let resource = ApiResource {
        group: "images.artifactory.dev".into(),
        version: "v1alpha1".into(),
        api_version: "images.artifactory.dev/v1alpha1".into(),
        kind: "BuilderPool".into(),
        plural: "builderpools".into(),
    };
    let api = Api::<DynamicObject>::namespaced_with(client.clone(), namespace, &resource);
    tokio::time::timeout_at(deadline, async {
        for name in plan_pools(plan) {
            match api.get(name).await {
                Ok(_) => (),
                Err(kube::Error::Api(error)) if error.code == 404 => {
                    return Err(PoolValidationError::MissingPool(name.into()));
                }
                Err(kube::Error::Api(error)) => {
                    return Err(PoolValidationError::Lookup(error.code));
                }
                Err(_) => return Err(PoolValidationError::Unavailable),
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| PoolValidationError::TimedOut)?
}

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
