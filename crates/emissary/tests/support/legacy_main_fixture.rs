//! [INPUT]: 冻结旧main、新旧迁移及正文引用SQL、Timescale与mTLS。
//! [OUTPUT]: 验证完整旧进程接收，追加迁移后拒绝旧程序且保留数据。
//! [POS]: 旧接收端门禁的进程级边界，不重写Service或迁移规则。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{certificates, private_json};
use anyhow::{Result, anyhow, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_options};
use serde_json::json;
use std::{fs, path::Path, process::Stdio, time::Duration};
use toasty::migration::{MigrationFile, MigrationSet};
use uuid::Uuid;

static BUSINESS: MigrationSet = toasty::embed_migrations!("../control-plane/migrations");

pub(super) async fn serve(root: &Path, scoped: &str, database: &str) -> Result<()> {
    let mut owner = connect(scoped, Default::default(), PoolSettings::new(2)).await?;
    artifactory_database::require_timescale(&owner).await?;
    let stopped: bool = sql::scalar(
        &mut owner,
        "SELECT _timescaledb_functions.stop_background_workers()",
        params![],
    )
    .await?;
    ensure!(stopped, "legacy main fixture scheduler did not stop");
    sql::execute(&mut owner, "CREATE SCHEMA business", params![]).await?;
    let business_url = with_options(scoped, &[("search_path", "business,public")])?;
    let mut business = connect(&business_url, Default::default(), PoolSettings::new(2)).await?;
    artifactory_database::migrate(&business, &BUSINESS).await?;
    let identity = certificates(root)?;
    let user = Uuid::new_v4();
    let tenant = Uuid::new_v4();
    let pool = Uuid::new_v4();
    let node = Uuid::new_v4();
    let startup = Uuid::new_v4();
    let mut tx = business.transaction().await?;
    sql::execute(&mut tx, "INSERT INTO users(id,username,password_hash,role,display_name) VALUES($1,'legacy-fixture','unused','admin','fixture')", params![user]).await?;
    sql::execute(&mut tx, "INSERT INTO tenants(id,name,owner_id,created_by,creation_key,slug) VALUES($1,'fixture',$2,$2,$3,'fixture')", params![tenant,user,Uuid::new_v4()]).await?;
    sql::execute(
        &mut tx,
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'owner')",
        params![tenant, user],
    )
    .await?;
    tx.commit().await?;
    sql::execute(
        &mut business,
        "INSERT INTO node_pools(id,name,tenant_id) VALUES($1,'fixture',$2)",
        params![pool, tenant],
    )
    .await?;
    sql::execute(&mut business, "INSERT INTO nodes(id,name,os,arch,pool_id,tenant_id,certificate_hash,certificate_expires_at,startup_id) VALUES($1,'fixture','linux','amd64',$2,$3,$4,now()+interval '1 day',$5)", params![node,pool,tenant,identity.client_hash,startup]).await?;
    let role = format!("legacy_auth_{}", &database["legacy_receiver_".len()..]);
    let secret = Uuid::new_v4().simple().to_string();
    sql::execute(
        &mut owner,
        &format!("CREATE ROLE {role} LOGIN NOINHERIT PASSWORD '{secret}'"),
        params![],
    )
    .await?;
    let result = run(
        root,
        scoped,
        &business_url,
        database,
        &role,
        &secret,
        node,
        startup,
    )
    .await;
    // 外层先关闭本随机数据库再回收角色，失败路径使用同一确定身份。
    result
}

#[allow(clippy::too_many_arguments)]
async fn run(
    root: &Path,
    scoped: &str,
    business_url: &str,
    database: &str,
    role: &str,
    secret: &str,
    node: Uuid,
    startup: Uuid,
) -> Result<()> {
    let mut owner = connect(scoped, Default::default(), PoolSettings::new(2)).await?;
    for statement in [
        format!("GRANT CONNECT ON DATABASE {database} TO {role}"),
        format!("GRANT USAGE ON SCHEMA public,business TO {role}"),
        format!(
            "GRANT SELECT ON business.nodes,business.attempts,business.__toasty_migrations TO {role}"
        ),
    ] {
        sql::execute(&mut owner, &statement, params![]).await?;
    }
    let mut restricted = url::Url::parse(business_url)?;
    restricted
        .set_username(role)
        .map_err(|_| anyhow!("fixture role URL failed"))?;
    restricted
        .set_password(Some(secret))
        .map_err(|_| anyhow!("fixture role URL failed"))?;
    let auth = with_options(
        restricted.as_str(),
        &[("default_transaction_read_only", "on")],
    )?;
    let mut readonly = connect(&auth, Default::default(), PoolSettings::new(1)).await?;
    ensure!(
        sql::execute(&mut readonly, "UPDATE nodes SET revoked=true", params![])
            .await
            .is_err(),
        "fixture business role allowed writes"
    );
    let mut acl = connect(
        restricted.as_str(),
        Default::default(),
        PoolSettings::new(1),
    )
    .await?;
    let writable_setting: String =
        sql::scalar(&mut acl, "SHOW transaction_read_only", params![]).await?;
    ensure!(
        writable_setting == "off",
        "fixture ACL check must not use read-only setting"
    );
    ensure!(
        sql::execute(&mut acl, "UPDATE nodes SET revoked=true", params![])
            .await
            .is_err(),
        "fixture role ACL allowed writes"
    );
    let versions: i64 = sql::scalar(
        &mut acl,
        "SELECT count(*) FROM __toasty_migrations",
        params![],
    )
    .await?;
    ensure!(
        versions as usize == BUSINESS.migrations().len(),
        "fixture business migration read-back mismatch"
    );
    let grpc = std::net::TcpListener::bind("127.0.0.1:0")?;
    let http = std::net::TcpListener::bind("127.0.0.1:0")?;
    let grpc_address = grpc.local_addr()?;
    let http_address = http.local_addr()?;
    let token = Uuid::new_v4().simple().to_string();
    let log = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("legacy-main.log"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        log.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let mut command = tokio::process::Command::new(
        std::env::var_os("ARTIFACTORY_LEGACY_MAIN_BINARY").expect("main mode"),
    );
    command
        .env("DATABASE_URL", scoped)
        .env("ARTIFACTORY_TELEMETRY_READ_DATABASE_URL", scoped)
        .env("ARTIFACTORY_BUSINESS_DATABASE_URL", auth)
        .env("ARTIFACTORY_TELEMETRY_TOKEN", &token)
        .env("ARTIFACTORY_PKI_DIR", root)
        .env("ARTIFACTORY_SERVICE_TLS_CERT", root.join("server.pem"))
        .env("ARTIFACTORY_SERVICE_TLS_KEY", root.join("server.key"))
        .env("ARTIFACTORY_TELEMETRY_GRPC_BIND", grpc_address.to_string())
        .env("ARTIFACTORY_TELEMETRY_HTTP_BIND", http_address.to_string())
        .env("ARTIFACTORY_TELEMETRY_WRITE_CONNECTIONS", "2")
        .env("ARTIFACTORY_TELEMETRY_READ_CONNECTIONS", "2")
        .env("ARTIFACTORY_TELEMETRY_AUTH_CONNECTIONS", "2")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .kill_on_drop(true);
    drop(grpc);
    drop(http);
    let mut child = command.spawn()?;
    let result = async {
        let deadline = tokio::time::Instant::now()+Duration::from_secs(20);
        loop {
            ensure!(child.try_wait()?.is_none(), "legacy main exited before ready; inspect private log");
            if probe(http_address, &token, root).await.is_ok() { break; }
            ensure!(tokio::time::Instant::now()<deadline, "legacy main readiness timed out");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        private_json(&root.join("ready.json"), &json!({"version":1,"endpoint":format!("https://{grpc_address}"),"node_id":node,"startup_id":startup,"database_name":database,"receiver_boundary":"frozen main with real migrations and restricted business authorization","business_schema":"business"}))?;
        let deadline = tokio::time::Instant::now()+Duration::from_secs(90);
        while !root.join("stop").try_exists()? {
            ensure!(child.try_wait()?.is_none(), "legacy main exited during delivery");
            ensure!(tokio::time::Instant::now()<deadline, "legacy main stop deadline exceeded");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_,anyhow::Error>(())
    }.await;
    #[cfg(unix)]
    if child.try_wait()?.is_none() {
        let status = std::process::Command::new("kill")
            .args(["-INT", &child.id().expect("live child").to_string()])
            .status()?;
        ensure!(status.success(), "legacy main interrupt failed");
    }
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            child.kill().await?;
            return Err(anyhow!("legacy main required forced shutdown"));
        }
    };
    result?;
    ensure!(
        status.success(),
        "legacy main shutdown failed; inspect private log"
    );
    // 先证明业务握手拒绝，再证明自身遥测迁移拒绝；不改迁移元数据。
    let batches: serde_json::Value = sql::scalar(&mut owner, "SELECT coalesce(jsonb_agg(to_jsonb(b) ORDER BY batch_id),'[]'::jsonb) FROM telemetry_batches b", params![]).await?;
    let mut business = connect(business_url, Default::default(), PoolSettings::new(1)).await?;
    let node_before: serde_json::Value = sql::scalar(
        &mut business,
        "SELECT to_jsonb(n) FROM nodes n WHERE id=$1",
        params![node],
    )
    .await?;
    let business_migrations = appended(
        &BUSINESS,
        MigrationFile::new(
            2026112503,
            "0010_node_operations.sql",
            include_str!("current_business.sql"),
        ),
    );
    artifactory_database::migrate(&business, &business_migrations).await?;
    rejects_after_migration(&mut command, root, "business").await?;
    let telemetry_migrations = appended(
        &artifactory_telemetry_service::store::MIGRATIONS,
        MigrationFile::new(
            2026112501,
            "0003_inventory_delivery.sql",
            include_str!("current_telemetry.sql"),
        ),
    );
    let telemetry_migrations = appended(
        &telemetry_migrations,
        MigrationFile::new(
            2026112601,
            "0004_inventory_integrity.sql",
            include_str!("current_integrity.sql"),
        ),
    );
    artifactory_database::migrate(&owner, &telemetry_migrations).await?;
    rejects_after_migration(&mut command, root, "telemetry").await?;
    artifactory_database::validate(&business, &business_migrations).await?;
    artifactory_database::validate(&owner, &telemetry_migrations).await?;
    let node_after: serde_json::Value = sql::scalar(
        &mut business,
        "SELECT to_jsonb(n) FROM nodes n WHERE id=$1",
        params![node],
    )
    .await?;
    let after: serde_json::Value = sql::scalar(&mut owner, "SELECT coalesce(jsonb_agg(to_jsonb(b) ORDER BY batch_id),'[]'::jsonb) FROM telemetry_batches b", params![]).await?;
    ensure!(
        batches == after && node_before == node_after,
        "old main rejection modified existing data"
    );
    private_json(
        &root.join("main-evidence.json"),
        &json!({"business_migrations":versions,"business_migration_handshake":true,"authorization_acl_read_only":true,"protected_http_ready":true,"graceful_exit":true,"old_main_rejected_new_business":true,"old_main_rejected_new_telemetry":true,"rejected_startup_preserved_data":true}),
    )?;
    Ok(())
}

fn appended(set: &MigrationSet, next: MigrationFile) -> MigrationSet {
    let mut migrations = set.migrations().to_vec();
    migrations.push(next);
    MigrationSet::new(Box::leak(migrations.into_boxed_slice()))
}

async fn rejects_after_migration(
    command: &mut tokio::process::Command,
    root: &Path,
    phase: &str,
) -> Result<()> {
    let path = root.join(format!("legacy-{phase}-rejected.log"));
    let log = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        log.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let mut child = command.stdout(log.try_clone()?).stderr(log).spawn()?;
    let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            child.kill().await?;
            return Err(anyhow!("old main migration rejection timed out"));
        }
    };
    ensure!(!status.success(), "old main accepted unknown migration");
    let output = fs::read_to_string(path)?;
    ensure!(
        output.contains("unrecognized database migration"),
        "old main failed without expected migration rejection; inspect private log"
    );
    Ok(())
}

async fn probe(address: std::net::SocketAddr, token: &str, root: &Path) -> Result<()> {
    let mut child = tokio::process::Command::new("openssl")
        .args([
            "s_client",
            "-quiet",
            "-verify_return_error",
            "-verify_ip",
            "127.0.0.1",
            "-CAfile",
        ])
        .arg(root.join("ca.pem"))
        .args(["-connect", &address.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    use tokio::io::AsyncWriteExt;
    child.stdin.take().expect("probe stdin").write_all(format!("GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let output = tokio::time::timeout(Duration::from_secs(2), child.wait_with_output()).await??;
    ensure!(
        output.stdout.starts_with(b"HTTP/1.1 200"),
        "legacy main protected HTTP not ready"
    );
    Ok(())
}
