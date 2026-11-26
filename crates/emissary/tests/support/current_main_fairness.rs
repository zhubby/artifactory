//! [INPUT]: 已运行的完整Telemetry main、双库、受限授权与真实采集发送器。
//! [OUTPUT]: 验证27MiB历史积压中当前startup查询持续新鲜、真实ACK及历史进展。
//! [POS]: 完整main的有界公平发送场景；延迟仅注入本次隔离库，不替代生产容量。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{durable_receiver, support};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::{config::Config, telemetry};
use artifactory_emissary_protocol::{
    nodes::MetadataSnapshot,
    telemetry::{Batch, DiskSample},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};
use uuid::Uuid;

struct Backlog {
    sources: Vec<(PathBuf, Vec<u8>, Batch)>,
    bytes: usize,
    current: Uuid,
    old: Uuid,
}

/// 初始队列接近真实历史额度；每个正文保留原ID、时间和字节。
fn prepare(cfg: &Config) -> Result<Backlog> {
    let root = cfg.root.join("telemetry");
    fs::create_dir_all(root.join("fresh"))?;
    let mut batch: Batch = serde_json::from_slice(include_bytes!(
        "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))?;
    batch.processes.clear();
    batch.inventory = None;
    batch.dropped_batches = 0;
    batch.omitted_processes = 0;
    let current = Uuid::new_v4();
    let old = Uuid::new_v4();
    batch.session_id = current;
    batch.sampled_at = Utc::now();
    batch.id = Uuid::new_v4();
    fs::write(
        root.join("fresh/pending.batch"),
        serde_json::to_vec(&batch)?,
    )?;
    batch.session_id = old;
    batch.sampled_at -= chrono::Duration::minutes(10);
    batch.host.as_mut().context("host fixture missing")?.disks = (0..256)
        .map(|_| DiskSample {
            device: "d".repeat(512),
            mount: "m".repeat(1024),
            total_bytes: 4096,
            available_bytes: 2048,
            read_bytes: None,
            written_bytes: None,
        })
        .collect();
    batch.validate()?;
    let mut sources = Vec::new();
    let mut bytes = 0;
    loop {
        batch.id = Uuid::new_v4();
        batch.sampled_at += chrono::Duration::milliseconds(1);
        let encoded = serde_json::to_vec(&batch)?;
        ensure!(
            encoded.len() <= 1 << 20,
            "history fixture exceeds wire budget"
        );
        if bytes + encoded.len() > (27 << 20) - (512 << 10) {
            break;
        }
        let path = root.join(format!("{:020}-{}.batch", sources.len(), batch.id));
        fs::write(&path, &encoded)?;
        bytes += encoded.len();
        sources.push((path, encoded, batch.clone()));
    }
    ensure!(bytes >= 26 << 20, "history fixture did not fill quota");
    Ok(Backlog {
        sources,
        bytes,
        current,
        old,
    })
}

pub async fn exercise(
    fixture: &support::Fixture,
    business: &toasty::Db,
    database: &toasty::Db,
    client: &reqwest::Client,
    http: &str,
    token: &str,
) -> Result<Value> {
    let mut cfg = fixture.cfg.clone();
    cfg.root = cfg.root.with_file_name("fairness-node");
    let backlog = prepare(&cfg)?;
    let node = Uuid::parse_str(&cfg.emissary_id)?;
    for startup in [backlog.old, backlog.current] {
        durable_receiver::register_startup(
            business,
            node,
            &MetadataSnapshot::new(startup, Default::default(), Vec::new())?,
        )
        .await?;
    }
    // === 仅本次隔离库的真实写入延迟 ===
    // 延迟回执事务，不改变生产RPC期限、采样周期、额度或限流。
    sql::execute(&mut database.clone(),
        &format!("CREATE FUNCTION fairness_delay() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.node_id='{node}'::uuid THEN PERFORM pg_sleep(8); END IF; RETURN NEW; END $$"), params![]).await?;
    sql::execute(&mut database.clone(),
        "CREATE TRIGGER fairness_delay BEFORE INSERT ON telemetry_batches FOR EACH ROW EXECUTE FUNCTION fairness_delay()", params![]).await?;
    let inventory = serde_json::from_slice::<Batch>(include_bytes!(
        "../../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))?
    .inventory
    .context("inventory fixture missing")?;
    let mut sender = Some(telemetry::Handle::start(
        cfg.clone(),
        backlog.current,
        inventory,
    ));
    let result = observe(&cfg, &backlog, database, client, http, token, &mut sender).await;
    let stopped = if let Some(sender) = sender.take() {
        sender.shutdown().await
    } else {
        Ok(())
    };
    let removed_trigger = sql::execute(
        &mut database.clone(),
        "DROP TRIGGER fairness_delay ON telemetry_batches",
        params![],
    )
    .await;
    let removed_function = sql::execute(
        &mut database.clone(),
        "DROP FUNCTION fairness_delay()",
        params![],
    )
    .await;
    let mut proof = result?;
    stopped?;
    removed_trigger?;
    removed_function?;
    proof["sender_stopped"] = json!(true);
    proof["isolated_delay_removed"] = json!(true);
    Ok(proof)
}

struct FreshSource {
    path: PathBuf,
    bytes: Vec<u8>,
    batch: Batch,
    acknowledged: bool,
}

async fn observe(
    cfg: &Config,
    backlog: &Backlog,
    database: &toasty::Db,
    client: &reqwest::Client,
    http: &str,
    token: &str,
    sender: &mut Option<telemetry::Handle>,
) -> Result<Value> {
    let node = Uuid::parse_str(&cfg.emissary_id)?;
    let url = format!(
        "https://{http}/v1/nodes/{node}/latest?startup_id={}",
        backlog.current
    );
    let fresh = cfg.root.join("telemetry/fresh");
    let started = std::time::Instant::now();
    let mut first_projection = None;
    let mut query_count = 0;
    let mut max_age_ms = 0;
    let mut max_ack_ms = 0;
    let mut sources = BTreeMap::<Uuid, FreshSource>::new();
    while started.elapsed() < Duration::from_secs(110) {
        for entry in fs::read_dir(&fresh)? {
            let entry = entry?;
            let path = entry.path();
            // pending可被合法合并；只跟踪真实发送器固定身份后的槽。
            if path.extension().is_none_or(|value| value != "batch")
                || entry.file_name() == "pending.batch"
            {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let batch: Batch = serde_json::from_slice(&bytes)?;
            ensure!(batch.session_id == backlog.current, "fresh startup changed");
            sources.entry(batch.id).or_insert(FreshSource {
                path,
                bytes,
                batch,
                acknowledged: false,
            });
        }
        for source in sources.values_mut().filter(|source| !source.acknowledged) {
            match fs::read(&source.path) {
                Ok(bytes) => ensure!(bytes == source.bytes, "inflight fresh bytes changed"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    durable_receiver::verify_database(
                        database,
                        node,
                        &[(
                            source.path.clone(),
                            source.bytes.clone(),
                            source.batch.clone(),
                        )],
                        &[],
                    )
                    .await?;
                    let latency = Utc::now()
                        .signed_duration_since(source.batch.sampled_at)
                        .num_milliseconds();
                    ensure!(
                        (0..=60_000).contains(&latency),
                        "fresh business ACK exceeded 60 seconds"
                    );
                    max_ack_ms = max_ack_ms.max(latency);
                    source.acknowledged = true;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let response = client.get(&url).bearer_auth(token).send().await?;
        ensure!(
            response.status().is_success(),
            "current-startup HTTPS query failed"
        );
        let value: Value = response.json().await?;
        if !value["host"].is_null() {
            ensure!(
                value["host"]["startup_id"] == backlog.current.to_string(),
                "old startup contaminated query"
            );
            ensure!(
                value["host"]["fresh"] == true,
                "real current-startup projection is stale"
            );
            for field in ["sampled_at", "received_at"] {
                let at: DateTime<Utc> = value["host"][field]
                    .as_str()
                    .context("projection timestamp missing")?
                    .parse()?;
                let age = Utc::now().signed_duration_since(at).num_milliseconds();
                ensure!(
                    (-1000..90_000).contains(&age),
                    "projection time exceeded freshness budget"
                );
                max_age_ms = max_age_ms.max(age);
            }
            if first_projection.is_none() {
                ensure!(
                    started.elapsed() < Duration::from_secs(45),
                    "first current-startup projection exceeded 45 seconds"
                );
                first_projection = Some(started.elapsed().as_millis());
            }
            query_count += 1;
        } else {
            ensure!(
                first_projection.is_none() && started.elapsed() < Duration::from_secs(45),
                "current projection absent after initial delivery"
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    // 结束实际发送后再核对静止文件，避免目录扫描与ACK删除竞态。
    sender
        .take()
        .context("fairness sender missing")?
        .shutdown()
        .await?;
    let mut historical_acks = 0;
    for (path, bytes, batch) in &backlog.sources {
        if path.try_exists()? {
            ensure!(fs::read(path)? == *bytes, "pending history bytes changed");
        } else {
            durable_receiver::verify_database(
                database,
                node,
                &[(path.clone(), bytes.clone(), batch.clone())],
                &[],
            )
            .await?;
            historical_acks += 1;
        }
    }
    let fresh_acks = sources
        .values()
        .filter(|source| source.acknowledged)
        .count();
    ensure!(
        fresh_acks >= 3 && historical_acks >= 3,
        "both lanes did not progress"
    );
    ensure!(
        historical_acks < backlog.sources.len(),
        "history was not continuously backlogged"
    );
    let queued = record_sizes(&cfg.root.join("telemetry"))?;
    ensure!(
        queued.iter().sum::<u64>() <= 27 << 20,
        "history logical quota exceeded"
    );
    let fresh_bytes = record_sizes(&fresh)?;
    ensure!(
        fresh_bytes.len() <= 2 && fresh_bytes.iter().sum::<u64>() <= 1 << 20,
        "fresh logical quota exceeded"
    );
    Ok(
        json!({"passed":true,"actual_current_main":true,"authorization_acl_proven":true,
        "native_service_executed":false,"production_capacity_proven":false,
        "duration_seconds":started.elapsed().as_secs_f64(),"rpc_transaction_delay_seconds":8,
        "initial_history_records":backlog.sources.len(),"initial_history_bytes":backlog.bytes,
        "historical_business_acks":historical_acks,"fresh_business_acks":fresh_acks,
        "current_startup_queries":query_count,"first_projection_ms":first_projection,
        "max_projection_age_ms":max_age_ms,"max_observed_fresh_ack_ms":max_ack_ms,
        "history_continuously_backlogged":true,"original_history_and_inflight_bytes_preserved":true,
        "history_logical_bytes":queued.iter().sum::<u64>(),"fresh_logical_bytes":fresh_bytes.iter().sum::<u64>()}),
    )
}

fn record_sizes(root: &std::path::Path) -> Result<Vec<u64>> {
    let mut sizes = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|value| value == "batch")
        {
            sizes.push(entry.metadata()?.len());
        }
    }
    Ok(sizes)
}
