//! [INPUT]: 真实Runner、共享mTLS、传输门和隔离Control/Telemetry存储。
//! [OUTPUT]: 验证同配置重启、真实Hello登记、原队列保留和事务ACK删除。
//! [POS]: 普通Runner恢复预演；不替代原生服务维护与回退验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
#[path = "native_service/queues.rs"]
mod queues;
#[path = "support/runner_task.rs"]
mod runner_task;
mod support;
use runner_task::Running;
#[path = "support/telemetry_gate.rs"]
mod telemetry_gate;
#[path = "support/telemetry_sources.rs"]
mod telemetry_sources;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary_protocol::nodes::MetadataSnapshot;
use durable_receiver::{FENCE, ReceiverStore};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, sync::Arc, time::Duration};
use telemetry_gate::TelemetryGate;
use telemetry_sources::Sources;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicit isolated local ARTIFACTORY_TEST_DATABASE_URL; no native services"]
async fn real_runner_restart_retains_sources_until_current_store_transaction_ack() {
    let proof = durable_receiver::isolated(Duration::from_secs(240), |url| async move {
        exercise(&url).await
    })
    .await
    .expect("real runner recovery regression failed");
    if let Some(path) = std::env::var_os("ARTIFACTORY_RUNNER_RECOVERY_EVIDENCE") {
        artifactory_emissary::config::atomic_json(Path::new(&path), &proof)
            .expect("sanitized runner recovery evidence write failed");
    }
}

async fn exercise(url: &str) -> Result<Value> {
    let root = tempfile::tempdir()?;
    let mut fixture = support::Fixture::start(root.path(), &[&rustls::version::TLS13]).await;
    fs::create_dir_all(&fixture.cfg.root)?;
    let mut source = Sources::prepare(&fixture.cfg).await?;
    let source_proof = queues::evidence(&source.files)?;
    let current = *source.startups.first().context("source startup missing")?;
    let receiver = Arc::new(
        durable_receiver::open(
            url,
            &fixture.cfg,
            &fixture.tls().client_hash(),
            &source.startups,
            current,
        )
        .await?,
    );
    let node = Uuid::parse_str(&fixture.cfg.emissary_id)?;
    let registration = receiver.clone();
    fixture.set_hello_hook(Arc::new(move |hello| {
        let registration = registration.clone();
        Box::pin(async move {
            let payload: Value = serde_json::from_str(&hello.payload_json)
                .map_err(|_| tonic::Status::invalid_argument("invalid hello"))?;
            let snapshot: MetadataSnapshot = serde_json::from_value(payload["startup"].clone())
                .map_err(|_| tonic::Status::invalid_argument("invalid startup"))?;
            registration
                .register_startup(node, &snapshot)
                .await
                .map_err(|_| tonic::Status::internal("startup transaction failed"))
        })
    }));
    let mut gate =
        TelemetryGate::start(receiver.service.clone(), fixture.tls().server_tls()).await?;
    fixture.cfg.telemetry_endpoint = gate.endpoint.clone();
    let mut running = Running::new();
    let result = recovery(&mut fixture, &mut source, &receiver, &gate, &mut running).await;
    let stopped = running.close().await;
    let closed = gate.close().await;
    let startups = result?;
    stopped?;
    closed?;
    Ok(json!({
        "passed":true,"boundary":"ordinary real Runner restart and current mTLS durable receiver",
        "native_service_executed":false,"real_runner_startups":startups,
        "fixed_configuration":true,"source_bytes_retained_before_commit":true,
        "business_ack_removed_sources":true,"transaction_fence_observed":true,
        "terminal_operation_unchanged":true,"source_batches":source.batches.len(),
        "source_inventories":source.inventories.len(),"queue_records":source_proof,
        "real_business_migrations":true,"authorization_acl_proven":false,
    }))
}

async fn recovery(
    fixture: &mut support::Fixture,
    source: &mut Sources,
    receiver: &ReceiverStore,
    gate: &TelemetryGate,
    running: &mut Running,
) -> Result<usize> {
    source.pin(&fixture.cfg, gate).await?;
    let configuration = serde_json::to_value(&fixture.cfg)?;
    let mut seen = BTreeSet::new();
    for _ in 0..2 {
        running.start(fixture.cfg.clone())?;
        let connection = fixture.connection().await;
        let hello: Value = serde_json::from_str(&connection.hello.payload_json)?;
        let snapshot: MetadataSnapshot = serde_json::from_value(hello["startup"].clone())?;
        snapshot.validate()?;
        ensure!(
            seen.insert(snapshot.startup_id),
            "restart reused startup identity"
        );
        let registered: String = sql::scalar(
            &mut receiver.business.clone(),
            "SELECT digest FROM node_startups WHERE node_id=$1 AND startup_id=$2",
            params![
                Uuid::parse_str(&fixture.cfg.emissary_id)?,
                snapshot.startup_id
            ],
        )
        .await?;
        ensure!(
            registered == snapshot.digest()?,
            "hello published before startup commit"
        );
        source.retained()?;
        ensure!(
            serde_json::to_value(&fixture.cfg)? == configuration,
            "configuration changed during restart"
        );
        if seen.len() == 1 {
            running.close().await?;
        }
    }
    let receipts: i64 = sql::scalar(&mut receiver.pool.clone(),
        "SELECT (SELECT count(*) FROM telemetry_batches)+(SELECT count(*) FROM inventory_deliveries)", params![]).await?;
    ensure!(receipts == 0, "closed gate returned a successful receipt");
    let mut db = receiver.pool.clone();
    let mut held = db.transaction().await?;
    sql::execute(&mut held, FENCE, params![]).await?;
    gate.open();
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let blocked: i64 = sql::scalar(&mut receiver.pool.clone(),
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'", params![]).await?;
            if blocked >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    }).await.context("actual runner batch/inventory transactions did not reach fence")??;
    source.retained()?;
    let receipts: i64 = sql::scalar(&mut receiver.pool.clone(),
        "SELECT (SELECT count(*) FROM telemetry_batches)+(SELECT count(*) FROM inventory_deliveries)", params![]).await?;
    ensure!(receipts == 0, "receipt committed before real fence release");
    held.rollback().await?;
    // 当前启动优先且清单真实限流；预算容纳原清单与两次新启动，不注入限流token。
    tokio::time::timeout(Duration::from_secs(150), async {
        while !source.removed()? {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("actual running Runner did not receive all original business ACKs")??;
    durable_receiver::verify_database(
        &receiver.pool,
        Uuid::parse_str(&fixture.cfg.emissary_id)?,
        &source.batches,
        &source.inventories,
    )
    .await?;
    source.journal_retained()?;
    Ok(seen.len())
}
