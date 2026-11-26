//! [INPUT]: 真实mTLS Service、隔离Timescale库与原始清单队列。
//! [OUTPUT]: 验证真实身份冲突不覆盖入库，持久隔离后刷新交付。
//! [POS]: 永久冲突的存储与发送组合验收；不代表原生服务验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
mod support;
#[path = "support/telemetry_gate.rs"]
mod telemetry_gate;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::{
    client,
    config::{Config, atomic_bytes, atomic_json},
    telemetry,
};
use artifactory_emissary_protocol::{
    InventoryObservation, telemetry::Inventory, telemetry_client::TelemetryClient,
};
use prost::Message;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicit loopback ARTIFACTORY_TEST_DATABASE_URL"]
async fn real_inventory_conflict_isolated_across_reopen_and_valid_refresh_commits() {
    let proof = durable_receiver::isolated(Duration::from_secs(75), |url| async move {
        exercise(&url).await
    })
    .await
    .expect("actual inventory conflict acceptance failed");
    if let Some(path) = std::env::var_os("ARTIFACTORY_INVENTORY_CONFLICT_EVIDENCE") {
        atomic_json(Path::new(&path), &proof).expect("sanitized conflict evidence write failed");
    }
}

async fn exercise(url: &str) -> Result<Value> {
    let root = tempfile::tempdir()?;
    let mut fixture = support::Fixture::start(root.path(), &[&rustls::version::TLS13]).await;
    let startup = Uuid::new_v4();
    let receiver = durable_receiver::open(
        url,
        &fixture.cfg,
        &fixture.tls().client_hash(),
        &BTreeSet::from([startup]),
        startup,
    )
    .await?;
    let mut gate =
        telemetry_gate::TelemetryGate::start(receiver.service, fixture.tls().server_tls()).await?;
    gate.open();
    fixture.cfg.telemetry_endpoint = gate.endpoint.clone();
    fixture.cfg.endpoint = gate.endpoint.clone();
    let mut rpc = TelemetryClient::new(client::dial(&fixture.cfg, false).await?);
    let inventory = Inventory::default();
    let occupied = InventoryObservation::new(startup, 0, chrono::Utc::now(), 0, &inventory)?;
    let ack = rpc.upload_inventory(occupied.clone()).await?.into_inner();
    ensure!(
        occupied.matches_ack(&ack),
        "initial real inventory ACK differs"
    );
    let conflict = InventoryObservation::new(startup, 0, chrono::Utc::now(), 1, &inventory)?;
    let refresh = InventoryObservation::new(startup, 1, chrono::Utc::now(), 2, &inventory)?;
    let queue = fixture.cfg.root.join("telemetry/inventories");
    let conflict_path = queue.join(format!("{}.inventory", conflict.observation_id));
    let refresh_path = queue.join(format!("{}.inventory", refresh.observation_id));
    let conflict_bytes = conflict.encode_to_vec();
    atomic_bytes(&conflict_path, &conflict_bytes, false)?;
    atomic_bytes(&refresh_path, &refresh.encode_to_vec(), false)?;
    atomic_json(
        &queue.join("startup.json"),
        &json!({"startup_id":startup,"observation_id":conflict.observation_id,"acknowledged":false}),
    )?;
    let checkpoint = fs::read(queue.join("startup.json"))?;
    let mut sender = Some(telemetry::Handle::start(
        fixture.cfg.clone(),
        startup,
        inventory.clone(),
    ));
    let result = delivery(
        &fixture.cfg,
        startup,
        &inventory,
        &conflict,
        &refresh,
        &conflict_path,
        &conflict_bytes,
        &checkpoint,
        &receiver.pool,
        &gate,
        &mut sender,
    )
    .await;
    let stopped = if let Some(sender) = sender.take() {
        sender.shutdown().await
    } else {
        Ok(())
    };
    let closed = gate.close().await;
    result?;
    stopped?;
    closed?;
    durable_receiver::verify_database(
        &receiver.pool,
        Uuid::parse_str(&fixture.cfg.emissary_id)?,
        &[],
        &[
            (root.path().into(), occupied.encode_to_vec(), occupied),
            (refresh_path, refresh.encode_to_vec(), refresh),
        ],
    )
    .await?;
    let count: i64 = sql::scalar(
        &mut receiver.pool.clone(),
        "SELECT count(*) FROM inventory_deliveries WHERE node_id=$1 AND observation_id=$2",
        params![
            Uuid::parse_str(&fixture.cfg.emissary_id)?,
            Uuid::parse_str(&conflict.observation_id)?
        ],
    )
    .await?;
    ensure!(count == 0, "conflict unexpectedly committed");
    Ok(
        json!({"passed":true,"actual_mtls_service_store":true,"conflict_generated_by_real_store":true,
        "source_and_startup_checkpoint_preserved":true,"valid_refresh_matching_ack":true,
        "conflict_uploads_across_reopen":gate.inventory_attempts(&conflict.observation_id)?,
        "native_service_executed":false}),
    )
}

#[allow(clippy::too_many_arguments)]
async fn delivery(
    cfg: &Config,
    startup: Uuid,
    inventory: &Inventory,
    conflict: &InventoryObservation,
    refresh: &InventoryObservation,
    source: &Path,
    original: &[u8],
    checkpoint: &[u8],
    pool: &toasty::Db,
    gate: &telemetry_gate::TelemetryGate,
    sender: &mut Option<telemetry::Handle>,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let metrics = sender
                .as_ref()
                .context("sender missing")?
                .snapshot()
                .context("snapshot missing")?
                .metrics;
            if metrics["telemetry"]["inventory_conflicts_total"] == 1
                && metrics["telemetry"]["last_inventory_ack_at"].is_string()
                && !source
                    .with_file_name(format!("{}.inventory", refresh.observation_id))
                    .exists()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("valid inventory did not receive real transaction ACK")??;
    sender.take().context("sender missing")?.shutdown().await?;
    ensure!(
        fs::read(source)? == original
            && fs::read(source.with_file_name("startup.json"))? == checkpoint,
        "permanent conflict changed original source or checkpoint"
    );
    ensure!(
        gate.inventory_attempts(&conflict.observation_id)? == 1
            && gate.inventory_attempts(&refresh.observation_id)? == 1,
        "unexpected conflict or refresh request count"
    );
    let latest: Uuid = sql::scalar(
        &mut pool.clone(),
        "SELECT observation_id FROM startup_inventories WHERE node_id=$1 AND startup_id=$2",
        params![Uuid::parse_str(&cfg.emissary_id)?, startup],
    )
    .await?;
    ensure!(
        latest.to_string() == refresh.observation_id,
        "current startup inventory did not advance"
    );
    *sender = Some(telemetry::Handle::start(
        cfg.clone(),
        startup,
        inventory.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let metrics = sender
                .as_ref()
                .context("sender missing")?
                .snapshot()
                .context("snapshot missing")?
                .metrics;
            if metrics["telemetry"]["inventory_state"] == "id_content_conflict" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("reopened sender lost persistent conflict state")??;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let metrics = sender
        .as_ref()
        .context("sender missing")?
        .snapshot()
        .context("snapshot missing")?
        .metrics;
    ensure!(
        metrics["telemetry"]["inventory_conflicts_total"] == 1
            && metrics["telemetry"]["last_inventory_ack_at"].is_null(),
        "reopen fabricated ACK or conflict count"
    );
    sender.take().context("sender missing")?.shutdown().await?;
    ensure!(
        gate.inventory_attempts(&conflict.observation_id)? == 1,
        "conflict was replayed after reopen"
    );
    ensure!(
        fs::read(source)? == original
            && fs::read(source.with_file_name("startup.json"))? == checkpoint,
        "reopen changed conflict source or checkpoint"
    );
    Ok(())
}
