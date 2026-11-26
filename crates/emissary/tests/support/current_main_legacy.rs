//! [INPUT]: 完整当前Telemetry进程、双库存储和父驱动监督的冻结旧Handle。
//! [OUTPUT]: 验证事务ACK、第二进程重放、原正文唯一和legacy清单投影。
//! [POS]: 当前main门禁的旧发送子场景；旧源码只加入测试入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    durable_receiver,
    frozen_sender::{Sender, request_sender, wait_sender},
    support,
};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary::config::{Config, atomic_json, private_dir};
use artifactory_emissary_protocol::{nodes::MetadataSnapshot, telemetry::Batch};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use uuid::Uuid;

pub async fn exercise(
    directory: &Path,
    fixture: &support::Fixture,
    business: &toasty::Db,
    telemetry: &toasty::Db,
    client: &reqwest::Client,
    metrics_url: &str,
    token: &str,
) -> Result<Value> {
    let root = directory.join("frozen");
    private_dir(&root)?;
    let mut cfg = Config {
        root: root.join("managed"),
        emissary_id: fixture.cfg.emissary_id.clone(),
        telemetry_endpoint: fixture.cfg.telemetry_endpoint.clone(),
        ..Default::default()
    };
    fixture.tls().write_config(&root, &mut cfg)?;
    let startup = Uuid::new_v4();
    atomic_json(&root.join("source-startup.json"), &startup)?;
    request_sender(&root, 0, "prepare_frozen_source").await?;
    wait_sender(&root, 0).await?;
    let bytes = fs::read(root.join("source.batch"))?;
    let batch: Batch = serde_json::from_slice(&bytes)?;
    batch.validate()?;
    ensure!(batch.session_id == startup, "frozen source startup differs");
    let node = Uuid::parse_str(&cfg.emissary_id)?;
    durable_receiver::register_startup(
        business,
        node,
        &MetadataSnapshot::new(startup, Default::default(), Vec::new())?,
    )
    .await?;
    atomic_json(
        &root.join("ready.json"),
        &json!({"version":1,"endpoint":cfg.telemetry_endpoint,
        "node_id":cfg.emissary_id,"startup_id":startup}),
    )?;
    let mut source = cfg
        .root
        .join("telemetry/00000000000000000000-frozen-source.batch");
    let receipt = vec![(source.clone(), bytes.clone(), batch.clone())];
    let before_inventory: i64 = sql::scalar(
        &mut telemetry.clone(),
        "SELECT count(*) FROM inventory_deliveries WHERE node_id=$1",
        params![node],
    )
    .await?;
    let mut writer = telemetry.clone();
    let mut fence = writer.transaction().await?;
    sql::execute(&mut fence, durable_receiver::FENCE, params![]).await?;
    let mut child = Sender::start(&root, 1).await?;
    let result = async {
        tokio::time::timeout(Duration::from_secs(12),async {
            loop {
                child.running()?;
                let blocked:i64=sql::scalar(&mut telemetry.clone(),"SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'",params![]).await?;
                if blocked>0 && root.join("started").try_exists()? {break;}
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await.context("old Handle did not reach actual main transaction fence")??;
        ensure!(fs::read(&source)? == bytes,"old source changed before commit");
        let count:i64=sql::scalar(&mut telemetry.clone(),"SELECT count(*) FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2",params![node,batch.id]).await?;
        ensure!(count==0,"old source ACK preceded transaction commit");
        fence.rollback().await?;
        delivered(&root,&source,node,startup,batch.id,0,telemetry,&child).await?;
        child.finish(&root).await?;
        no_drops(source.parent().context("old queue directory missing")?)?;
        durable_receiver::verify_database(telemetry,node,&receipt,&[]).await?;
        let prior_hosts:i64=sql::scalar(&mut telemetry.clone(),"SELECT count(*) FROM host_samples WHERE node_id=$1 AND session_id=$2 AND batch_id<>$3",params![node,startup,batch.id]).await?;
        ensure!(prior_hosts>0,"old Collector did not create a new host sample");
        fs::remove_file(root.join("finish"))?;
        fs::remove_file(root.join("started"))?;
        // 第二进程使用全新队列；第一进程未确认的其他采样保留，不能解释duplicate计数。
        ensure!(!root.join("managed-replay").try_exists()?,"replay queue already exists");
        atomic_json(&root.join("ready.json"), &json!({"version":1,"endpoint":cfg.telemetry_endpoint,
            "node_id":cfg.emissary_id,"startup_id":startup,"queue_directory":"managed-replay"}))?;
        source=root.join("managed-replay/telemetry/00000000000000000000-frozen-source.batch");
        idle(client,metrics_url,token).await?;
        let previous = counter(client,metrics_url,token,"artifactory_telemetry_replayed_total").await?;
        let rejected = counter(client,metrics_url,token,"artifactory_telemetry_rejected_total").await?;
        child = Sender::start(&root,2).await?;
        delivered(&root,&source,node,startup,batch.id,prior_hosts,telemetry,&child).await?;
        child.finish(&root).await?;
        no_drops(source.parent().context("old queue directory missing")?)?;
        let current = counter(client,metrics_url,token,"artifactory_telemetry_replayed_total").await?;
        ensure!(counter(client,metrics_url,token,"artifactory_telemetry_rejected_total").await?==rejected,"actual main rejected a replay-window request");
        ensure!(current>previous,"actual main did not report another duplicate ACK");
        durable_receiver::verify_database(telemetry,node,&receipt,&[]).await?;
        let after_inventory:i64=sql::scalar(&mut telemetry.clone(),"SELECT count(*) FROM inventory_deliveries WHERE node_id=$1",params![node]).await?;
        ensure!(after_inventory==before_inventory,"old sender created independent inventory receipts");
        let base=metrics_url.strip_suffix("/metrics").context("invalid metrics endpoint")?;
        let latest:Value=client.get(format!("{base}/v1/nodes/{node}/latest?startup_id={startup}"))
            .bearer_auth(token).send().await?.error_for_status()?.json().await?;
        ensure!(latest["inventory"]["provenance"]=="legacy" && latest["inventory"]["startup_id"].is_null(),
            "old inline inventory lost its legacy projection");
        Ok::<_,anyhow::Error>(())
    }.await;
    let closed = child.close().await;
    result?;
    closed?;
    Ok(
        json!({"frozen_sender_executed":true,"actual_current_main":true,
        "transaction_fence_observed":true,"source_bytes_retained_before_commit":true,
        "second_process_original_source_acked":true,"original_id_digest_time_startup_unique":true,
        "actual_main_duplicate_counter_increased":true,"no_replay_window_rejection":true,"both_old_queues_zero_drops":true,"new_collector_host_persisted":true,
        "legacy_inventory_projection_verified":true,"independent_inventory_receipts_unchanged":true,
        "wire_schema_basis":"unchanged frozen generated protocol; live per-RPC schema not instrumented"}),
    )
}

#[allow(clippy::too_many_arguments)]
async fn delivered(
    root: &Path,
    source: &Path,
    node: Uuid,
    startup: Uuid,
    batch: Uuid,
    previous_hosts: i64,
    pool: &toasty::Db,
    child: &Sender,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            child.running()?;
            let hosts: i64 = sql::scalar(
                &mut pool.clone(),
                "SELECT count(*) FROM host_samples WHERE node_id=$1 AND session_id=$2 AND batch_id<>$3",
                params![node, startup, batch],
            )
            .await?;
            if root.join("started").try_exists()? && !source.try_exists()? && hosts > previous_hosts {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("old Handle source ACK missing on actual main")?
}

async fn counter(client: &reqwest::Client, url: &str, token: &str, name: &str) -> Result<u64> {
    let body = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    body.lines()
        .find_map(|line| line.strip_prefix(&format!("{name} ")))
        .context("actual main replay counter missing")?
        .parse()
        .context("invalid replay counter")
}

fn no_drops(queue: &Path) -> Result<()> {
    let path = queue.join("stats.json");
    if path.try_exists()? {
        let stats: Value = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(
            stats["dropped"] == 0,
            "old source deletion could be a permanent rejection or quota eviction"
        );
    }
    Ok(())
}

async fn idle(client: &reqwest::Client, url: &str, token: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        while counter(client, url, token, "artifactory_telemetry_ingest_inflight").await? != 0 {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("actual main still has prior in-flight requests")?
}

pub async fn verify_after_shutdown(directory: &Path, node: Uuid, pool: &toasty::Db) -> Result<()> {
    let root = directory.join("frozen");
    let bytes = fs::read(root.join("source.batch"))?;
    let batch: Batch = serde_json::from_slice(&bytes)?;
    durable_receiver::verify_database(
        pool,
        node,
        &[(
            root.join("managed-replay/telemetry/00000000000000000000-frozen-source.batch"),
            bytes,
            batch,
        )],
        &[],
    )
    .await
}
