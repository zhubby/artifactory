//! [INPUT]: 依赖 mTLS 身份、共享名称和心跳预算校验与持久化授权。
//! [OUTPUT]: 提供节点注册、任务、有限操作结果与材料 RPC。
//! [POS]: 节点与控制平面的认证传输边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    crypto, pki,
    protocol::{self, Command, EmissaryEvent},
    recipe::safe_relative_path,
    state::{AppState, Connection},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    Request, Response, Status, Streaming,
    transport::server::{TcpConnectInfo, TlsConnectInfo},
};
use uuid::Uuid;

#[cfg(test)]
#[path = "grpc_tests.rs"]
mod tests;

#[derive(Clone)]
pub struct EmissaryService(pub AppState);

pub(crate) fn internal(error: impl std::fmt::Display) -> Status {
    tracing::error!(%error,"emissary request failed");
    Status::internal("control plane operation failed")
}

fn peer_certificate_hash<T>(request: &Request<T>) -> Result<String, Status> {
    let certificate = request
        .extensions()
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .and_then(|info| info.peer_certs())
        .and_then(|certificates| certificates.first().cloned())
        .ok_or_else(|| Status::unauthenticated("node certificate required"))?;
    Ok(crypto::hash(certificate.as_ref()))
}
pub(crate) async fn identity<T>(state: &AppState, request: &Request<T>) -> Result<Uuid, Status> {
    state
        .store
        .node_identity(peer_certificate_hash(request)?)
        .await
}

#[tonic::async_trait]
impl protocol::enrollment_server::Enrollment for EmissaryService {
    async fn enroll(
        &self,
        request: Request<protocol::EnrollRequest>,
    ) -> Result<Response<protocol::EnrollReply>, Status> {
        let input = request.into_inner();
        let state = &self.0;
        if input.protocol_major != 1 {
            return Err(Status::failed_precondition("unsupported protocol major"));
        }
        if input.token.len() != 64
            || !["linux", "darwin", "windows"].contains(&input.os.as_str())
            || !["amd64", "arm64"].contains(&input.arch.as_str())
        {
            return Err(Status::invalid_argument("invalid enrollment request"));
        }
        let name = artifactory_emissary_protocol::nodes::normalize_node_name(&input.name)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let enrolled = state
            .store
            .enroll_node(
                crate::store::NodeEnrollment {
                    token: &input.token,
                    name: &name,
                    os: &input.os,
                    arch: &input.arch,
                },
                |id| async move {
                    pki::sign(&state.settings, id, &input.csr_pem)
                        .await
                        .map_err(|_| Status::invalid_argument("CSR rejected or issuer unavailable"))
                },
            )
            .await?;
        let id = enrolled.id;
        let certificate = enrolled.certificate;
        state
            .audit("enrollment", "node_enrolled", id, json!({}))
            .await
            .map_err(internal)?;
        Ok(Response::new(protocol::EnrollReply {
            emissary_id: id.to_string(),
            certificate_pem: certificate,
            ca_pem: tokio::fs::read_to_string(state.settings.pki_dir.join("ca.pem"))
                .await
                .map_err(internal)?,
            heartbeat_seconds: 10,
            lease_seconds: 90,
            telemetry_endpoint: state
                .settings
                .telemetry
                .as_ref()
                .map(|t| t.endpoint.clone())
                .unwrap_or_default(),
        }))
    }
}

#[tonic::async_trait]
impl protocol::control_server::Control for EmissaryService {
    type ConnectStream = ReceiverStream<Result<Command, Status>>;

    async fn renew_emissary_asset(
        &self,
        request: Request<protocol::EmissaryAssetRenewRequest>,
    ) -> Result<Response<protocol::EmissaryAssetRenewReply>, Status> {
        crate::ota::renew_asset(&self.0, request).await
    }

    async fn acquire_source_material(
        &self,
        request: Request<protocol::SourceMaterialRequest>,
    ) -> Result<Response<protocol::SourceMaterialReply>, Status> {
        let node = identity(&self.0, &request).await?;
        let input = request.into_inner();
        let attempt = Uuid::parse_str(&input.attempt_id)
            .map_err(|_| Status::invalid_argument("invalid attempt ID"))?;
        let material = self
            .0
            .store
            .native_source_material(
                &self.0.settings.master_key,
                node,
                attempt,
                &input.lease_token,
                &input.source_digest,
            )
            .await
            .map_err(|_| Status::permission_denied("source execution is stale or unauthorized"))?;
        Ok(Response::new(protocol::SourceMaterialReply {
            material_json: serde_json::to_string(&material).map_err(internal)?,
        }))
    }

    async fn acquire_signing_material(
        &self,
        request: Request<protocol::SigningMaterialRequest>,
    ) -> Result<Response<protocol::SigningMaterialReply>, Status> {
        let node = identity(&self.0, &request).await?;
        let input = request.into_inner();
        let id = |s: &str| {
            Uuid::parse_str(s).map_err(|_| Status::invalid_argument("invalid signing resource ID"))
        };
        let keys = self
            .0
            .settings
            .signing_keys
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("managed signing not configured"))?;
        let material_json = self
            .0
            .store
            .acquire_signing_material(
                crate::store::SigningAccess {
                    node,
                    attempt: id(&input.attempt_id)?,
                    session: id(&input.session_id)?,
                    token: &input.lease_token,
                    profile: &input.profile,
                    version: id(&input.version_id)?,
                },
                input.step_index,
                keys,
            )
            .await?;
        Ok(Response::new(protocol::SigningMaterialReply {
            material_json,
        }))
    }

    async fn record_signing_evidence(
        &self,
        request: Request<protocol::SigningEvidenceRequest>,
    ) -> Result<Response<protocol::SigningEvidenceReply>, Status> {
        let node = identity(&self.0, &request).await?;
        let input = request.into_inner();
        if input.evidence_json.len() > 16384 {
            return Err(Status::invalid_argument("oversized signing evidence"));
        }
        let id = |s: &str| {
            Uuid::parse_str(s).map_err(|_| Status::invalid_argument("invalid signing resource ID"))
        };
        let evidence = serde_json::from_str(&input.evidence_json)
            .map_err(|_| Status::invalid_argument("invalid signing evidence"))?;
        self.0
            .store
            .record_signing_evidence(
                node,
                id(&input.attempt_id)?,
                id(&input.session_id)?,
                &input.lease_token,
                evidence,
            )
            .await?;
        Ok(Response::new(protocol::SigningEvidenceReply {
            accepted: true,
        }))
    }

    async fn connect(
        &self,
        mut request: Request<Streaming<EmissaryEvent>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        if self.0.coordination.is_draining() {
            return Err(Status::unavailable("control replica is draining"));
        }
        let hello = tokio::time::timeout(Duration::from_secs(10), request.get_mut().message())
            .await
            .map_err(|_| Status::deadline_exceeded("hello required"))??
            .ok_or_else(|| Status::invalid_argument("hello required"))?;
        // Queue reconnects before they enter the shared database pool. A slow
        // session replacement must not put every heartbeat behind the burst.
        let queued = Instant::now();
        let admission = self
            .0
            .coordination
            .admissions
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Status::unavailable("control admission stopped"))?;
        let admitted = Instant::now();
        if self.0.coordination.is_draining() {
            return Err(Status::unavailable("control replica is draining"));
        }
        let certificate_hash = peer_certificate_hash(&request)?;
        let node = Uuid::parse_str(&hello.emissary_id)
            .map_err(|_| Status::invalid_argument("invalid node ID"))?;
        let identified = Instant::now();
        let mut stream = request.into_inner();
        if hello.kind != "hello"
            || hello.emissary_id != node.to_string()
            || hello.protocol_major != 1
        {
            return Err(Status::failed_precondition(
                "invalid hello or protocol version",
            ));
        }
        let session = Uuid::new_v4();
        let payload = heartbeat_payload(&hello)?;
        // Bind the claimed node to its current certificate while registering
        // the session and hello. Reconnects need only one database round trip.
        let startup = payload
            .get("startup")
            .filter(|v| !v.is_null())
            .map(|v| {
                serde_json::from_value::<artifactory_emissary_protocol::nodes::MetadataSnapshot>(
                    v.clone(),
                )
                .map_err(|_| Status::invalid_argument("invalid startup metadata"))
            })
            .transpose()?;
        if let Some(snapshot) = &startup {
            self.0
                .store
                .register_metadata_session(
                    node,
                    session,
                    self.0.coordination.id,
                    &certificate_hash,
                    snapshot,
                )
                .await?;
            self.0
                .store
                .record_heartbeat(
                    node,
                    session,
                    &payload,
                    crate::store::SessionUpdate::Current,
                )
                .await?;
        } else {
            self.0
                .store
                .record_heartbeat(
                    node,
                    session,
                    &payload,
                    crate::store::SessionUpdate::Register {
                        owner: self.0.coordination.id,
                        certificate_hash,
                    },
                )
                .await?;
        }
        let registered = Instant::now();
        // Drain can start while the hello or the node-row update is in flight.
        if self.0.coordination.is_draining() {
            self.0
                .store
                .clear_connection_owner(node, session, self.0.coordination.id)
                .await
                .map_err(internal)?;
            return Err(Status::unavailable("control replica is draining"));
        }
        let (sender, receiver) = mpsc::channel(128);
        let connection = Connection {
            session,
            sender: sender.clone(),
        };
        if let Some(previous) = self.0.connections.write().await.insert(node, connection) {
            let _ = previous
                .sender
                .try_send(Err(Status::aborted("node connection superseded")));
        }
        self.0.coordination.wake.notify_one();
        self.0.coordination.commands.notify_one();
        let state = self.0.clone();
        tokio::spawn(async move {
            let result = async {
                let initialized =
                    report_heartbeat(&state, node, session, &hello, &payload, &sender).await;
                tracing::debug!(
                    %node,
                    admission_ms = admitted.duration_since(queued).as_secs_f64() * 1000.0,
                    identity_ms = identified.duration_since(admitted).as_secs_f64() * 1000.0,
                    session_ms = registered.duration_since(identified).as_secs_f64() * 1000.0,
                    hello_ms = registered.elapsed().as_secs_f64() * 1000.0,
                    "device connection initialized"
                );
                drop(admission);
                initialized?;
                if startup.as_ref().is_some_and(|snapshot|snapshot.capabilities.iter().any(|value|value==artifactory_emissary_protocol::commands::CAPABILITY)) {
                    let mut responses=vec!["command_rejected".into(),"node_operation_update".into()];
                    if startup.as_ref().is_some_and(|snapshot|snapshot.capabilities.iter().any(|value|value=="emissary-ota-observability-v2")) {responses.push("emissary-ota-observability-v2".into());}
                    let ack=artifactory_emissary_protocol::commands::CapabilitiesAck {schema_version:1,server_time_ms:chrono::Utc::now().timestamp_millis(),responses};
                    sender.send(Ok(Command {kind:"capabilities_ack".into(),signing_session_id:session.to_string(),payload_json:serde_json::to_string(&ack).map_err(internal)?,..Default::default()})).await.map_err(|_|Status::unavailable("connection closed"))?;
                }
                if let Some(snapshot)=&startup { metadata_ack(&state,node,session,snapshot,&sender).await?; }
                loop {
                    let message = tokio::select! {
                        _ = state.coordination.stop.cancelled() => return Err(Status::unavailable("control replica stopped")),
                        message = tokio::time::timeout(Duration::from_secs(35), stream.message()) => message,
                    };
                    let event = message
                        .map_err(|_| Status::deadline_exceeded("heartbeat timeout"))??;
                    let Some(event) = event else {
                        return Ok::<(), Status>(());
                    };
                    if event.emissary_id != node.to_string() {
                        return Err(Status::permission_denied("node ID mismatch"));
                    }
                    if event.kind == "heartbeat" {
                        heartbeat(&state, node, session, &event, &sender).await?;
                    } else if event.kind == "node_operation_update" {
                        node_operation_update(&state,node,session,&event,&sender).await?;
                    } else if event.kind == "command_rejected" {
                        if event.payload_json.len()>4096 {return Err(Status::invalid_argument("oversized command rejection"))}
                        let rejection:artifactory_emissary_protocol::commands::CommandRejected=serde_json::from_str(&event.payload_json).map_err(|_|Status::invalid_argument("invalid command rejection"))?;
                        let valid=state.store.node_operation_session_valid(node,session).await.map_err(internal)?;
                        if !valid {return Err(Status::permission_denied("stale node session"))}
                        if rejection.command_id.len()>128 || rejection.kind.len()>128 {return Err(Status::invalid_argument("invalid command rejection"))}
                        tracing::debug!(%node,code=?rejection.code,"device rejected command");
                    } else if event.kind == "metadata" {
                        if event.payload_json.len()>artifactory_emissary_protocol::nodes::MAX_METADATA_BYTES { return Err(Status::invalid_argument("oversized metadata")); }
                        let snapshot=serde_json::from_str(&event.payload_json).map_err(|_|Status::invalid_argument("invalid metadata"))?;
                        metadata_ack(&state,node,session,&snapshot,&sender).await?;
                    } else {
                        apply_event(&state, node, session, &event, &sender).await?;
                    }
                }
            }
            .await;
            if let Err(error) = result {
                let _ = sender.try_send(Err(error));
            }
            {
                let mut connections = state.connections.write().await;
                if connections
                    .get(&node)
                    .is_some_and(|entry| entry.session == session)
                {
                    connections.remove(&node);
                }
            }
            let _ = state
                .store
                .clear_connection_owner(node, session, state.coordination.id)
                .await;
        });
        Ok(Response::new(ReceiverStream::new(receiver)))
    }

    async fn upload_logs(
        &self,
        request: Request<Streaming<protocol::LogChunk>>,
    ) -> Result<Response<protocol::LogAck>, Status> {
        let node = identity(&self.0, &request).await?;
        let mut stream = request.into_inner();
        let mut through = 0;
        let mut chunks = 0;
        while let Some(chunk) = stream.message().await? {
            chunks += 1;
            if chunks > 64
                || chunk.data.len() > 65536
                || chunk.data.is_empty()
                || chunk.sequence == 0
                || chunk.step.len() > 120
            {
                return Err(Status::invalid_argument("invalid log chunk"));
            }
            let attempt = self
                .0
                .store
                .validate_attempt(node, &chunk.attempt_id, &chunk.lease_token, true)
                .await?;
            let hash = crypto::hash(&chunk.data);
            let key = format!("logs/{attempt}/{}/{hash}", chunk.sequence);
            let size = chunk.data.len() as i64;
            self.0
                .storage
                .put(&key, chunk.data)
                .await
                .map_err(internal)?;
            self.0
                .store
                .commit_log_chunk(
                    node,
                    attempt,
                    crate::store::LogCommit {
                        token: &chunk.lease_token,
                        sequence: chunk.sequence,
                        step: &chunk.step,
                        key: &key,
                        hash: &hash,
                        size,
                    },
                )
                .await?;
            through = chunk.sequence;
        }
        Ok(Response::new(protocol::LogAck {
            through_sequence: through,
        }))
    }

    async fn prepare_artifact(
        &self,
        request: Request<protocol::ArtifactRequest>,
    ) -> Result<Response<protocol::UploadTicket>, Status> {
        let node = identity(&self.0, &request).await?;
        let input = request.into_inner();
        let attempt = self
            .0
            .store
            .validate_attempt(node, &input.attempt_id, &input.lease_token, false)
            .await?;
        if !safe_relative_path(&input.path)
            || input.size == 0
            || input.size > 20 * 1024 * 1024 * 1024
            || input.sha256.len() != 64
            || !input
                .sha256
                .bytes()
                .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase())
        {
            return Err(Status::invalid_argument("invalid artifact declaration"));
        }
        let upload = self
            .0
            .store
            .prepare_upload(
                node,
                attempt,
                crate::store::ArtifactDeclaration {
                    path: &input.path,
                    size: input.size,
                    sha256: &input.sha256,
                    lease_token: &input.lease_token,
                },
            )
            .await?;
        let url = if upload.completed {
            String::new()
        } else {
            self.0
                .storage
                .upload_url(&upload.object_key)
                .map_err(|_| Status::internal("artifact ticket signing failed"))?
        };
        Ok(Response::new(protocol::UploadTicket {
            upload_id: upload.id.to_string(),
            url,
            expires_seconds: 900,
        }))
    }

    async fn complete_artifact(
        &self,
        request: Request<protocol::CompleteArtifactRequest>,
    ) -> Result<Response<protocol::ArtifactRecord>, Status> {
        let node = identity(&self.0, &request).await?;
        let input = request.into_inner();
        let attempt = self
            .0
            .store
            .validate_attempt(node, &input.attempt_id, &input.lease_token, false)
            .await?;
        let id = Uuid::parse_str(&input.upload_id)
            .map_err(|_| Status::invalid_argument("invalid upload ID"))?;
        let artifact = self
            .0
            .store
            .complete_upload(node, attempt, id, &input.lease_token, |upload| async move {
                self.0
                    .storage
                    .verify_and_promote(
                        &upload.object_key,
                        &upload.final_key,
                        upload.expected_size,
                        &upload.expected_sha256,
                    )
                    .await
                    .map_err(|_| {
                        Status::failed_precondition("artifact verification or promotion failed")
                    })
            })
            .await?;
        Ok(Response::new(protocol::ArtifactRecord {
            artifact_id: artifact.id.to_string(),
            path: artifact.path,
            sha256: artifact.sha256,
            size: artifact.size,
        }))
    }
}

fn heartbeat_payload(event: &EmissaryEvent) -> Result<Value, Status> {
    artifactory_emissary_protocol::nodes::decode_heartbeat(&event.payload_json)
        .map_err(|error| Status::invalid_argument(error.to_string()))
}

pub(crate) fn ordinary_health(payload: &Value) -> Option<&Value> {
    payload.get("ota").filter(|status| {
        status.is_object()
            && status["update_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .is_none()
    })
}

async fn heartbeat(
    state: &AppState,
    node: Uuid,
    session: Uuid,
    event: &EmissaryEvent,
    sender: &mpsc::Sender<Result<Command, Status>>,
) -> Result<(), Status> {
    let payload = heartbeat_payload(event)?;
    state
        .store
        .record_heartbeat(
            node,
            session,
            &payload,
            crate::store::SessionUpdate::Current,
        )
        .await?;
    report_heartbeat(state, node, session, event, &payload, sender).await
}

async fn report_heartbeat(
    state: &AppState,
    node: Uuid,
    session: Uuid,
    event: &EmissaryEvent,
    payload: &Value,
    sender: &mpsc::Sender<Result<Command, Status>>,
) -> Result<(), Status> {
    if let Some(active) = payload.get("active_attempts").and_then(Value::as_array) {
        if active.len() > 32 {
            return Err(Status::invalid_argument("too many active attempts"));
        }
        for entry in active {
            let Some(id) = entry
                .get("attempt_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
            else {
                continue;
            };
            let token = entry
                .get("lease_token")
                .and_then(Value::as_str)
                .unwrap_or("");
            let renewed = state.store.renew_attempt(node, session, id, token).await?;
            let command = Command {
                command_id: Uuid::new_v4().to_string(),
                kind: if renewed { "renew" } else { "cancel" }.into(),
                attempt_id: id.to_string(),
                lease_token: token.into(),
                lease_seconds: if renewed { 90 } else { 0 },
                signing_session_id: session.to_string(),
                request_id: event.command_id.clone(),
                message: if renewed {
                    String::new()
                } else {
                    "lease_invalid".into()
                },
                ..Default::default()
            };
            sender
                .send(Ok(command))
                .await
                .map_err(|_| Status::unavailable("connection closed"))?;
        }
    }
    let ota_healthy = if ordinary_health(payload).is_some() {
        true
    } else {
        crate::ota::report(state, node, session, payload)
            .await
            .map_err(internal)?
    };
    if ota_healthy && event.kind == "heartbeat" {
        // The health update and any lease/OTA writes already authorize this
        // session. Idle heartbeats need no extra database read for their ACK.
        sender
            .send(Ok(Command {
                kind: "emissary_health".into(),
                request_id: event.command_id.clone(),
                ..Default::default()
            }))
            .await
            .map_err(|_| Status::unavailable("connection closed"))?;
    }
    Ok(())
}

async fn apply_event(
    state: &AppState,
    node: Uuid,
    session: Uuid,
    event: &EmissaryEvent,
    sender: &mpsc::Sender<Result<Command, Status>>,
) -> Result<(), Status> {
    let id = Uuid::parse_str(&event.attempt_id)
        .map_err(|_| Status::invalid_argument("invalid attempt ID"))?;
    if event.payload_json.len() > 16384 {
        return Err(Status::invalid_argument("oversized task event"));
    }
    let receipt = state
        .store
        .submit_execution_event(
            node,
            session,
            id,
            crate::store::ExecutionEvent {
                kind: &event.kind,
                sequence: event.sequence,
                lease_token: &event.lease_token,
                payload_json: &event.payload_json,
            },
        )
        .await?;
    let command = match receipt {
        crate::store::EventReceipt::Start => Command {
            command_id: Uuid::new_v4().to_string(),
            kind: "start".into(),
            signing_session_id: session.to_string(),
            attempt_id: id.to_string(),
            lease_token: event.lease_token.clone(),
            lease_seconds: 90,
            request_id: event.command_id.clone(),
            ..Default::default()
        },
        crate::store::EventReceipt::Ack(sequence) => Command {
            kind: "event_ack".into(),
            attempt_id: id.to_string(),
            ack_sequence: sequence,
            ..Default::default()
        },
    };
    sender
        .send(Ok(command))
        .await
        .map_err(|_| Status::unavailable("connection closed"))?;
    Ok(())
}

async fn metadata_ack(
    state: &AppState,
    node: Uuid,
    session: Uuid,
    snapshot: &artifactory_emissary_protocol::nodes::MetadataSnapshot,
    sender: &mpsc::Sender<Result<Command, Status>>,
) -> Result<(), Status> {
    state.store.retry_metadata(node, session, snapshot).await?;
    sender
        .send(Ok(Command {
            kind: "metadata_ack".into(),
            request_id: snapshot.startup_id.to_string(),
            signing_session_id: session.to_string(),
            ..Default::default()
        }))
        .await
        .map_err(|_| Status::unavailable("connection closed"))
}

async fn node_operation_update(
    state: &AppState,
    node: Uuid,
    session: Uuid,
    event: &EmissaryEvent,
    sender: &mpsc::Sender<Result<Command, Status>>,
) -> Result<(), Status> {
    if event.payload_json.len() > artifactory_emissary_protocol::operations::MAX_RESULT_BYTES + 8192
    {
        return Err(Status::invalid_argument("oversized operation update"));
    }
    let update: artifactory_emissary_protocol::operations::OperationUpdate =
        serde_json::from_str(&event.payload_json)
            .map_err(|_| Status::invalid_argument("invalid operation update"))?;
    let ack = state
        .store
        .record_node_operation_update(node, session, &update)
        .await
        .map_err(internal)?;
    if let Some(ack) = ack {
        sender
            .send(Ok(Command {
                kind: "node_operation_result_ack".into(),
                command_id: ack.operation_id.to_string(),
                signing_session_id: session.to_string(),
                payload_json: serde_json::to_string(&ack).map_err(internal)?,
                ..Default::default()
            }))
            .await
            .map_err(|_| Status::unavailable("connection closed"))?;
    }
    Ok(())
}
