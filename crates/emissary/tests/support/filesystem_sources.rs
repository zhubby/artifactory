//! [INPUT]: 父驱动私有卷、真实源队列与当前mTLS Service/store。
//! [OUTPUT]: 固定原队列与身份，验证故障后事务ACK及设备端源删除。
//! [POS]: OTA小卷测试的数据恢复边界；不安装服务或重建原记录。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::telemetry_sources::Sources;
use crate::{durable_receiver, queues, support::TlsMaterial, telemetry_gate::TelemetryGate};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::{
    config::{Config, atomic_json, read_json},
    telemetry,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
type BatchRecord = (
    PathBuf,
    Vec<u8>,
    artifactory_emissary_protocol::telemetry::Batch,
);

#[derive(Serialize, Deserialize)]
struct Saved {
    cfg: Config,
    records: Vec<(PathBuf, Vec<u8>)>,
    identity: Vec<(PathBuf, Vec<u8>)>,
    configuration: Vec<u8>,
    certificate_hash: String,
}

fn marker(root: &Path, case: &str) -> PathBuf {
    root.join(format!("sources-{case}.json"))
}

pub async fn prepare(root: &Path, mount: &Path, case: &str) -> Result<()> {
    let mut cfg = Config {
        root: mount.join("node"),
        emissary_id: Uuid::new_v4().to_string(),
        ..Default::default()
    };
    fs::create_dir(&cfg.root)?;
    let material = TlsMaterial::generate()?;
    material.write_config(&cfg.root.clone(), &mut cfg)?;
    let source = Sources::prepare(&cfg).await?;
    // 证书必须在故障卷上；服务密钥留在父目录的私有快照中。
    let server_root = root.join(format!("server-{case}"));
    artifactory_emissary::config::private_dir(&server_root)?;
    material.write_server(&server_root)?;
    atomic_json(&cfg.root.join("config.json"), &cfg)?;
    let configuration = fs::read(cfg.root.join("config.json"))?;
    let identity = [&cfg.ca_file, &cfg.certificate_file, &cfg.key_file]
        .into_iter()
        .map(|path| Ok((path.clone(), fs::read(path)?)))
        .collect::<Result<Vec<_>>>()?;
    atomic_json(
        &marker(root, case),
        &Saved {
            cfg,
            records: source.files,
            identity,
            configuration,
            certificate_hash: material.client_hash(),
        },
    )?;
    Ok(())
}

fn saved(root: &Path, mount: &Path, case: &str) -> Result<Saved> {
    let saved: Saved = read_json(&marker(root, case))?;
    ensure!(
        saved.cfg.root == mount.join("node"),
        "source configuration root differs"
    );
    ensure!(
        saved.records.len() == 6 && saved.identity.len() == 3,
        "frozen source matrix differs"
    );
    for (path, _) in saved.records.iter().chain(saved.identity.iter()) {
        let relative = path.strip_prefix(&saved.cfg.root)?;
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "unsafe frozen source path"
        );
        ensure!(
            fs::symlink_metadata(path)?.file_type().is_file(),
            "frozen source is not a regular file"
        );
    }
    Ok(saved)
}

pub fn retained(root: &Path, mount: &Path, case: &str) -> Result<Value> {
    let saved = saved(root, mount, case)?;
    let source = Sources::from_files(saved.records)?;
    source.retained()?;
    identity_retained(&saved.cfg, &saved.identity, &saved.configuration)?;
    queues::evidence(&source.files)
}

fn identity_retained(
    cfg: &Config,
    identity: &[(PathBuf, Vec<u8>)],
    configuration: &[u8],
) -> Result<()> {
    ensure!(
        fs::read(cfg.root.join("config.json"))? == configuration,
        "source config changed"
    );
    for (path, bytes) in identity {
        ensure!(
            fs::read(path)? == *bytes,
            "source certificate identity changed"
        );
    }
    Ok(())
}

pub async fn recover(root: &Path, mount: &Path, case: &str, url: &str) -> Result<Value> {
    let saved = saved(root, mount, case)?;
    let mut source = Sources::from_files(saved.records)?;
    source.retained()?;
    identity_retained(&saved.cfg, &saved.identity, &saved.configuration)?;
    let proof = queues::evidence(&source.files)?;
    let current = source
        .batches
        .iter()
        .find(|(_, _, b)| b.id == source.fresh_id)
        .context("original startup missing")?
        .2
        .session_id;
    let receiver = durable_receiver::open(
        url,
        &saved.cfg,
        &saved.certificate_hash,
        &source.startups,
        current,
    )
    .await?;
    let server_root = root.join(format!("server-{case}"));
    let tls = tonic::transport::ServerTlsConfig::new()
        .identity(tonic::transport::Identity::from_pem(
            fs::read(server_root.join("server.pem"))?,
            fs::read(server_root.join("server.key"))?,
        ))
        .client_ca_root(tonic::transport::Certificate::from_pem(fs::read(
            server_root.join("ca.pem"),
        )?));
    let mut gate = TelemetryGate::start(receiver.service, tls).await?;
    let mut cfg = saved.cfg.clone();
    cfg.telemetry_endpoint = gate.endpoint.clone();
    let pinned = source.pin(&cfg, &gate).await;
    if let Err(error) = pinned {
        gate.close().await?;
        return Err(error);
    }
    let active = if std::env::var_os("ARTIFACTORY_OTA_FAULT_ACTIVE_SPOOL").is_some() {
        let result = active_collection(&cfg, &source, mount, current).await;
        if let Err(error) = result {
            gate.close().await?;
            return Err(error);
        }
        Some(result?)
    } else {
        None
    };
    let mut db = receiver.pool.clone();
    let mut held = db.transaction().await?;
    sql::execute(&mut held, durable_receiver::FENCE, params![]).await?;
    gate.open();
    let sender = telemetry::Handle::start(cfg, current, source.inventory.clone());
    let result = async {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let blocked: i64 = sql::scalar(&mut receiver.pool.clone(),
                    "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'", params![]).await?;
                if blocked >= 2 {break;}
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_, anyhow::Error>(())
        }).await.context("recovered sender transactions did not reach fence")??;
        if let Some((active, _)) = &active {
            let snapshot = sender.snapshot().context("reopened collector snapshot missing")?.metrics;
            ensure!(snapshot["telemetry"]["disk_errors"].as_u64().unwrap_or(0) >= active["observed_disk_errors"].as_u64().unwrap(),
                "disk error counters lost when reopening spool");
            ensure!(snapshot["telemetry"]["dropped_batches"].as_u64().unwrap_or(0) >= 2,
                "disk gaps lost when reopening spool");
        }
        source.retained()?;
        let receipts: i64 = sql::scalar(&mut receiver.pool.clone(),
            "SELECT (SELECT count(*) FROM telemetry_batches)+(SELECT count(*) FROM inventory_deliveries)",params![]).await?;
        ensure!(receipts == 0, "source receipt committed before fence release");
        held.rollback().await?;
        tokio::time::timeout(Duration::from_secs(100), async {
            while !source.removed()? {tokio::time::sleep(Duration::from_millis(25)).await;}
            Ok::<_, anyhow::Error>(())
        }).await.context("recovered source business ACK missing")??;
        durable_receiver::verify_database(&receiver.pool, Uuid::parse_str(&saved.cfg.emissary_id)?, &source.batches, &source.inventories).await?;
        if let Some((_, resumed)) = &active {
            let node = Uuid::parse_str(&saved.cfg.emissary_id)?;
            tokio::time::timeout(Duration::from_secs(45), async {
                loop {
                    let count: i64 = sql::scalar(&mut receiver.pool.clone(),
                        "SELECT count(*) FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2",params![node,resumed.2.id]).await?;
                    let inflight = saved.cfg.root.join("telemetry/fresh").join(format!("{}.batch",resumed.2.id));
                    let pending = match fs::read(&resumed.0) {
                        Ok(bytes) => serde_json::from_slice::<artifactory_emissary_protocol::telemetry::Batch>(&bytes)?.id == resumed.2.id,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                        Err(error) => return Err(error.into()),
                    };
                    if count == 1 && gate.saw_batch(resumed.2.id)? && !inflight.try_exists()? && !pending {break;}
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Ok::<_,anyhow::Error>(())
            }).await.context("post-ENOSPC host sample did not receive business ACK")??;
            durable_receiver::verify_database(&receiver.pool,node,std::slice::from_ref(resumed),&[]).await?;
            let gaps: i64 = sql::scalar(&mut receiver.pool.clone(),
                "SELECT dropped_batches FROM host_samples WHERE node_id=$1 AND batch_id=$2",params![node,resumed.2.id]).await?;
            ensure!(gaps == resumed.2.dropped_batches as i64 && gaps >= 2, "post-ENOSPC database sample lost gap count");
        }
        source.journal_retained()?;
        identity_retained(&saved.cfg, &saved.identity, &saved.configuration)?;
        Ok::<_, anyhow::Error>(())
    }.await;
    let shutdown = sender.shutdown().await;
    let closed = gate.close().await;
    result?;
    shutdown?;
    closed?;
    Ok(
        json!({"passed":true,"case":case,"source_batches":2,"source_inventories":3,
        "real_business_migrations":true,"transaction_fence_observed":true,
        "fresh_pinned_by_real_sender":true,
        "source_bytes_retained_before_commit":true,"business_ack_removed_sources":true,
        "original_ids_digests_times_verified":true,"terminal_operation_unchanged":true,
        "identity_configuration_unchanged":true,"queue_records":proof,
        "active_collection":active.map(|(mut proof,_)| {proof["disk_counters_survive_reopen"]=json!(true);proof["resumed_sample_business_ack_verified"]=json!(true);proof["resumed_sample_source_removed"]=json!(true);proof}),
        "native_service_executed":false,"authorization_acl_proven":false}),
    )
}

/// 门保持关闭；真实采集尝试失败后，释放压力并等待同Handle写入新主机槽。
async fn active_collection(
    cfg: &Config,
    source: &Sources,
    mount: &Path,
    startup: Uuid,
) -> Result<(Value, BatchRecord)> {
    let handle = telemetry::Handle::start(cfg.clone(), startup, source.inventory.clone());
    let result = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if handle
                    .snapshot()
                    .and_then(|s| s.metrics["telemetry"]["spool_records"].as_u64())
                    .is_some()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .context("active collector did not open original spool")?;
        let pressure = mount.join("collector-pressure");
        let mut file = fs::File::create(&pressure)?;
        use std::io::Write;
        let mut writes = 0;
        let mut one_byte_enospc = false;
        // 大块失败可能留下可容纳遥测的小空间，逐级填至单字节也失败。
        for size in [1 << 20, 4096, 1] {
            let block = vec![0x74; size];
            loop {
                writes += 1;
                ensure!(
                    writes <= 1024,
                    "active collector fill exceeded private volume"
                );
                match file.write_all(&block) {
                    Ok(()) => {}
                    Err(error) => {
                        ensure!(
                            error.raw_os_error() == Some(28)
                                || error.kind() == std::io::ErrorKind::StorageFull,
                            "pressure fill failed without ENOSPC"
                        );
                        one_byte_enospc |= size == 1;
                        break;
                    }
                }
            }
        }
        ensure!(one_byte_enospc, "single-byte pressure did not reach ENOSPC");
        drop(file);
        let free = fs2::available_space(mount)?;
        let total = fs2::total_space(mount)?;
        let observed = tokio::time::timeout(Duration::from_secs(35), async {
            loop {
                let metrics = handle
                    .snapshot()
                    .context("active collector snapshot missing")?
                    .metrics;
                let errors = metrics["telemetry"]["disk_errors"].as_u64().unwrap_or(0);
                if errors >= 2 {
                    ensure!(
                        metrics["telemetry"]["dropped_batches"]
                            .as_u64()
                            .unwrap_or(0)
                            >= 2,
                        "disk failures not counted as gaps"
                    );
                    return Ok::<_, anyhow::Error>(errors);
                }
                source.retained()?;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        if !matches!(&observed, Ok(Ok(_))) {
            atomic_json(
                &mount.parent().unwrap().join("active-failure.json"),
                &json!({
                    "free_bytes_at_fill":free,"free_bytes_now":fs2::available_space(mount).ok(),
                    "snapshot":handle.snapshot().map(|s|s.metrics)
                }),
            )?;
        }
        let errors = observed.context("real collector did not report repeated disk failures")??;
        source.retained()?;
        fs::remove_file(pressure)?;
        let released_at = chrono::Utc::now();
        let fresh = cfg.root.join("telemetry/fresh/pending.batch");
        let resumed = tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                match fs::read(&fresh) {
                    Ok(bytes) => {
                        let batch: artifactory_emissary_protocol::telemetry::Batch =
                            serde_json::from_slice(&bytes)?;
                        batch.validate()?;
                        if batch.sampled_at >= released_at && batch.id != source.fresh_id {
                            ensure!(
                                batch.host.is_some(),
                                "recovered collector did not persist a host sample"
                            );
                            ensure!(batch.dropped_batches >= 2, "recovered sample hid disk gaps");
                            break Ok::<_, anyhow::Error>((fresh.clone(), bytes, batch));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                source.retained()?;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .context("same collector did not recover after space release")??;
        source.retained()?;
        Ok::<_, anyhow::Error>((
            json!({"passed":true,"actual_enospc":true,
            "one_byte_write_enospc":one_byte_enospc,
            "filesystem_total_bytes":total,"free_bytes_at_failure":free,"observed_disk_errors":errors,
            "repeated_collection_failures_observed":true,"original_sources_retained":true,
            "same_handle_resumed_after_space_release":true,"resumed_batch_id":resumed.2.id,
            "native_service_executed":false}),
            resumed,
        ))
    }
    .await;
    let stopped = handle.shutdown().await;
    result.and_then(|(proof, resumed)| {
        stopped?;
        ensure!(
            fs::read(&resumed.0)? == resumed.1,
            "resumed host sample changed before spool close"
        );
        let stats: Value = read_json(&cfg.root.join("telemetry/stats.json"))?;
        ensure!(
            stats["reasons"]["disk_error"].as_u64().unwrap_or(0)
                >= proof["observed_disk_errors"].as_u64().unwrap(),
            "disk errors not persisted after space recovery"
        );
        ensure!(
            stats["dropped"].as_u64().unwrap_or(0) >= 2,
            "disk gaps not persisted after space recovery"
        );
        Ok((proof, resumed))
    })
}
