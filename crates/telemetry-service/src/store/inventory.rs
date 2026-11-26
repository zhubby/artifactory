//! [INPUT]: 已认证节点、不可变清单观察与遥测事务池。
//! [OUTPUT]: 提交幂等清单回执、startup摘要与跨副本准入。
//! [POS]: 独立清单持久边界，和删除流程共用围栏锁。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::internal;
use artifactory_database::{params, sql};
use artifactory_emissary_protocol::{InventoryObservation, telemetry_negotiation::RejectionReason};
use artifactory_service_transport::crypto;
use chrono::{DateTime, Utc};
use toasty::Db;
use tonic::Status;
use uuid::Uuid;

/// 只有事务提交成功才允许调用者返回ACK；旧startup只更新自己的分区。
pub async fn ingest_inventory(
    pool: &Db,
    node: Uuid,
    observation: &InventoryObservation,
    received: DateTime<Utc>,
) -> Result<bool, Status> {
    if observation.inventory_schema != 1 {
        return Err(RejectionReason::SchemaUnsupported.status());
    }
    let inventory = observation
        .validate()
        .map_err(|_| RejectionReason::Malformed.status())?;
    let id = Uuid::parse_str(&observation.observation_id)
        .map_err(|_| RejectionReason::Malformed.status())?;
    let startup = Uuid::parse_str(&observation.source_startup_id)
        .map_err(|_| RejectionReason::Malformed.status())?;
    let source = if observation.source_batch_id.is_empty() {
        None
    } else {
        Some(
            Uuid::parse_str(&observation.source_batch_id)
                .map_err(|_| RejectionReason::Malformed.status())?,
        )
    };
    let observed = DateTime::from_timestamp_millis(observation.observed_at_ms)
        .ok_or_else(|| RejectionReason::Malformed.status())?;
    if observed > received + chrono::Duration::minutes(5) {
        return Err(RejectionReason::ClockFuture.status());
    }
    let value = serde_json::to_value(inventory).map_err(internal)?;
    let digest = crypto::hash(serde_json::to_vec(&value).map_err(internal)?);
    let mut db = pool.clone();
    let mut tx = db.transaction().await.map_err(internal)?;
    sql::execute(
        &mut tx,
        "SELECT pg_advisory_xact_lock_shared(438284731022932)",
        params![],
    )
    .await
    .map_err(internal)?;
    let retired: bool = sql::scalar(
        &mut tx,
        "SELECT EXISTS(SELECT 1 FROM telemetry_deleted_identities WHERE kind='node' AND id=$1)",
        params![node],
    )
    .await
    .map_err(internal)?;
    if retired {
        return Err(RejectionReason::IdentityRetired.status());
    }
    // 事务锁跨副本限制单节点清单写入，不保留无限内存节点映射。
    let locked: bool = sql::scalar(
        &mut tx,
        "SELECT pg_try_advisory_xact_lock(hashtextextended($1,47))",
        params![format!("inventory:{node}")],
    )
    .await
    .map_err(internal)?;
    if !locked {
        return Err(RejectionReason::Overloaded.status());
    }
    let prior: Option<String> = sql::optional_scalar(
        &mut tx,
        "SELECT delivery_digest FROM inventory_deliveries WHERE node_id=$1 AND observation_id=$2",
        params![node, id],
    )
    .await
    .map_err(internal)?;
    if let Some(prior) = prior {
        if prior != observation.delivery_digest {
            return Err(RejectionReason::IdContentConflict.status());
        }
        tx.commit().await.map_err(internal)?;
        return Ok(true);
    }
    let conflict: bool = sql::scalar(&mut tx,
        "SELECT EXISTS(SELECT 1 FROM inventory_deliveries WHERE node_id=$1 AND ((source_batch_id IS NULL AND $2::uuid IS NULL AND startup_id=$3 AND revision=$4) OR (source_batch_id=$2 AND $2::uuid IS NOT NULL)))",
        params![node,source,startup,observation.revision as i64]).await.map_err(internal)?;
    if conflict {
        return Err(RejectionReason::IdContentConflict.status());
    }
    // 每分钟补两个token、burst四条；成功回执重放不消耗token。
    sql::execute(&mut tx, "INSERT INTO inventory_ingress(node_id,tokens,updated_at) VALUES($1,4,$2) ON CONFLICT DO NOTHING", params![node,received]).await.map_err(internal)?;
    let allowed: Option<f64> = sql::optional_scalar(&mut tx,
        "UPDATE inventory_ingress SET tokens=LEAST(4.0,tokens+GREATEST(0,EXTRACT(EPOCH FROM ($2-updated_at))) / 30.0)-1,updated_at=$2 WHERE node_id=$1 AND LEAST(4.0,tokens+GREATEST(0,EXTRACT(EPOCH FROM ($2-updated_at))) / 30.0)>=1 RETURNING tokens",
        params![node,received]).await.map_err(internal)?;
    if allowed.is_none() {
        return Err(RejectionReason::Overloaded.status());
    }
    super::models::InventoryVersion::upsert_by_node_id_and_digest(node, &digest)
        .first_seen(sql::timestamp(observed).map_err(internal)?)
        .inventory(toasty::Json(value))
        .or_ignore()
        .exec(&mut tx)
        .await
        .map_err(internal)?;
    sql::execute(&mut tx,
        "INSERT INTO inventory_deliveries(node_id,observation_id,startup_id,revision,source_batch_id,observed_at,received_at,delivery_digest,inventory_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        params![node,id,startup,observation.revision as i64,source,observed,received,&observation.delivery_digest,&digest]).await.map_err(internal)?;
    // 原生revision优先；legacy回填在自己的时间序中前进，不能覆盖原生观察。
    sql::execute(&mut tx,
        "INSERT INTO startup_inventories(node_id,startup_id,observation_id,revision,source_batch_id,observed_at,received_at,inventory_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(node_id,startup_id) DO UPDATE SET observation_id=excluded.observation_id,revision=excluded.revision,source_batch_id=excluded.source_batch_id,observed_at=excluded.observed_at,received_at=excluded.received_at,inventory_digest=excluded.inventory_digest WHERE (excluded.source_batch_id IS NULL AND (startup_inventories.source_batch_id IS NOT NULL OR excluded.revision>startup_inventories.revision)) OR (excluded.source_batch_id IS NOT NULL AND startup_inventories.source_batch_id IS NOT NULL AND excluded.observed_at>startup_inventories.observed_at)",
        params![node,startup,id,observation.revision as i64,source,observed,received,digest]).await.map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(false)
}
