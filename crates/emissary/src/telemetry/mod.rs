//! [INPUT]: 依赖固定主机诊断、Product 型号、平台接口枚举与有界磁盘队列。
//! [OUTPUT]: 固定启动元数据，隔离清单存储故障并异步发送资源采样。
//! [POS]: 独立于任务租约和构建日志的节点遥测循环。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod collector;
mod durability;
pub(crate) mod inventory;
mod inventory_delivery;
mod inventory_lane;
mod inventory_upload;
mod managed;
mod network;
mod sample_bounds;
mod scheduler;
mod spool;
mod storage_observation;
mod upload;
#[cfg(any(windows, test))]
mod windows_network;

use crate::config::Config;
use collector::Collector;
pub use managed::Registry;
use serde_json::{Value, json};
use spool::Spool;
use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Capture static observations once, before the runtime opens any control connection.
pub async fn startup(
    cfg: &Config,
    startup_id: uuid::Uuid,
    diagnostics: &Value,
    capabilities: Vec<String>,
) -> anyhow::Result<(
    artifactory_emissary_protocol::nodes::MetadataSnapshot,
    artifactory_emissary_protocol::telemetry::Inventory,
)> {
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    system.refresh_memory();
    let mut inventory = inventory::collect(&system, diagnostics, capabilities.clone());
    inventory.accelerators = inventory::accelerators(cfg).await;
    inventory.collectors.insert(
        "accelerator_inventory".into(),
        if inventory.accelerators.is_empty() {
            "unknown_or_unsupported"
        } else {
            "available"
        }
        .into(),
    );
    if inventory
        .accelerators
        .iter()
        .any(|card| card.vendor == "NVIDIA")
    {
        inventory
            .collectors
            .insert("accelerator_utilization".into(), "nvidia-smi".into());
    }
    let mut metadata = std::collections::BTreeMap::new();
    let value = serde_json::to_value(&inventory)?;
    for (key, value) in value
        .as_object()
        .expect("inventory serializes to an object")
    {
        let namespace = match key.as_str() {
            "tools" => "tools",
            "emissary_version" | "collectors" => "emissary",
            "os" | "os_version" | "kernel_version" | "arch" | "boot_time" | "container"
            | "virtualization" => "system",
            "capabilities" | "signing_profile_refs" => continue,
            _ => "hardware",
        };
        if key == "tools" {
            if let Some(tools) = value.as_object() {
                for (tool, value) in tools {
                    metadata.insert(format!("tools.{tool}"), value.clone());
                }
            }
        } else if !value.is_null() {
            metadata.insert(format!("{namespace}.{key}"), value.clone());
        }
    }
    for (key, value) in &cfg.metadata {
        metadata.insert(format!("custom.{key}"), value.clone());
    }
    identity_metadata(
        &mut metadata,
        diagnostics,
        sysinfo::Product::name(),
        sysinfo::Product::vendor_name(),
    );
    let snapshot = artifactory_emissary_protocol::nodes::MetadataSnapshot::new(
        startup_id,
        metadata,
        capabilities,
    )?;
    Ok((snapshot, inventory))
}

fn identity_metadata(
    metadata: &mut std::collections::BTreeMap<String, Value>,
    diagnostics: &Value,
    model: Option<String>,
    vendor: Option<String>,
) {
    for (key, value) in [
        (
            "system.hostname",
            diagnostics["hostname"].as_str().map(str::to_owned),
        ),
        ("hardware.device_model", model),
        ("hardware.device_vendor", vendor),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            metadata.insert(key.into(), json!(value));
        }
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub metrics: Value,
    pub network_interfaces: Option<Value>,
}
pub struct Handle {
    pub registry: Registry,
    snapshot: Arc<RwLock<Snapshot>>,
    cancel: CancellationToken,
    spool: Arc<Mutex<Option<Spool>>>,
    notify: Arc<Notify>,
    startup_id: uuid::Uuid,
    started: std::time::Instant,
    jobs: Vec<tokio::task::JoinHandle<()>>,
}
impl Handle {
    /// Start best-effort background collection. Telemetry errors never propagate into execution.
    pub fn start(
        cfg: Config,
        startup_id: uuid::Uuid,
        inventory: artifactory_emissary_protocol::telemetry::Inventory,
    ) -> Self {
        let observed_at = chrono::Utc::now();
        let started = std::time::Instant::now();
        let registry = Registry::default();
        let cancel = CancellationToken::new();
        let snapshot = Arc::new(RwLock::new(Snapshot {
            metrics: json!({}),
            network_interfaces: None,
        }));
        let spool = Arc::new(Mutex::new(None::<Spool>));
        let notify = Arc::new(Notify::new());
        let collect_cfg = cfg.clone();
        let collect_cancel = cancel.clone();
        let collect_registry = registry.clone();
        let collect_spool = spool.clone();
        let collect_snapshot = snapshot.clone();
        let collect_notify = notify.clone();
        let collection = tokio::spawn(async move {
            let initial_spool = collect_spool.clone();
            let initial_cfg = collect_cfg.clone();
            let initial_inventory = inventory.clone();
            let initialized = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let budget = if initial_cfg.telemetry_buffer_bytes == 0 {
                    32 << 20
                } else {
                    initial_cfg.telemetry_buffer_bytes
                };
                anyhow::ensure!(budget >= 8 << 20, "telemetry_configuration_invalid");
                let mut opened = Spool::open(
                    initial_cfg.root.join("telemetry"),
                    budget - inventory_delivery::QUOTA - (1 << 20),
                )?;
                let inventory_ready = opened
                    .inventories
                    .seed_startup(startup_id, observed_at, &initial_inventory)
                    .is_ok();
                if !inventory_ready {
                    eprintln!("telemetry inventory storage unavailable; ordinary samples continue");
                }
                *initial_spool
                    .lock()
                    .map_err(|_| anyhow::anyhow!("telemetry spool poisoned"))? = Some(opened);
                Ok(())
            })
            .await;
            if !matches!(initialized, Ok(Ok(()))) {
                if let Ok(mut snapshot) = collect_snapshot.write() {
                    snapshot.metrics["telemetry"] =
                        json!({"state":"storage_or_configuration_unavailable"});
                }
                eprintln!("telemetry storage or configuration unavailable; execution continues");
            }
            collect_notify.notify_waiters();
            let Ok(mut collector) =
                tokio::task::spawn_blocking(move || Collector::new(startup_id)).await
            else {
                if let Ok(mut snapshot) = collect_snapshot.write() {
                    snapshot.metrics["telemetry"]["collector_error"] =
                        json!("collector_unavailable");
                }
                return;
            };
            let start = tokio::time::Instant::now() + crate::client::retry_delay(10);
            let mut tick = tokio::time::interval_at(start, Duration::from_secs(10));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut index = 0u64;
            let cards = inventory.accelerators.clone();
            let mut pending_inventory = Some(inventory.clone());
            let startup_inventory = inventory;
            loop {
                tokio::select! {_=collect_cancel.cancelled()=>break,_=tick.tick()=>{}}
                let host = index.is_multiple_of(3);
                let inventory = pending_inventory.clone();
                let cfg = collect_cfg.clone();
                let registry = collect_registry.clone();
                let collect_started = std::time::Instant::now();
                let result = tokio::task::spawn_blocking(move || {
                    let data = collector.sample(&cfg, &registry, host, inventory);
                    (collector, data)
                })
                .await;
                let Ok((next, (mut batch, mut observations))) = result else {
                    if let Ok(mut snapshot) = collect_snapshot.write() {
                        snapshot.metrics["telemetry"]["collector_error"] =
                            json!("collector_unavailable");
                    }
                    break;
                };
                let collector_bounds = take_host_bounds(&mut observations);
                collector = next;
                if let Some(host) = &mut batch.host {
                    host.accelerators = tokio::select! {_=collect_cancel.cancelled()=>break,v=inventory::accelerator_samples(&collect_cfg,&cards)=>v};
                }
                if let Ok(mut snapshot) = collect_snapshot.write() {
                    let delivery = snapshot.metrics.get("telemetry").cloned();
                    *snapshot = observations;
                    if let Some(delivery) = delivery {
                        snapshot.metrics["telemetry"] = delivery;
                    }
                    if !snapshot.metrics["telemetry"].is_object() {
                        snapshot.metrics["telemetry"] = json!({});
                    }
                    snapshot.metrics["telemetry"]["collector_duration_ms"] =
                        json!(collect_started.elapsed().as_millis() as u64);
                }
                index = index.saturating_add(1);
                if batch.host.is_none() && batch.processes.is_empty() && batch.inventory.is_none() {
                    continue;
                }
                let spool = collect_spool.clone();
                let cfg = collect_cfg.clone();
                let snapshot = collect_snapshot.clone();
                let startup_inventory = startup_inventory.clone();
                let buffered = tokio::task::spawn_blocking(move || {
                    let mut guard = spool.lock().expect("telemetry spool lock poisoned");
                    if guard.is_none() {
                        if cfg.telemetry_buffer_bytes != 0 && cfg.telemetry_buffer_bytes < 8 << 20 {
                            return false;
                        }
                        match Spool::open(
                            cfg.root.join("telemetry"),
                            if cfg.telemetry_buffer_bytes == 0 {
                                27 << 20
                            } else {
                                cfg.telemetry_buffer_bytes
                                    .saturating_sub(inventory_delivery::QUOTA + (1 << 20))
                            },
                        ) {
                            Ok(mut s) => {
                                let _ = s.inventories.seed_startup(
                                    startup_id,
                                    observed_at,
                                    &startup_inventory,
                                );
                                *guard = Some(s);
                            }
                            Err(_) => {
                                eprintln!("telemetry spool unavailable; execution continues");
                                return false;
                            }
                        }
                    }
                    let mut buffered = false;
                    if let Some(spool) = guard.as_mut() {
                        let inventory_deferred =
                            batch.inventory.is_some() && !spool.inventories.available();
                        let buffered_result = (|| {
                            spool.note_host_bounds(collector_bounds.0, collector_bounds.1)?;
                            // 仅拆分本轮新样本，旧磁盘批次保持原字节。
                            if batch.host.is_some()
                                && (!batch.processes.is_empty() || batch.inventory.is_some())
                            {
                                let mut host = batch.clone();
                                host.id = uuid::Uuid::new_v4();
                                host.processes.clear();
                                host.inventory = None;
                                spool.push(&mut host)?;
                                batch.host = None;
                            }
                            if inventory_deferred {
                                batch.inventory = None;
                            }
                            if batch.host.is_none()
                                && batch.processes.is_empty()
                                && batch.inventory.is_none()
                            {
                                return Ok(());
                            }
                            spool.push(&mut batch)
                        })();
                        if buffered_result.is_err() {
                            let _ = spool.note_reason("disk_error");
                            let _ = spool.note_gap();
                            eprintln!(
                                "telemetry sample could not be buffered; execution continues"
                            );
                        } else {
                            buffered = !inventory_deferred;
                        }
                        spool.refresh_storage();
                        if let Ok(mut snapshot) = snapshot.write() {
                            snapshot.metrics["telemetry_dropped_batches"] = json!(spool.dropped());
                            let delivery = snapshot
                                .metrics
                                .get_mut("telemetry")
                                .and_then(Value::as_object_mut);
                            if let Some(delivery) = delivery {
                                delivery.extend(
                                    spool.diagnostics().as_object().cloned().unwrap_or_default(),
                                );
                            } else {
                                snapshot.metrics["telemetry"] = spool.diagnostics();
                            }
                        }
                    }
                    buffered
                })
                .await
                .unwrap_or(false);
                if buffered {
                    pending_inventory = None;
                }
                collect_notify.notify_waiters();
            }
        });
        let inventory_job = tokio::spawn(inventory_upload::run(
            cfg.clone(),
            spool.clone(),
            snapshot.clone(),
            notify.clone(),
            cancel.clone(),
        ));
        let batch_job = tokio::spawn(upload::run(
            cfg,
            spool.clone(),
            snapshot.clone(),
            notify.clone(),
            cancel.clone(),
        ));
        Self {
            registry,
            snapshot,
            cancel,
            spool,
            notify,
            startup_id,
            started,
            jobs: vec![collection, inventory_job, batch_job],
        }
    }
    /// 停止采样和两个发送器，释放队列锁后才允许同root重启。
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.cancel.cancel();
        for job in self.jobs.drain(..) {
            tokio::time::timeout(Duration::from_secs(10), job).await??;
        }
        Ok(())
    }
    pub fn inventory_sink(&self) -> crate::operations::InventorySink {
        let spool = self.spool.clone();
        let notify = self.notify.clone();
        let startup = self.startup_id;
        let started = self.started;
        Arc::new(move |inventory| {
            let spool = spool.clone();
            let notify = notify.clone();
            Box::pin(async move {
                let result = tokio::task::spawn_blocking(move || {
                    let mut guard = spool
                        .lock()
                        .map_err(|_| anyhow::anyhow!("telemetry spool poisoned"))?;
                    let spool = guard
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("telemetry spool unavailable"))?;
                    spool.inventories.refresh(
                        startup,
                        &inventory,
                        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                    )
                })
                .await??;
                notify.notify_one();
                Ok(result)
            })
        })
    }
    /// Read cached observations without any OS probes on the lease heartbeat path.
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.snapshot.read().ok().map(|s| s.clone())
    }
}

fn take_host_bounds(snapshot: &mut Snapshot) -> (u64, u64) {
    let Some(metrics) = snapshot.metrics.as_object_mut() else {
        return (0, 0);
    };
    let mut count = |key| {
        metrics
            .remove(key)
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
    };
    (
        count("collector_omitted_host_devices"),
        count("collector_truncated_host_fields"),
    )
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
pub(crate) fn cached_test_handle(snapshot: Snapshot) -> Handle {
    Handle {
        registry: Registry::default(),
        snapshot: Arc::new(RwLock::new(snapshot)),
        cancel: CancellationToken::new(),
        spool: Arc::new(Mutex::new(None)),
        notify: Arc::new(Notify::new()),
        startup_id: uuid::Uuid::new_v4(),
        started: std::time::Instant::now(),
        jobs: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collector_bounds_enter_durable_diagnostics_once_without_private_metric_keys() {
        let directory = tempfile::tempdir().unwrap();
        let mut snapshot = Snapshot {
            metrics: json!({"collector_omitted_host_devices":3,"collector_truncated_host_fields":2,"logical_cpus":8}),
            network_interfaces: None,
        };
        let bounds = take_host_bounds(&mut snapshot);
        assert_eq!(bounds, (3, 2));
        assert_eq!(take_host_bounds(&mut snapshot), (0, 0));
        assert_eq!(snapshot.metrics, json!({"logical_cpus":8}));
        let mut spool = Spool::open(directory.path().into(), 4096).unwrap();
        spool.note_host_bounds(bounds.0, bounds.1).unwrap();
        drop(spool);
        let diagnostics = Spool::open(directory.path().into(), 4096)
            .unwrap()
            .diagnostics();
        assert_eq!(diagnostics["omitted_host_devices"], 3);
        assert_eq!(diagnostics["truncated_host_fields"], 2);
    }

    #[tokio::test]
    async fn invalid_queue_configuration_reports_unknown_storage_without_failing_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let handle = Handle::start(
            Config {
                root: directory.path().into(),
                telemetry_buffer_bytes: 1,
                ..Default::default()
            },
            uuid::Uuid::new_v4(),
            Default::default(),
        );
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if handle.snapshot().unwrap().metrics["telemetry"]["state"]
                    == "storage_or_configuration_unavailable"
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(handle.spool.lock().unwrap().is_none());
    }

    #[test]
    fn identity_metadata_preserves_raw_product_model_and_omits_missing_values() {
        let diagnostics = json!({"hostname":"build-host.local"});
        let mut metadata = std::collections::BTreeMap::new();
        identity_metadata(
            &mut metadata,
            &diagnostics,
            Some("Mac16,7".into()),
            Some("Apple Inc.".into()),
        );
        assert_eq!(metadata["system.hostname"], "build-host.local");
        assert_eq!(metadata["hardware.device_model"], "Mac16,7");
        assert_eq!(metadata["hardware.device_vendor"], "Apple Inc.");
        let mut missing = std::collections::BTreeMap::new();
        identity_metadata(
            &mut missing,
            &json!({"hostname":""}),
            None,
            Some(" ".into()),
        );
        assert!(missing.is_empty());
    }
}
