//! [INPUT]: 依赖 loopback mTLS、HTTP Git、测试任务和日志存储。
//! [OUTPUT]: 提供跨平台源码与独立终端 mTLS 夹具和首连诊断。
//! [POS]: Emissary 集成测试共享支持；诊断不输出配置和命令。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! Loopback mTLS peer for runtime lifecycle tests; no business scheduler is simulated.
#![allow(dead_code, reason = "共享夹具由独立集成测试按需调用")]
use artifactory_emissary::{config::Config, protocol as pb};
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status, Streaming};

pub struct Connection {
    pub hello: pb::EmissaryEvent,
    pub commands: mpsc::Sender<pb::Command>,
    pub events: mpsc::Receiver<pb::EmissaryEvent>,
}
impl Connection {
    pub async fn event(&mut self, kind: &str) -> pb::EmissaryEvent {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let event = self
                    .events
                    .recv()
                    .await
                    .expect("runtime disconnected before event");
                if event.kind == kind {
                    return event;
                }
            }
        })
        .await
        .expect("runtime event timed out")
    }
    pub async fn closed(&mut self) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while self.events.recv().await.is_some() {}
        })
        .await
        .expect("runtime did not disconnect");
    }
}
#[derive(Clone)]
struct Peer {
    connected: mpsc::Sender<Connection>,
    terminals: mpsc::Sender<TerminalConnection>,
    logs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    ack_delay: Duration,
    metrics: Arc<TransportMetrics>,
    materials: Arc<Mutex<HashMap<String, String>>>,
}

pub struct TerminalConnection {
    pub hello: pb::TerminalFrame,
    pub commands: mpsc::Sender<Result<pb::TerminalFrame, Status>>,
    pub events: mpsc::Receiver<pb::TerminalFrame>,
}

#[tonic::async_trait]
impl pb::terminal_server::Terminal for Peer {
    type ConnectStream = ReceiverStream<Result<pb::TerminalFrame, Status>>;

    async fn connect(
        &self,
        request: Request<Streaming<pb::TerminalFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        if request.extensions().get::<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>()
            .and_then(|info| info.peer_certs()).is_none() {
            return Err(Status::unauthenticated("client certificate required"));
        }
        let mut incoming = request.into_inner();
        let hello = incoming
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("terminal hello required"))?;
        let (commands, response) = mpsc::channel(64);
        let (events, receiver) = mpsc::channel(64);
        self.terminals
            .send(TerminalConnection {
                hello,
                commands,
                events: receiver,
            })
            .await
            .map_err(|_| Status::unavailable("fixture completed"))?;
        tokio::spawn(async move {
            while let Ok(Some(frame)) = incoming.message().await {
                if events.send(frame).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(response)))
    }
}

#[derive(Default)]
pub struct TransportMetrics {
    pub tcp_accepted: AtomicUsize,
    pub accepted: AtomicUsize,
    pub tls_rejected: AtomicUsize,
    pub last_tls_error: Mutex<Option<String>>,
    pub log_batches: AtomicUsize,
    pub fail_next_log: AtomicBool,
}
#[tonic::async_trait]
impl pb::control_server::Control for Peer {
    async fn acquire_source_material(
        &self,
        request: Request<pb::SourceMaterialRequest>,
    ) -> Result<Response<pb::SourceMaterialReply>, Status> {
        if request.extensions().get::<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>()
            .and_then(|info|info.peer_certs()).is_none() { return Err(Status::unauthenticated("client certificate required")); }
        let input = request.into_inner();
        let material = self
            .materials
            .lock()
            .expect("source materials mutex poisoned")
            .get(&input.source_digest)
            .cloned()
            .ok_or_else(|| Status::permission_denied("source manifest is not authorized"))?;
        Ok(Response::new(pb::SourceMaterialReply {
            material_json: material,
        }))
    }
    async fn acquire_signing_material(
        &self,
        _: Request<pb::SigningMaterialRequest>,
    ) -> Result<Response<pb::SigningMaterialReply>, Status> {
        Err(Status::unimplemented("test peer has no signing materials"))
    }
    async fn record_signing_evidence(
        &self,
        _: Request<pb::SigningEvidenceRequest>,
    ) -> Result<Response<pb::SigningEvidenceReply>, Status> {
        Err(Status::unimplemented(
            "test peer has no signing evidence store",
        ))
    }

    type ConnectStream = ReceiverStream<Result<pb::Command, Status>>;
    async fn connect(
        &self,
        request: Request<Streaming<pb::EmissaryEvent>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        if request.extensions().get::<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>()
            .and_then(|info|info.peer_certs()).is_none() {
            return Err(Status::unauthenticated("client certificate required"));
        }
        let mut incoming = request.into_inner();
        let hello = incoming
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("hello required"))?;
        let (commands, mut input) = mpsc::channel(32);
        let (events, receiver) = mpsc::channel(128);
        let (output, response) = mpsc::channel(64);
        self.connected
            .send(Connection {
                hello: hello.clone(),
                commands,
                events: receiver,
            })
            .await
            .map_err(|_| Status::unavailable("test completed"))?;
        let ack_delay = self.ack_delay;
        tokio::spawn(async move {
            let mut next = Some(hello);
            loop {
                let event = if let Some(event) = next.take() {
                    event
                } else {
                    tokio::select! {
                        result = incoming.message()=>match result {Ok(Some(event))=>event,_=>break},
                        command = input.recv()=>{match command {Some(command)=>if output.send(Ok(command)).await.is_err(){break},None=>break} continue;},
                        _=output.closed()=>break,
                    }
                };
                let command = if event.kind == "accept" {
                    Some(pb::Command {
                        kind: "start".into(),
                        attempt_id: event.attempt_id.clone(),
                        lease_token: event.lease_token.clone(),
                        request_id: event.command_id.clone(),
                        lease_seconds: 90,
                        ..Default::default()
                    })
                } else if event.sequence > 0 {
                    Some(pb::Command {
                        kind: "event_ack".into(),
                        attempt_id: event.attempt_id.clone(),
                        ack_sequence: event.sequence,
                        ..Default::default()
                    })
                } else {
                    None
                };
                let payload: serde_json::Value =
                    serde_json::from_str(&event.payload_json).unwrap_or_default();
                if matches!(event.kind.as_str(), "hello" | "heartbeat") {
                    if !payload["ota"].is_null()
                        && output
                            .send(Ok(pb::Command {
                                kind: "emissary_health".into(),
                                request_id: event.command_id.clone(),
                                ..Default::default()
                            }))
                            .await
                            .is_err()
                    {
                        break;
                    }
                    if let Some(active) = payload["active_attempts"].as_array() {
                        for attempt in active {
                            if output
                                .send(Ok(pb::Command {
                                    kind: "renew".into(),
                                    attempt_id: attempt["attempt_id"].as_str().unwrap().into(),
                                    lease_token: attempt["lease_token"].as_str().unwrap().into(),
                                    request_id: event.command_id.clone(),
                                    lease_seconds: 90,
                                    ..Default::default()
                                }))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                if let Some(command) = command {
                    tokio::time::sleep(ack_delay).await;
                    if output.send(Ok(command)).await.is_err() {
                        break;
                    }
                }
                if events.send(event).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(response)))
    }
    async fn upload_logs(
        &self,
        request: Request<Streaming<pb::LogChunk>>,
    ) -> Result<Response<pb::LogAck>, Status> {
        self.metrics.log_batches.fetch_add(1, Ordering::SeqCst);
        if self.metrics.fail_next_log.swap(false, Ordering::SeqCst) {
            return Err(Status::unavailable("injected log transport failure"));
        }
        let mut stream = request.into_inner();
        let mut sequence = 0;
        while let Some(chunk) = stream.message().await? {
            self.logs
                .lock()
                .expect("logs mutex poisoned")
                .entry(chunk.attempt_id)
                .or_default()
                .extend(chunk.data);
            sequence = chunk.sequence;
        }
        Ok(Response::new(pb::LogAck {
            through_sequence: sequence,
        }))
    }
    async fn prepare_artifact(
        &self,
        _: Request<pb::ArtifactRequest>,
    ) -> Result<Response<pb::UploadTicket>, Status> {
        Err(Status::unimplemented("no artifacts in lifecycle fixture"))
    }
    async fn complete_artifact(
        &self,
        _: Request<pb::CompleteArtifactRequest>,
    ) -> Result<Response<pb::ArtifactRecord>, Status> {
        Err(Status::unimplemented("no artifacts in lifecycle fixture"))
    }
}
pub struct Fixture {
    pub cfg: Config,
    pub connections: mpsc::Receiver<Connection>,
    pub terminals: mpsc::Receiver<TerminalConnection>,
    pub metrics: Arc<TransportMetrics>,
    #[cfg_attr(
        windows,
        allow(
            dead_code,
            reason = "Windows runtime 的源码用例仅在 Unix 编译，native_service 跨平台使用此夹具"
        )
    )]
    materials: Arc<Mutex<HashMap<String, String>>>,
    #[cfg(unix)]
    logs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    cancel: CancellationToken,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    pub async fn start(
        directory: &Path,
        versions: &[&'static rustls::SupportedProtocolVersion],
    ) -> Self {
        Self::with_ack_delay(directory, versions, Duration::ZERO).await
    }
    pub async fn with_ack_delay(
        directory: &Path,
        versions: &[&'static rustls::SupportedProtocolVersion],
        ack_delay: Duration,
    ) -> Self {
        let ca_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::from_params(&params, ca_key);
        let server_key = KeyPair::generate().unwrap();
        let mut params =
            CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let certificate = params.signed_by(&server_key, &issuer).unwrap();
        let client_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client = params.signed_by(&client_key, &issuer).unwrap();
        fs::write(directory.join("ca.pem"), ca.pem()).unwrap();
        fs::write(directory.join("client.pem"), client.pem()).unwrap();
        fs::write(directory.join("client.key"), client_key.serialize_pem()).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = rustls::RootCertStore::empty();
        roots.add(ca.der().clone()).unwrap();
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()
        .unwrap();
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(versions)
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![certificate.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(server_key.serialize_der()).into(),
            )
            .unwrap();
        tls.alpn_protocols = vec![b"h2".to_vec()];
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let metrics = Arc::new(TransportMetrics::default());
        let accepted = metrics.clone();
        let incoming = futures::stream::unfold(
            (listener, acceptor, accepted),
            |(listener, acceptor, accepted)| async move {
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    accepted.tcp_accepted.fetch_add(1, Ordering::SeqCst);
                    match acceptor.accept(socket).await {
                        Ok(tls) => {
                            accepted.accepted.fetch_add(1, Ordering::SeqCst);
                            return Some((
                                Ok::<_, std::io::Error>(tls),
                                (listener, acceptor, accepted),
                            ));
                        }
                        Err(error) => {
                            accepted.tls_rejected.fetch_add(1, Ordering::SeqCst);
                            *accepted
                                .last_tls_error
                                .lock()
                                .expect("TLS diagnostics mutex poisoned") = Some(error.to_string());
                        }
                    }
                }
            },
        );
        let (connected, connections) = mpsc::channel(8);
        let (terminals, terminal_connections) = mpsc::channel(8);
        let logs = Arc::new(Mutex::new(HashMap::new()));
        let materials = Arc::new(Mutex::new(HashMap::new()));
        let peer = Peer {
            connected,
            terminals,
            logs: logs.clone(),
            ack_delay,
            metrics: metrics.clone(),
            materials: materials.clone(),
        };
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(pb::control_server::ControlServer::new(peer.clone()))
                .add_service(pb::terminal_server::TerminalServer::new(peer))
                .serve_with_incoming_shutdown(incoming, shutdown.cancelled())
                .await
                .unwrap();
        });
        Self {
            cfg: Config {
                endpoint,
                ca_file: directory.join("ca.pem"),
                certificate_file: directory.join("client.pem"),
                key_file: directory.join("client.key"),
                emissary_id: uuid::Uuid::new_v4().to_string(),
                root: directory.join("managed"),
                allow_http_uploads: true,
                ..Default::default()
            },
            connections,
            terminals: terminal_connections,
            metrics,
            materials,
            #[cfg(unix)]
            logs,
            cancel,
            server,
        }
    }
    #[cfg_attr(
        windows,
        allow(
            dead_code,
            reason = "Windows runtime 的源码用例仅在 Unix 编译，native_service 跨平台使用此夹具"
        )
    )]
    pub async fn source_manifest(&self, repository: &Path, commit: &str) -> String {
        use artifactory_source_checkout::{PrepareResponse, SourceManifest, SourceRepository};
        let repository = repository.to_owned();
        let app = axum::Router::new()
            .fallback(axum::routing::any(source_backend))
            .with_state(repository);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(cancel.cancelled_owned())
                .await
                .unwrap();
        });
        let manifest = SourceManifest::new(
            uuid::Uuid::new_v4(),
            vec![SourceRepository {
                path: String::new(),
                upstream_url: format!("http://{address}/git"),
                commit_sha: commit.to_owned(),
                tags: Default::default(),
                lfs_objects: Vec::new(),
            }],
        )
        .unwrap();
        let material = PrepareResponse {
            source_id: manifest.id,
            repository_url: manifest.repositories[0].upstream_url.clone(),
            reference: commit.to_owned(),
            pinned_ref_oid: Some(commit.to_owned()),
            frozen_tags: Default::default(),
            manifest: Some(manifest.clone()),
            credentials: Vec::new(),
            completed_result: None,
        };
        self.materials
            .lock()
            .expect("source materials mutex poisoned")
            .insert(
                manifest.digest.clone(),
                serde_json::to_string(&material).unwrap(),
            );
        serde_json::to_string(&manifest).unwrap()
    }
    pub async fn connection(&mut self) -> Connection {
        match tokio::time::timeout(Duration::from_secs(30), self.connections.recv()).await {
            Ok(connection) => connection.expect("mTLS fixture connection channel closed"),
            Err(_) => {
                self.connection_diagnostics();
                panic!("runtime did not establish an mTLS control connection within 30 seconds");
            }
        }
    }
    fn connection_diagnostics(&self) {
        eprintln!(
            "mTLS fixture transport: tcp_accepted={} tls_accepted={} tls_rejected={} last_tls_error={:?} server_finished={}",
            self.metrics.tcp_accepted.load(Ordering::SeqCst),
            self.metrics.accepted.load(Ordering::SeqCst),
            self.metrics.tls_rejected.load(Ordering::SeqCst),
            self.metrics
                .last_tls_error
                .lock()
                .expect("TLS diagnostics mutex poisoned"),
            self.server.is_finished(),
        );
        let mut system = sysinfo::System::new();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing().without_tasks(),
        );
        let root = sysinfo::Pid::from_u32(std::process::id());
        let mut descendants = HashSet::from([root]);
        loop {
            let children = system
                .processes()
                .iter()
                .filter(|(pid, process)| {
                    !descendants.contains(*pid)
                        && process
                            .parent()
                            .is_some_and(|parent| descendants.contains(&parent))
                })
                .map(|(pid, _)| *pid)
                .collect::<Vec<_>>();
            if children.is_empty() {
                break;
            }
            descendants.extend(children);
        }
        let mut processes = system
            .processes()
            .iter()
            .filter(|(pid, _)| descendants.contains(*pid))
            .collect::<Vec<_>>();
        processes.sort_unstable_by_key(|(pid, _)| pid.as_u32());
        for (pid, process) in processes {
            eprintln!(
                "mTLS fixture startup process: {}",
                serde_json::json!({
                    "pid": pid.as_u32(),
                    "parent_pid": process.parent().map(|parent| parent.as_u32()),
                    "name": process.name().to_string_lossy(),
                    "status": process.status().to_string(),
                    "run_seconds": process.run_time(),
                }),
            );
        }
    }
    #[cfg(unix)]
    pub fn logs(&self, id: &str) -> Vec<u8> {
        self.logs
            .lock()
            .expect("logs mutex poisoned")
            .get(id)
            .cloned()
            .unwrap_or_default()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        eprintln!(
            "mTLS fixture: connections={} log_batches={} tcp_accepted={} tls_rejected={}",
            self.metrics.accepted.load(Ordering::SeqCst),
            self.metrics.log_batches.load(Ordering::SeqCst),
            self.metrics.tcp_accepted.load(Ordering::SeqCst),
            self.metrics.tls_rejected.load(Ordering::SeqCst),
        );
        self.cancel.cancel();
        self.server.abort();
    }
}

#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Windows runtime 的源码用例仅在 Unix 编译，native_service 跨平台使用此夹具"
    )
)]
async fn source_backend(
    axum::extract::State(repository): axum::extract::State<std::path::PathBuf>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> (axum::http::StatusCode, axum::http::HeaderMap, Vec<u8>) {
    use std::io::Write;
    let output = tokio::task::spawn_blocking(move || {
        let mut child = std::process::Command::new("git")
            .args(["-c", "safe.directory=*", "http-backend"])
            .env("GIT_PROJECT_ROOT", repository.parent().unwrap())
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env(
                "PATH_INFO",
                format!(
                    "/{}/{}",
                    repository.file_name().unwrap().to_string_lossy(),
                    uri.path().trim_start_matches("/git/")
                ),
            )
            .env("REQUEST_METHOD", method.as_str())
            .env("QUERY_STRING", uri.query().unwrap_or(""))
            .env(
                "CONTENT_TYPE",
                headers
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&body).unwrap();
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    assert!(output.status.success());
    let split = output
        .stdout
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    let mut status = axum::http::StatusCode::OK;
    for line in String::from_utf8_lossy(&output.stdout[..split]).lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-type") {
                headers.insert("content-type", value.trim().parse().unwrap());
            }
            if name.eq_ignore_ascii_case("status") {
                status = axum::http::StatusCode::from_u16(
                    value.split_whitespace().next().unwrap().parse().unwrap(),
                )
                .unwrap();
            }
        }
    }
    (status, headers, output.stdout[split + 4..].to_vec())
}
