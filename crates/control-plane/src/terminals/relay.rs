//! [INPUT]: 依赖实例租约、每次进程启动的证书与 Ed25519 身份。
//! [OUTPUT]: 启动私有 TLS 终端流，预留时钟差并验证单次委托。
//! [POS]: 只在数据库固定的实例间转发，不接受客户端提供的 URL。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Frames, Runtime, frame};
use crate::{
    protocol::{
        TerminalFrame, terminal_relay_client::TerminalRelayClient,
        terminal_relay_server::TerminalRelayServer,
    },
    state::AppState,
    store::terminals::TerminalInstance,
};
use anyhow::ensure;
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    Request, Status,
    transport::{Certificate, ClientTlsConfig, Endpoint, Identity, Server, ServerTlsConfig},
};
use uuid::Uuid;

// 为副本时钟差预留一秒，接收端仍拒绝已过期的委托。
const DELEGATION_LIFETIME_MS: i64 = 4_000;
const DELEGATION_MAX_FUTURE_MS: i64 = 5_000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Delegation {
    pub source: Uuid,
    pub target: Uuid,
    pub nonce: Uuid,
    pub expires_at_ms: i64,
    pub role: String,
    pub context: serde_json::Value,
}

fn valid_window(delegation: &Delegation, target: Uuid, now: i64) -> bool {
    delegation.target == target
        && delegation.source != target
        && delegation.expires_at_ms >= now
        && delegation.expires_at_ms <= now.saturating_add(DELEGATION_MAX_FUTURE_MS)
}

pub(super) async fn start(state: AppState) -> anyhow::Result<Arc<Runtime>> {
    let bind =
        std::env::var("ARTIFACTORY_TERMINAL_RELAY_BIND").unwrap_or_else(|_| "127.0.0.1:0".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let address = listener.local_addr()?;
    let advertised = std::env::var("ARTIFACTORY_TERMINAL_RELAY_HOST")
        .ok()
        .map(|host| {
            host.parse::<std::net::IpAddr>()
                .map(|host| std::net::SocketAddr::new(host, address.port()))
        })
        .transpose()?
        .unwrap_or(address);
    let endpoint = std::env::var("ARTIFACTORY_TERMINAL_RELAY_ENDPOINT")
        .unwrap_or_else(|_| format!("https://{advertised}"));
    let endpoint = url::Url::parse(&endpoint)?;
    ensure!(
        endpoint.scheme() == "https"
            && endpoint.host_str().is_some()
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.path() == "/"
            && endpoint.query().is_none()
            && endpoint.fragment().is_none(),
        "invalid terminal relay endpoint"
    );
    let certified = rcgen::generate_simple_self_signed(vec!["terminal.internal".into()])?;
    let certificate = certified.cert.pem();
    let identity = Identity::from_pem(&certificate, certified.signing_key.serialize_pem());
    let signing = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let instance = TerminalInstance {
        id: state.coordination.id(),
        endpoint: endpoint.as_str().trim_end_matches('/').into(),
        certificate,
        public_key: STANDARD.encode(signing.verifying_key().as_bytes()),
    };
    state.store.register_terminal_instance(&instance).await?;
    let runtime = Arc::new(Runtime {
        instance,
        signing,
        brokers: Mutex::new(HashMap::new()),
        nonces: Mutex::new(HashMap::new()),
        incoming: Arc::new(tokio::sync::Semaphore::new(128)),
    });
    let server = Server::builder()
        .tls_config(ServerTlsConfig::new().identity(identity))?
        .add_service(
            TerminalRelayServer::new(super::rpc::Relay(state.clone()))
                .max_decoding_message_size(32 * 1024)
                .max_encoding_message_size(32 * 1024),
        );
    let stop = state.coordination.clone();
    tokio::spawn(async move {
        if let Err(error) = server
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                stop.stopped(),
            )
            .await
        {
            tracing::error!(%error,"terminal relay stopped");
            stop.drain();
            stop.stop();
        }
    });
    let active = runtime.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {_=state.coordination.stopped()=>break,_=tick.tick()=>{}}
            if !state.coordination.is_draining() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    state.store.register_terminal_instance(&active.instance),
                )
                .await;
            }
            let _ =
                tokio::time::timeout(Duration::from_secs(2), state.store.terminal_maintenance())
                    .await;
        }
    });
    Ok(runtime)
}

pub(super) async fn forward(
    state: &AppState,
    runtime: &Arc<Runtime>,
    owner: Uuid,
    id: Uuid,
    role: &str,
    context: serde_json::Value,
    mut input: Frames,
) -> Result<Frames, Status> {
    let permit = runtime
        .incoming
        .clone()
        .try_acquire_owned()
        .map_err(|_| Status::resource_exhausted("terminal forwarding capacity reached"))?;
    let target = state
        .store
        .terminal_instance(owner)
        .await
        .map_err(|_| Status::unavailable("terminal owner unavailable"))?;
    let endpoint = Endpoint::from_shared(target.endpoint)
        .map_err(crate::grpc::internal)?
        .connect_timeout(Duration::from_secs(3))
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(target.certificate))
                .domain_name("terminal.internal"),
        )
        .map_err(crate::grpc::internal)?;
    let channel = endpoint
        .connect()
        .await
        .map_err(|_| Status::unavailable("terminal owner connection failed"))?;
    let delegation = Delegation {
        source: runtime.instance.id,
        target: owner,
        nonce: Uuid::new_v4(),
        expires_at_ms: chrono::Utc::now().timestamp_millis() + DELEGATION_LIFETIME_MS,
        role: role.into(),
        context,
    };
    let mut first = frame(id, "relay_hello");
    first.payload_json = serde_json::to_string(&delegation).map_err(crate::grpc::internal)?;
    let signature = STANDARD.encode(runtime.signing.sign(&first.encode_to_vec()).to_bytes());
    let (tx, rx) = mpsc::channel(32);
    tx.send(first)
        .await
        .map_err(|_| Status::internal("terminal forwarding failed"))?;
    let stop = tx.clone();
    tokio::spawn(async move {
        let _permit = permit;
        while let Some(Ok(frame)) = input.recv().await {
            if tx.send(frame).await.is_err() {
                break;
            }
        }
    });
    let mut request = Request::new(ReceiverStream::new(rx));
    request.metadata_mut().insert(
        "x-terminal-signature",
        signature.parse().map_err(crate::grpc::internal)?,
    );
    let mut upstream = TerminalRelayClient::new(channel)
        .max_decoding_message_size(32 * 1024)
        .connect(request)
        .await?
        .into_inner();
    drop(stop);
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        loop {
            match upstream.message().await {
                Ok(Some(frame)) => {
                    if tx.send(Ok(frame)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error)).await;
                    break;
                }
            }
        }
    });
    Ok(rx)
}

pub(super) async fn verify(
    state: &AppState,
    runtime: &Arc<Runtime>,
    first: &TerminalFrame,
    signature: &str,
) -> Result<Delegation, Status> {
    crate::protocol::terminal::validate_frame(first)
        .map_err(|_| Status::invalid_argument("invalid relay envelope"))?;
    if first.kind != "relay_hello" {
        return Err(Status::unauthenticated("relay identity required"));
    }
    let delegation: Delegation = serde_json::from_str(&first.payload_json)
        .map_err(|_| Status::unauthenticated("invalid relay identity"))?;
    let now = chrono::Utc::now().timestamp_millis();
    if !valid_window(&delegation, runtime.instance.id, now) {
        return Err(Status::permission_denied(
            "relay identity expired or wrong owner",
        ));
    }
    let source = state
        .store
        .terminal_instance(delegation.source)
        .await
        .map_err(|_| Status::unauthenticated("relay instance expired"))?;
    let key: [u8; 32] = STANDARD
        .decode(source.public_key)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| Status::unauthenticated("invalid relay key"))?;
    let key =
        VerifyingKey::from_bytes(&key).map_err(|_| Status::unauthenticated("invalid relay key"))?;
    let signature = Signature::from_slice(
        &STANDARD
            .decode(signature)
            .map_err(|_| Status::unauthenticated("invalid relay signature"))?,
    )
    .map_err(|_| Status::unauthenticated("invalid relay signature"))?;
    key.verify(&first.encode_to_vec(), &signature)
        .map_err(|_| Status::unauthenticated("relay signature rejected"))?;
    let mut nonces = runtime.nonces.lock().await;
    nonces.retain(|_, at| at.elapsed() < Duration::from_secs(10));
    if nonces.len() >= 4096
        || nonces
            .insert(delegation.nonce, std::time::Instant::now())
            .is_some()
    {
        return Err(Status::permission_denied("relay delegation replayed"));
    }
    Ok(delegation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_accepts_bounded_clock_difference_without_extending_expiry() {
        let target = Uuid::new_v4();
        let sent_at = 100_000;
        let mut delegation = Delegation {
            source: Uuid::new_v4(),
            target,
            nonce: Uuid::new_v4(),
            expires_at_ms: sent_at + DELEGATION_LIFETIME_MS,
            role: "device".into(),
            context: serde_json::json!({}),
        };
        assert!(valid_window(&delegation, target, sent_at - 75));
        assert!(valid_window(&delegation, target, sent_at - 1_000));
        assert!(valid_window(&delegation, target, delegation.expires_at_ms));
        assert!(!valid_window(
            &delegation,
            target,
            delegation.expires_at_ms + 1
        ));
        assert!(!valid_window(&delegation, Uuid::new_v4(), sent_at));
        assert!(!valid_window(&delegation, delegation.source, sent_at));
        delegation.expires_at_ms = sent_at + 5_001;
        assert!(!valid_window(&delegation, target, sent_at));
    }
}
