//! [INPUT]: 原生服务、固定源队列、可选私有卷故障和真实接收存储。
//! [OUTPUT]: 维护或满盘恢复后由原生服务补交原记录，逐项读回事务ACK。
//! [POS]: 独立原生加数据库门禁；数据库凭据只留测试父进程。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{
    Machine, disk, durable_receiver, queues, support, telemetry_gate::TelemetryGate,
    telemetry_sources::Sources,
};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::config::Config;
use artifactory_emissary_protocol::nodes::MetadataSnapshot;
use durable_receiver::{FENCE, ReceiverStore};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

pub(super) struct Prepared {
    receiver: Arc<ReceiverStore>,
    gate: TelemetryGate,
    sources: Sources,
    startups: Arc<Mutex<std::collections::BTreeSet<Uuid>>>,
    resumed: Option<disk::Resumed>,
}
impl Prepared {
    pub async fn prepare(url: &str, fixture: &mut support::Fixture) -> Result<Self> {
        let sources = Sources::prepare(&fixture.cfg).await?;
        let current = *sources.startups.first().context("source startup missing")?;
        let receiver = Arc::new(
            durable_receiver::open(
                url,
                &fixture.cfg,
                &fixture.tls().client_hash(),
                &sources.startups,
                current,
            )
            .await?,
        );
        let gate =
            TelemetryGate::start(receiver.service.clone(), fixture.tls().server_tls()).await?;
        fixture.cfg.telemetry_endpoint = gate.endpoint.clone();
        let node = Uuid::parse_str(&fixture.cfg.emissary_id)?;
        let registration = receiver.clone();
        let startups = Arc::new(Mutex::new(std::collections::BTreeSet::new()));
        let registered = startups.clone();
        fixture.set_hello_hook(Arc::new(move |hello| {
            let registration = registration.clone();
            let registered = registered.clone();
            Box::pin(async move {
                let payload: Value = serde_json::from_str(&hello.payload_json)
                    .map_err(|_| tonic::Status::invalid_argument("invalid hello"))?;
                let snapshot: MetadataSnapshot = serde_json::from_value(payload["startup"].clone())
                    .map_err(|_| tonic::Status::invalid_argument("invalid startup"))?;
                registration
                    .register_startup(node, &snapshot)
                    .await
                    .map_err(|_| tonic::Status::internal("startup transaction failed"))?;
                registered
                    .lock()
                    .map_err(|_| tonic::Status::internal("startup evidence unavailable"))?
                    .insert(snapshot.startup_id);
                Ok(())
            })
        }));
        let mut prepared = Self {
            receiver,
            gate,
            sources,
            startups,
            resumed: None,
        };
        prepared.sources.pin(&fixture.cfg, &prepared.gate).await?;
        Ok(prepared)
    }
    pub fn queues(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.sources.files.clone()
    }

    pub async fn disk_fault(
        &mut self,
        machine: &Machine,
        cfg: &Config,
        connection: &mut support::Connection,
    ) -> Result<Value> {
        let resumed = disk::exercise(machine, cfg, connection, || {
            retained(machine, &self.sources)
        })
        .await?;
        let proof = resumed.proof.clone();
        self.resumed = Some(resumed);
        Ok(proof)
    }

    pub async fn deliver(&mut self, machine: &Machine, cfg: &Config) -> Result<Value> {
        // 只观察当前系统服务；放行后不启动额外Handle代替它发送。
        let pid = machine.running().await;
        let configuration: Config = serde_json::from_slice(&read(machine, &machine.config)?)?;
        ensure!(
            configuration.telemetry_endpoint == self.gate.endpoint,
            "native service endpoint changed after preparation"
        );
        retained(machine, &self.sources)?;
        let counts = inventory_count(machine, &cfg.root.join("telemetry/inventories"))?;
        ensure!(
            counts <= 16,
            "native recovery exceeds declared inventory budget"
        );
        let mut db = self.receiver.pool.clone();
        let mut held = db.transaction().await?;
        sql::execute(&mut held, FENCE, params![]).await?;
        self.gate.open();
        tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                let blocked: i64 = sql::scalar(&mut self.receiver.pool.clone(),
                    "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'", params![]).await?;
                if blocked >= 2 { break; }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_, anyhow::Error>(())
        }).await.context("native service did not reach real store transaction fence")??;
        let receipts: i64 = sql::scalar(&mut self.receiver.pool.clone(),
            "SELECT (SELECT count(*) FROM telemetry_batches)+(SELECT count(*) FROM inventory_deliveries)", params![]).await?;
        ensure!(
            receipts == 0,
            "receipt committed before native fence release"
        );
        retained(machine, &self.sources)?;
        held.rollback().await?;
        if let Some(resumed) = &mut self.resumed {
            let (bytes, batch) = tokio::time::timeout(Duration::from_secs(75), async {
                loop {
                    if let Some(bytes) = self.gate.last_host()? {
                        let batch: artifactory_emissary_protocol::telemetry::Batch =
                            serde_json::from_slice(&bytes)?;
                        if batch.sampled_at >= resumed.released && batch.dropped_batches >= 2 {
                            break Ok::<_, anyhow::Error>((bytes, batch));
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .context("native sender did not select a post-recovery host sample")??;
            resumed.bytes = bytes;
            resumed.batch = batch;
        }
        // 真实burst为4，每30秒补一个token；为全部已排队清单保留有界重试时间。
        let budget = Duration::from_secs(45 + counts.saturating_sub(4) as u64 * 30);
        tokio::time::timeout(budget, async {
            loop {
                let mut pending = false;
                for (path, _, _) in &self.sources.batches {
                    pending |= exists(machine, path)?;
                }
                for (path, _, _) in &self.sources.inventories {
                    pending |= exists(machine, path)?;
                }
                if !pending {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("native service did not receive all original durable ACKs")??;
        ensure!(
            machine.running().await == pid,
            "service changed during ACK delivery"
        );
        durable_receiver::verify_database(
            &self.receiver.pool,
            Uuid::parse_str(&cfg.emissary_id)?,
            &self.sources.batches,
            &self.sources.inventories,
        )
        .await?;
        if let Some(resumed) = &self.resumed {
            let node = Uuid::parse_str(&cfg.emissary_id)?;
            // 确认恢复后真实发送器选择的记录；pending槽被后续采样替换不能算ACK。
            tokio::time::timeout(Duration::from_secs(75), async {
                loop {
                    let count: i64 = sql::scalar(
                        &mut self.receiver.pool.clone(),
                        "SELECT count(*) FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2",
                        params![node, resumed.batch.id],
                    )
                    .await?;
                    let inflight = cfg
                        .root
                        .join("telemetry/fresh")
                        .join(format!("{}.batch", resumed.batch.id));
                    let pending_same =
                        exists(machine, &resumed.path)?
                            && serde_json::from_slice::<
                                artifactory_emissary_protocol::telemetry::Batch,
                            >(&read(machine, &resumed.path)?)?
                            .id == resumed.batch.id;
                    if count == 1
                        && self.gate.saw_batch(resumed.batch.id)?
                        && !exists(machine, &inflight)?
                        && !pending_same
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await
            .context("native resumed sample did not receive its business ACK")??;
            let record = (
                resumed.path.clone(),
                resumed.bytes.clone(),
                resumed.batch.clone(),
            );
            durable_receiver::verify_database(&self.receiver.pool, node, &[record], &[]).await?;
            let gaps: i64 = sql::scalar(
                &mut self.receiver.pool.clone(),
                "SELECT dropped_batches FROM host_samples WHERE node_id=$1 AND batch_id=$2",
                params![node, resumed.batch.id],
            )
            .await?;
            ensure!(
                gaps == resumed.batch.dropped_batches as i64 && gaps >= 2,
                "native recovered database sample lost disk gap count"
            );
        }
        ensure!(
            machine.running().await == pid,
            "service changed before final ACK readback"
        );
        queues::verify(
            &self.sources.journal.0,
            &self.sources.journal.1,
            &read(machine, &self.sources.journal.0)?,
        )?;
        let startups = self
            .startups
            .lock()
            .map_err(|_| anyhow::anyhow!("startup evidence unavailable"))?
            .len();
        ensure!(
            startups >= 4,
            "native maintenance/OTA/rollback startup evidence incomplete"
        );
        self.gate.close().await?;
        Ok(json!({
            "passed":true,"native_service_executed":true,"real_business_migrations":true,
            "boundary":"native maintenance, runtime OTA and rollback followed by current durable receiver",
            "native_startups_registered":startups,"source_batches":self.sources.batches.len(),
            "source_inventories":self.sources.inventories.len(),"pending_inventories_at_release":counts,
            "transaction_fence_observed":true,"source_bytes_retained_before_commit":true,
            "business_ack_removed_sources":true,"terminal_operation_unchanged":true,
            "authorization_acl_proven":false,"queue_records":queues::evidence(&self.sources.files)?,
            "resumed_sample_business_ack_verified":self.resumed.is_some(),
            "resumed_sample_batch_id":self.resumed.as_ref().map(|r|r.batch.id),
        }))
    }
}

fn read(machine: &Machine, path: &Path) -> Result<Vec<u8>> {
    if cfg!(unix) {
        let output = machine.try_command_limited(true, "cat", &[path.as_os_str()], 2 << 20)?;
        ensure!(output.status.success(), "private source read failed");
        Ok(output.stdout)
    } else {
        Ok(fs::read(path)?)
    }
}
fn exists(machine: &Machine, path: &Path) -> Result<bool> {
    if cfg!(unix) {
        let output = machine.try_command(true, "test", &[OsStr::new("-e"), path.as_os_str()])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => anyhow::bail!("private source lookup failed"),
        }
    } else {
        Ok(path.try_exists()?)
    }
}
fn retained(machine: &Machine, sources: &Sources) -> Result<()> {
    for (path, expected) in &sources.files {
        queues::verify(path, expected, &read(machine, path)?)?;
    }
    Ok(())
}
fn inventory_count(machine: &Machine, root: &Path) -> Result<usize> {
    if cfg!(unix) {
        let output = machine.try_command(
            true,
            "find",
            &[
                root.as_os_str(),
                OsStr::new("-type"),
                OsStr::new("f"),
                OsStr::new("-name"),
                OsStr::new("*.inventory"),
            ],
        )?;
        ensure!(
            output.status.success(),
            "private inventory enumeration failed"
        );
        let names = String::from_utf8(output.stdout)?;
        for name in names.lines() {
            let path = Path::new(name);
            ensure!(
                path.parent() == Some(root),
                "unexpected inventory subdirectory"
            );
            Uuid::parse_str(
                path.file_stem()
                    .and_then(OsStr::to_str)
                    .context("invalid inventory filename")?,
            )?;
        }
        Ok(names.lines().count())
    } else {
        let mut count = 0;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|value| value == "inventory")
            {
                ensure!(
                    entry.file_type()?.is_file(),
                    "inventory is not a regular file"
                );
                count += 1;
            }
        }
        Ok(count)
    }
}

pub(super) fn write_evidence(proof: &Value) -> Result<()> {
    let root = std::env::var_os("ARTIFACTORY_NATIVE_EVIDENCE_DIR")
        .context("native evidence directory required")?;
    let root = PathBuf::from(root);
    fs::create_dir_all(&root)?;
    if proof["passed"] == false {
        ensure!(
            !root.join("durable_recovery.json").try_exists()?,
            "native durable evidence already exists"
        );
    }
    artifactory_emissary::config::atomic_json(&root.join("durable_recovery.json"), proof)
}
