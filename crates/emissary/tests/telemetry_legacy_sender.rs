//! [INPUT]: Python监督的冻结旧发送进程、当前mTLS Service和具名隔离TimescaleDB。
//! [OUTPUT]: 验证旧字段号与事务ACK，提供具名库预检、精确清理和逐项读回。
//! [POS]: 反向跨版本门禁；不替代混合Control部署或系统服务验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
#[path = "support/frozen_sender.rs"]
mod frozen_sender;
use frozen_sender::{Sender, request_sender, wait_sender};
mod support;
#[path = "support/telemetry_gate.rs"]
mod telemetry_gate;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::config::{Config, atomic_json};
use artifactory_emissary_protocol::telemetry::Batch;
use durable_receiver::FENCE;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use telemetry_gate::TelemetryGate;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "run through scripts/check_emissary_legacy_sender.py with frozen sender binary"]
async fn frozen_legacy_sender_to_current_durable_receiver() {
    let name =
        std::env::var("ARTIFACTORY_LEGACY_DATABASE_NAME").expect("owned database name required");
    let proof =
        durable_receiver::isolated_named(Duration::from_secs(220), &name, |url| async move {
            let root = driver_root()?;
            atomic_json(
                &root.join("database-owned.json"),
                &json!({"name":name_from_url(&url)?}),
            )?;
            exercise(&url).await
        })
        .await
        .expect("frozen sender to current durable receiver failed");
    let output = std::env::var_os("ARTIFACTORY_LEGACY_SENDER_EVIDENCE")
        .expect("sanitized evidence path required");
    atomic_json(Path::new(&output), &proof).expect("cross-version evidence write failed");
}

async fn exercise(url: &str) -> Result<Value> {
    let directory = driver_root()?;
    let root = directory.as_path();
    let mut cfg = Config {
        emissary_id: Uuid::new_v4().to_string(),
        root: root.join("managed"),
        ..Default::default()
    };
    let tls = support::TlsMaterial::generate()?;
    tls.write_config(root, &mut cfg)?;
    // 正文由冻结旧类型与其原fixture生成；当前端只解码校验与读回，不重编码。
    atomic_json(&root.join("source-startup.json"), &Uuid::new_v4())?;
    request_sender(root, 0, "prepare_frozen_source").await?;
    wait_sender(root, 0).await?;
    let bytes = fs::read(root.join("source.batch"))?;
    let batch: Batch = serde_json::from_slice(&bytes)?;
    batch.validate()?;
    let receiver = durable_receiver::open(
        url,
        &cfg,
        &tls.client_hash(),
        &BTreeSet::from([batch.session_id]),
        batch.session_id,
    )
    .await?;
    let mut gate = TelemetryGate::start(receiver.service.clone(), tls.server_tls()).await?;
    atomic_json(
        &root.join("ready.json"),
        &json!({"version":1,"endpoint":gate.endpoint,
        "node_id":cfg.emissary_id,"startup_id":batch.session_id}),
    )?;
    let source = cfg
        .root
        .join("telemetry/00000000000000000000-frozen-source.batch");
    let fixture = Fixture {
        cfg,
        source,
        root: root.to_owned(),
        batch,
        bytes,
    };
    let result = deliveries(&fixture, &receiver.pool, &gate).await;
    let closed = gate.close().await;
    result?;
    closed?;
    ensure!(
        gate.legacy_wire()?,
        "old sender unexpectedly negotiated or changed schema default"
    );
    let node = Uuid::parse_str(&fixture.cfg.emissary_id)?;
    let latest = receiver
        .service
        .store
        .latest_for_startup(node, fixture.batch.session_id)
        .await
        .map_err(|_| anyhow::anyhow!("legacy startup projection read-back failed"))?;
    ensure!(
        latest
            .host
            .as_ref()
            .is_some_and(|value| value["session_id"] == fixture.batch.session_id.to_string()),
        "legacy host missing from current startup projection"
    );
    let inventory = latest.inventory.context("legacy inventory missing")?;
    ensure!(
        inventory["provenance"] == "legacy" && inventory["startup_id"].is_null(),
        "legacy inline inventory was presented as independent delivery"
    );
    ensure!(
        receiver
            .service
            .store
            .latest_for_startup(node, Uuid::new_v4())
            .await
            .map_err(|_| anyhow::anyhow!("unrelated startup projection read-back failed"))?
            .host
            .is_none(),
        "legacy host leaked into unrelated startup projection"
    );
    let independent: i64 = sql::scalar(
        &mut receiver.pool.clone(),
        "SELECT count(*) FROM inventory_deliveries",
        params![],
    )
    .await?;
    ensure!(
        independent == 0,
        "old sender fabricated an independent inventory delivery"
    );
    Ok(
        json!({"passed":true,"boundary":"frozen legacy Handle and generated wire to current Service/store",
        "native_service_executed":false,"old_sender_executed":true,"current_receiver_executed":true,
        "real_business_migrations":true,"transaction_fence_observed":true,
        "source_bytes_retained_before_commit":true,"source_deleted_after_ack":true,
        "original_id_replay_unique":true,"new_host_persisted":true,"legacy_schema_default_accepted":true,
        "old_sender_negotiation_calls":0,"old_sender_wire_schema_versions":[0],
        "second_process_duplicate_ack_observed":true,"legacy_inventory_projection_verified":true,
        "authorization_acl_proven":false}),
    )
}

struct Fixture {
    cfg: Config,
    source: PathBuf,
    root: PathBuf,
    batch: Batch,
    bytes: Vec<u8>,
}

async fn deliveries(fixture: &Fixture, pool: &toasty::Db, gate: &TelemetryGate) -> Result<()> {
    let mut db = pool.clone();
    let mut held = db.transaction().await?;
    sql::execute(&mut held, FENCE, params![]).await?;
    let mut child = Sender::start(&fixture.root, 1).await?;
    let result = async {
        tokio::time::timeout(Duration::from_secs(35), async {
            loop {
                if gate.saw_batch(fixture.batch.id)? {break;}
                child.running()?;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await.context("frozen old Handle did not upload original wire")??;
        ensure!(fs::read(&fixture.source)? == fixture.bytes, "old source changed before ACK");
        gate.open();
        tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                let blocked:i64=sql::scalar(&mut pool.clone(),
                    "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'",params![]).await?;
                if blocked>=1 {break;}
                child.running()?;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await.context("legacy request did not reach real transaction fence")??;
        let count:i64=sql::scalar(&mut pool.clone(),"SELECT count(*) FROM telemetry_batches",params![]).await?;
        ensure!(count==0 && fs::read(&fixture.source)? == fixture.bytes,"old source was removed before transaction commit");
        held.rollback().await?;
        delivered(fixture,pool,gate,&mut child,None).await?;
        child.finish(&fixture.root).await?;
        // 新进程重新打开同root，并明确重投相同正文。只在前个旧进程退出后重写源文件。
        ensure!(!fixture.source.try_exists()?, "original old sender left source pending");
        durable_receiver::verify_database(pool, Uuid::parse_str(&fixture.cfg.emissary_id)?,
            &[(fixture.source.clone(),fixture.bytes.clone(),fixture.batch.clone())],&[]).await?;
        fs::remove_file(fixture.root.join("finish"))?;
        fs::remove_file(fixture.root.join("started"))?;
        let previous_duplicates = gate.duplicate_acks(fixture.batch.id)?;
        child = Sender::start(&fixture.root, 2).await?;
        delivered(fixture,pool,gate,&mut child,Some(previous_duplicates)).await?;
        child.finish(&fixture.root).await?;
        durable_receiver::verify_database(pool, Uuid::parse_str(&fixture.cfg.emissary_id)?,
            &[(fixture.source.clone(),fixture.bytes.clone(),fixture.batch.clone())],&[]).await?;
        Ok::<_,anyhow::Error>(())
    }.await;
    let stopped = child.close().await;
    result?;
    stopped?;
    Ok(())
}

async fn delivered(
    fixture: &Fixture,
    pool: &toasty::Db,
    gate: &TelemetryGate,
    child: &mut Sender,
    previous_duplicates: Option<usize>,
) -> Result<()> {
    // 必须先观察该次旧进程确实发布源文件，不能把启动前不存在误判为已ACK。
    tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            child.running()?;
            let count: i64 = sql::scalar(
                &mut pool.clone(),
                "SELECT count(*) FROM host_samples WHERE node_id=$1 AND batch_id<>$2",
                params![Uuid::parse_str(&fixture.cfg.emissary_id)?, fixture.batch.id],
            )
            .await?;
            if fixture.root.join("started").try_exists()?
                && !fixture.source.try_exists()?
                && count > 0
                && match previous_duplicates {
                    Some(previous) => gate.duplicate_acks(fixture.batch.id)? > previous,
                    None => true,
                }
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("legacy source ACK or new host sample missing")??;
    Ok(())
}

fn driver_root() -> Result<PathBuf> {
    std::env::var_os("ARTIFACTORY_LEGACY_SENDER_DIR")
        .map(PathBuf::from)
        .context("private sender directory required")
}

fn name_from_url(url: &str) -> Result<String> {
    Ok(url::Url::parse(url)?
        .path()
        .trim_start_matches('/')
        .to_owned())
}

#[tokio::test]
#[ignore = "driver-only absent check before authorizing exact owned-name cleanup"]
async fn reserve_frozen_sender_database() {
    let root = driver_root().expect("private reservation directory required");
    let name =
        std::env::var("ARTIFACTORY_LEGACY_DATABASE_NAME").expect("owned database name required");
    durable_receiver::assert_absent(&name)
        .await
        .expect("owned database name already exists");
    atomic_json(
        &root.join("database-reserved.json"),
        &json!({"name":name,"absent_before_run":true}),
    )
    .expect("owned database reservation evidence failed");
}

#[tokio::test]
#[ignore = "driver-only cleanup for its exact private owned database manifest"]
async fn cleanup_frozen_sender_database() {
    let root = driver_root().expect("private cleanup directory required");
    let name =
        std::env::var("ARTIFACTORY_LEGACY_DATABASE_NAME").expect("owned database name required");
    let reserved: Value = serde_json::from_slice(
        &fs::read(root.join("database-reserved.json")).expect("reservation manifest required"),
    )
    .expect("invalid reservation manifest");
    assert_eq!(
        reserved,
        json!({"name":name,"absent_before_run":true}),
        "owned database reservation differs"
    );
    durable_receiver::cleanup_owned(&name)
        .await
        .expect("owned database cleanup failed");
    atomic_json(
        &root.join("database-cleaned.json"),
        &json!({"name":name,"database_removed":true}),
    )
    .expect("cleanup evidence failed");
}
