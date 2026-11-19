//! [INPUT]: 依赖租户、项目和业务状态。
//! [OUTPUT]: 启动业务、规划运行时与调度服务。
//! [POS]: 控制平面的业务边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_control_plane::{
    api, auth,
    grpc::EmissaryService,
    protocol::{control_server::ControlServer, enrollment_server::EnrollmentServer},
    scheduler,
    settings::Settings,
    state::AppState,
    storage::Storage,
};
use clap::{Parser, Subcommand};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

#[derive(Parser)]
#[command(version)]
struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Serve,
    Migrate,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "artifactory_control_plane=info,tower_http=info".into()),
        )
        .init();
    let arguments = Arguments::parse();
    let settings = Settings::from_env()?;
    tokio::fs::create_dir_all(&settings.data_dir).await?;
    let store = artifactory_control_plane::store::ControlStore::connect_with_capacity(
        &settings.database_url,
        // The notification listener owns one dedicated PostgreSQL session.
        settings.control.database_connections - 1,
    )
    .await?;
    if matches!(arguments.command, Some(Command::Migrate)) {
        return Ok(());
    }
    if store.users_empty().await? {
        let username =
            std::env::var("ARTIFACTORY_BOOTSTRAP_USERNAME").unwrap_or_else(|_| "admin".into());
        let password = std::env::var("ARTIFACTORY_BOOTSTRAP_PASSWORD")
            .context("ARTIFACTORY_BOOTSTRAP_PASSWORD is required for the first administrator")?;
        ensure!(
            !username.is_empty() && username.len() <= 100,
            "invalid bootstrap username"
        );
        let hash = auth::password_hash(&password)?;
        store.bootstrap_admin(username, hash).await?;
    }
    let storage = Storage::new(&settings)?;
    storage.initialize().await?;
    let state = AppState {
        coordination: Arc::new(artifactory_control_plane::coordination::Coordination::new(
            settings.control.database_connections,
        )),
        store,
        settings: Arc::new(settings.clone()),
        storage: Arc::new(storage),
        connections: Arc::new(RwLock::new(HashMap::new())),
    };
    scheduler::start(state.clone());
    artifactory_control_plane::images::start(state.clone())?;
    let runtime = if std::env::var_os("ARTIFACTORY_RUNTIME_URL").is_some() {
        let database_url =
            artifactory_runtime_protocol::service::required("ARTIFACTORY_RUNTIME_DATABASE_URL")?;
        let runtime = artifactory_planner_runtime::server::from_env(&database_url).await?;
        let background = runtime.clone();
        let stop = state.coordination.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.stopped() => break,
                    result = artifactory_planner_runtime::server::maintain(&background) => {
                        if let Err(error) = result {
                            tracing::warn!(%error, "runtime reconciliation failed");
                        }
                    }
                }
                tokio::select! {
                    _ = stop.stopped() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                }
            }
        });
        Some(runtime)
    } else {
        None
    };
    let lifecycle = state.clone();
    let drain_seconds = settings.control.drain_seconds;
    tokio::spawn(async move {
        shutdown().await;
        if let Err(error) = artifactory_control_plane::coordination::drain(&lifecycle).await {
            tracing::warn!(%error,"could not clear draining replica ownership");
        }
        tokio::time::sleep(std::time::Duration::from_secs(drain_seconds)).await;
        lifecycle.coordination.stop();
    });
    let grpc_stop = state.coordination.clone();
    let http_stop = state.coordination.clone();
    let server_identity = Identity::from_pem(
        tokio::fs::read(settings.pki_dir.join("server.pem")).await?,
        tokio::fs::read(settings.pki_dir.join("server.key")).await?,
    );
    let ca = Certificate::from_pem(tokio::fs::read(settings.pki_dir.join("ca.pem")).await?);
    let tls = ServerTlsConfig::new()
        .identity(server_identity)
        .client_ca_root(ca)
        .client_auth_optional(true);
    let emissary = EmissaryService(state.clone());
    let grpc = Server::builder()
        .tls_config(tls)?
        .add_service(EnrollmentServer::new(emissary.clone()).max_decoding_message_size(64 * 1024))
        .add_service(
            ControlServer::new(emissary)
                .max_decoding_message_size(128 * 1024)
                .max_encoding_message_size(8 * 1024 * 1024),
        )
        .serve_with_shutdown(settings.grpc_bind.parse()?, async move {
            grpc_stop.stopped().await
        });
    let listener = tokio::net::TcpListener::bind(&settings.http_bind).await?;
    tracing::info!(http=%settings.http_bind,grpc=%settings.grpc_bind,"artifactory control plane ready");
    let mut router = api::router(state);
    if let Some(runtime) = runtime {
        let token = artifactory_runtime_protocol::service::required("ARTIFACTORY_SERVICE_TOKEN")?;
        router = api::mount_planner(router, runtime, token);
    }
    let http = axum::serve(listener, router)
        .with_graceful_shutdown(async move { http_stop.stopped().await });
    tokio::try_join!(async { http.await.map_err(anyhow::Error::from) }, async {
        grpc.await.map_err(anyhow::Error::from)
    })?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
