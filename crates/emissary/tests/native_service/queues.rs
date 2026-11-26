//! [INPUT]: 依赖公开遥测Handle、有限操作Runtime和隔离配置。
//! [OUTPUT]: 生成有效未确认队列，验证原字节并输出脱敏摘要。
//! [POS]: 原生维护与Runner恢复共用的数据准备；不启动服务。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_emissary::{
    config::Config,
    operations::{Identity, Runtime, ServerClock},
    telemetry,
};
use artifactory_emissary_protocol::{
    InventoryObservation,
    operations::{
        self, InventoryDelivery, OperationCancel, OperationEnvelope, OperationKind, OperationState,
        OperationUpdate,
    },
    telemetry::Batch,
};
use prost::Message;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

// 首轮采样含5–10秒抖动；三十秒限制本地准备，关闭仍等待真实任务回收。
const PREPARE_BUDGET: Duration = Duration::from_secs(30);

/// 通过真实采集、清单刷新和接受后取消生成待确认记录。
pub(crate) async fn prepare(cfg: &Config) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    ensure!(
        cfg.telemetry_endpoint.is_empty(),
        "native queue preparation requires no telemetry receiver"
    );
    let telemetry_root = cfg.root.join("telemetry");
    let operation_root = cfg.root.join("operations");
    ensure!(
        !telemetry_root.try_exists()? && !operation_root.try_exists()?,
        "native queue preparation requires unused queue directories"
    );
    let deadline = tokio::time::Instant::now() + PREPARE_BUDGET;
    let startup = Uuid::new_v4();
    let (_, inventory) = tokio::time::timeout_at(
        deadline,
        telemetry::startup(cfg, startup, &json!({}), Vec::new()),
    )
    .await
    .context("native inventory preparation timed out")??;
    // 空endpoint在URL解析时失败，发送器不会建立TCP连接。
    let handle = telemetry::Handle::start(cfg.clone(), startup, inventory.clone());
    let refresh = tokio::time::timeout_at(deadline, async {
        loop {
            let files = telemetry_files(&telemetry_root)?;
            let ordinary = files
                .iter()
                .any(|path| path.parent() == Some(telemetry_root.join("fresh").as_path()));
            let history = files
                .iter()
                .any(|path| path.parent() == Some(telemetry_root.as_path()));
            if ordinary && history {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        (handle.inventory_sink())(inventory).await
    })
    .await
    .context("native telemetry preparation timed out")
    .and_then(|result| result);
    let shutdown = handle.shutdown().await;
    let refresh = refresh?;
    shutdown.context("native telemetry queue did not close")?;
    ensure!(
        refresh.revision > 0 && refresh.delivery == InventoryDelivery::Pending,
        "inventory refresh was not durably pending"
    );
    let mut queues = telemetry_files(&telemetry_root)?
        .into_iter()
        .map(|path| Ok((path.clone(), fs::read(path)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut ordinary = false;
    let mut history = false;
    let mut independent_startup = false;
    let mut independent_refresh = false;
    for (path, bytes) in &queues {
        let kind = validate(path, bytes)?;
        ordinary |= kind == "ordinary_batch";
        history |= kind == "history_batch";
        independent_startup |= kind == "startup_inventory";
        if kind == "refresh_inventory" {
            let observation = InventoryObservation::decode(bytes.as_slice())?;
            independent_refresh |= observation.observation_id == refresh.observation_id.to_string()
                && observation.revision == refresh.revision;
        }
    }
    ensure!(
        ordinary && history && independent_startup && independent_refresh,
        "native telemetry preparation did not create every queue class"
    );
    prepare_operation(cfg, startup).await?;
    let journal = operation_root.join("journal.json");
    let bytes = fs::read(&journal)?;
    validate(&journal, &bytes)?;
    queues.push((journal, bytes));
    queues.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(queues)
}

fn telemetry_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for (directory, extension) in [
        (root.to_owned(), "batch"),
        (root.join("fresh"), "batch"),
        (root.join("inventories"), "inventory"),
    ] {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|value| value == extension)
            {
                ensure!(entry.file_type()?.is_file(), "queue record is not a file");
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

async fn prepare_operation(cfg: &Config, startup_id: Uuid) -> Result<()> {
    let identity = Identity {
        tenant_id: Uuid::new_v4(),
        node_id: cfg
            .emissary_id
            .parse()
            .context("native node ID is invalid")?,
        startup_id,
        control_session: Uuid::new_v4(),
    };
    let input = operations::validate_input(
        OperationKind::CollectDiagnostics,
        1,
        &json!({"probes":["git"]}),
    )?;
    let now = chrono::Utc::now().timestamp_millis();
    let instant = Instant::now();
    let clock = ServerClock::new(now, instant, instant)?;
    let envelope = OperationEnvelope {
        operation_id: Uuid::new_v4(),
        tenant_id: identity.tenant_id,
        node_id: identity.node_id,
        startup_id,
        control_session: identity.control_session,
        actor_id: Uuid::new_v4(),
        kind: input.kind(),
        schema: 1,
        input: input.value(),
        input_digest: input.digest(),
        issued_at_ms: now,
        expires_at_ms: now + 30_000,
    };
    let operation_id = envelope.operation_id;
    let runtime = Runtime::open(cfg.root.join("operations"), startup_id)?;
    let accepted = runtime
        .accept(envelope, &identity, Some(&clock))
        .await
        .map_err(|reason| anyhow::anyhow!("native operation acceptance failed: {reason:?}"))?;
    ensure!(
        accepted.state == OperationState::Accepted,
        "operation was not accepted"
    );
    let update = runtime
        .cancel(
            &OperationCancel {
                operation_id,
                startup_id,
                control_session: identity.control_session,
            },
            &identity,
        )
        .await?;
    update.validate()?;
    ensure!(
        update.state == OperationState::Canceled
            && !runtime.active().await
            && runtime.updates().await.iter().any(|pending| {
                pending.operation_id == operation_id
                    && pending.result_digest == update.result_digest
            }),
        "native operation terminal was not durably pending"
    );
    runtime.shutdown(Duration::from_secs(1)).await?;
    drop(runtime);
    Ok(())
}

/// 解码真实记录并返回证据类别；不重新编码或写入正文。
pub(crate) fn validate(path: &Path, bytes: &[u8]) -> Result<&'static str> {
    if path.extension().is_some_and(|value| value == "batch") {
        let batch: Batch = serde_json::from_slice(bytes)?;
        batch.validate()?;
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|value| value == "fresh")
        {
            ensure!(
                batch.host.is_some() && batch.inventory.is_none() && batch.processes.is_empty(),
                "ordinary batch contains another queue class"
            );
            return Ok("ordinary_batch");
        }
        ensure!(
            batch.inventory.is_some() || !batch.processes.is_empty(),
            "history batch is empty"
        );
        return Ok("history_batch");
    }
    if path.extension().is_some_and(|value| value == "inventory") {
        let observation = InventoryObservation::decode(bytes)?;
        observation.validate()?;
        ensure!(
            path.file_stem()
                .is_some_and(|value| value == observation.observation_id.as_str()),
            "inventory filename differs from its identity"
        );
        return Ok(if !observation.source_batch_id.is_empty() {
            "legacy_inventory"
        } else if observation.revision == 0 {
            "startup_inventory"
        } else {
            "refresh_inventory"
        });
    }
    ensure!(
        path.file_name()
            .is_some_and(|value| value == "journal.json"),
        "unknown queue record"
    );
    let journal: Value = serde_json::from_slice(bytes)?;
    let entries = journal["entries"]
        .as_array()
        .context("operation entries missing")?;
    ensure!(!entries.is_empty(), "operation journal is empty");
    for entry in entries {
        let envelope: OperationEnvelope = serde_json::from_value(entry["envelope"].clone())?;
        envelope.validate()?;
        let update: OperationUpdate = serde_json::from_value(entry["update"].clone())?;
        update.validate()?;
        ensure!(
            update.operation_id == envelope.operation_id
                && update.startup_id == envelope.startup_id
                && update.state.terminal()
                && update.result.is_some()
                && entry["acknowledged"] == false
                && entry["finished_at"].as_str().is_some(),
            "operation journal is not an unacknowledged terminal"
        );
    }
    Ok("terminal_operation")
}

/// 用调用者读取的原生私有文件验证字节和协议，避免重开权限。
pub(crate) fn verify(path: &Path, expected: &[u8], current: &[u8]) -> Result<()> {
    ensure!(
        expected == current,
        "native maintenance changed queue bytes"
    );
    validate(path, current)?;
    Ok(())
}

/// 仅公开类别、数量、字节数和摘要，排除路径与机器正文。
pub(crate) fn evidence(queues: &[(PathBuf, Vec<u8>)]) -> Result<Value> {
    let mut categories = BTreeMap::<&str, Vec<Value>>::new();
    for (path, bytes) in queues {
        categories
            .entry(validate(path, bytes)?)
            .or_default()
            .push(json!({
                "bytes": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            }));
    }
    Ok(Value::Array(categories.into_iter().map(|(category, records)| {
        json!({"category":category,"count":records.len(),"records":records})
    }).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_pending_queues_validate_and_release_writer_locks() {
        let root = tempfile::tempdir().unwrap();
        let cfg = Config {
            root: root.path().join("node"),
            emissary_id: Uuid::new_v4().to_string(),
            ..Default::default()
        };
        fs::create_dir(&cfg.root).unwrap();
        let queues = prepare(&cfg).await.unwrap();
        let proof = evidence(&queues).unwrap();
        let categories = proof.as_array().unwrap();
        for required in [
            "ordinary_batch",
            "history_batch",
            "startup_inventory",
            "refresh_inventory",
            "terminal_operation",
        ] {
            assert!(
                categories.iter().any(|value| value["category"] == required),
                "missing {required}"
            );
        }
        let bytes = serde_json::to_string(&proof).unwrap();
        assert!(!bytes.contains(root.path().to_str().unwrap()));
        let inventory = queues
            .iter()
            .find_map(|(path, bytes)| {
                (validate(path, bytes).unwrap() == "startup_inventory").then(|| {
                    InventoryObservation::decode(bytes.as_slice())
                        .unwrap()
                        .validate()
                        .unwrap()
                })
            })
            .unwrap();
        let reopened = telemetry::Handle::start(cfg.clone(), Uuid::new_v4(), inventory.clone());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if (reopened.inventory_sink())(inventory.clone()).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("telemetry writer lock remained held");
        reopened.shutdown().await.unwrap();
        let operations = Runtime::open(cfg.root.join("operations"), Uuid::new_v4()).unwrap();
        assert!(!operations.active().await);
        assert_eq!(operations.updates().await.len(), 1);
        operations.shutdown(Duration::from_secs(1)).await.unwrap();
        drop(operations);
        for (path, expected) in &queues {
            let actual = fs::read(path).unwrap();
            verify(path, expected, &actual).unwrap();
        }
        let (path, expected) = &queues[0];
        let mut changed = expected.clone();
        changed[0] ^= 1;
        assert!(verify(path, expected, &changed).is_err());
    }
}
