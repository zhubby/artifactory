//! Compatibility owner for pre-existing source snapshots and the poll planning backend.
use crate::{crypto, source_cache, state::AppState};
use anyhow::Result;
use artifactory_runtime_protocol::service::{self, Client};
use axum::{Json, Router, extract::State, routing::post};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

fn token(state: &AppState) -> String {
    let mut bytes = b"artifactory:local-source:v1:".to_vec();
    bytes.extend_from_slice(&state.settings.master_key);
    crypto::hash(bytes)
}

pub(crate) async fn client(state: &AppState) -> Result<Option<&Client>> {
    let Some(origin) = &state.settings.local_source_url else {
        return Ok(None);
    };
    let client = state
        .source_client
        .get_or_try_init(|| async {
            let mut client = Client::new(origin.clone(), token(state))?;
            // Trust the existing device CA and retain one HTTP pool per process.
            let ca = tokio::fs::read(state.settings.pki_dir.join("ca.pem")).await?;
            client.http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(std::time::Duration::from_secs(5))
                .add_root_certificate(reqwest::Certificate::from_pem(&ca)?)
                .build()?;
            Ok::<_, anyhow::Error>(client)
        })
        .await?;
    Ok(Some(client))
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Purge {
        project_id: Uuid,
    },
    Ensure {
        snapshot_id: Uuid,
    },
    Evidence {
        snapshot_id: Uuid,
        paths: Vec<String>,
    },
    Inspect {
        snapshot_id: Uuid,
        name: String,
        arguments: Value,
    },
    Refs {
        project_id: Uuid,
    },
}

/// Serve source bytes directly and expose authenticated metadata operations to replicas.
pub fn router(state: AppState) -> Router {
    let protected = service::protected(
        Router::new()
            .route("/local-source", post(operation))
            .with_state(state.clone()),
        token(&state),
    );
    crate::api::health_routes()
        .merge(crate::api::source_routes())
        .with_state(state)
        .nest("/internal", protected)
}

async fn operation(
    State(state): State<AppState>,
    Json(operation): Json<Operation>,
) -> artifactory_runtime_protocol::error::Result<Json<Value>> {
    let result = match operation {
        Operation::Purge { project_id } => {
            purge_project(&state, project_id).await?;
            json!({"ok":true})
        }
        Operation::Ensure { snapshot_id } => {
            let snapshot = source_cache::load(&state, snapshot_id).await?;
            source_cache::ensure_available(&state, &snapshot).await?;
            json!({"available":true})
        }
        Operation::Evidence { snapshot_id, paths } => {
            let snapshot = source_cache::load(&state, snapshot_id).await?;
            source_cache::validate_evidence(&state, &snapshot, &paths).await?;
            json!({"valid":true})
        }
        Operation::Inspect {
            snapshot_id,
            name,
            arguments,
        } => source_cache::inspect_tool(&state, snapshot_id, &name, &arguments).await?,
        Operation::Refs { project_id } => {
            state
                .store
                .source
                .capture_sources(project_id, || async {
                    let (url, credential) = state.store.source_refs_project(project_id).await?;
                    Ok(serde_json::to_value(
                        source_cache::list_refs(&state, &url, credential).await?,
                    )?)
                })
                .await?
        }
    };
    Ok(Json(result))
}

pub(crate) async fn purge_project(state: &AppState, project: Uuid) -> Result<()> {
    let inventory = state.store.source.prepare_purge(project).await?;
    if inventory["completed"] == true {
        return Ok(());
    }
    let core = artifactory_source_core::AppState {
        store: state.store.source.clone(),
        settings: std::sync::Arc::new(artifactory_source_core::Settings {
            data_dir: state.settings.data_dir.clone(),
            master_key: state.settings.master_key,
        }),
    };
    artifactory_source_core::store::purge_cache(&core, &inventory).await?;
    state.store.source.finish_purge(project, Vec::new()).await
}
