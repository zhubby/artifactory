//! [INPUT]: 依赖工作区租约、私有对象存储与服务清理接口。
//! [OUTPUT]: 按持久化游标删除对象并清扫原生及镜像日志前缀。
//! [POS]: 所有外部清理 I/O 在数据库锁外执行。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{state::AppState, store::lifecycle::Lease};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

pub async fn tick(state: &AppState) -> Result<()> {
    let owner = Uuid::new_v4();
    for _ in 0..8 {
        let Some(lease) = state.store.lease_lifecycle(owner).await? else {
            break;
        };
        // Losing ownership cancels I/O; the replacement repeats an idempotent step.
        let result = async {
            let work = step(state, &lease);
            tokio::pin!(work);
            loop {
                tokio::select! {
                    result = &mut work => return result,
                    _ = tokio::time::sleep(Duration::from_secs(30)) => {
                        if !state.store.renew_lifecycle(&lease).await? { return Ok(()) }
                    }
                }
            }
        }
        .await;
        if result.is_err() {
            state.store.fail_lifecycle(&lease).await?;
            // Transport errors may contain credentials or signed URLs.
            tracing::warn!(operation=%lease.id,phase=%lease.phase,"workspace cleanup step failed");
        }
    }
    Ok(())
}

enum ServiceStep {
    Revoke,
    Runtime(Uuid),
    Telemetry(Vec<Uuid>, Vec<Uuid>),
}

fn service_steps(lease: &Lease, telemetry: bool) -> Result<Vec<ServiceStep>> {
    let mut steps = vec![ServiceStep::Revoke];
    let projects: Vec<Uuid> = serde_json::from_value(lease.manifest["projects"].clone())?;
    for project in projects {
        if lease.manifest["runtime"] == true {
            steps.push(ServiceStep::Runtime(project));
        }
    }
    if telemetry {
        let nodes: Vec<Uuid> = serde_json::from_value(lease.manifest["nodes"].clone())?;
        let attempts: Vec<Uuid> = serde_json::from_value(lease.manifest["attempts"].clone())?;
        steps.extend(
            nodes
                .chunks(512)
                .map(|batch| ServiceStep::Telemetry(batch.to_vec(), vec![])),
        );
        steps.extend(
            attempts
                .chunks(10000)
                .map(|batch| ServiceStep::Telemetry(vec![], batch.to_vec())),
        );
    }
    Ok(steps)
}

async fn purge_service(state: &AppState, lease: &Lease, service: &ServiceStep) -> Result<()> {
    match service {
        ServiceStep::Revoke => {
            state.store.revoke_lifecycle_identities(lease).await?;
            let nodes: Vec<Uuid> = serde_json::from_value(lease.manifest["nodes"].clone())?;
            let mut connections = state.connections.write().await;
            for node in nodes {
                connections.remove(&node);
            }
        }
        ServiceStep::Runtime(project) => {
            let _: Value = crate::runtime_bridge::runtime()?
                .post("/v1/purge", &json!({"project_id":project}))
                .await?;
        }
        ServiceStep::Telemetry(nodes, attempts) => {
            let cfg = state
                .settings
                .telemetry
                .as_ref()
                .context("telemetry configuration missing")?;
            let ca = tokio::fs::read(state.settings.pki_dir.join("ca.pem")).await?;
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .add_root_certificate(reqwest::Certificate::from_pem(&ca)?)
                .timeout(Duration::from_secs(90))
                .build()?;
            http.post(format!("{}/v1/purge", cfg.query_url.trim_end_matches('/')))
                .bearer_auth(&cfg.token)
                .json(&json!({"nodes":nodes,"attempts":attempts}))
                .send()
                .await?
                .error_for_status()?;
        }
    }
    Ok(())
}

async fn step(state: &AppState, lease: &Lease) -> Result<()> {
    match lease.phase.as_str() {
        "draining" => state.store.advance_lifecycle_drain(lease).await?,
        "services" => {
            let steps = service_steps(lease, state.settings.telemetry.is_some())?;
            let cursor = usize::try_from(lease.object_cursor)?;
            if let Some(service) = steps.get(cursor) {
                purge_service(state, lease, service).await?;
                state
                    .store
                    .advance_lifecycle_cursor(lease, lease.object_cursor + 1)
                    .await?;
            } else {
                state.store.checkpoint_lifecycle(lease, "objects").await?;
            }
        }
        "objects" => {
            if !state.store.lifecycle_uploads_expired(lease).await? {
                state.store.defer_lifecycle(lease).await?;
                return Ok(());
            }
            let objects = lease.manifest["objects"]
                .as_array()
                .context("missing object inventory")?;
            let attempts = lease.manifest["attempts"]
                .as_array()
                .context("missing attempt inventory")?;
            let ota = lease.manifest["ota"]
                .as_array()
                .context("missing release inventory")?;
            let cursor = usize::try_from(lease.object_cursor)?;
            let object_batches = objects.len().div_ceil(32);
            let image_projects: Vec<Uuid> =
                serde_json::from_value(lease.manifest["image_log_projects"].clone())?;
            if cursor < object_batches {
                for key in objects.iter().skip(cursor * 32).take(32) {
                    state
                        .storage
                        .delete(key.as_str().context("invalid object key")?)
                        .await?;
                }
            } else if cursor < object_batches + attempts.len() {
                let attempt: Uuid =
                    serde_json::from_value(attempts[cursor - object_batches].clone())?;
                state
                    .storage
                    .delete_prefix(&format!("logs/{attempt}/"))
                    .await?;
            } else if cursor < object_batches + attempts.len() + image_projects.len() {
                let project = image_projects[cursor - object_batches - attempts.len()];
                state
                    .storage
                    .delete_prefix(&format!("{}/{project}/image-logs/", lease.tenant_id))
                    .await?;
            } else if cursor < object_batches + attempts.len() + image_projects.len() + ota.len() {
                let release = &ota[cursor - object_batches - attempts.len() - image_projects.len()];
                let hash = release["hash"].as_str().context("missing asset identity")?;
                ensure!(
                    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid asset identity"
                );
                match state.store.claim_ota_cleanup(hash, lease).await? {
                    Some(true) => {
                        state
                            .storage
                            .delete_prefix(&format!("emissary-releases/{hash}/"))
                            .await?
                    }
                    Some(false) => {}
                    None => {
                        state.store.defer_lifecycle(lease).await?;
                        return Ok(());
                    }
                }
            } else {
                state.store.checkpoint_lifecycle(lease, "control").await?;
                return Ok(());
            }
            state
                .store
                .advance_lifecycle_cursor(lease, lease.object_cursor + 1)
                .await?;
        }
        "control" => state.store.purge_lifecycle_control(lease).await?,
        _ => anyhow::bail!("unknown cleanup phase"),
    }
    Ok(())
}
