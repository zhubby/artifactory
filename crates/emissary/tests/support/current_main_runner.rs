//! [INPUT]: 已运行的真实Telemetry main、业务owner连接和同源Control夹具。
//! [OUTPUT]: 验证两次真实Runner启动的Hello提交、队列保留和持久ACK。
//! [POS]: 完整main验收的Runner子场景；Control仍为测试对端。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{durable_receiver, runner_task::Running, support, telemetry_sources::Sources};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_emissary_protocol::nodes::MetadataSnapshot;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use uuid::Uuid;

pub async fn exercise(
    fixture: &mut support::Fixture,
    business: &toasty::Db,
    telemetry: &toasty::Db,
) -> Result<Value> {
    // 同节点另开私有队列目录，既有Handle已关闭；服务端进程和授权角色保持。
    fixture.cfg.root = fixture.cfg.root.with_file_name("runner-node");
    std::fs::create_dir_all(&fixture.cfg.root)?;
    let mut source = Sources::prepare(&fixture.cfg).await?;
    let node = Uuid::parse_str(&fixture.cfg.emissary_id)?;
    for startup in &source.startups {
        let snapshot = MetadataSnapshot::new(*startup, Default::default(), Vec::new())?;
        durable_receiver::register_startup(business, node, &snapshot).await?;
    }
    let registration = business.clone();
    fixture.set_hello_hook(Arc::new(move |hello| {
        let registration = registration.clone();
        Box::pin(async move {
            let value: Value = serde_json::from_str(&hello.payload_json)
                .map_err(|_| tonic::Status::invalid_argument("invalid hello"))?;
            let snapshot: MetadataSnapshot = serde_json::from_value(value["startup"].clone())
                .map_err(|_| tonic::Status::invalid_argument("invalid startup"))?;
            durable_receiver::register_startup(&registration, node, &snapshot)
                .await
                .map_err(|_| tonic::Status::internal("startup transaction failed"))
        })
    }));
    let mut running = Running::new();
    let result = recover(fixture, business, telemetry, &mut source, &mut running).await;
    let closed = running.close().await;
    result?;
    closed?;
    source.journal_retained()?;
    Ok(
        json!({"real_runner_startups":2,"hello_committed_before_publication":true,
        "same_configuration":true,"restart_source_bytes_retained":true,"source_batches":source.batches.len(),
        "source_inventories":source.inventories.len(),"business_ack_removed_sources":true,
        "current_main_endpoint_unchanged":true,"real_control_main":false}),
    )
}

async fn recover(
    fixture: &mut support::Fixture,
    business: &toasty::Db,
    telemetry: &toasty::Db,
    source: &mut Sources,
    running: &mut Running,
) -> Result<()> {
    let mut writer = telemetry.clone();
    let mut fence = writer.transaction().await?;
    sql::execute(&mut fence, durable_receiver::FENCE, params![]).await?;
    let configuration = serde_json::to_value(&fixture.cfg)?;
    let mut seen = BTreeSet::new();
    let node = Uuid::parse_str(&fixture.cfg.emissary_id)?;
    for index in 0..2 {
        running.start(fixture.cfg.clone())?;
        let connection = fixture.connection().await;
        let value: Value = serde_json::from_str(&connection.hello.payload_json)?;
        let snapshot: MetadataSnapshot = serde_json::from_value(value["startup"].clone())?;
        ensure!(
            seen.insert(snapshot.startup_id),
            "restart reused startup identity"
        );
        let digest: String = sql::scalar(
            &mut business.clone(),
            "SELECT digest FROM node_startups WHERE node_id=$1 AND startup_id=$2",
            params![node, snapshot.startup_id],
        )
        .await?;
        ensure!(
            digest == snapshot.digest()?,
            "Hello published before immutable startup commit"
        );
        ensure!(
            serde_json::to_value(&fixture.cfg)? == configuration,
            "Runner configuration changed"
        );
        if index == 0 {
            tokio::time::timeout(Duration::from_secs(12), async {
                loop {
                    let blocked: i64 = sql::scalar(&mut telemetry.clone(), "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'", params![]).await?;
                    if blocked >= 2 { break; }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Ok::<_, anyhow::Error>(())
            }).await.context("Runner transactions did not reach actual main fence")??;
            running.close().await?;
            source.locate_fresh(&fixture.cfg)?;
        }
        source.retained()?;
    }
    // 取消请求的服务端事务仍受同一围栏约束，未提交前绝不构造ACK。
    for (_, _, batch) in &source.batches {
        let count: i64 = sql::scalar(
            &mut telemetry.clone(),
            "SELECT count(*) FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2",
            params![node, batch.id],
        )
        .await?;
        ensure!(count == 0, "Runner source committed before fence release");
    }
    fence.rollback().await?;
    tokio::time::timeout(Duration::from_secs(180), async {
        while !source.removed()? {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("restarted real Runner did not receive actual main ACK")??;
    durable_receiver::verify_database(telemetry, node, &source.batches, &source.inventories)
        .await?;
    Ok(())
}
