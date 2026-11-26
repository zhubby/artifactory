//! [INPUT]: 依赖已维护服务、真实mTLS命令和候选原字节。
//! [OUTPUT]: 验证签名runtime OTA并返回真实连接和指针基线。
//! [POS]: 原生维护成功后的运行时回归，不确认Telemetry业务入库。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Prepared, image_pointers};
use crate::{Machine, support};
use anyhow::{Context, ensure};
use artifactory_emissary::{VERSION, ota, platform, protocol as pb};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(super) struct Activated {
    pub connection: support::Connection,
    pub instance: String,
    pub pointers: Value,
}

pub(super) fn candidate_commit(output: &str) -> anyhow::Result<String> {
    let (os, arch) = platform();
    let prefix = format!("artifactory-emissary {VERSION} {os}/{arch} protocol=1 commit=");
    let commit = output
        .trim()
        .strip_prefix(&prefix)
        .context("candidate product version, platform or protocol differs")?;
    ensure!(
        commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "candidate must report its full Git build commit"
    );
    Ok(commit.into())
}

fn signed_request(
    version: &str,
    bytes: &[u8],
    generation: i64,
    url: &str,
) -> anyhow::Result<ota::Request> {
    ensure!(generation > 0, "runtime OTA generation must be positive");
    let (os, arch) = platform();
    let manifest = serde_json::to_vec(&ota::Manifest {
        version: VERSION.into(),
        commit: candidate_commit(version)?,
        protocol_min: 1,
        protocol_max: 1,
        assets: vec![ota::Asset::from_bytes(os, arch, bytes)],
    })?;
    let key = SigningKey::from_bytes(&[29; 32]);
    Ok(ota::Request {
        id: uuid::Uuid::new_v4().to_string(),
        generation,
        action: "apply".into(),
        signature: STANDARD.encode(key.sign(&manifest).to_bytes()),
        manifest: String::from_utf8(manifest)?,
        url: url.into(),
    })
}

pub(super) async fn exercise(
    prepared: &Prepared,
    machine: &Machine,
    fixture: &mut support::Fixture,
    mut connection: support::Connection,
    instance: &str,
) -> Activated {
    const PHASE: &str = "runtime_ota";
    prepared.evidence(&machine.binary, PHASE, false, Value::Null);
    let before: ota::State = machine.read_json(&prepared.ota.join("state.json"));
    let before_hello: Value = serde_json::from_str(&connection.hello.payload_json).unwrap();
    let startup = before_hello["startup"]["startup_id"]
        .as_str()
        .expect("maintained runtime startup identity missing")
        .to_owned();
    let before_health: ota::Health = machine.read_json(&prepared.ota.join("health.json"));
    let launcher_pid = machine.running().await;
    let generation = before
        .generation
        .checked_add(1)
        .expect("runtime OTA generation overflow");
    let previous_digest = hex::encode(Sha256::digest(
        machine.read_bytes(std::path::Path::new(&before.current.path)),
    ));
    let candidate = fs::read(&prepared.candidate).unwrap();
    let candidate_digest = hex::encode(Sha256::digest(&candidate));
    let installed = machine.read_bytes(&machine.binary);
    let host = prepared.contract(machine);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/runtime", listener.local_addr().unwrap());
    let body = candidate.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let received = requests.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/runtime",
                axum::routing::get(move || {
                    let body = body.clone();
                    received.fetch_add(1, Ordering::Relaxed);
                    async move { body }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let abort = server.abort_handle();
    // 用例失败或取消时也关闭测试下载端，不保留文件句柄。
    struct StopServer(tokio::task::AbortHandle);
    impl Drop for StopServer {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _stop_server = StopServer(abort);
    let request =
        signed_request(&prepared.candidate_version, &candidate, generation, &url).unwrap();
    let update_id = request.id.clone();
    let result = tokio::time::timeout(Duration::from_secs(100), async {
        connection
            .commands
            .send(pb::Command {
                command_id: request.id.clone(),
                kind: "emissary_update".into(),
                payload_json: serde_json::to_string(&request).unwrap(),
                ..Default::default()
            })
            .await
            .unwrap();
        while connection.events.recv().await.is_some() {}
        let mut candidate_connection = fixture.connection().await;
        assert!(
            candidate_connection.hello.emissary_id == fixture.cfg.emissary_id,
            "runtime OTA changed device identity"
        );
        let hello: Value = serde_json::from_str(&candidate_connection.hello.payload_json).unwrap();
        assert_eq!(hello["diagnostics"]["emissary_version"], VERSION);
        let candidate_startup = hello["startup"]["startup_id"]
            .as_str()
            .expect("candidate runtime startup identity missing");
        assert_ne!(candidate_startup, startup);
        let mut anchors = HashSet::new();
        anchors.insert(candidate_connection.hello.command_id.clone());
        let mut confirmations = HashSet::new();
        let mut health_session = None;
        loop {
            while let Ok(event) = candidate_connection.events.try_recv() {
                if matches!(event.kind.as_str(), "hello" | "heartbeat") {
                    anchors.insert(event.command_id);
                }
            }
            if let Some(health) =
                machine.optional_json::<ota::Health>(&prepared.ota.join("health.json"))
                && !health.session.is_empty()
                && anchors.contains(&health.id)
            {
                assert_ne!(health.session, before_health.session);
                if let Some(session) = &health_session {
                    assert_eq!(&health.session, session);
                } else {
                    health_session = Some(health.session);
                }
                confirmations.insert(health.id);
            }
            let state: ota::State = machine.read_json(&prepared.ota.join("state.json"));
            if state.generation == generation && state.update_id == update_id {
                assert!(
                    !matches!(state.phase.as_str(), "failed" | "rolled_back" | "canceled"),
                    "runtime OTA failed in {}",
                    state.phase
                );
                if state.phase == "succeeded" && confirmations.len() >= 3 {
                    let capabilities: ota::LauncherCapabilities =
                        machine.read_json(&prepared.ota.join("capabilities.json"));
                    assert_eq!(capabilities.instance_id, instance);
                    assert_eq!(state.current.version, VERSION);
                    assert_eq!(state.previous.path, before.current.path);
                    assert_eq!(state.previous.version, before.current.version);
                    assert_eq!(
                        hex::encode(Sha256::digest(machine.read_bytes(std::path::Path::new(
                            &state.current.path,
                        )))),
                        candidate_digest
                    );
                    assert_eq!(
                        hex::encode(Sha256::digest(machine.read_bytes(std::path::Path::new(
                            &state.previous.path,
                        )))),
                        previous_digest
                    );
                    assert!(machine.read_bytes(&machine.binary) == installed);
                    assert!(prepared.contract(machine) == host);
                    assert!(
                        prepared
                            .identity
                            .iter()
                            .all(|(path, bytes)| machine.read_bytes(path) == *bytes),
                        "runtime OTA changed retained identity or queue bytes"
                    );
                    let (queue_unchanged, queue_records) = prepared.queue_status(machine);
                    assert!(queue_unchanged, "runtime OTA changed retained queue bytes");
                    assert_eq!(machine.running().await, launcher_pid);
                    let downloads = requests.load(Ordering::Relaxed);
                    if candidate_digest != previous_digest {
                        assert!(downloads > 0, "different runtime asset was never downloaded");
                    }
                    let pointers = image_pointers(&serde_json::to_value(state).unwrap());
                    let checks = json!({
                        "identity_unchanged":true,"account_and_acl_unchanged":true,
                        "retained_queue_unchanged":true,"queue_records":queue_records,
                        "installed_launcher_unchanged":true,"launcher_instance_unchanged":true,
                        "launcher_pid_unchanged":true,"runtime_startup_changed":true,
                        "current_digest_matches":true,"previous_image_preserved":true,
                        "same_product_version":true,"generation_advanced_once":true,
                        "different_runtime_asset":candidate_digest!=previous_digest,
                        "execution_mode":if candidate_digest!=previous_digest {"different_runtime_asset"} else {"same_asset_mechanical_reapply"},
                        "download_requests":downloads,
                        "runtime_asset_path_changed":pointers["current"]["path"]!=pointers["previous"]["path"],
                        "health_confirmations":confirmations.len(),"telemetry_business_ack_verified":false,
                        "generation":generation,"current_sha256":candidate_digest,"previous_sha256":previous_digest
                    });
                    prepared.evidence(&machine.binary, PHASE, true, checks);
                    break Activated {
                        connection: candidate_connection,
                        instance: capabilities.instance_id,
                        pointers,
                    };
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    server.abort();
    let _ = server.await;
    match result {
        Ok(activated) => activated,
        Err(_) => {
            let state: ota::State = machine.read_json(&prepared.ota.join("state.json"));
            panic!(
                "maintained service runtime OTA timed out: phase={} generation={}",
                state.phase, state.generation
            );
        }
    }
}

#[test]
fn runtime_request_binds_candidate_commit_product_version_and_exact_bytes() {
    use artifactory_emissary::{VERSION, platform};
    use base64::{Engine, engine::general_purpose::STANDARD};
    use ed25519_dalek::SigningKey;
    let (os, arch) = platform();
    let version = format!(
        "artifactory-emissary {VERSION} {os}/{arch} protocol=1 commit=0123456789abcdef0123456789abcdef01234567\n"
    );
    let bytes = b"signed runtime candidate";
    let request = signed_request(&version, bytes, 7, "http://127.0.0.1:12345/runtime").unwrap();
    assert_eq!(request.generation, 7);
    assert_eq!(request.action, "apply");
    assert_eq!(request.url, "http://127.0.0.1:12345/runtime");
    let manifest: ota::Manifest = serde_json::from_str(&request.manifest).unwrap();
    assert_eq!(manifest.version, VERSION);
    assert_eq!(manifest.commit, "0123456789abcdef0123456789abcdef01234567");
    let asset = ota::verify(
        request.manifest.as_bytes(),
        &request.signature,
        &[STANDARD.encode(SigningKey::from_bytes(&[29; 32]).verifying_key().to_bytes())],
        os,
        arch,
    )
    .unwrap();
    ota::verify_bytes(bytes, &asset).unwrap();
    assert!(ota::verify_bytes(b"modified runtime", &asset).is_err());
    for invalid in [
        version.replace(VERSION, "unified-version-mismatch"),
        version.replace("protocol=1", "protocol=2"),
        version.replace("0123456789abcdef0123456789abcdef01234567", "development"),
    ] {
        assert!(signed_request(&invalid, bytes, 7, &request.url).is_err());
    }
}
