//! [INPUT]: 真实mTLS清单发送器、损坏队列与缓存诊断。
//! [OUTPUT]: 验证本地故障优先、永久冲突隔离与真实后续ACK。
//! [POS]: 清单发送器状态回归；不连接外部环境。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use crate::protocol as pb;
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use std::sync::atomic::{AtomicUsize, Ordering};
use tonic::{Request, Response, Status};

#[derive(Clone)]
struct Peer {
    legacy: bool,
    negotiations: Arc<AtomicUsize>,
    conflict: Arc<Mutex<Option<String>>>,
    uploads: Arc<Mutex<Vec<String>>>,
    #[cfg(unix)]
    fail_ack_sync: Arc<Mutex<Option<std::path::PathBuf>>>,
}
#[tonic::async_trait]
impl pb::telemetry_server::Telemetry for Peer {
    async fn negotiate(
        &self,
        request: Request<pb::TelemetryNegotiateRequest>,
    ) -> Result<Response<pb::TelemetryNegotiateReply>, Status> {
        self.negotiations.fetch_add(1, Ordering::SeqCst);
        if self.legacy {
            return Err(Status::unimplemented("legacy receiver"));
        }
        Ok(Response::new(telemetry_negotiation::negotiate(
            request.get_ref(),
            chrono::Utc::now().timestamp_millis(),
        )?))
    }
    async fn upload(
        &self,
        _: Request<pb::TelemetryBatch>,
    ) -> Result<Response<pb::TelemetryAck>, Status> {
        Err(Status::unimplemented("not used by inventory sender"))
    }
    async fn upload_inventory(
        &self,
        request: Request<pb::InventoryObservation>,
    ) -> Result<Response<pb::InventoryAck>, Status> {
        let observation = request.into_inner();
        observation.validate().unwrap();
        self.uploads
            .lock()
            .unwrap()
            .push(observation.observation_id.clone());
        if self.conflict.lock().unwrap().as_deref() == Some(&observation.observation_id) {
            return Err(RejectionReason::IdContentConflict.status());
        }
        #[cfg(unix)]
        if let Some(directory) = self.fail_ack_sync.lock().unwrap().take() {
            use std::os::unix::fs::PermissionsExt;
            // 保留unlink所需的写入和搜索，拒绝随后打开目录进行同步。
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o300)).unwrap();
        }
        Ok(Response::new(pb::InventoryAck {
            observation_id: observation.observation_id,
            delivery_digest: observation.delivery_digest,
            ..Default::default()
        }))
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    spool: Arc<Mutex<Option<Spool>>>,
    snapshot: Arc<RwLock<Snapshot>>,
    negotiations: Arc<AtomicUsize>,
    conflict: Arc<Mutex<Option<String>>>,
    uploads: Arc<Mutex<Vec<String>>>,
    cfg: Config,
    notify: Arc<Notify>,
    #[cfg(unix)]
    fail_ack_sync: Arc<Mutex<Option<std::path::PathBuf>>>,
    cancel: CancellationToken,
    sender: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn start(legacy: bool, damaged: bool, missing: bool, stale: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let ca_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::from_params(&params, ca_key);
        let server_key = KeyPair::generate().unwrap();
        let mut params =
            CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_cert = params.signed_by(&server_key, &issuer).unwrap();
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let certificate = params.signed_by(&key, &issuer).unwrap();
        std::fs::write(directory.path().join("ca.pem"), ca.pem()).unwrap();
        std::fs::write(directory.path().join("client.pem"), certificate.pem()).unwrap();
        std::fs::write(directory.path().join("client.key"), key.serialize_pem()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let negotiations = Arc::new(AtomicUsize::new(0));
        let conflict = Arc::new(Mutex::new(None));
        let uploads = Arc::new(Mutex::new(Vec::new()));
        #[cfg(unix)]
        let fail_ack_sync = Arc::new(Mutex::new(None));
        let peer = Peer {
            legacy,
            negotiations: negotiations.clone(),
            conflict: conflict.clone(),
            uploads: uploads.clone(),
            #[cfg(unix)]
            fail_ack_sync: fail_ack_sync.clone(),
        };
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(
                    tonic::transport::ServerTlsConfig::new()
                        .identity(tonic::transport::Identity::from_pem(
                            server_cert.pem(),
                            server_key.serialize_pem(),
                        ))
                        .client_ca_root(tonic::transport::Certificate::from_pem(ca.pem())),
                )
                .unwrap()
                .add_service(pb::telemetry_server::TelemetryServer::new(peer))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    shutdown.cancelled(),
                )
                .await
                .unwrap();
        });
        let cfg = Config {
            endpoint: format!("https://{address}"),
            telemetry_endpoint: format!("https://{address}"),
            ca_file: directory.path().join("ca.pem"),
            certificate_file: directory.path().join("client.pem"),
            key_file: directory.path().join("client.key"),
            root: directory.path().join("managed"),
            ..Default::default()
        };
        let root = cfg.root.join("telemetry");
        if damaged {
            std::fs::create_dir_all(root.join("inventories")).unwrap();
            std::fs::write(root.join("inventories/stats.json"), b"{broken").unwrap();
        }
        let local = (!missing).then(|| Spool::open(root, 32 << 20).unwrap());
        let mut metrics = local.as_ref().map_or_else(
            || json!({"inventory_state":"storage_unavailable"}),
            Spool::diagnostics,
        );
        if stale {
            metrics["inventory_state"] = json!("storage_unavailable");
            metrics["inventory_backoff_seconds"] = json!(8);
        }
        let spool = Arc::new(Mutex::new(local));
        let snapshot = Arc::new(RwLock::new(Snapshot {
            metrics: json!({"telemetry":metrics}),
            network_interfaces: None,
        }));
        let notify = Arc::new(Notify::new());
        let sender = tokio::spawn(run(
            cfg.clone(),
            spool.clone(),
            snapshot.clone(),
            notify.clone(),
            cancel.clone(),
        ));
        Self {
            _directory: directory,
            spool,
            snapshot,
            negotiations,
            conflict,
            uploads,
            cfg,
            notify,
            #[cfg(unix)]
            fail_ack_sync,
            cancel,
            sender,
            server,
        }
    }
    async fn observe(&self, predicate: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let metrics = self.snapshot.read().unwrap().metrics["telemetry"].clone();
                if self.negotiations.load(Ordering::SeqCst) > 0 && predicate(&metrics) {
                    return metrics;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sender did not publish the expected local state")
    }
    async fn stop(self) {
        self.cancel.cancel();
        self.sender.await.unwrap();
        self.server.await.unwrap();
    }
}

#[tokio::test]
async fn permanent_inventory_conflict_does_not_block_later_matching_ack() {
    use artifactory_emissary_protocol::telemetry::Inventory;
    let fixture = Fixture::start(false, false, false, false).await;
    fixture
        .observe(|value| value["inventory_state"] == "ready")
        .await;
    let startup = uuid::Uuid::new_v4();
    let inventory = Inventory::default();
    let (conflict, refresh) = {
        let mut guard = fixture.spool.lock().unwrap();
        let queue = &mut guard.as_mut().unwrap().inventories;
        queue
            .seed_startup(startup, chrono::Utc::now(), &inventory)
            .unwrap();
        let conflict = queue.first().unwrap().unwrap();
        *fixture.conflict.lock().unwrap() = Some(conflict.observation_id.clone());
        let refresh = queue.refresh(startup, &inventory, 1).unwrap();
        (conflict, refresh)
    };
    let path = fixture
        ._directory
        .path()
        .join("managed/telemetry/inventories")
        .join(format!("{}.inventory", conflict.observation_id));
    let original = std::fs::read(&path).unwrap();
    let progress = fixture
        .observe(|value| value["last_inventory_ack_at"].is_string())
        .await;
    assert!(progress["last_inventory_ack_at"].is_string());
    assert_eq!(
        std::fs::read(path).unwrap(),
        original,
        "conflict source is not ACKed or rewritten"
    );
    let uploads = fixture.uploads.lock().unwrap().clone();
    assert_eq!(
        uploads
            .iter()
            .filter(|id| **id == conflict.observation_id)
            .count(),
        1
    );
    assert!(uploads.contains(&refresh.observation_id.to_string()));
    assert_eq!(progress["inventory_conflict_records"], 1);
    assert_eq!(progress["inventory_conflicts_total"], 1);
    assert!(progress["inventory_last_conflict_at"].is_string());
    fixture.stop().await;
}

#[tokio::test]
async fn all_conflicts_remain_visible_without_ack_and_are_not_replayed_after_reopen() {
    let mut fixture = Fixture::start(false, false, false, false).await;
    fixture
        .observe(|value| value["inventory_state"] == "ready")
        .await;
    let conflict = {
        let mut guard = fixture.spool.lock().unwrap();
        let queue = &mut guard.as_mut().unwrap().inventories;
        queue
            .seed_startup(
                uuid::Uuid::new_v4(),
                chrono::Utc::now(),
                &Default::default(),
            )
            .unwrap();
        let conflict = queue.first().unwrap().unwrap();
        *fixture.conflict.lock().unwrap() = Some(conflict.observation_id.clone());
        conflict
    };
    fixture.notify.notify_one();
    let metrics = fixture
        .observe(|value| {
            value["inventory_state"] == "id_content_conflict"
                && value["inventory_conflict_records"] == 1
        })
        .await;
    assert!(metrics["last_inventory_ack_at"].is_null());
    assert!(metrics["inventory_last_ack_at"].is_null());
    assert!(metrics["inventory_last_progress_at"].is_null());
    fixture.sender.abort();
    assert!((&mut fixture.sender).await.unwrap_err().is_cancelled());
    // 中断发送任务后重开队列，再连接同一真实mTLS接收端。
    let root = fixture.cfg.root.join("telemetry");
    *fixture.spool.lock().unwrap() = None;
    let mut reopened = Spool::open(root, 32 << 20).unwrap();
    assert!(reopened.inventories.first().unwrap().is_none());
    assert_eq!(reopened.inventories.conflicts(), Some(1));
    assert_eq!(reopened.diagnostics()["inventory_conflicts_total"], 1);
    *fixture.spool.lock().unwrap() = Some(reopened);
    let negotiations = fixture.negotiations.load(Ordering::SeqCst);
    fixture.sender = tokio::spawn(run(
        fixture.cfg.clone(),
        fixture.spool.clone(),
        fixture.snapshot.clone(),
        fixture.notify.clone(),
        fixture.cancel.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.negotiations.load(Ordering::SeqCst) == negotiations {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let metrics = fixture.snapshot.read().unwrap().metrics["telemetry"].clone();
    assert_eq!(metrics["inventory_state"], "id_content_conflict");
    assert!(metrics["last_inventory_ack_at"].is_null());
    assert_eq!(
        fixture
            .uploads
            .lock()
            .unwrap()
            .iter()
            .filter(|id| **id == conflict.observation_id)
            .count(),
        1
    );
    fixture.stop().await;
}

#[tokio::test]
async fn legacy_receiver_never_masks_local_inventory_storage_failure() {
    let fixture = Fixture::start(true, true, false, false).await;
    let metrics = fixture
        .observe(|value| value.get("inventory_schema_wait_records").is_some())
        .await;
    assert_eq!(metrics["inventory_state"], "storage_unavailable");
    assert!(metrics["inventory_schema_wait_records"].is_null());
    fixture.stop().await;
}

#[tokio::test]
async fn unreadable_or_missing_spool_never_reports_zero_inventory_schema_wait() {
    for missing in [false, true] {
        let fixture = Fixture::start(false, !missing, missing, false).await;
        let metrics = fixture
            .observe(|value| value.get("inventory_schema_wait_records").is_some())
            .await;
        assert!(metrics["inventory_schema_wait_records"].is_null());
        assert_eq!(metrics["inventory_state"], "storage_unavailable");
        fixture.stop().await;
    }
}

#[tokio::test]
async fn healthy_empty_queue_clears_transient_storage_error_without_fabricating_ack() {
    let fixture = Fixture::start(false, false, false, true).await;
    assert_eq!(
        fixture
            .spool
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .inventories
            .len(),
        Some(0)
    );
    let metrics = fixture
        .observe(|value| value.get("inventory_schema_wait_records").is_some())
        .await;
    assert_eq!(metrics["inventory_state"], "ready");
    assert_eq!(metrics["inventory_backoff_seconds"], 0);
    assert_eq!(metrics["inventory_schema_wait_records"], 0);
    assert!(metrics["last_inventory_ack_at"].is_null());
    assert!(metrics["inventory_last_ack_at"].is_null());
    assert!(metrics["inventory_last_progress_at"].is_null());
    fixture.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unlink_sync_failure_recovers_empty_queue_without_fabricating_ack() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::start(false, false, false, false).await;
    fixture
        .observe(|value| value["inventory_state"] == "ready")
        .await;
    let directory = fixture
        ._directory
        .path()
        .join("managed/telemetry/inventories");
    // root绕过目录读取权限，不能以该方法制造同步故障。
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o300)).unwrap();
    let denied = std::fs::File::open(&directory).is_err();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    if !denied {
        fixture.stop().await;
        return;
    }
    *fixture.fail_ack_sync.lock().unwrap() = Some(directory.clone());
    let observation = pb::InventoryObservation::new(
        uuid::Uuid::new_v4(),
        0,
        chrono::Utc::now(),
        0,
        &artifactory_emissary_protocol::telemetry::Inventory::default(),
    )
    .unwrap();
    fixture
        .spool
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .inventories
        .push(observation)
        .unwrap();
    fixture.notify.notify_one();
    let failed = fixture
        .observe(|value| value["inventory_state"] == "storage_unavailable")
        .await;
    assert_eq!(
        fixture
            .spool
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .inventories
            .len(),
        Some(0),
        "unlink and live index removal preceded the failed directory sync"
    );
    assert!(failed["last_inventory_ack_at"].is_null());
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let recovered = fixture
        .observe(|value| value["inventory_state"] == "ready")
        .await;
    assert_eq!(recovered["inventory_backoff_seconds"], 0);
    assert_eq!(recovered["inventory_schema_wait_records"], 0);
    assert!(recovered["last_inventory_ack_at"].is_null());
    assert!(recovered["inventory_last_ack_at"].is_null());
    assert!(recovered["inventory_last_progress_at"].is_null());
    fixture.stop().await;
}
