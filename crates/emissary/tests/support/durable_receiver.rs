//! [INPUT]: 真实Control/Telemetry迁移、测试节点身份和隔离TimescaleDB。
//! [OUTPUT]: 提供具名隔离库、真实业务种子、独立精确清理与原正文逐项读回。
//! [POS]: 普通重开、原生恢复与父进程补偿共用存储边界；不替代完整Control注册或ACL验收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![allow(dead_code, reason = "共享夹具由独立集成测试按需调用")]
use anyhow::{Context, Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_database, with_options};
use artifactory_emissary::config::Config;
use artifactory_emissary_protocol::{
    InventoryObservation, nodes::MetadataSnapshot, telemetry::Batch,
};
use artifactory_telemetry_service::{Service, store};
use futures::FutureExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet, future::Future, panic::AssertUnwindSafe, path::PathBuf, time::Duration,
};
use uuid::Uuid;

static BUSINESS: toasty::migration::MigrationSet =
    toasty::embed_migrations!("../control-plane/migrations");
pub const FENCE: &str = "SELECT pg_advisory_xact_lock(438284731022932)";

/// 所有执行结果先清理自有数据库，再返回成功证据或原错误。
pub async fn isolated<F, Fut>(budget: Duration, run: F) -> Result<Value>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let name = format!("durable_receiver_{}", Uuid::new_v4().simple());
    isolated_named(budget, &name, run).await
}

/// 调用方预选精确库名；业务执行只在CREATE成功后开始。
pub async fn isolated_named<F, Fut>(budget: Duration, name: &str, run: F) -> Result<Value>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    validate_owned_name(name)?;
    let url = isolated_database_url()?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
    let scoped = with_database(&url, name)?;
    sql::execute(&mut admin, &format!("CREATE DATABASE {name}"), params![]).await?;
    let outcome = AssertUnwindSafe(tokio::time::timeout(
        budget,
        async move { run(scoped).await },
    ))
    .catch_unwind()
    .await;
    drop_owned(&mut admin, name).await?;
    match outcome {
        Ok(Ok(Ok(mut proof))) => {
            ensure!(
                proof.is_object(),
                "durable receiver proof must be an object"
            );
            proof["database_removed"] = json!(true);
            Ok(proof)
        }
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) => {
            anyhow::bail!("durable receiver regression timed out; isolated database removed")
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// 父进程凭自有记录重试精确清理；缺失库也必须读回确认。
pub async fn cleanup_owned(name: &str) -> Result<()> {
    validate_owned_name(name)?;
    let url = isolated_database_url()?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
    drop_owned(&mut admin, name).await
}

/// 父进程保留库名前只读确认；已有名字不得取得清理授权。
pub async fn assert_absent(name: &str) -> Result<()> {
    validate_owned_name(name)?;
    let url = isolated_database_url()?;
    let mut admin = connect(&url, Default::default(), PoolSettings::new(2)).await?;
    let exists: bool = sql::scalar(
        &mut admin,
        "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)",
        params![name],
    )
    .await
    .context("owned isolated database reservation check failed")?;
    ensure!(!exists, "owned isolated database name already exists");
    Ok(())
}

fn validate_owned_name(name: &str) -> Result<()> {
    let valid = name
        .strip_prefix("durable_receiver_")
        .is_some_and(|suffix| {
            suffix.len() == 32
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        });
    ensure!(valid, "invalid owned isolated database name");
    Ok(())
}

fn isolated_database_url() -> Result<String> {
    let url =
        std::env::var("ARTIFACTORY_TEST_DATABASE_URL").context("isolated database required")?;
    let address = url::Url::parse(&url).context("invalid isolated database URL")?;
    ensure!(
        matches!(address.host_str(), Some("127.0.0.1" | "localhost" | "::1")),
        "durable receiver requires loopback database"
    );
    Ok(url)
}

async fn drop_owned(admin: &mut toasty::Db, name: &str) -> Result<()> {
    sql::execute(
        admin,
        &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        params![],
    )
    .await
    .context("owned isolated database cleanup failed")?;
    let remains: bool = sql::scalar(
        admin,
        "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)",
        params![name],
    )
    .await
    .context("owned isolated database cleanup read-back failed")?;
    ensure!(
        !remains,
        "owned isolated database still exists after cleanup"
    );
    Ok(())
}

pub struct ReceiverStore {
    pub pool: toasty::Db,
    pub business: toasty::Db,
    pub service: Service,
}

/// 仅在本次隔离数据库创建扩展、停止后台任务和应用真实迁移。
pub async fn open(
    url: &str,
    cfg: &Config,
    certificate_hash: &str,
    startups: &BTreeSet<Uuid>,
    current: Uuid,
) -> Result<ReceiverStore> {
    let address = url::Url::parse(url).context("invalid isolated database URL")?;
    ensure!(
        matches!(address.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
            && address
                .path()
                .trim_start_matches('/')
                .starts_with("durable_receiver_"),
        "receiver store requires its owned loopback database"
    );
    let pool = connect(url, store::models(), PoolSettings::new(8)).await?;
    sql::execute(
        &mut pool.clone(),
        "CREATE EXTENSION IF NOT EXISTS timescaledb",
        params![],
    )
    .await?;
    artifactory_database::require_timescale(&pool).await?;
    let stopped: bool = sql::scalar(
        &mut pool.clone(),
        "SELECT _timescaledb_functions.stop_background_workers()",
        params![],
    )
    .await?;
    ensure!(stopped, "isolated database scheduler did not stop");
    let (business, authorization) =
        business(url, &pool, cfg, certificate_hash, startups, current).await?;
    artifactory_database::migrate(&pool, &store::MIGRATIONS).await?;
    let service = Service {
        store: store::TelemetryStore::new(pool.clone(), pool.clone()),
        authorization: store::BusinessAuthorizationRepository::new(authorization),
    };
    Ok(ReceiverStore {
        pool,
        business,
        service,
    })
}

impl ReceiverStore {
    /// 使用真实hello快照固定启动摘要；重连不得改变同startup的内容。
    pub async fn register_startup(&self, node: Uuid, snapshot: &MetadataSnapshot) -> Result<()> {
        register_startup(&self.business, node, snapshot).await
    }
}

/// 将测试Control的Hello事务绑定到调用方业务库，不授予Telemetry写权限。
pub async fn register_startup(
    business: &toasty::Db,
    node: Uuid,
    snapshot: &MetadataSnapshot,
) -> Result<()> {
    snapshot.validate()?;
    let digest = snapshot.digest()?;
    let mut db = business.clone();
    let mut tx = db.transaction().await?;
    let authorized: Option<Uuid> = sql::optional_scalar(&mut tx,
            "SELECT id FROM nodes WHERE id=$1 AND NOT revoked AND certificate_expires_at>clock_timestamp() FOR UPDATE",
            params![node]).await?;
    ensure!(authorized.is_some(), "fixture node authorization expired");
    sql::execute(&mut tx,
            "INSERT INTO node_startups(node_id,startup_id,digest) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            params![node,snapshot.startup_id,digest.clone()]).await?;
    let stored: String = sql::scalar(
        &mut tx,
        "SELECT digest FROM node_startups WHERE node_id=$1 AND startup_id=$2",
        params![node, snapshot.startup_id],
    )
    .await?;
    ensure!(
        stored == digest,
        "immutable fixture startup metadata changed"
    );
    sql::execute(&mut tx,
            "UPDATE nodes SET startup_id=$2,metadata=$3,metadata_digest=$4,metadata_received_at=clock_timestamp(),capabilities=$5 WHERE id=$1",
            params![node,snapshot.startup_id,json!(snapshot.metadata),digest,json!(snapshot.capabilities)]).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn verify_database(
    pool: &toasty::Db,
    node: Uuid,
    batches: &[(PathBuf, Vec<u8>, Batch)],
    inventories: &[(PathBuf, Vec<u8>, InventoryObservation)],
) -> Result<()> {
    for (_, bytes, batch) in batches {
        let (count, digest): (i64, String) = sql::tuple(
            &mut pool.clone(),
            "SELECT count(*),min(digest) FROM telemetry_batches WHERE node_id=$1 AND batch_id=$2",
            params![node, batch.id],
        )
        .await?;
        ensure!(
            count == 1 && digest == hex::encode(Sha256::digest(bytes)),
            "batch receipt differs from original ID/digest"
        );
        if batch.host.is_some() {
            let (count, sampled, session, monotonic): (i64, i64, Uuid, i64) = sql::tuple(&mut pool.clone(),
                "SELECT count(*),min((extract(epoch FROM sampled_at)*1000000)::bigint),min(session_id::text)::uuid,min(monotonic_ms) FROM host_samples WHERE node_id=$1 AND batch_id=$2",
                params![node,batch.id]).await?;
            ensure!(
                count == 1
                    && sampled == batch.sampled_at.timestamp_micros()
                    && session == batch.session_id
                    && monotonic == batch.monotonic_ms as i64,
                "host sample time or startup changed during replay"
            );
        }
        if let Some(inventory) = &batch.inventory {
            let expected = serde_json::to_value(inventory)?;
            let digest = hex::encode(Sha256::digest(serde_json::to_vec(&expected)?));
            let count: i64 = sql::scalar(&mut pool.clone(),
                "SELECT count(*) FROM inventory_observations WHERE node_id=$1 AND sampled_at=$2 AND digest=$3",
                params![node,batch.sampled_at,digest.clone()]).await?;
            let actual: Value = sql::scalar(
                &mut pool.clone(),
                "SELECT inventory FROM inventory_versions WHERE node_id=$1 AND digest=$2",
                params![node, digest],
            )
            .await?;
            ensure!(
                count == 1 && actual == expected,
                "history inventory time or payload changed"
            );
        }
    }
    for (_, _, observation) in inventories {
        let value: Value = sql::scalar(
            &mut pool.clone(),
            "SELECT to_jsonb(d) FROM inventory_deliveries d WHERE node_id=$1 AND observation_id=$2",
            params![node, Uuid::parse_str(&observation.observation_id)?],
        )
        .await?;
        ensure!(
            value["startup_id"] == observation.source_startup_id
                && value["revision"].as_u64() == Some(observation.revision)
                && value["delivery_digest"] == observation.delivery_digest,
            "inventory receipt identity, revision or digest changed"
        );
        let expected_source = if observation.source_batch_id.is_empty() {
            Value::Null
        } else {
            json!(observation.source_batch_id)
        };
        let expected_inventory = serde_json::to_value(observation.validate()?)?;
        let expected_digest = hex::encode(Sha256::digest(serde_json::to_vec(&expected_inventory)?));
        ensure!(
            value["source_batch_id"] == expected_source
                && value["inventory_digest"] == expected_digest,
            "inventory source identity or payload digest changed"
        );
        let actual: Value = sql::scalar(
            &mut pool.clone(),
            "SELECT inventory FROM inventory_versions WHERE node_id=$1 AND digest=$2",
            params![node, expected_digest],
        )
        .await?;
        ensure!(
            actual == expected_inventory,
            "independent inventory payload changed"
        );
        let observed = value["observed_at"]
            .as_str()
            .context("inventory time missing")?
            .parse::<chrono::DateTime<chrono::Utc>>()?;
        ensure!(
            observed.timestamp_millis() == observation.observed_at_ms,
            "inventory observed time changed"
        );
        let count: i64 = sql::scalar(
            &mut pool.clone(),
            "SELECT count(*) FROM inventory_deliveries WHERE node_id=$1 AND observation_id=$2",
            params![node, Uuid::parse_str(&observation.observation_id)?],
        )
        .await?;
        ensure!(count == 1, "inventory replay duplicated receipt");
    }
    Ok(())
}

async fn business(
    url: &str,
    telemetry: &toasty::Db,
    cfg: &Config,
    certificate_hash: &str,
    startups: &BTreeSet<Uuid>,
    current: Uuid,
) -> Result<(toasty::Db, toasty::Db)> {
    sql::execute(&mut telemetry.clone(), "CREATE SCHEMA business", params![]).await?;
    let scoped = with_options(url, &[("search_path", "business,public")])?;
    let business = connect(&scoped, Default::default(), PoolSettings::new(2)).await?;
    seed_business(&business, cfg, certificate_hash, startups, current).await?;
    let readonly = with_options(&scoped, &[("default_transaction_read_only", "on")])?;
    Ok((
        business,
        connect(&readonly, Default::default(), PoolSettings::new(2)).await?,
    ))
}

/// 在调用方专属空库应用完整Control迁移，不改写迁移历史。
pub async fn seed_business(
    business: &toasty::Db,
    cfg: &Config,
    certificate_hash: &str,
    startups: &BTreeSet<Uuid>,
    current: Uuid,
) -> Result<()> {
    artifactory_database::migrate(business, &BUSINESS).await?;
    let user = Uuid::new_v4();
    let tenant = Uuid::new_v4();
    let pool = Uuid::new_v4();
    let node = Uuid::parse_str(&cfg.emissary_id)?;
    let mut writer = business.clone();
    let mut tx = writer.transaction().await?;
    sql::execute(&mut tx,"INSERT INTO users(id,username,password_hash,role,display_name) VALUES($1,'durable-fixture','unused','admin','fixture')",params![user]).await?;
    sql::execute(&mut tx,"INSERT INTO tenants(id,name,owner_id,created_by,creation_key,slug) VALUES($1,'fixture',$2,$2,$3,'fixture')",params![tenant,user,Uuid::new_v4()]).await?;
    sql::execute(
        &mut tx,
        "INSERT INTO tenant_members(tenant_id,user_id,role) VALUES($1,$2,'owner')",
        params![tenant, user],
    )
    .await?;
    sql::execute(
        &mut tx,
        "INSERT INTO node_pools(id,name,tenant_id) VALUES($1,'fixture',$2)",
        params![pool, tenant],
    )
    .await?;
    let (os, arch) = artifactory_emissary::platform();
    sql::execute(&mut tx,"INSERT INTO nodes(id,name,os,arch,pool_id,tenant_id,certificate_hash,certificate_expires_at,startup_id) VALUES($1,'fixture',$2,$3,$4,$5,$6,now()+interval '1 day',$7)",params![node,os,arch,pool,tenant,certificate_hash,current]).await?;
    for startup in startups {
        let digest = artifactory_emissary_protocol::nodes::MetadataSnapshot::new(
            *startup,
            Default::default(),
            Vec::new(),
        )?
        .digest()?;
        sql::execute(
            &mut tx,
            "INSERT INTO node_startups(node_id,startup_id,digest) VALUES($1,$2,$3)",
            params![node, *startup, digest],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
