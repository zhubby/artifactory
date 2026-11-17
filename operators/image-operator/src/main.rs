//! [INPUT]: 依赖集群身份、独立 Lease 和命名空间。
//! [OUTPUT]: 启动 Image Operator 协调服务。
//! [POS]: 镜像控制器独立进程入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_image_operator::{
    controller::{self, Context},
    leadership::Leadership,
};
use std::{sync::Arc, time::Duration};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let namespace = std::env::var("ARTIFACTORY_NAMESPACE")?;
    let client = kube::Client::try_default().await?;
    let leader = Leadership {
        api: kube::Api::namespaced(client.clone(), &namespace),
        identity: Arc::new(
            std::env::var("POD_UID").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string()),
        ),
        name: "artifactory-image-operator".into(),
    };
    let ctx = Arc::new(Context {
        client,
        namespace,
        leader,
        admission: tokio::sync::Mutex::new(()),
    });
    let election = ctx.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = election.leader.renew().await {
                tracing::warn!(%error,"image leader renewal failed");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
    tokio::select! { _=controller::run(ctx)=>{}, _=tokio::signal::ctrl_c()=>{} }
    Ok(())
}
