//! [INPUT]: 父驱动的精确资源清单、回环数据库和真实Control迁移。
//! [OUTPUT]: 创建独立双库与受限角色，核对ACL并按所有权标记定点清理。
//! [POS]: 完整Telemetry进程验收的资源边界；不扫描或删除共享资源。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use artifactory_database::{PoolSettings, connect, params, sql, with_database};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fs, path::PathBuf};

#[derive(Deserialize, Serialize)]
pub struct Resources {
    pub business: String,
    pub telemetry: String,
    pub role: String,
    pub password: String,
    pub nonce: String,
}

pub fn root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("ARTIFACTORY_CURRENT_MAIN_DIR")
            .context("private current main directory required")?,
    ))
}

impl Resources {
    pub fn read() -> Result<Self> {
        let value: Self = serde_json::from_slice(&fs::read(root()?.join("resources.json"))?)?;
        ensure!(
            value.nonce.len() == 32
                && value
                    .nonce
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid resource nonce"
        );
        ensure!(
            value.business == format!("emissary_business_{}", value.nonce)
                && value.telemetry == format!("emissary_telemetry_{}", value.nonce)
                && value.role == format!("emissary_auth_{}", value.nonce),
            "resource identities differ"
        );
        ensure!(
            value.password.len() == 32 && value.password.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid private role password"
        );
        Ok(value)
    }

    pub fn url(&self, database: &str) -> Result<String> {
        let value = std::env::var("ARTIFACTORY_TEST_DATABASE_URL")?;
        let address = url::Url::parse(&value)?;
        ensure!(
            matches!(address.host_str(), Some("127.0.0.1" | "localhost" | "::1")),
            "loopback database required"
        );
        with_database(&value, database)
    }

    pub async fn reserve(&self) -> Result<()> {
        let mut db = connect(
            &self.url("postgres")?,
            Default::default(),
            PoolSettings::new(2),
        )
        .await?;
        for name in [&self.business, &self.telemetry] {
            let present: bool = sql::scalar(
                &mut db,
                "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)",
                params![name.as_str()],
            )
            .await?;
            ensure!(!present, "requested database already exists");
        }
        let present: bool = sql::scalar(
            &mut db,
            "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)",
            params![self.role.as_str()],
        )
        .await?;
        ensure!(!present, "requested authorization role already exists");
        artifactory_emissary::config::atomic_json(
            &root()?.join("reserved.json"),
            &json!({"nonce":self.nonce,"absent_before_run":true}),
        )?;
        Ok(())
    }

    pub async fn create(&self) -> Result<()> {
        let reservation: serde_json::Value =
            serde_json::from_slice(&fs::read(root()?.join("reserved.json"))?)?;
        ensure!(
            reservation == json!({"nonce":self.nonce,"absent_before_run":true}),
            "private reservation differs"
        );
        let mut admin = connect(
            &self.url("postgres")?,
            Default::default(),
            PoolSettings::new(2),
        )
        .await?;
        for name in [&self.business, &self.telemetry] {
            sql::execute(&mut admin, &format!("CREATE DATABASE {name}"), params![]).await?;
            sql::execute(
                &mut admin,
                &format!("COMMENT ON DATABASE {name} IS '{}'", self.nonce),
                params![],
            )
            .await?;
            let mut db =
                connect(&self.url(name)?, Default::default(), PoolSettings::new(2)).await?;
            sql::execute(
                &mut db,
                "CREATE EXTENSION IF NOT EXISTS timescaledb",
                params![],
            )
            .await?;
            let stopped: bool = sql::scalar(
                &mut db,
                "SELECT _timescaledb_functions.stop_background_workers()",
                params![],
            )
            .await?;
            ensure!(stopped, "owned scheduler did not stop");
        }
        sql::execute(&mut admin, &format!("CREATE ROLE {} LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD '{}'", self.role, self.password), params![]).await?;
        sql::execute(
            &mut admin,
            &format!("COMMENT ON ROLE {} IS '{}'", self.role, self.nonce),
            params![],
        )
        .await?;
        artifactory_emissary::config::atomic_json(
            &root()?.join("created.json"),
            &json!({"nonce":self.nonce,"two_databases_and_role_created":true}),
        )?;
        Ok(())
    }

    pub async fn grant(&self, business: &mut toasty::Db) -> Result<String> {
        // 执行部署文件的原授权语句；仅替换三个psql标识符。
        let script = include_str!("../../../../deploy/telemetry-business-readonly.sql")
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace(":\"business_database\"", &format!("\"{}\"", self.business))
            .replace(":\"business_schema\"", "\"public\"")
            .replace(
                ":\"telemetry_business_role\"",
                &format!("\"{}\"", self.role),
            );
        for statement in script.split(';').filter(|value| !value.trim().is_empty()) {
            sql::execute(business, statement, params![]).await?;
        }
        let mut address = url::Url::parse(&self.url(&self.business)?)?;
        address
            .set_username(&self.role)
            .map_err(|_| anyhow::anyhow!("restricted username failed"))?;
        address
            .set_password(Some(&self.password))
            .map_err(|_| anyhow::anyhow!("restricted password failed"))?;
        address.set_query(None);
        let mut restricted =
            connect(address.as_str(), Default::default(), PoolSettings::new(2)).await?;
        let readonly: String =
            sql::scalar(&mut restricted, "SHOW transaction_read_only", params![]).await?;
        ensure!(
            readonly == "off",
            "ACL test must not rely on read-only setting"
        );
        for table in ["nodes", "attempts", "node_startups", "__toasty_migrations"] {
            let _: i64 = sql::scalar(
                &mut restricted,
                &format!("SELECT count(*) FROM {table} WHERE false"),
                params![],
            )
            .await?;
        }
        for denied in [
            "UPDATE nodes SET revoked=true",
            "DELETE FROM attempts WHERE false",
            "DELETE FROM node_startups WHERE false",
            "UPDATE __toasty_migrations SET id=id WHERE false",
            "CREATE TABLE forbidden_by_acl(id int)",
        ] {
            ensure!(
                sql::execute(&mut restricted, denied, params![])
                    .await
                    .is_err(),
                "restricted role escaped grant boundary"
            );
        }
        ensure!(
            sql::scalar::<i64>(&mut restricted, "SELECT count(*) FROM users", params![])
                .await
                .is_err(),
            "restricted role read private users"
        );
        let count: i64 = sql::scalar(
            &mut restricted,
            "SELECT count(*) FROM __toasty_migrations",
            params![],
        )
        .await?;
        ensure!(count > 0, "real business migration history missing");
        Ok(address.to_string())
    }

    pub async fn cleanup(&self) -> Result<()> {
        let mut admin = connect(
            &self.url("postgres")?,
            Default::default(),
            PoolSettings::new(2),
        )
        .await?;
        for name in [&self.telemetry, &self.business] {
            let marker: Option<String> = sql::optional_scalar(&mut admin, "SELECT coalesce(shobj_description(oid,'pg_database'),'') FROM pg_database WHERE datname=$1", params![name.as_str()]).await?;
            if let Some(marker) = marker {
                ensure!(
                    marker == self.nonce,
                    "database ownership marker differs; cleanup unconfirmed"
                );
                sql::execute(
                    &mut admin,
                    &format!("DROP DATABASE {name} WITH (FORCE)"),
                    params![],
                )
                .await?;
            }
            let exists: bool = sql::scalar(
                &mut admin,
                "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)",
                params![name.as_str()],
            )
            .await?;
            ensure!(!exists, "owned database cleanup read-back failed");
        }
        let marker: Option<String> = sql::optional_scalar(
            &mut admin,
            "SELECT coalesce(shobj_description(oid,'pg_authid'),'') FROM pg_roles WHERE rolname=$1",
            params![self.role.as_str()],
        )
        .await?;
        if let Some(marker) = marker {
            ensure!(
                marker == self.nonce,
                "role ownership marker differs; cleanup unconfirmed"
            );
            sql::execute(&mut admin, &format!("DROP ROLE {}", self.role), params![]).await?;
        }
        let exists: bool = sql::scalar(
            &mut admin,
            "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)",
            params![self.role.as_str()],
        )
        .await?;
        ensure!(!exists, "owned role cleanup read-back failed");
        artifactory_emissary::config::atomic_json(
            &root()?.join("cleaned.json"),
            &json!({"nonce":self.nonce,"databases_removed":true,"role_removed":true}),
        )?;
        Ok(())
    }
}
