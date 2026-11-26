//! [INPUT]: 冻结旧发送实现、当前接收就绪配置、受限队列目录和原始批次。
//! [OUTPUT]: 运行未改动旧Handle，等待接收方核对后结束进程。
//! [POS]: 驱动复制到冻结旧Emissary测试包；不持有数据库凭据。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_emissary::{config::Config, telemetry};
use artifactory_emissary_protocol::telemetry::Batch;
use serde::Deserialize;
use std::{fs, path::PathBuf, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
struct Ready {
    version: u8,
    endpoint: String,
    node_id: Uuid,
    startup_id: Uuid,
    #[serde(default)]
    queue_directory: Option<String>,
}

#[tokio::test]
#[ignore = "run only through current receiver fixture"]
async fn prepare_frozen_source() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("ARTIFACTORY_LEGACY_SENDER_DIR")
            .context("private sender directory required")?,
    );
    let startup: Uuid = serde_json::from_slice(&fs::read(root.join("source-startup.json"))?)?;
    let mut batch: Batch = serde_json::from_slice(include_bytes!(
        "../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))?;
    batch.id = Uuid::new_v4();
    batch.session_id = startup;
    batch.sampled_at = chrono::Utc::now();
    batch.processes.clear();
    batch.dropped_batches = 0;
    batch.omitted_processes = 0;
    batch.validate()?;
    artifactory_emissary::config::atomic_bytes(
        &root.join("source.batch"),
        &serde_json::to_vec(&batch)?,
        false,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run only through check_emissary_legacy_sender.py"]
async fn frozen_sender_to_current_receiver() -> Result<()> {
    for name in [
        "DATABASE_URL",
        "ARTIFACTORY_TEST_DATABASE_URL",
        "ARTIFACTORY_BUSINESS_DATABASE_URL",
        "ARTIFACTORY_TELEMETRY_READ_DATABASE_URL",
        "ARTIFACTORY_PLANNING_DATABASE_URL",
    ] {
        ensure!(
            std::env::var_os(name).is_none(),
            "database credential reached old sender"
        );
    }
    let root = PathBuf::from(
        std::env::var_os("ARTIFACTORY_LEGACY_SENDER_DIR")
            .context("private sender directory required")?,
    );
    let ready: Ready = serde_json::from_slice(&fs::read(root.join("ready.json"))?)?;
    ensure!(ready.version == 1, "unsupported sender fixture");
    let address = url::Url::parse(&ready.endpoint)?;
    ensure!(
        address.scheme() == "https" && address.host_str() == Some("127.0.0.1"),
        "old sender fixture requires loopback mTLS"
    );
    let bytes = fs::read(root.join("source.batch"))?;
    let batch: Batch = serde_json::from_slice(&bytes)?;
    batch.validate()?;
    ensure!(
        batch.session_id == ready.startup_id && batch.processes.is_empty(),
        "frozen source identity changed"
    );
    let queue_directory = ready.queue_directory.as_deref().unwrap_or("managed");
    ensure!(matches!(queue_directory,"managed" | "managed-replay"),"invalid private queue directory");
    let cfg = Config {
        endpoint: ready.endpoint.clone(),
        telemetry_endpoint: ready.endpoint,
        ca_file: root.join("ca.pem"),
        certificate_file: root.join("client.pem"),
        key_file: root.join("client.key"),
        root: root.join(queue_directory),
        emissary_id: ready.node_id.to_string(),
        ..Default::default()
    };
    let queue = cfg.root.join("telemetry");
    fs::create_dir_all(&queue)?;
    let source = queue.join("00000000000000000000-frozen-source.batch");
    ensure!(!source.try_exists()?, "frozen source queue already exists");
    artifactory_emissary::config::atomic_bytes(&source, &bytes, false)?;
    artifactory_emissary::config::atomic_bytes(&root.join("started"), b"source published", false)?;
    let handle = telemetry::Handle::start(
        cfg,
        ready.startup_id,
        batch.inventory.context("source inventory missing")?,
    );
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if root.join("finish").try_exists()? {
                ensure!(
                    !source.try_exists()?,
                    "receiver finished without source ACK"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("old sender receiver-verification deadline exceeded")?;
    drop(handle);
    result
}
