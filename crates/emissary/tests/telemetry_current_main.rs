//! [INPUT]: 父驱动监督的当前Telemetry二进制、双库资源与真实Runner。
//! [OUTPUT]: 验证完整启动、授权拒绝、重启、积压新鲜度与mTLS事务ACK。
//! [POS]: 普通进程双库门禁；不替代真实Control注册、原生服务或滚动部署。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[path = "support/current_main_fairness.rs"]
mod current_main_fairness;
#[path = "support/current_main_legacy.rs"]
mod current_main_legacy;
#[path = "support/current_main_runner.rs"]
mod current_main_runner;
#[path = "support/current_main_databases.rs"]
mod databases;
#[path = "support/durable_receiver.rs"]
mod durable_receiver;
#[path = "support/frozen_sender.rs"]
mod frozen_sender;
#[path = "native_service/queues.rs"]
mod queues;
#[path = "support/runner_task.rs"]
mod runner_task;
mod support;
#[path = "support/telemetry_gate.rs"]
mod telemetry_gate;
#[path = "support/telemetry_sources.rs"]
mod telemetry_sources;
use anyhow::{Context, Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql};
use artifactory_emissary::{config::atomic_json, telemetry};
use databases::{Resources, root};
use serde_json::{Value, json};
use std::{fs, time::Duration};
use telemetry_sources::Sources;
use uuid::Uuid;

#[tokio::test]
#[ignore = "private current-main driver only; reserves exact owned resources"]
async fn reserve_current_main_resources() {
    Resources::read().unwrap().reserve().await.unwrap();
}

#[tokio::test]
#[ignore = "private current-main driver only; independently confirms exact cleanup"]
async fn cleanup_current_main_resources() {
    Resources::read().unwrap().cleanup().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicit parent process supervision and loopback dual databases"]
async fn actual_current_main_uses_separate_databases_and_restricted_business_role() {
    let budget = if std::env::var_os("ARTIFACTORY_CURRENT_MAIN_FAIRNESS").is_some() {
        450
    } else {
        290
    };
    let proof = tokio::time::timeout(Duration::from_secs(budget), exercise())
        .await
        .expect("current main acceptance timed out")
        .expect("current main acceptance failed; inspect private logs");
    atomic_json(&root().unwrap().join("proof.json"), &proof).unwrap();
}

async fn phase(index: usize, phase: &str) -> Result<()> {
    atomic_json(
        &root()?.join(format!("request-{index}.json")),
        &json!({"phase":phase}),
    )?;
    tokio::time::timeout(Duration::from_secs(45), async {
        let path = root()?.join(format!("result-{index}.json"));
        loop {
            if path.try_exists()? {
                let value: Value = serde_json::from_slice(&fs::read(path)?)?;
                ensure!(
                    value == json!({"phase":phase,"success":true}),
                    "parent process phase failed"
                );
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("supervised main phase deadline exceeded")?
}

fn address() -> Result<String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.to_string())
}

async fn exercise() -> Result<Value> {
    let resources = Resources::read()?;
    resources.create().await?;
    let directory = root()?;
    let mut fixture = support::Fixture::start(&directory, &[&rustls::version::TLS13]).await;
    fs::create_dir_all(&fixture.cfg.root)?;
    let mut source = Sources::prepare(&fixture.cfg).await?;
    let source_proof = queues::evidence(&source.files)?;
    let current = *source.startups.first().context("source startup missing")?;
    let mut business = connect(
        &resources.url(&resources.business)?,
        Default::default(),
        PoolSettings::new(2),
    )
    .await?;
    durable_receiver::seed_business(
        &business,
        &fixture.cfg,
        &fixture.tls().client_hash(),
        &source.startups,
        current,
    )
    .await?;
    let authorization = resources.grant(&mut business).await?;
    let pki = directory.join("pki");
    artifactory_emissary::config::private_dir(&pki)?;
    fixture.tls().write_server(&pki)?;
    let grpc = address()?;
    let http = address()?;
    ensure!(grpc != http, "main listeners collided");
    fixture.cfg.telemetry_endpoint = format!("https://{grpc}");
    let token = Uuid::new_v4().simple().to_string();
    atomic_json(
        &directory.join("service-env.json"),
        &json!({
            "DATABASE_URL":resources.url(&resources.telemetry)?,
            "ARTIFACTORY_BUSINESS_DATABASE_URL":authorization,
            "ARTIFACTORY_TELEMETRY_GRPC_BIND":grpc,"ARTIFACTORY_TELEMETRY_HTTP_BIND":http,
            "ARTIFACTORY_PKI_DIR":pki,"ARTIFACTORY_SERVICE_TLS_CERT":pki.join("server.pem"),
            "ARTIFACTORY_SERVICE_TLS_KEY":pki.join("server.key"),
            "ARTIFACTORY_TELEMETRY_TOKEN":token,"ARTIFACTORY_TELEMETRY_WRITE_CONNECTIONS":"4",
            "ARTIFACTORY_TELEMETRY_READ_CONNECTIONS":"2","ARTIFACTORY_TELEMETRY_AUTH_CONNECTIONS":"2"
        }),
    )?;
    sql::execute(
        &mut business,
        &format!("REVOKE SELECT ON node_startups FROM {}", resources.role),
        params![],
    )
    .await?;
    phase(0, "missing_startup_grant").await?;
    // 失败main已完成Telemetry迁移，业务库的真实历史不由测试伪造。
    let telemetry = connect(
        &resources.url(&resources.telemetry)?,
        artifactory_telemetry_service::store::models(),
        PoolSettings::new(6),
    )
    .await?;
    artifactory_database::validate(
        &telemetry,
        &artifactory_telemetry_service::store::MIGRATIONS,
    )
    .await?;
    sql::execute(
        &mut business,
        &format!("GRANT SELECT ON node_startups TO {}", resources.role),
        params![],
    )
    .await?;
    phase(1, "authorized_main").await?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .add_root_certificate(reqwest::Certificate::from_pem(&fs::read(
            pki.join("ca.pem"),
        )?)?)
        .build()?;
    let metrics = format!("https://{http}/metrics");
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(response) = client.get(&metrics).bearer_auth(&token).send().await
                && response.status().is_success()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .context("strict CA/IP HTTPS readiness failed")?;
    ensure!(
        client.get(&metrics).send().await?.status() == reqwest::StatusCode::UNAUTHORIZED,
        "anonymous metrics access accepted"
    );
    ensure!(
        client
            .get(&metrics)
            .bearer_auth("invalid-service-token")
            .send()
            .await?
            .status()
            == reqwest::StatusCode::UNAUTHORIZED,
        "invalid service token accepted"
    );
    let mut writer = telemetry.clone();
    let mut held = writer.transaction().await?;
    sql::execute(&mut held, durable_receiver::FENCE, params![]).await?;
    let sender = telemetry::Handle::start(fixture.cfg.clone(), current, source.inventory.clone());
    let result = delivery(&fixture.cfg, &mut source, &telemetry, &business).await;
    // ACK前断言已结束；释放真实事务围栏，不修改生产超时和限流。
    let released = held.rollback().await;
    let proof = async {
        result?;
        released?;
        tokio::time::timeout(Duration::from_secs(100), async {
            while !source.removed()? {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("real main did not ACK original queue records")??;
        durable_receiver::verify_database(
            &telemetry,
            Uuid::parse_str(&fixture.cfg.emissary_id)?,
            &source.batches,
            &source.inventories,
        )
        .await?;
        source.journal_retained()?;
        let url = format!(
            "https://{http}/v1/nodes/{}/inventory",
            fixture.cfg.emissary_id
        );
        let response = client.get(url).bearer_auth(&token).send().await?;
        ensure!(
            response.status().is_success(),
            "real main inventory query failed"
        );
        let _: Value = response.json().await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let shutdown = sender.shutdown().await;
    proof?;
    shutdown?;
    let runner_proof = current_main_runner::exercise(&mut fixture, &business, &telemetry).await?;
    let fairness_proof = if std::env::var_os("ARTIFACTORY_CURRENT_MAIN_FAIRNESS").is_some() {
        Some(
            current_main_fairness::exercise(
                &fixture, &business, &telemetry, &client, &http, &token,
            )
            .await?,
        )
    } else {
        None
    };
    let legacy_proof = if std::env::var_os("ARTIFACTORY_CURRENT_MAIN_LEGACY").is_some() {
        Some(
            current_main_legacy::exercise(
                &directory, &fixture, &business, &telemetry, &client, &metrics, &token,
            )
            .await?,
        )
    } else {
        None
    };
    phase(2, "stop_main").await?;
    if legacy_proof.is_some() {
        current_main_legacy::verify_after_shutdown(
            &directory,
            Uuid::parse_str(&fixture.cfg.emissary_id)?,
            &telemetry,
        )
        .await?;
    }
    durable_receiver::verify_database(
        &telemetry,
        Uuid::parse_str(&fixture.cfg.emissary_id)?,
        &source.batches,
        &source.inventories,
    )
    .await?;
    Ok(
        json!({"passed":true,"boundary":"ordinary current Telemetry main, real Handle and Runner restart, distinct dual databases and restricted business ACL",
        "actual_current_main":true,"real_control_main":false,"native_service_executed":false,
        "runner":runner_proof,
        "fairness":fairness_proof,
        "frozen_sender":legacy_proof,
        "distinct_databases":true,"authorization_acl_proven":true,"deployment_grant_sql_used":true,
        "missing_startup_grant_rejected_at_startup":true,"real_business_migrations":true,
        "strict_ca_ip_https":true,"anonymous_and_wrong_token_denied":true,
        "transaction_fence_observed":true,"source_bytes_retained_before_commit":true,
        "business_ack_removed_sources":true,"post_process_database_readback":true,
        "terminal_operation_unchanged":true,"source_batches":source.batches.len(),
        "source_inventories":source.inventories.len(),"queue_records":source_proof}),
    )
}

async fn delivery(
    cfg: &artifactory_emissary::config::Config,
    source: &mut Sources,
    telemetry: &toasty::Db,
    business: &toasty::Db,
) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let blocked: i64 = sql::scalar(&mut telemetry.clone(), "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND lower(wait_event)='advisory' AND query LIKE '%pg_advisory_xact_lock_shared(438284731022932)%'", params![]).await?;
            if blocked >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    }).await.context("actual main batch/inventory transactions did not reach fence")??;
    source.locate_fresh(cfg)?;
    source.retained()?;
    let receipts: i64 = sql::scalar(&mut telemetry.clone(), "SELECT (SELECT count(*) FROM telemetry_batches)+(SELECT count(*) FROM inventory_deliveries)", params![]).await?;
    ensure!(receipts == 0, "main acknowledged before commit");
    let forbidden: bool = sql::scalar(
        &mut business.clone(),
        "SELECT to_regclass('public.telemetry_batches') IS NOT NULL",
        params![],
    )
    .await?;
    ensure!(!forbidden, "telemetry data escaped into business database");
    let forbidden: bool = sql::scalar(
        &mut telemetry.clone(),
        "SELECT to_regclass('public.nodes') IS NOT NULL",
        params![],
    )
    .await?;
    ensure!(
        !forbidden,
        "business tables escaped into telemetry database"
    );
    Ok(())
}
