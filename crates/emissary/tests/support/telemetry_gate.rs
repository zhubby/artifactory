//! [INPUT]: 当前真实Telemetry Service和共享mTLS服务端配置。
//! [OUTPUT]: 暂拒上传、透传协商，记录请求及最后主机样本并等待真实事务ACK。
//! [POS]: 恢复与冻结旧端的传输边界；只统计真实schema和事务回执。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![allow(dead_code, reason = "共享夹具由独立集成测试按需调用")]
use anyhow::{Context, Result};
use artifactory_emissary_protocol::{
    InventoryAck, InventoryObservation, TelemetryAck, TelemetryBatch, TelemetryNegotiateReply,
    TelemetryNegotiateRequest,
    telemetry_server::{Telemetry, TelemetryServer},
};
use artifactory_telemetry_service::Service;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tonic::{
    Request, Response, Status,
    transport::{Server, ServerTlsConfig},
};

#[derive(Clone)]
struct Gate {
    service: Service,
    open: Arc<AtomicBool>,
    batches: Arc<Mutex<BTreeMap<String, usize>>>,
    duplicates: Arc<Mutex<BTreeMap<String, usize>>>,
    schemas: Arc<Mutex<BTreeSet<u32>>>,
    negotiations: Arc<AtomicUsize>,
    inventories: Arc<Mutex<BTreeSet<String>>>,
    inventory_attempts: Arc<Mutex<BTreeMap<String, usize>>>,
    last_host: Arc<Mutex<Option<Vec<u8>>>>,
}
#[tonic::async_trait]
impl Telemetry for Gate {
    async fn negotiate(
        &self,
        request: Request<TelemetryNegotiateRequest>,
    ) -> Result<Response<TelemetryNegotiateReply>, Status> {
        self.negotiations.fetch_add(1, Ordering::SeqCst);
        self.service.negotiate(request).await
    }
    async fn upload(
        &self,
        request: Request<TelemetryBatch>,
    ) -> Result<Response<TelemetryAck>, Status> {
        let batch: artifactory_emissary_protocol::telemetry::Batch =
            serde_json::from_slice(&request.get_ref().json)
                .map_err(|_| Status::invalid_argument("test batch is invalid"))?;
        if batch.host.is_some() {
            *self
                .last_host
                .lock()
                .map_err(|_| Status::internal("gate state unavailable"))? =
                Some(request.get_ref().json.clone());
        }
        *self
            .batches
            .lock()
            .map_err(|_| Status::internal("gate state unavailable"))?
            .entry(batch.id.to_string())
            .or_default() += 1;
        self.schemas
            .lock()
            .map_err(|_| Status::internal("gate state unavailable"))?
            .insert(request.get_ref().schema_version);
        if !self.open.load(Ordering::SeqCst) {
            return Err(Status::unavailable("test receiver temporarily unavailable"));
        }
        // 原Request携带TlsConnectInfo；身份校验与ACK均由真实Service执行。
        let result = self.service.upload(request).await?;
        if result.get_ref().duplicate {
            *self
                .duplicates
                .lock()
                .map_err(|_| Status::internal("gate state unavailable"))?
                .entry(batch.id.to_string())
                .or_default() += 1;
        }
        Ok(result)
    }
    async fn upload_inventory(
        &self,
        request: Request<InventoryObservation>,
    ) -> Result<Response<InventoryAck>, Status> {
        *self
            .inventory_attempts
            .lock()
            .map_err(|_| Status::internal("gate state unavailable"))?
            .entry(request.get_ref().observation_id.clone())
            .or_default() += 1;
        self.inventories
            .lock()
            .map_err(|_| Status::internal("gate state unavailable"))?
            .insert(request.get_ref().observation_id.clone());
        if !self.open.load(Ordering::SeqCst) {
            return Err(Status::unavailable("test receiver temporarily unavailable"));
        }
        self.service.upload_inventory(request).await
    }
}

pub struct TelemetryGate {
    pub endpoint: String,
    gate: Gate,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<Result<(), tonic::transport::Error>>>,
}
impl TelemetryGate {
    pub async fn start(service: Service, tls: ServerTlsConfig) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("https://{}", listener.local_addr()?);
        let gate = Gate {
            service,
            open: Arc::new(AtomicBool::new(false)),
            batches: Default::default(),
            duplicates: Default::default(),
            schemas: Default::default(),
            negotiations: Default::default(),
            inventories: Default::default(),
            inventory_attempts: Default::default(),
            last_host: Default::default(),
        };
        let transport = gate.clone();
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            let incoming = futures::stream::unfold(listener, |listener| async move {
                Some((listener.accept().await.map(|(socket, _)| socket), listener))
            });
            Server::builder()
                .tls_config(tls)?
                .add_service(TelemetryServer::new(transport))
                .serve_with_incoming_shutdown(incoming, shutdown.cancelled())
                .await
        });
        Ok(Self {
            endpoint,
            gate,
            stop,
            task: Some(task),
        })
    }
    pub fn open(&self) {
        self.gate.open.store(true, Ordering::SeqCst);
    }
    pub fn saw_batch(&self, id: uuid::Uuid) -> Result<bool> {
        Ok(self
            .gate
            .batches
            .lock()
            .map_err(|_| anyhow::anyhow!("gate state unavailable"))?
            .contains_key(&id.to_string()))
    }
    pub fn last_host(&self) -> Result<Option<Vec<u8>>> {
        Ok(self
            .gate
            .last_host
            .lock()
            .map_err(|_| anyhow::anyhow!("gate state unavailable"))?
            .clone())
    }
    pub fn duplicate_acks(&self, id: uuid::Uuid) -> Result<usize> {
        Ok(self
            .gate
            .duplicates
            .lock()
            .map_err(|_| anyhow::anyhow!("gate state unavailable"))?
            .get(&id.to_string())
            .copied()
            .unwrap_or_default())
    }
    pub fn legacy_wire(&self) -> Result<bool> {
        Ok(self.gate.negotiations.load(Ordering::SeqCst) == 0
            && *self
                .gate
                .schemas
                .lock()
                .map_err(|_| anyhow::anyhow!("gate state unavailable"))?
                == BTreeSet::from([0]))
    }
    pub fn saw_inventory(&self) -> Result<bool> {
        Ok(!self
            .gate
            .inventories
            .lock()
            .map_err(|_| anyhow::anyhow!("gate state unavailable"))?
            .is_empty())
    }
    pub fn inventory_attempts(&self, id: &str) -> Result<usize> {
        Ok(self
            .gate
            .inventory_attempts
            .lock()
            .map_err(|_| anyhow::anyhow!("gate state unavailable"))?
            .get(id)
            .copied()
            .unwrap_or_default())
    }
    pub async fn close(&mut self) -> Result<()> {
        self.stop.cancel();
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
                Ok(result) => result.context("telemetry gate task failed")??,
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    anyhow::bail!("telemetry gate shutdown timed out");
                }
            }
        }
        Ok(())
    }
}
impl Drop for TelemetryGate {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
