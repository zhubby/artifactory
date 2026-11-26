//! [INPUT]: 依赖真实mTLS遥测服务、原字节旧队列和后台发送器。
//! [OUTPUT]: 验证混合版本、大小等待和持续积压下的实际确认新鲜度。
//! [POS]: 跨版本发送器集成回归，不连接正式环境。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_emissary::{client, config::Config, protocol as pb, telemetry};
use artifactory_emissary_protocol::{telemetry::Batch, telemetry_negotiation as negotiation};
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};
type Acknowledged = Vec<(Batch, chrono::DateTime<chrono::Utc>)>;
#[derive(Clone)]
struct Peer {
    mode: Arc<AtomicU8>,
    received: Arc<Mutex<Vec<Vec<u8>>>>,
    inventories: Arc<Mutex<Vec<String>>>,
    acknowledged: Arc<Mutex<Acknowledged>>,
    negotiations: Arc<AtomicUsize>,
}
#[tonic::async_trait]
impl pb::telemetry_server::Telemetry for Peer {
    async fn negotiate(
        &self,
        request: Request<pb::TelemetryNegotiateRequest>,
    ) -> Result<Response<pb::TelemetryNegotiateReply>, Status> {
        self.negotiations.fetch_add(1, Ordering::Relaxed);
        if matches!(self.mode.load(Ordering::Relaxed), 0 | 17) {
            return Err(Status::unimplemented("legacy"));
        }
        if self.mode.load(Ordering::Relaxed) == 5 {
            return Err(negotiation::RejectionReason::AuthorizationDenied.status());
        }
        match self.mode.load(Ordering::Relaxed) {
            11 => return Err(Status::unavailable("temporary negotiation failure")),
            12 => return Err(negotiation::RejectionReason::Overloaded.status()),
            13 => tokio::time::sleep(Duration::from_secs(25)).await,
            _ => {}
        }
        let mut reply =
            negotiation::negotiate(request.get_ref(), chrono::Utc::now().timestamp_millis())?;
        if self.mode.load(Ordering::Relaxed) == 7 {
            reply.max_batch_bytes = 2048;
            reply.max_inventory_bytes = 128;
        }
        if self.mode.load(Ordering::Relaxed) == 14 {
            reply.batch_schemas.clear();
            reply.inventory_schemas.clear();
        }
        Ok(Response::new(reply))
    }
    async fn upload(
        &self,
        request: Request<pb::TelemetryBatch>,
    ) -> Result<Response<pb::TelemetryAck>, Status> {
        assert!(request.extensions().get::<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>().and_then(|v|v.peer_certs()).is_some());
        let value = request.into_inner();
        self.received.lock().unwrap().push(value.json.clone());
        if self.mode.load(Ordering::Relaxed) == 16 {
            self.mode.store(11, Ordering::Relaxed);
            return Err(Status::unavailable("backend changed after negotiation"));
        }
        if self.mode.load(Ordering::Relaxed) == 3 {
            return Err(Status::invalid_argument("unclassified old decoder"));
        }
        if self.mode.load(Ordering::Relaxed) == 15 && !self.acknowledged.lock().unwrap().is_empty()
        {
            return Err(negotiation::RejectionReason::BoundsExceeded.status());
        }
        let batch: Batch = serde_json::from_slice(&value.json).unwrap();
        if self.mode.load(Ordering::Relaxed) == 7 {
            assert!(value.json.len() <= 2048);
        }
        if self.mode.load(Ordering::Relaxed) == 6 {
            tokio::time::sleep(Duration::from_secs(8)).await;
        }
        self.acknowledged
            .lock()
            .unwrap()
            .push((batch.clone(), chrono::Utc::now()));
        Ok(Response::new(pb::TelemetryAck {
            batch_id: batch.id.to_string(),
            sha256: if self.mode.load(Ordering::Relaxed) == 2 {
                "0".repeat(64)
            } else if matches!(self.mode.load(Ordering::Relaxed), 0 | 11 | 12 | 13 | 17) {
                String::new()
            } else {
                negotiation::sha256(&value.json)
            },
            schema_version: if matches!(self.mode.load(Ordering::Relaxed), 0 | 11 | 12 | 13 | 17) {
                0
            } else {
                1
            },
            ..Default::default()
        }))
    }
    async fn upload_inventory(
        &self,
        request: Request<pb::InventoryObservation>,
    ) -> Result<Response<pb::InventoryAck>, Status> {
        if matches!(self.mode.load(Ordering::Relaxed), 4 | 6) {
            tokio::time::sleep(Duration::from_secs(10)).await;
            return Err(Status::unavailable("slow inventory"));
        }
        let observation = request.into_inner();
        if self.mode.load(Ordering::Relaxed) == 7 {
            assert!(observation.inventory_json.len() <= 128);
        }
        observation.validate().unwrap();
        self.inventories
            .lock()
            .unwrap()
            .push(observation.observation_id.clone());
        Ok(Response::new(pb::InventoryAck {
            observation_id: observation.observation_id,
            delivery_digest: observation.delivery_digest,
            ..Default::default()
        }))
    }
}
#[tokio::test]
async fn mixed_receivers_preserve_original_bytes_and_only_matching_ack_deletes() {
    run_modes(&[0, 1, 2, 3, 4, 5]).await;
}
#[tokio::test]
async fn negotiated_size_wait_preserves_large_records_and_delivers_small_records() {
    run_modes(&[7]).await;
}
#[tokio::test]
async fn continuous_backlog_and_slow_inventory_keep_fresh_ack_under_sixty_seconds() {
    run_modes(&[6]).await;
}
#[tokio::test]
async fn damaged_inventory_storage_keeps_ordinary_mtls_delivery_and_original_source() {
    run_modes(&[8, 9, 10, 17]).await;
}
#[tokio::test]
async fn transient_negotiation_keeps_safe_v1_and_retries_without_claiming_inventory_support() {
    run_modes(&[11, 12, 13]).await;
}
#[tokio::test]
async fn no_schema_intersection_preserves_sources_without_upload() {
    run_modes(&[14]).await;
}
#[tokio::test]
async fn permanent_rejection_after_success_remains_explainable_when_queue_is_empty() {
    run_modes(&[15]).await;
}
#[tokio::test]
async fn transient_reconnect_never_weakens_an_established_digest_ack() {
    run_modes(&[16]).await;
}
async fn run_modes(modes: &[u8]) {
    for &mode in modes {
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
        let server = params.signed_by(&server_key, &issuer).unwrap();
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let cert = params.signed_by(&key, &issuer).unwrap();
        fs::write(directory.path().join("ca.pem"), ca.pem()).unwrap();
        fs::write(directory.path().join("client.pem"), cert.pem()).unwrap();
        fs::write(directory.path().join("client.key"), key.serialize_pem()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = Peer {
            mode: Arc::new(AtomicU8::new(mode)),
            received: Default::default(),
            inventories: Default::default(),
            acknowledged: Default::default(),
            negotiations: Default::default(),
        };
        let observed = peer.clone();
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let server_task = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(
                    tonic::transport::ServerTlsConfig::new()
                        .identity(tonic::transport::Identity::from_pem(
                            server.pem(),
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
            endpoint: format!("https://{addr}"),
            telemetry_endpoint: format!("https://{addr}"),
            ca_file: directory.path().join("ca.pem"),
            certificate_file: directory.path().join("client.pem"),
            key_file: directory.path().join("client.key"),
            root: directory.path().join("managed"),
            emissary_id: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        };
        client::dial(&cfg, false).await.unwrap();
        let root = cfg.root.join("telemetry");
        fs::create_dir_all(&root).unwrap();
        let mut batch: Batch = serde_json::from_slice(include_bytes!(
            "../../emissary-protocol/tests/fixtures/telemetry-batch.json"
        ))
        .unwrap();
        batch.sampled_at = chrono::Utc::now();
        if mode == 7 {
            for index in 0..10 {
                batch.inventory.as_mut().unwrap().tools.insert(
                    format!("pad{index}"),
                    artifactory_emissary_protocol::telemetry::Tool {
                        available: true,
                        version: Some("x".repeat(400)),
                    },
                );
            }
        }
        let bytes = serde_json::to_vec(&batch).unwrap();
        let path = root.join("00000000000000000000-existing.batch");
        fs::write(&path, &bytes).unwrap();
        let damaged = if (8..=10).contains(&mode) || mode == 17 {
            let inventories = root.join("inventories");
            fs::create_dir(&inventories).unwrap();
            let damaged = inventories.join(match mode {
                8 => "broken.inventory",
                9 => "startup.json",
                _ => "stats.json",
            });
            fs::write(&damaged, b"{incomplete").unwrap();
            Some(damaged)
        } else {
            None
        };
        if matches!(mode, 4 | 6 | 7) {
            // 旧历史保持积压；慢清单RPC不能占用批次发送任务。
            for index in 1..if mode == 6 { 1000 } else { 20 } {
                let mut old = batch.clone();
                old.id = uuid::Uuid::new_v4();
                old.inventory = None;
                old.sampled_at -= chrono::Duration::minutes(10);
                fs::write(
                    root.join(format!("{index:020}-{}.batch", old.id)),
                    serde_json::to_vec(&old).unwrap(),
                )
                .unwrap();
            }
            let fresh = root.join("fresh");
            fs::create_dir_all(&fresh).unwrap();
            let mut host = batch.clone();
            host.id = uuid::Uuid::new_v4();
            host.inventory = None;
            host.processes.clear();
            fs::write(
                fresh.join("pending.batch"),
                serde_json::to_vec(&host).unwrap(),
            )
            .unwrap();
        }
        let handle = telemetry::Handle::start(
            cfg.clone(),
            batch.session_id,
            batch.inventory.clone().unwrap(),
        );
        if mode == 16 {
            tokio::time::timeout(Duration::from_secs(10), async {
                while observed
                    .received
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|value| *value == &bytes)
                    .count()
                    < 2
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("reconnect retries the same original record");
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(
                fs::read(&path).unwrap(),
                bytes,
                "old ID-only ACK cannot satisfy established digest policy"
            );
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        if (11..=13).contains(&mode) {
            tokio::time::timeout(Duration::from_secs(55), async {
                while !observed
                    .received
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|record| record == &bytes)
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("temporary negotiation failure must still send exact safe v1");
            assert!(observed.inventories.lock().unwrap().is_empty());
            tokio::time::timeout(Duration::from_secs(15), async {
                while observed.negotiations.load(Ordering::Relaxed) < 3 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("both lanes started negotiation and batch must retry it");
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        if mode == 14 {
            tokio::time::timeout(Duration::from_secs(5), async {
                while handle.snapshot().unwrap().metrics["telemetry"]["state"] != "schema_wait" {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert!(observed.received.lock().unwrap().is_empty());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        if mode == 15 {
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let metrics = handle.snapshot().unwrap().metrics;
                    if metrics["telemetry"]["bounds_exceeded_batches"]
                        .as_u64()
                        .unwrap_or(0)
                        > 0
                        && metrics["telemetry"]["spool_records"] == 0
                        && metrics["telemetry"]["fresh_records"] == 0
                    {
                        assert_eq!(
                            metrics["telemetry"]["last_rejection_reason"],
                            "bounds_exceeded"
                        );
                        assert!(metrics["telemetry"]["last_rejection_at"].is_string());
                        assert_eq!(metrics["telemetry"]["backoff_seconds"], 0);
                        assert!(metrics["telemetry"]["last_successful_ack_at"].is_string());
                        assert!(
                            metrics["telemetry"]["fresh_last_ack_at"].is_null(),
                            "a discarded fresh record is never an ACK"
                        );
                        assert!(metrics["telemetry"]["fresh_last_progress_at"].is_string());
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("durable permanent loss must have finite diagnostics");
            assert!(!path.exists());
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            let stats: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("stats.json")).unwrap()).unwrap();
            assert!(stats["reasons"]["bounds_exceeded"].as_u64().unwrap() > 0);
            continue;
        }
        if let Some(damaged) = damaged {
            tokio::time::timeout(Duration::from_secs(30),async {
                loop {
                    if !observed.received.lock().unwrap().is_empty() {break;}
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }).await.unwrap_or_else(|error| panic!("ordinary samples must arrive despite broken inventory storage: {error}; snapshot={:?}", handle.snapshot().unwrap().metrics));
            assert!(observed.inventories.lock().unwrap().is_empty());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::read(damaged).unwrap(), b"{incomplete");
            assert_eq!(
                handle.snapshot().unwrap().metrics["telemetry"]["inventory_state"],
                "storage_unavailable"
            );
            assert!(
                handle.snapshot().unwrap().metrics["telemetry"]["inventory_schema_wait_records"]
                    .is_null(),
                "inaccessible inventory schema waiting count stays unknown"
            );
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        if mode == 6 {
            let started = std::time::Instant::now();
            while started.elapsed() < Duration::from_secs(100) {
                {
                    let acks = observed.acknowledged.lock().unwrap();
                    if let Some((host, received)) = acks
                        .iter()
                        .rev()
                        .find(|(batch, _)| batch.host.is_some() && batch.processes.is_empty())
                    {
                        assert!(
                            chrono::Utc::now()
                                .signed_duration_since(host.sampled_at)
                                .num_seconds()
                                < 90,
                            "receiver current-startup host must stay fresh"
                        );
                        assert!(
                            received
                                .signed_duration_since(host.sampled_at)
                                .num_seconds()
                                <= 60,
                            "uncoalesced fresh ACK latency"
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            {
                let acks = observed.acknowledged.lock().unwrap();
                let hosts = acks
                    .iter()
                    .filter(|(value, _)| value.host.is_some() && value.processes.is_empty())
                    .count();
                assert!(
                    hosts >= 3,
                    "continuous real30s sampling must deliver multiple fresh hosts"
                );
                assert!(
                    acks.len() - hosts >= 3,
                    "historical backlog must also advance"
                );
            }
            assert!(
                path.exists()
                    || fs::read_dir(&root)
                        .unwrap()
                        .filter_map(Result::ok)
                        .any(|entry| entry.path().extension().is_some_and(|s| s == "batch")),
                "historical backlog must remain during measurement"
            );
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        if mode == 7 {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let metrics = handle.snapshot().unwrap().metrics;
                    if metrics["telemetry"]["schema_wait_records"]
                        .as_u64()
                        .unwrap_or(0)
                        > 0
                        && metrics["telemetry"]["inventory_schema_wait_records"]
                            .as_u64()
                            .unwrap_or(0)
                            > 0
                        && !observed.received.lock().unwrap().is_empty()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                fs::read(&path).unwrap(),
                bytes,
                "oversized old bytes remain unchanged while smaller records progress"
            );
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        if mode == 5 {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if handle.snapshot().unwrap().metrics["telemetry"]["state"]
                        == "authorization_denied"
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            assert!(observed.received.lock().unwrap().is_empty());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            handle.shutdown().await.unwrap();
            cancel.cancel();
            server_task.await.unwrap();
            continue;
        }
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if !observed.received.lock().unwrap().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        if mode == 4 {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if observed.received.lock().unwrap().len() >= 3 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            let received = observed.received.lock().unwrap();
            let first: Batch = serde_json::from_slice(&received[0]).unwrap();
            assert!(first.host.is_some() && first.processes.is_empty());
            assert!(first.sampled_at > chrono::Utc::now() - chrono::Duration::seconds(60));
            assert_eq!(
                received[1], bytes,
                "fresh ACK must give original history the next turn"
            );
            assert!(
                received.len() >= 3,
                "history continues while inventory RPC is pending"
            );
            drop(received);
            let diagnostics = handle.snapshot().unwrap().metrics;
            assert!(diagnostics["telemetry"]["fresh_last_ack_at"].is_string());
            assert!(diagnostics["telemetry"]["history_last_ack_at"].is_string());
            assert!(diagnostics["telemetry"]["fresh_last_progress_at"].is_string());
            assert!(diagnostics["telemetry"]["history_last_progress_at"].is_string());
        } else {
            assert_eq!(observed.received.lock().unwrap()[0], bytes);
        }
        if mode <= 1 || mode == 4 {
            tokio::time::timeout(Duration::from_secs(3), async {
                while path.exists() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        if mode > 0 && mode != 4 {
            tokio::time::timeout(Duration::from_secs(5), async {
                while observed.inventories.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                while handle.snapshot().unwrap().metrics["telemetry"]["inventory_last_progress_at"]
                    .is_null()
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("independent inventory ACK reports its own progress");
            let diagnostics = handle.snapshot().unwrap().metrics;
            assert!(diagnostics["telemetry"]["inventory_upload_latency_ms"].is_number());
            assert_eq!(diagnostics["telemetry"]["inventory_backoff_seconds"], 0);
        } else {
            assert!(observed.inventories.lock().unwrap().is_empty());
        }
        handle.shutdown().await.unwrap();
        if mode == 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let records = fs::read_dir(root.join("inventories"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|e| e == "inventory"))
                .count();
            assert!(
                records >= 2,
                "legacy batch ACK cannot remove pending independent backfill"
            );
            observed.mode.store(1, Ordering::Relaxed);
            let replay =
                telemetry::Handle::start(cfg, batch.session_id, batch.inventory.clone().unwrap());
            tokio::time::timeout(Duration::from_secs(5), async {
                while observed.inventories.lock().unwrap().len() < 2 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            replay.shutdown().await.unwrap();
        }
        cancel.cancel();
        server_task.await.unwrap();
    }
}
