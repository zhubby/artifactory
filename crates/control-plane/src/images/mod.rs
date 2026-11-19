//! [INPUT]: 依赖镜像执行存储、源码快照、OCI 客户端和 Kubernetes。
//! [OUTPUT]: 提供镜像准入、启动校验、单轮调度与内容校验。
//! [POS]: 控制平面的镜像编排边界；独立于设备调度循环。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod dispatch;
pub mod registry;
mod release;
mod tasks;
use anyhow::{Context, Result, ensure};
use artifactory_image_protocol::RegistryLogin;
pub use dispatch::tick;
pub(crate) use release::publish as publish_release;
pub(crate) use release::stopped as release_stopped;
use std::path::PathBuf;
pub use tasks::verify_report;

pub const DISABLED_MESSAGE: &str = "image builds are disabled; configure ARTIFACTORY_IMAGE_NAMESPACE on the control plane before submitting BuildKit targets";
pub fn enabled() -> bool {
    std::env::var("ARTIFACTORY_IMAGE_NAMESPACE")
        .is_ok_and(|namespace| artifactory_image_protocol::dns_label(&namespace))
}
pub fn ensure_targets_enabled<'a>(
    targets: impl IntoIterator<Item = &'a crate::recipe::Target>,
    enabled: bool,
) -> Result<()> {
    ensure!(
        enabled
            || targets
                .into_iter()
                .all(|target| target.executor != "buildkit"),
        "{DISABLED_MESSAGE}"
    );
    Ok(())
}

#[derive(Clone)]
pub struct Config {
    pub namespace: String,
    pub registry_url: String,
    pub control_url: String,
    pub service: String,
    pub issuer: String,
    pub signing_key: PathBuf,
    pub signing_cert: PathBuf,
}
impl Config {
    pub fn load() -> Result<Self> {
        let required = |name| {
            std::env::var(name).with_context(|| format!("{name} is required for image builds"))
        };
        let config = Self {
            namespace: required("ARTIFACTORY_IMAGE_NAMESPACE")?,
            registry_url: required("ARTIFACTORY_REGISTRY_URL")?,
            control_url: required("ARTIFACTORY_IMAGE_CONTROL_URL")?,
            service: std::env::var("ARTIFACTORY_REGISTRY_SERVICE")
                .unwrap_or_else(|_| "artifactory-registry".into()),
            issuer: std::env::var("ARTIFACTORY_REGISTRY_ISSUER")
                .unwrap_or_else(|_| "artifactory".into()),
            signing_key: required("ARTIFACTORY_REGISTRY_SIGNING_KEY")?.into(),
            signing_cert: required("ARTIFACTORY_REGISTRY_SIGNING_CERT")?.into(),
        };
        ensure!(
            artifactory_image_protocol::dns_label(&config.namespace),
            "invalid image namespace"
        );
        let insecure = std::env::var("ARTIFACTORY_INSECURE_DEV").as_deref() == Ok("true");
        config.login(uuid::Uuid::nil(), "").validate(insecure)?;
        artifactory_service_transport::service::endpoint(&config.control_url)?;
        Ok(config)
    }
    pub fn login(&self, id: uuid::Uuid, token: &str) -> RegistryLogin {
        RegistryLogin {
            endpoint: self.registry_url.clone(),
            username: id.to_string(),
            password: token.into(),
        }
    }
}
pub fn start(state: crate::state::AppState) -> Result<()> {
    if std::env::var_os("ARTIFACTORY_IMAGE_NAMESPACE").is_none() {
        return Ok(());
    }
    let config = Config::load().context("image configuration rejected")?;
    let key = std::fs::read(&config.signing_key).context("reading registry signing key")?;
    let cert =
        std::fs::read(&config.signing_cert).context("reading registry signing certificate")?;
    registry::validate_signing_material(&key, &cert)?;
    tokio::spawn(async move {
        let client = loop {
            if state.coordination.is_draining() {
                return;
            }
            match kube::Client::try_default().await {
                Ok(client) => break client,
                Err(error) => {
                    tracing::warn!(%error,"image Kubernetes client initialization deferred")
                }
            }
            tokio::select! {_=state.coordination.stop.cancelled()=>return,_=tokio::time::sleep(std::time::Duration::from_secs(3))=>{}}
        };
        while !state.coordination.is_draining() {
            if let Err(error) = tick(&state, &config, &client).await {
                tracing::warn!(%error,"image dispatch deferred");
            }
            tokio::select! {_=state.coordination.stop.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_secs(3))=>{}}
        }
    });
    Ok(())
}
pub async fn save_publication(
    state: &crate::state::AppState,
    id: uuid::Uuid,
    token: &str,
) -> Result<()> {
    let config = Config::load()?;
    let execution = state.store.image_execution(id).await?;
    let task = tasks::publication(state, &config, &execution, token).await?;
    let bytes =
        serde_json::to_vec(&serde_json::json!({"publish.json":serde_json::to_string(&task)?}))?;
    let envelope = crate::crypto::seal_context(&state.settings.master_key, &bytes, id.as_bytes())?;
    state.store.save_image_task(id, &envelope).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_images_reject_mixed_targets_but_keep_native_selection() {
        let recipe=crate::recipe::Recipe::parse("version: 1\ntargets:\n  native:\n    os: linux\n    arch: amd64\n    steps: [{name: build, run: make}]\n    artifacts: [dist/app]\n  image:\n    os: linux\n    arch: amd64\n    executor: buildkit\n    image: {name: app}\n").unwrap();
        let error = ensure_targets_enabled(recipe.targets.values(), false).unwrap_err();
        assert_eq!(error.to_string(), DISABLED_MESSAGE);
        assert!(
            ensure_targets_enabled(
                recipe
                    .targets
                    .iter()
                    .filter(|(name, _)| name.as_str() == "native")
                    .map(|(_, target)| target),
                false
            )
            .is_ok()
        );
        assert!(ensure_targets_enabled(recipe.targets.values(), true).is_ok());
    }
}
