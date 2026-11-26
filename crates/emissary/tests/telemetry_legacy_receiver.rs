//! [INPUT]: 冻结旧接收器的私有就绪文件、真实mTLS和随机Timescale数据库。
//! [OUTPUT]: 验证当前发送器原字节重放、新采样入库和独立清单保留。
//! [POS]: 由scripts门禁启动的跨版本集成测试；只读旧数据库。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, anyhow, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_database, with_options};
use artifactory_emissary::{config::Config, telemetry};
use artifactory_emissary_protocol::{InventoryObservation, telemetry::Batch};
use prost::Message;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
struct Ready {
    version: u8,
    endpoint: String,
    node_id: Uuid,
    startup_id: Uuid,
    database_name: String,
    #[serde(default)]
    receiver_boundary: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run through scripts/check_emissary_legacy_receiver.py"]
async fn current_sender_to_frozen_legacy_receiver() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("ARTIFACTORY_LEGACY_FIXTURE_DIR")
            .ok_or_else(|| anyhow!("legacy fixture directory is required"))?,
    );
    let ready: Ready = serde_json::from_slice(&fs::read(root.join("ready.json"))?)?;
    ensure!(ready.version == 1, "unsupported fixture version");
    ensure!(
        ready.database_name.starts_with("legacy_receiver_")
            && ready.database_name.len() == "legacy_receiver_".len() + 32
            && ready.database_name["legacy_receiver_".len()..]
                .chars()
                .all(|value| value.is_ascii_hexdigit()),
        "fixture database must have a random isolated name"
    );
    let admin_url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")
        .map_err(|_| anyhow!("isolated database connection is required"))?;
    let scoped = with_database(&admin_url, &ready.database_name)
        .map_err(|_| anyhow!("fixture database configuration failed"))?;
    let readonly = with_options(&scoped, &[("default_transaction_read_only", "on")])
        .map_err(|_| anyhow!("fixture read-only configuration failed"))?;
    let mut database = connect(&readonly, Default::default(), PoolSettings::new(2))
        .await
        .map_err(|_| anyhow!("fixture read-only connection failed"))?;
    let cfg = Config {
        endpoint: ready.endpoint.clone(),
        telemetry_endpoint: ready.endpoint,
        ca_file: root.join("ca.pem"),
        certificate_file: root.join("client.pem"),
        key_file: root.join("client.key"),
        root: root.join("managed"),
        emissary_id: ready.node_id.to_string(),
        ..Default::default()
    };
    let queue = cfg.root.join("telemetry");
    fs::create_dir_all(&queue)?;
    let mut original: Batch = serde_json::from_slice(include_bytes!(
        "../../emissary-protocol/tests/fixtures/telemetry-batch.json"
    ))?;
    original.id = Uuid::new_v4();
    original.session_id = ready.startup_id;
    original.sampled_at = chrono::Utc::now();
    original.processes.clear();
    original.dropped_batches = 0;
    original.omitted_processes = 0;
    let inventory = original.inventory.clone().expect("fixture inventory");
    let bytes = serde_json::to_vec(&original)?;
    let digest = artifactory_emissary_protocol::telemetry_negotiation::sha256(&bytes);
    let source = queue.join("00000000000000000000-frozen-source.batch");
    fs::write(&source, &bytes)?;
    let handle = telemetry::Handle::start(cfg.clone(), ready.startup_id, inventory.clone());
    let result = async {
        tokio::time::timeout(Duration::from_secs(40), async {
            loop {
                let count: i64 = sql::scalar(
                    &mut database,
                    "SELECT count(*) FROM host_samples WHERE node_id=$1 AND batch_id<>$2",
                    params![ready.node_id, original.id],
                )
                .await
                .map_err(|_| anyhow!("new host read-back failed"))?;
                let metrics = handle.snapshot().expect("telemetry snapshot lock").metrics;
                if !source.exists()
                    && count > 0
                    && metrics["telemetry"]["inventory_state"] == "waiting_inventory_receiver"
                {
                    break Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| anyhow!("legacy delivery deadline exceeded"))??;
        let stored: String = sql::scalar(
            &mut database,
            "SELECT digest FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2",
            params![ready.node_id, original.id],
        )
        .await
        .map_err(|_| anyhow!("original digest read-back failed"))?;
        ensure!(stored == digest, "old store must persist the exact source digest");
        let count: i64 = sql::scalar(
            &mut database,
            "SELECT count(*) FROM host_samples WHERE node_id=$1 AND batch_id=$2 AND sampled_at=$3 AND session_id=$4",
            params![ready.node_id, original.id, original.sampled_at, ready.startup_id],
        )
        .await
        .map_err(|_| anyhow!("original host read-back failed"))?;
        ensure!(count == 1, "old store must persist one original host");
        assert_waiting(&handle.snapshot().expect("telemetry snapshot lock").metrics)?;
        inventories(&queue.join("inventories"))
    }
    .await;
    handle.shutdown().await?;
    let preserved = result?;
    ensure!(
        preserved.len() >= 2,
        "startup and extracted legacy inventories must remain"
    );
    ensure!(
        preserved.values().any(|value| {
            InventoryObservation::decode(value.as_slice()).is_ok_and(|observation| {
                observation.source_batch_id == original.id.to_string()
                    && observation.observed_at_ms == original.sampled_at.timestamp_millis()
                    && observation.source_startup_id == original.session_id.to_string()
            })
        }),
        "legacy inventory must retain its original batch identity and timestamp"
    );

    // 重启后仅重建相同来源，确认旧端业务幂等而非只测试本地去重。
    fs::write(&source, &bytes)?;
    let handle = telemetry::Handle::start(cfg, ready.startup_id, inventory);
    let replay = async {
        tokio::time::timeout(Duration::from_secs(15), async {
            while source.exists() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .map_err(|_| anyhow!("duplicate acknowledgement deadline exceeded"))?;
        let batches: i64 = sql::scalar(
            &mut database,
            "SELECT count(*) FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2 AND digest=$3",
            params![ready.node_id, original.id, digest.clone()],
        )
        .await
        .map_err(|_| anyhow!("duplicate batch read-back failed"))?;
        let hosts: i64 = sql::scalar(
            &mut database,
            "SELECT count(*) FROM host_samples WHERE node_id=$1 AND batch_id=$2",
            params![ready.node_id, original.id],
        )
        .await
        .map_err(|_| anyhow!("duplicate host read-back failed"))?;
        ensure!(batches == 1 && hosts == 1, "duplicate replay must stay unique");
        let retained = inventories(&queue.join("inventories"))?;
        for (name, bytes) in &preserved {
            ensure!(retained.get(name) == Some(bytes), "pending inventory bytes changed");
        }
        let metrics = handle.snapshot().expect("telemetry snapshot lock").metrics;
        assert_waiting(&metrics)?;
        Ok::<_, anyhow::Error>(json!({
            "version":1,"source_batch_id":original.id,"source_digest":digest,
            "source_sampled_at":original.sampled_at,"exact_digest_persisted":true,
            "source_deleted_after_ack":true,"new_host_persisted":true,
            "duplicate_batches":batches,"duplicate_hosts":hosts,
            "preserved_inventory_records":preserved.len(),
            "inventory_state":metrics["telemetry"]["inventory_state"],
            "no_fabricated_loss":true,
            "receiver_boundary":ready.receiver_boundary.as_deref().unwrap_or("frozen generated router, Service and store; main migration handshake bypassed")
        }))
    }
    .await;
    handle.shutdown().await?;
    fs::write(
        root.join("sender-evidence.json"),
        serde_json::to_vec_pretty(&replay?)?,
    )?;
    Ok(())
}

fn inventories(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut values = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|value| value == "inventory")
        {
            let bytes = fs::read(entry.path())?;
            InventoryObservation::decode(bytes.as_slice())?.validate()?;
            values.insert(entry.file_name().to_string_lossy().into_owned(), bytes);
        }
    }
    Ok(values)
}

fn assert_waiting(metrics: &Value) -> Result<()> {
    let telemetry = &metrics["telemetry"];
    ensure!(
        telemetry["inventory_state"] == "waiting_inventory_receiver",
        "old router must leave inventory waiting"
    );
    ensure!(
        telemetry["inventory_last_ack_at"].is_null(),
        "old receiver cannot acknowledge independent inventory"
    );
    for key in [
        "dropped_batches",
        "expired_batches",
        "malformed_batches",
        "bounds_exceeded_batches",
        "id_content_conflict_batches",
        "identity_retired_batches",
    ] {
        ensure!(
            telemetry[key].as_u64() == Some(0),
            "legacy success must not fabricate loss: {key}"
        );
    }
    Ok(())
}
