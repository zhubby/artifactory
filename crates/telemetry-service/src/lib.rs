//! [INPUT]: 设备mTLS身份、版本化遥测契约与独立存储。
//! [OUTPUT]: 协商接收格式，事务确认采样和独立清单，按持久接收结果导出有限指标。
//! [POS]: 遥测RPC入口，不使用Control能力推断数据格式。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod query;
pub mod store;

use artifactory_emissary_protocol::{
    InventoryAck, InventoryObservation, TelemetryAck, TelemetryBatch, TelemetryNegotiateReply,
    TelemetryNegotiateRequest,
    inventory::MAX_INVENTORY_BYTES,
    telemetry::{Batch, MAX_BATCH_BYTES},
    telemetry_negotiation::{self, RejectionReason},
    telemetry_server::Telemetry,
};
use artifactory_service_transport::crypto;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use tonic::{
    Request, Response, Status,
    transport::server::{TcpConnectInfo, TlsConnectInfo},
};
use uuid::Uuid;

// 采样、清单与协商共用解码及授权连接预算。
static INGEST_LIMIT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(64);
static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REPLAYED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);
static REASONS: [AtomicU64; 14] = [const { AtomicU64::new(0) }; 14];

/// 采样与清单共用 accepted/replayed；成功协商不计入持久接收。
/// rejected包含三个RPC的错误；未知Status归入unclassified。
/// inflight覆盖解码、授权和持久写入，三个RPC共用64个许可。
/// 固定reason标签，不包含节点、路径、操作ID和错误文本。
pub fn metrics() -> String {
    let mut metrics = format!(
        "{}# TYPE artifactory_telemetry_accepted_total counter\nartifactory_telemetry_accepted_total {}\n# TYPE artifactory_telemetry_replayed_total counter\nartifactory_telemetry_replayed_total {}\n# TYPE artifactory_telemetry_rejected_total counter\nartifactory_telemetry_rejected_total {}\n# TYPE artifactory_telemetry_ingest_inflight gauge\nartifactory_telemetry_ingest_inflight {}\n# TYPE artifactory_telemetry_rejection_reason_total counter\n",
        artifactory_service_transport::metrics::render(),
        ACCEPTED.load(Relaxed),
        REPLAYED.load(Relaxed),
        REJECTED.load(Relaxed),
        64 - INGEST_LIMIT.available_permits()
    );
    for (index, reason) in RejectionReason::ALL.iter().enumerate() {
        metrics.push_str(&format!(
            "artifactory_telemetry_rejection_reason_total{{reason=\"{}\"}} {}\n",
            reason.as_str(),
            REASONS[index].load(Relaxed)
        ));
    }
    metrics.push_str(&format!(
        "artifactory_telemetry_rejection_reason_total{{reason=\"unclassified\"}} {}\n",
        REASONS[13].load(Relaxed)
    ));
    metrics
}

fn rejected(status: &Status) {
    REJECTED.fetch_add(1, Relaxed);
    let index = RejectionReason::from_status(status)
        .and_then(|reason| RejectionReason::ALL.iter().position(|r| *r == reason))
        .unwrap_or(13);
    REASONS[index].fetch_add(1, Relaxed);
}

/// 遥测读写池与只读业务授权池保持独立。
#[derive(Clone)]
pub struct Service {
    pub store: store::TelemetryStore,
    pub authorization: store::BusinessAuthorizationRepository,
}

pub(crate) fn internal(error: impl std::fmt::Display) -> Status {
    tracing::error!(%error,"telemetry operation failed");
    RejectionReason::DatabaseUnavailable.status()
}

fn permit() -> Result<tokio::sync::SemaphorePermit<'static>, Status> {
    INGEST_LIMIT
        .try_acquire()
        .map_err(|_| RejectionReason::Overloaded.status())
}

#[tonic::async_trait]
impl Telemetry for Service {
    async fn negotiate(
        &self,
        request: Request<TelemetryNegotiateRequest>,
    ) -> Result<Response<TelemetryNegotiateReply>, Status> {
        let operation = async {
            let _permit = permit()?;
            self.identify(&request).await?;
            telemetry_negotiation::negotiate(
                request.get_ref(),
                chrono::Utc::now().timestamp_millis(),
            )
            .map(Response::new)
        }
        .await;
        if let Err(status) = &operation {
            rejected(status);
        }
        operation
    }

    async fn upload(
        &self,
        request: Request<TelemetryBatch>,
    ) -> Result<Response<TelemetryAck>, Status> {
        let result = self.receive(request).await;
        match &result {
            Ok(ack) if ack.get_ref().duplicate => {
                REPLAYED.fetch_add(1, Relaxed);
            }
            Ok(_) => {
                ACCEPTED.fetch_add(1, Relaxed);
            }
            Err(status) => rejected(status),
        }
        result
    }

    async fn upload_inventory(
        &self,
        request: Request<InventoryObservation>,
    ) -> Result<Response<InventoryAck>, Status> {
        let result = self.receive_inventory(request).await;
        match &result {
            Ok(ack) if ack.get_ref().duplicate => {
                REPLAYED.fetch_add(1, Relaxed);
            }
            Ok(_) => {
                ACCEPTED.fetch_add(1, Relaxed);
            }
            Err(status) => rejected(status),
        }
        result
    }
}

impl Service {
    /// 当前摘要与历史分开清扫；每页只读业务库后再提交遥测事务。
    pub async fn retain(&self) -> anyhow::Result<()> {
        let mut cursor = None;
        loop {
            let nodes = self.authorization.startup_page(cursor).await?;
            if nodes.is_empty() {
                break;
            }
            self.store.retain_startups(&nodes).await?;
            cursor = nodes.last().map(|(node, _)| *node);
            if nodes.len() < 512 {
                break;
            }
        }
        self.store.retain().await
    }

    async fn identify<T>(&self, request: &Request<T>) -> Result<Uuid, Status> {
        let cert = request
            .extensions()
            .get::<TlsConnectInfo<TcpConnectInfo>>()
            .and_then(|info| info.peer_certs())
            .and_then(|certs| certs.first().cloned())
            .ok_or_else(|| RejectionReason::AuthenticationFailed.status())?;
        self.authorization
            .identify(crypto::hash(cert.as_ref()))
            .await
    }

    async fn receive(
        &self,
        request: Request<TelemetryBatch>,
    ) -> Result<Response<TelemetryAck>, Status> {
        let _permit = permit()?;
        let node = self.identify(&request).await?;
        let envelope = request.into_inner();
        if !matches!(envelope.schema_version, 0 | 1) {
            return Err(RejectionReason::SchemaUnsupported.status());
        }
        let bytes = envelope.json;
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(RejectionReason::BoundsExceeded.status());
        }
        let batch: Batch =
            serde_json::from_slice(&bytes).map_err(|_| RejectionReason::Malformed.status())?;
        batch
            .validate()
            .map_err(|_| RejectionReason::BoundsExceeded.status())?;
        let now = chrono::Utc::now();
        if batch.sampled_at > now + chrono::Duration::minutes(5) {
            return Err(RejectionReason::ClockFuture.status());
        }
        if batch.sampled_at < now - chrono::Duration::hours(48) {
            return Err(RejectionReason::ExpiredSample.status());
        }
        let attempts: Vec<Uuid> = batch
            .processes
            .iter()
            .map(|p| p.attempt_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        self.authorization
            .authorize_attempts(node, &attempts)
            .await?;
        let duplicate = self.store.ingest(node, &batch, &bytes, now).await?;
        Ok(Response::new(TelemetryAck {
            batch_id: batch.id.to_string(),
            duplicate,
            server_time_ms: now.timestamp_millis(),
            sha256: crypto::hash(&bytes),
            schema_version: 1,
        }))
    }

    async fn receive_inventory(
        &self,
        request: Request<InventoryObservation>,
    ) -> Result<Response<InventoryAck>, Status> {
        let _permit = permit()?;
        let node = self.identify(&request).await?;
        let observation = request.into_inner();
        if observation.inventory_schema != 1 {
            return Err(RejectionReason::SchemaUnsupported.status());
        }
        if observation.inventory_json.len() > MAX_INVENTORY_BYTES {
            return Err(RejectionReason::BoundsExceeded.status());
        }
        observation
            .validate()
            .map_err(|_| RejectionReason::Malformed.status())?;
        let startup = Uuid::parse_str(&observation.source_startup_id)
            .map_err(|_| RejectionReason::Malformed.status())?;
        self.authorization.authorize_startup(node, startup).await?;
        let now = chrono::Utc::now();
        let duplicate = self.store.ingest_inventory(node, &observation, now).await?;
        Ok(Response::new(InventoryAck {
            observation_id: observation.observation_id,
            delivery_digest: observation.delivery_digest,
            duplicate,
            server_time_ms: now.timestamp_millis(),
        }))
    }
}

#[cfg(test)]
mod metrics_tests {
    use super::*;

    #[test]
    fn unknown_status_never_becomes_a_request_controlled_reason_label() {
        let before = REJECTED.load(Relaxed);
        let unknown_before = REASONS[13].load(Relaxed);
        let malformed_before = REASONS[9].load(Relaxed);
        rejected(&Status::invalid_argument(
            "/private/metrics-secret operation-123",
        ));
        let forged = Status::with_details(
            tonic::Code::Unavailable,
            "malformed",
            RejectionReason::Malformed
                .status()
                .details()
                .to_vec()
                .into(),
        );
        rejected(&forged);
        rejected(&RejectionReason::Malformed.status());
        assert_eq!(REJECTED.load(Relaxed) - before, 3);
        assert_eq!(REASONS[13].load(Relaxed) - unknown_before, 2);
        assert_eq!(REASONS[9].load(Relaxed) - malformed_before, 1);
        let rendered = metrics();
        let samples: Vec<_> = rendered
            .lines()
            .filter(|line| line.starts_with("artifactory_telemetry_rejection_reason_total{"))
            .collect();
        assert_eq!(samples.len(), 14);
        for sample in samples {
            assert!(sample.starts_with("artifactory_telemetry_rejection_reason_total{reason=\""));
            assert_eq!(sample.matches('=').count(), 1);
        }
        assert!(!rendered.contains("metrics-secret"));
        assert!(!rendered.contains("operation-123"));
        assert_eq!(64 - INGEST_LIMIT.available_permits(), 0);
    }
}
