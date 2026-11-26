//! [INPUT]: 依赖已授权升级、冻结发行清单及租户和节点事务锁。
//! [OUTPUT]: 串行导入发行，派发手动升级并记录固定清单健康回执。
//! [POS]: 升级传输持久化边界；不自行创建更新。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use toasty::stmt::Type;

use uuid::Uuid;
#[path = "ota_renewal.rs"]
mod renewal;
pub(crate) struct OtaOffer {
    pub id: Uuid,
    pub generation: i64,
    pub cancel: bool,
    pub manifest: String,
    pub signature: String,
    pub object_key: String,
}
impl ControlStore {
    pub(crate) async fn existing_emissary_release(
        &self,
        tag: &str,
        hash: &str,
    ) -> Result<Option<Uuid>> {
        if let Some(row) = sql::optional_record(
            &mut self.pool.clone(),
            "SELECT id,manifest_hash FROM emissary_releases WHERE version=$1 AND tenant_id=$2",
            params![tag, self.tenant()?],
            &[("id", Type::Uuid), ("manifest_hash", Type::String)],
        )
        .await?
        {
            ensure!(
                row.decode::<String>("manifest_hash")? == hash,
                "release version is immutable"
            );
            return Ok(Some(row.decode("id")?));
        }
        Ok(None)
    }

    pub(crate) async fn import_emissary_release(
        &self,
        tag: &str,
        raw: String,
        signature: &str,
        hash: &str,
    ) -> Result<Uuid> {
        let tenant = self.tenant()?;
        let id = Uuid::new_v4();
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        // 与手动接受同序列化点；网络校验和对象下载已在事务前完成。
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,105))",
            params![tenant.to_string()],
        )
        .await?;
        sql::execute(&mut tx,"INSERT INTO emissary_releases(id,tenant_id,version,manifest,signature,manifest_hash) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(tenant_id,version) DO NOTHING",params![id,tenant,tag,raw,signature,hash]).await?;
        let row = sql::record(
            &mut tx,
            "SELECT id,manifest_hash FROM emissary_releases WHERE version=$1 AND tenant_id=$2",
            params![tag, tenant],
            &[("id", Type::Uuid), ("manifest_hash", Type::String)],
        )
        .await?;
        ensure!(
            row.decode::<String>("manifest_hash")? == hash,
            "release version is immutable"
        );
        let id = row.decode("id")?;
        tx.commit().await?;
        Ok(id)
    }

    pub(crate) async fn next_emissary_update(
        &self,
        node: Uuid,
        session: Uuid,
    ) -> Result<Option<OtaOffer>> {
        let pool: Uuid = sql::scalar(
            &mut self.pool.clone(),
            "SELECT pool_id FROM nodes WHERE id=$1",
            params![node],
        )
        .await?;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        let locked: bool = sql::scalar(
            &mut tx,
            "SELECT pg_try_advisory_xact_lock(hashtextextended($1,57))",
            params![pool.to_string()],
        )
        .await?;
        if !locked {
            tx.rollback().await?;
            return Ok(None);
        }
        let Some(n)=sql::optional_record(&mut tx, "SELECT os,arch FROM nodes WHERE id=$1 AND pool_id=$2 AND active_session=$3 AND NOT revoked AND certificate_expires_at>clock_timestamp() AND last_seen>clock_timestamp()-interval '30 seconds' FOR UPDATE SKIP LOCKED", params![node, pool, session], &[("os", Type::String),("arch", Type::String)]).await? else{tx.rollback().await?;return Ok(None)};
        let update = sql::optional_record(
            &mut tx,
            "SELECT id,generation,node_id,pool_id,release_id,previous_version,status,cancel_requested,error,retry_of,created_at,updated_at,finished_at,last_command_at FROM emissary_updates WHERE node_id=$1 AND finished_at IS NULL",
            params![node],
            &[
                ("id", Type::Uuid),
                ("generation", Type::I64),
                ("node_id", Type::Uuid),
                ("pool_id", Type::Uuid),
                ("release_id", Type::Uuid),
                ("previous_version", Type::String),
                ("status", Type::String),
                ("cancel_requested", Type::Bool),
                ("error", Type::String),
                ("retry_of", Type::Uuid),
                ("created_at", Type::Timestamp),
                ("updated_at", Type::Timestamp),
                ("finished_at", Type::Timestamp),
                ("last_command_at", Type::Timestamp),
            ],
        )
        .await?;
        let Some(u) = update else {
            tx.rollback().await?;
            return Ok(None);
        };
        if u.decode::<Option<chrono::DateTime<chrono::Utc>>>("last_command_at")?
            .is_some_and(|t| chrono::Utc::now() - t < chrono::Duration::seconds(10))
        {
            tx.rollback().await?;
            return Ok(None);
        }
        let id: Uuid = u.decode("id")?;
        let r = sql::record(
            &mut tx,
            "SELECT id,version,manifest,signature,manifest_hash,created_at FROM emissary_releases WHERE id=$1",
            params![u.decode::<Uuid>("release_id")?],
            &[
                ("id", Type::Uuid),
                ("version", Type::String),
                ("manifest", Type::String),
                ("signature", Type::String),
                ("manifest_hash", Type::String),
                ("created_at", Type::Timestamp),
            ],
        )
        .await?;
        let raw: String = r.decode("manifest")?;
        let m: Value = serde_json::from_str(&raw)?;
        let node_os: String = n.decode("os")?;
        let node_arch: String = n.decode("arch")?;
        let a = m["assets"]
            .as_array()
            .context("invalid stored manifest")?
            .iter()
            .find(|a| {
                a["os"].as_str() == Some(node_os.as_str())
                    && a["arch"].as_str() == Some(node_arch.as_str())
            })
            .context("platform not present")?;
        let object_key = format!(
            "emissary-releases/{}/{}",
            r.decode::<String>("manifest_hash")?,
            a["name"].as_str().context("invalid asset")?
        );
        sql::execute(&mut tx, "UPDATE emissary_updates SET last_command_at=now(),status=CASE WHEN status='queued' THEN 'offered' ELSE status END WHERE id=$1", params![id]).await?;
        tx.commit().await?;
        Ok(Some(OtaOffer {
            id,
            generation: u.decode("generation")?,
            cancel: u.decode("cancel_requested")?,
            manifest: raw,
            signature: r.decode("signature")?,
            object_key,
        }))
    }

    pub(crate) async fn report_emissary_update(
        &self,
        node: Uuid,
        session: Uuid,
        payload: &Value,
    ) -> Result<bool> {
        let Some(status) = payload.get("ota").filter(|s| s.is_object()) else {
            return Ok(false);
        };
        let mut status = status.clone();
        crate::ota::observability::observation(&status)?;
        let update_id = status["update_id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok());
        let runtime = payload["diagnostics"]["emissary_version"]
            .as_str()
            .unwrap_or("");
        if update_id.is_none() {
            let updated = sql::execute(&mut self.pool.clone(), "UPDATE nodes SET emissary_current_version=$3,emissary_launcher_version=$4,emissary_ota_status=$5 WHERE id=$1 AND active_session=$2 AND NOT revoked AND certificate_expires_at>clock_timestamp()", params![node,session,runtime,status["launcher_version"].as_str(),status]).await?;
            ensure!(updated == 1, "node session revoked or superseded");
            return Ok(true);
        }
        let pool: Uuid = sql::scalar(
            &mut self.pool.clone(),
            "SELECT pool_id FROM nodes WHERE id=$1",
            params![node],
        )
        .await?;
        let mut tx_db = self.pool.clone();
        let mut tx = tx_db.transaction().await?;
        sql::execute(
            &mut tx,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,57))",
            params![pool.to_string()],
        )
        .await?;
        let valid=sql::optional_record(&mut tx, "SELECT os,arch,emissary_ota_status FROM nodes WHERE id=$1 AND active_session=$2 AND NOT revoked AND certificate_expires_at>clock_timestamp() FOR UPDATE", params![node, session], &[("os",Type::String),("arch",Type::String),("emissary_ota_status",Type::String)]).await?;
        let valid = valid.context("node session revoked or superseded")?;
        let still_valid:bool=sql::scalar(&mut tx, "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=$1 AND active_session=$2 AND NOT revoked AND certificate_expires_at>clock_timestamp())", params![node, session]).await?;
        ensure!(
            still_valid,
            "node certificate expired while waiting for its lock"
        );
        let runtime = payload["diagnostics"]["emissary_version"]
            .as_str()
            .unwrap_or("");
        {
            let frozen=sql::optional_record(&mut tx,"SELECT u.generation,r.manifest FROM emissary_updates u JOIN emissary_releases r ON r.id=u.release_id WHERE u.id=$1 AND u.node_id=$2",params![update_id.expect("parsed update"),node],&[("generation",Type::I64),("manifest",Type::String)]).await?.context("unknown observed update")?;
            let generation: i64 = frozen.decode("generation")?;
            ensure!(
                status["generation"]
                    .as_i64()
                    .is_none_or(|v| v == generation),
                "stale update generation"
            );
            crate::ota::observability::accept(
                &mut status,
                &valid.decode::<Value>("emissary_ota_status")?,
                Some(generation),
                Some(&frozen.decode::<String>("manifest")?),
                &valid.decode::<String>("os")?,
                &valid.decode::<String>("arch")?,
            )?;
        }
        sql::execute(&mut tx, "UPDATE nodes SET emissary_current_version=$2,emissary_launcher_version=$3,emissary_ota_status=$4 WHERE id=$1", params![node, runtime, status["launcher_version"].as_str(), status]).await?;
        let mut health = true;
        if let Some(id) = status["update_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        && let Some(u)=sql::optional_record(&mut tx, "SELECT u.id,u.generation,u.node_id,u.pool_id,u.release_id,u.previous_version,u.status,u.cancel_requested,u.error,u.retry_of,u.created_at,u.updated_at,u.finished_at,u.last_command_at,r.manifest FROM emissary_updates u JOIN emissary_releases r ON r.id=u.release_id WHERE u.id=$1 AND u.node_id=$2 AND u.finished_at IS NULL", params![id, node], &[("id", Type::Uuid),("generation", Type::I64),("node_id", Type::Uuid),("pool_id", Type::Uuid),("release_id", Type::Uuid),("previous_version", Type::String),("status", Type::String),("cancel_requested", Type::Bool),("error", Type::String),("retry_of", Type::Uuid),("created_at", Type::Timestamp),("updated_at", Type::Timestamp),("finished_at", Type::Timestamp),("last_command_at", Type::Timestamp),("manifest", Type::String)]).await? {
            ensure!(status["generation"].as_i64().is_none_or(|v|Some(v)==u.decode::<i64>("generation").ok()),"stale update generation");
            let phase=status["phase"].as_str().unwrap_or("");
            let terminal=["succeeded","failed","rolled_back","canceled"].contains(&phase);
            if phase=="activating"||phase=="succeeded" {
                let frozen:Value=serde_json::from_str(&u.decode::<String>("manifest")?)?;
                health=Some(runtime)==frozen["version"].as_str()
                    || (phase=="activating"&&status["activation_pending"]==true&&Some(runtime)==u.decode::<Option<String>>("previous_version")?.as_deref());
            }
            if ["downloading","draining","activating","succeeded","failed","rolled_back","canceled"].contains(&phase)&&health {
                sql::execute(&mut tx, "UPDATE emissary_updates SET status=$2,error=$3,updated_at=now(),finished_at=CASE WHEN $4 THEN now() ELSE NULL END WHERE id=$1", params![id, phase, status["error"].as_str().map(|s|s.chars().take(1000).collect::<String>()), terminal]).await?;
                if terminal {
                    sql::execute(&mut tx, "UPDATE nodes SET ota_draining=false WHERE id=$1", params![node]).await?;
                    if phase=="failed"||phase=="rolled_back" {sql::execute(&mut tx, "UPDATE node_pools SET emissary_updates_paused=true WHERE id=$1", params![u.decode::<Uuid>("pool_id")?]).await?;}
                    super::models::AuditEvent::create().actor("emissary").action("emissary_update_finished").resource_id(id.to_string()).details(toasty::Json((json!({"node_id":node,"status":phase,"error":status["error"]})).clone())).exec(&mut tx).await?;
                }
            }
    }
        tx.commit().await?;
        Ok(health)
    }
}
