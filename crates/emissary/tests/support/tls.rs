//! [INPUT]: 随机测试证书、节点配置和真实Hello回调。
//! [OUTPUT]: 共享Control与Telemetry的mTLS身份材料及私有进程证书文件。
//! [POS]: 集成测试的证书边界；仅显式进程测试落盘服务私钥。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::Result;
use artifactory_emissary::config::{Config, atomic_bytes};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

#[derive(Clone)]
pub struct TlsMaterial(Arc<Material>);

struct Material {
    ca_pem: String,
    ca_der: rustls::pki_types::CertificateDer<'static>,
    server_pem: String,
    server_der: rustls::pki_types::CertificateDer<'static>,
    server_key_pem: String,
    server_key_der: Vec<u8>,
    client_pem: String,
    client_der: rustls::pki_types::CertificateDer<'static>,
    client_key_pem: String,
}

impl TlsMaterial {
    pub fn generate() -> Result<Self> {
        let ca_key = KeyPair::generate()?;
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        ca_params.distinguished_name.push(
            DnType::CommonName,
            format!("Artifactory test CA {}", uuid::Uuid::new_v4()),
        );
        let ca = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::from_params(&ca_params, ca_key);
        let server_key = KeyPair::generate()?;
        let mut server_params =
            CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()])?;
        server_params
            .distinguished_name
            .push(DnType::CommonName, "Artifactory loopback server");
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server = server_params.signed_by(&server_key, &issuer)?;
        let client_key = KeyPair::generate()?;
        let mut client_params = CertificateParams::default();
        client_params
            .distinguished_name
            .push(DnType::CommonName, "Artifactory test node");
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client = client_params.signed_by(&client_key, &issuer)?;
        Ok(Self(Arc::new(Material {
            ca_pem: ca.pem(),
            ca_der: ca.der().clone(),
            server_pem: server.pem(),
            server_der: server.der().clone(),
            server_key_pem: server_key.serialize_pem(),
            server_key_der: server_key.serialize_der(),
            client_pem: client.pem(),
            client_der: client.der().clone(),
            client_key_pem: client_key.serialize_pem(),
        })))
    }

    pub fn write_config(&self, directory: &Path, cfg: &mut Config) -> Result<()> {
        cfg.ca_file = directory.join("ca.pem");
        cfg.certificate_file = directory.join("client.pem");
        cfg.key_file = directory.join("client.key");
        for (path, value) in [
            (&cfg.ca_file, &self.0.ca_pem),
            (&cfg.certificate_file, &self.0.client_pem),
            (&cfg.key_file, &self.0.client_key_pem),
        ] {
            atomic_bytes(path, value.as_bytes(), false)?;
        }
        Ok(())
    }

    pub fn server_tls(&self) -> ServerTlsConfig {
        ServerTlsConfig::new()
            .identity(Identity::from_pem(
                &self.0.server_pem,
                &self.0.server_key_pem,
            ))
            .client_ca_root(Certificate::from_pem(&self.0.ca_pem))
    }

    pub fn write_server(&self, directory: &Path) -> Result<()> {
        for (name, value) in [
            ("ca.pem", &self.0.ca_pem),
            ("server.pem", &self.0.server_pem),
            ("server.key", &self.0.server_key_pem),
        ] {
            atomic_bytes(&directory.join(name), value.as_bytes(), false)?;
        }
        Ok(())
    }

    pub fn client_hash(&self) -> String {
        hex::encode(Sha256::digest(self.0.client_der.as_ref()))
    }

    pub(super) fn control_config(
        &self,
        versions: &[&'static rustls::SupportedProtocolVersion],
    ) -> Result<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = rustls::RootCertStore::empty();
        roots.add(self.0.ca_der.clone())?;
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()?;
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(versions)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![self.0.server_der.clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(self.0.server_key_der.clone()).into(),
            )?;
        tls.alpn_protocols = vec![b"h2".to_vec()];
        Ok(tls)
    }
}

#[cfg(test)]
mod tests {
    use crate::support::Fixture;
    use artifactory_emissary::runner;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::{Notify, Semaphore};
    use tokio_util::sync::CancellationToken;
    use tonic::Status;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_runner_hello_waits_for_hook_before_connection_is_published() {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = Fixture::start(root.path(), &[&rustls::version::TLS13]).await;
        let entered = Arc::new(Notify::new());
        let released = Arc::new(Semaphore::new(0));
        let notified = entered.clone();
        let gate = released.clone();
        let expected = fixture.cfg.emissary_id.clone();
        fixture.set_hello_hook(Arc::new(move |hello| {
            let notified = notified.clone();
            let gate = gate.clone();
            let expected = expected.clone();
            Box::pin(async move {
                assert_eq!(hello.kind, "hello");
                assert_eq!(hello.emissary_id, expected);
                let payload: serde_json::Value = serde_json::from_str(&hello.payload_json).unwrap();
                let startup: artifactory_emissary_protocol::nodes::MetadataSnapshot =
                    serde_json::from_value(payload["startup"].clone()).unwrap();
                startup.validate().unwrap();
                notified.notify_one();
                gate.acquire().await.unwrap().forget();
                Ok(())
            })
        }));
        let cfg = fixture.cfg.clone();
        let stop = CancellationToken::new();
        let token = stop.clone();
        let task = tokio::spawn(async move { runner::run(cfg, token).await });
        tokio::time::timeout(Duration::from_secs(15), entered.notified())
            .await
            .expect("real runner never reached hello hook");
        assert!(fixture.connections.try_recv().is_err());
        released.add_permits(1);
        let connection = fixture.connection().await;
        assert_eq!(connection.hello.emissary_id, fixture.cfg.emissary_id);
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(15), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failing_hello_hook_rejects_real_runner_before_connection_publication() {
        let root = tempfile::tempdir().unwrap();
        let mut fixture = Fixture::start(root.path(), &[&rustls::version::TLS13]).await;
        let entered = Arc::new(Notify::new());
        let notified = entered.clone();
        fixture.set_hello_hook(Arc::new(move |_| {
            let notified = notified.clone();
            Box::pin(async move {
                notified.notify_one();
                Err(Status::permission_denied("startup registration rejected"))
            })
        }));
        let cfg = fixture.cfg.clone();
        let stop = CancellationToken::new();
        let token = stop.clone();
        let task = tokio::spawn(async move { runner::run(cfg, token).await });
        tokio::time::timeout(Duration::from_secs(15), entered.notified())
            .await
            .expect("real runner never reached rejecting hello hook");
        assert!(
            tokio::time::timeout(Duration::from_millis(100), fixture.connections.recv())
                .await
                .is_err(),
            "failed registration must not publish a connection"
        );
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(15), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(fixture.connections.try_recv().is_err());
    }
}
