//! [INPUT]: 冻结旧接收器、可选旧main、私有目录与隔离数据库。
//! [OUTPUT]: 启动旧路由或完整旧进程，验证真实上报并回收隔离数据库。
//! [POS]: 由驱动复制到旧Telemetry测试包；完整进程另走真实迁移握手。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, anyhow, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_database, with_options};
use artifactory_emissary_protocol::telemetry_server::TelemetryServer;
use artifactory_telemetry_service::{Service, store};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use serde_json::json;
use std::{fs, path::Path, time::Duration};
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use uuid::Uuid;
#[path = "legacy_main_fixture.rs"]
mod legacy_main;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires private fixture directory and isolated TimescaleDB administrator connection"]
async fn serve_legacy_receiver() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("ARTIFACTORY_LEGACY_FIXTURE_DIR")
            .ok_or_else(|| anyhow!("legacy fixture directory is required"))?,
    );
    ensure!(root.is_dir(), "legacy fixture directory must exist");
    let url = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")
        .map_err(|_| anyhow!("isolated database connection is required"))?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2))
        .await
        .map_err(|_| anyhow!("legacy fixture administrator connection failed"))?;
    let database = format!("legacy_receiver_{}", Uuid::new_v4().simple());
    sql::execute(
        &mut admin,
        &format!("CREATE DATABASE {database}"),
        params![],
    )
    .await
    .map_err(|_| anyhow!("legacy fixture database creation failed"))?;

    let result = serve(&root, &url, &database).await;
    // 只删除本次随机数据库；FORCE使失败路径不遗留连接和数据库。
    let cleanup = sql::execute(
        &mut admin,
        &format!("DROP DATABASE {database} WITH (FORCE)"),
        params![],
    )
    .await
    .map_err(|_| anyhow!("legacy fixture database cleanup failed"));
    // 角色名绑定本次随机库；数据库删除后连同所有授权一起回收。
    let role_cleanup = if std::env::var_os("ARTIFACTORY_LEGACY_MAIN_BINARY").is_some() {
        let role = format!("legacy_auth_{}", &database["legacy_receiver_".len()..]);
        sql::execute(
            &mut admin,
            &format!("DROP ROLE IF EXISTS {role}"),
            params![],
        )
        .await
        .map_err(|_| anyhow!("legacy fixture role cleanup failed"))
    } else {
        Ok(0)
    };
    let _ = fs::remove_file(root.join("ready.json"));
    private_json(
        &root.join("closed.json"),
        &json!({"version":1,"database_name":database,"database_removed":cleanup.is_ok(),"authorization_role_removed":role_cleanup.is_ok()}),
    )?;
    result?;
    cleanup?;
    role_cleanup?;
    Ok(())
}

async fn serve(root: &Path, url: &str, database: &str) -> Result<()> {
    let scoped = with_database(url, database)
        .map_err(|_| anyhow!("legacy fixture database configuration failed"))?;
    if std::env::var_os("ARTIFACTORY_LEGACY_MAIN_BINARY").is_some() {
        return legacy_main::serve(root, &scoped, database).await;
    }
    let pool = connect(&scoped, store::models(), PoolSettings::new(4))
        .await
        .map_err(|_| anyhow!("legacy fixture database connection failed"))?;
    artifactory_database::require_timescale(&pool)
        .await
        .map_err(|_| anyhow!("legacy fixture requires TimescaleDB"))?;
    let stopped: bool = sql::scalar(
        &mut pool.clone(),
        "SELECT _timescaledb_functions.stop_background_workers()",
        params![],
    )
    .await
    .map_err(|_| anyhow!("legacy fixture scheduler stop failed"))?;
    ensure!(stopped, "legacy fixture scheduler did not stop");
    artifactory_database::migrate(&pool, &store::MIGRATIONS)
        .await
        .map_err(|_| anyhow!("legacy fixture migration failed"))?;
    sql::execute(&mut pool.clone(), "DO $fixture$ BEGIN CREATE TABLE nodes(id uuid PRIMARY KEY,certificate_hash text,revoked boolean NOT NULL DEFAULT false,certificate_expires_at timestamptz NOT NULL DEFAULT now()+interval '1 day'); CREATE TABLE attempts(id uuid PRIMARY KEY,node_id uuid NOT NULL); END $fixture$", params![])
        .await
        .map_err(|_| anyhow!("legacy fixture authorization schema failed"))?;
    let certificates = certificates(root)?;
    let node = Uuid::new_v4();
    let startup = Uuid::new_v4();
    sql::execute(
        &mut pool.clone(),
        "INSERT INTO nodes(id,certificate_hash) VALUES($1,$2)",
        params![node, certificates.client_hash],
    )
    .await
    .map_err(|_| anyhow!("legacy fixture certificate registration failed"))?;
    let readonly_url = with_options(&scoped, &[("default_transaction_read_only", "on")])
        .map_err(|_| anyhow!("legacy fixture read-only configuration failed"))?;
    let readonly = connect(&readonly_url, store::models(), PoolSettings::new(2))
        .await
        .map_err(|_| anyhow!("legacy fixture read-only connection failed"))?;
    ensure!(
        sql::execute(
            &mut readonly.clone(),
            "UPDATE nodes SET revoked=true",
            params![]
        )
        .await
        .is_err(),
        "legacy fixture authorization connection must reject writes"
    );
    let service = Service {
        store: store::TelemetryStore::new(pool.clone(), readonly.clone()),
        authorization: store::BusinessAuthorizationRepository::new(readonly.clone()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| anyhow!("legacy fixture listener failed"))?;
    let address = listener
        .local_addr()
        .map_err(|_| anyhow!("legacy fixture address failed"))?;
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            certificates.server,
            certificates.server_key,
        ))
        .client_ca_root(Certificate::from_pem(certificates.ca));
    let server = Server::builder()
        .tls_config(tls)
        .map_err(|_| anyhow!("legacy fixture TLS configuration failed"))?
        .add_service(TelemetryServer::new(service));
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let mut task = tokio::spawn(async move {
        server
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stopped.await;
                },
            )
            .await
    });
    let ready = private_json(
        &root.join("ready.json"),
        &json!({"version":1,"endpoint":format!("https://{address}"),"node_id":node,"startup_id":startup,"database_name":database}),
    );
    let outcome = if let Err(error) = ready {
        Err(error)
    } else {
        let deadline = tokio::time::sleep(Duration::from_secs(90));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break Err(anyhow!("legacy fixture stop deadline exceeded")),
                result = &mut task => {
                    return match result {
                        Ok(Ok(())) => Err(anyhow!("legacy fixture server stopped before stop request")),
                        Ok(Err(_)) => Err(anyhow!("legacy fixture server failed")),
                        Err(_) => Err(anyhow!("legacy fixture server task failed")),
                    };
                },
                _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    if root.join("stop").try_exists().unwrap_or(false) {
                        break Ok(());
                    }
                }
            }
        }
    };
    let _ = stop.send(());
    let shutdown = match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(_))) => Err(anyhow!("legacy fixture server shutdown failed")),
        Ok(Err(_)) => Err(anyhow!("legacy fixture server shutdown task failed")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err(anyhow!("legacy fixture server shutdown timed out"))
        }
    };
    drop(readonly);
    drop(pool);
    outcome?;
    shutdown
}

struct Certificates {
    ca: String,
    server: String,
    server_key: String,
    client_hash: String,
}

fn certificates(root: &Path) -> Result<Certificates> {
    let result = (|| -> Result<Certificates> {
        let ca_key = KeyPair::generate()?;
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Emissary Legacy Fixture CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let ca = params.self_signed(&ca_key)?;
        let issuer = Issuer::new(params, ca_key);
        let server_key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()])?;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server = params.signed_by(&server_key, &issuer)?;
        let client_key = KeyPair::generate()?;
        let mut params = CertificateParams::default();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client = params.signed_by(&client_key, &issuer)?;
        private_file(&root.join("ca.pem"), ca.pem().as_bytes())?;
        private_file(&root.join("server.pem"), server.pem().as_bytes())?;
        private_file(
            &root.join("server.key"),
            server_key.serialize_pem().as_bytes(),
        )?;
        private_file(&root.join("client.pem"), client.pem().as_bytes())?;
        private_file(
            &root.join("client.key"),
            client_key.serialize_pem().as_bytes(),
        )?;
        Ok(Certificates {
            ca: ca.pem(),
            server: server.pem(),
            server_key: server_key.serialize_pem(),
            client_hash: artifactory_service_transport::crypto::hash(client.der()),
        })
    })();
    result.map_err(|_| anyhow!("legacy fixture certificate creation failed"))
}

fn private_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| anyhow!("legacy fixture state encoding failed"))?;
    private_file(path, &bytes)
}

fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(path.parent().expect("fixture file has parent"))?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map_err(|_| anyhow!("legacy fixture private file write failed"))
}
