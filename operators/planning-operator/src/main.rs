//! [INPUT]: 依赖Kubernetes 身份、选主循环和 规划协调器 地址。
//! [OUTPUT]: 启动独立 Planning Operator 服务。
//! [POS]: Planning Operator 进程入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_planning_operator::{
    controller::{self, Context},
    leadership::Leadership,
};
use artifactory_service_transport::service;
use std::{sync::Arc, time::Duration};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let namespace = service::required("ARTIFACTORY_NAMESPACE")?;
    let client = kube::Client::try_default().await?;
    let leader = Leadership {
        api: kube::Api::namespaced(client.clone(), &namespace),
        identity: Arc::new(
            std::env::var("POD_UID").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string()),
        ),
        name: "artifactory-planning-operator".into(),
    };
    let ctx = Arc::new(Context {
        client,
        namespace,
        coordinator: service::Client::new(
            service::required("ARTIFACTORY_PLANNING_URL")?,
            service::required("ARTIFACTORY_SERVICE_TOKEN")?,
        )?,
        leader,
        source_admission: tokio::sync::Mutex::new(()),
    });
    let election = ctx.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = election.leader.renew().await {
                tracing::warn!(%error,"operator lease renewal failed");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
    let background = ctx.clone();
    tokio::spawn(async move {
        loop {
            if background.leader.ensure().await.is_ok()
                && let Err(error) = controller::dispatch(&background).await
            {
                tracing::warn!(%error,"operator dispatch failed");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
    tokio::join!(
        controller::controller(ctx.clone()),
        artifactory_planning_operator::source_tasks::controller(ctx)
    );
    Ok(())
}
