//! [INPUT]: 真实发送队列、共享durable_receiver夹具与当前mTLS Service。
//! [OUTPUT]: 验证事务确认前保留原字节、确认后删除和原ID幂等重放。
//! [POS]: 普通进程重开与真实接收存储的组合验收，不代表原生维护重传。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
#[path = "native_service/queues.rs"]
mod queues;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::{config::Config, telemetry};
use artifactory_emissary_protocol::{
    InventoryObservation,
    telemetry::{Batch, Inventory},
    telemetry_server::TelemetryServer,
};
use durable_receiver::{FENCE, verify_database};
use prost::Message;
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated local ARTIFACTORY_TEST_DATABASE_URL via explicit database wrapper"]
async fn reopened_sender_waits_for_real_transaction_ack_and_replays_original_ids() {
    let proof = durable_receiver::isolated(Duration::from_secs(180), |url| async move {
        exercise(&url).await
    })
    .await
    .expect("durable receiver regression failed");
    if let Some(path) = std::env::var_os("ARTIFACTORY_DURABLE_RECEIVER_EVIDENCE") {
        artifactory_emissary::config::atomic_json(Path::new(&path), &proof)
            .expect("sanitized durable receiver evidence write failed");
    }
}

async fn exercise(url: &str) -> Result<Value> {
    let root = tempfile::tempdir()?;
    let mut cfg = Config {
        root: root.path().join("node"),
        emissary_id: Uuid::new_v4().to_string(),
        ..Default::default()
    };
    fs::create_dir(&cfg.root)?;
    let (tls, certificate_hash) = certificates(root.path(), &mut cfg)?;
    let source = queues::prepare(&cfg)
        .await
        .context("prepare real source queues")?;
    let proof = queues::evidence(&source)?;
    let mut batches = Vec::new();
    let mut inventories = Vec::new();
    let mut startups = BTreeSet::new();
    let mut journal = None;
    for (path, bytes) in &source {
        match queues::validate(path, bytes)? {
            "ordinary_batch" | "history_batch" => {
                let batch: Batch = serde_json::from_slice(bytes)?;
                startups.insert(batch.session_id);
                batches.push((path.clone(), bytes.clone(), batch));
            }
            "startup_inventory" | "refresh_inventory" | "legacy_inventory" => {
                let observation = InventoryObservation::decode(bytes.as_slice())?;
                startups.insert(Uuid::parse_str(&observation.source_startup_id)?);
                inventories.push((path.clone(), bytes.clone(), observation));
            }
            "terminal_operation" => journal = Some((path.clone(), bytes.clone())),
            _ => anyhow::bail!("unexpected prepared queue category"),
        }
    }
    ensure!(
        batches.len() == 2 && inventories.len() == 3,
        "unexpected original queue matrix"
    );
    let journal = journal.context("terminal operation queue missing")?;
    let inventory = inventories
        .iter()
        .find(|(_, _, value)| value.source_batch_id.is_empty() && value.revision == 0)
        .context("startup inventory missing")?
        .2
        .validate()?;
    let reopened_startup = Uuid::new_v4();
    startups.insert(reopened_startup);
    let receiver =
        durable_receiver::open(url, &cfg, &certificate_hash, &startups, reopened_startup).await?;
    let pool = receiver.pool;
    let service = receiver.service;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    cfg.telemetry_endpoint = format!("https://{}", listener.local_addr()?);
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        let incoming = futures::stream::unfold(listener, |listener| async move {
            Some((listener.accept().await.map(|(socket, _)| socket), listener))
        });
        Server::builder()
            .tls_config(tls)?
            .add_service(TelemetryServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown.cancelled())
            .await
    });
    let mut running = Running {
        sender: None,
        stop,
        server: Some(server),
    };
    let result = delivery(
        &cfg,
        reopened_startup,
        inventory,
        &source,
        &batches,
        &inventories,
        &journal,
        &pool,
        &mut running,
    )
    .await;
    let closed = running.close().await;
    result?;
    closed?;
    Ok(json!({
        "passed":true,"boundary":"ordinary sender reopen with current mTLS Service and real TimescaleDB transactions",
        "native_service_executed":false,"transaction_fence_observed":true,
        "source_batches":batches.len(),"source_inventories":inventories.len(),
        "queue_records":proof,"source_bytes_retained_before_commit":true,
        "business_ack_removed_sources":true,"original_id_replay_unique":true,
        "terminal_operation_unchanged":true,"real_business_migrations":true,
    }))
}

struct Running {
    sender: Option<telemetry::Handle>,
    stop: CancellationToken,
    server: Option<tokio::task::JoinHandle<Result<(), tonic::transport::Error>>>,
}
impl Running {
    async fn close(&mut self) -> Result<()> {
        if let Some(sender) = self.sender.take() {
            sender.shutdown().await?;
        }
        self.stop.cancel();
        if let Some(mut server) = self.server.take() {
            if let Ok(result) = tokio::time::timeout(Duration::from_secs(5), &mut server).await {
                result??;
            } else {
                server.abort();
                let _ = server.await;
                anyhow::bail!("durable receiver shutdown timed out");
            }
        }
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.sender.take();
        self.stop.cancel();
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn delivery(
    cfg: &Config,
    startup: Uuid,
    inventory: Inventory,
    source: &[(std::path::PathBuf, Vec<u8>)],
    batches: &[(std::path::PathBuf, Vec<u8>, Batch)],
    inventories: &[(std::path::PathBuf, Vec<u8>, InventoryObservation)],
    journal: &(std::path::PathBuf, Vec<u8>),
    pool: &toasty::Db,
    running: &mut Running,
) -> Result<()> {
    let node = Uuid::parse_str(&cfg.emissary_id)?;
    let mut held_db = pool.clone();
    let mut held = held_db.transaction().await?;
    sql::execute(&mut held, FENCE, params![]).await?;
    running.sender = Some(telemetry::Handle::start(
        cfg.clone(),
        startup,
        inventory.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let blocked: i64 = sql::scalar(&mut pool.clone(),
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'", params![]).await?;
            if blocked >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, anyhow::Error>(())
    }).await.context("real batch and inventory ingest did not wait on transaction fence")??;
    let receipts: i64 = sql::scalar(&mut pool.clone(),
        "SELECT (SELECT count(*) FROM telemetry_batches)+(SELECT count(*) FROM inventory_deliveries)", params![]).await?;
    ensure!(receipts == 0, "receipt committed before fence release");
    for (path, bytes) in source {
        if queues::validate(path, bytes)? == "ordinary_batch" {
            let batch: Batch = serde_json::from_slice(bytes)?;
            let retained = fresh_source(cfg, batch.id)?
                .context("original fresh sample missing before commit")?;
            queues::verify(&retained, bytes, &fs::read(&retained)?)?;
        } else {
            queues::verify(
                path,
                bytes,
                &fs::read(path).context("source record missing before commit")?,
            )?;
        }
    }
    held.rollback().await?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if batches.iter().all(|(path, _, batch)| {
                if path
                    .parent()
                    .is_some_and(|parent| parent.ends_with("fresh"))
                {
                    fresh_source(cfg, batch.id).is_ok_and(|source| source.is_none())
                } else {
                    !path.exists()
                }
            }) && inventories.iter().all(|(path, _, _)| !path.exists())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("business ACK did not delete all original source records")?;
    running
        .sender
        .take()
        .context("sender missing")?
        .shutdown()
        .await?;
    verify_database(pool, node, batches, inventories).await?;
    queues::verify(&journal.0, &journal.1, &fs::read(&journal.0)?)?;
    // 明确重投已确认正文，复用原ID与原字节；不复制队列持久化逻辑。
    let history = batches
        .iter()
        .find(|(path, _, _)| path.parent() == Some(cfg.root.join("telemetry").as_path()))
        .context("history source missing")?;
    artifactory_emissary::config::atomic_bytes(&history.0, &history.1, false)?;
    running.sender = Some(telemetry::Handle::start(cfg.clone(), startup, inventory));
    tokio::time::timeout(Duration::from_secs(10), async {
        while history.0.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("original batch replay did not receive business ACK")?;
    running
        .sender
        .take()
        .context("replay sender missing")?
        .shutdown()
        .await?;
    verify_database(pool, node, batches, inventories).await?;
    queues::verify(&journal.0, &journal.1, &fs::read(&journal.0)?)?;
    Ok(())
}

// 新鲜槽从pending原子改名为固定ID；按身份追踪同一份正文。
fn fresh_source(cfg: &Config, id: Uuid) -> Result<Option<std::path::PathBuf>> {
    let root = cfg.root.join("telemetry/fresh");
    for path in [root.join("pending.batch"), root.join(format!("{id}.batch"))] {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let batch: Batch = serde_json::from_slice(&bytes)?;
        if batch.id == id {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn certificates(root: &Path, cfg: &mut Config) -> Result<(ServerTlsConfig, String)> {
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca = ca_params.self_signed(&ca_key)?;
    let issuer = Issuer::new(ca_params, ca_key);
    let server_key = KeyPair::generate()?;
    let mut server_params = CertificateParams::new(vec!["127.0.0.1".into()])?;
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server = server_params.signed_by(&server_key, &issuer)?;
    let client_key = KeyPair::generate()?;
    let mut client_params = CertificateParams::default();
    client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client = client_params.signed_by(&client_key, &issuer)?;
    cfg.ca_file = root.join("ca.pem");
    cfg.certificate_file = root.join("client.pem");
    cfg.key_file = root.join("client.key");
    for (path, bytes) in [
        (&cfg.ca_file, ca.pem()),
        (&cfg.certificate_file, client.pem()),
        (&cfg.key_file, client_key.serialize_pem()),
    ] {
        artifactory_emissary::config::atomic_bytes(path, bytes.as_bytes(), false)?;
    }
    Ok((
        ServerTlsConfig::new()
            .identity(Identity::from_pem(server.pem(), server_key.serialize_pem()))
            .client_ca_root(Certificate::from_pem(ca.pem())),
        hex::encode(Sha256::digest(client.der())),
    ))
}
