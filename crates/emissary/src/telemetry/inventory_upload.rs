//! [INPUT]: 依赖独立清单队列、mTLS协商和摘要确认。
//! [OUTPUT]: 独立发送清单；永久冲突持久隔离，只有业务确认才记录ACK。
//! [POS]: 隔离清单RPC延迟；本地检查恢复不合成确认时间。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{
    Snapshot,
    spool::Spool,
    upload::{lane_update, update},
};
use crate::{client, config::Config};
use artifactory_emissary_protocol::{
    telemetry_client::TelemetryClient,
    telemetry_negotiation::{self, Negotiated, RejectionReason},
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
pub(super) async fn run(
    mut cfg: Config,
    spool: Arc<Mutex<Option<Spool>>>,
    snapshot: Arc<RwLock<Snapshot>>,
    notify: Arc<Notify>,
    cancel: CancellationToken,
) {
    cfg.endpoint = cfg.telemetry_endpoint.clone();
    let mut connection = None;
    let mut backoff = 1;
    loop {
        if cancel.is_cancelled() {
            break;
        }
        if connection.is_none() {
            let connect = async {
                let mut rpc = TelemetryClient::new(
                    client::dial(&cfg, false)
                        .await
                        .map_err(|_| tonic::Status::unavailable("connection_unavailable"))?,
                );
                let negotiated = match rpc.negotiate(telemetry_negotiation::offer()).await {
                    Ok(reply) => Negotiated::from_reply(&reply.into_inner())?,
                    Err(status) if status.code() == tonic::Code::Unimplemented => {
                        Negotiated::legacy()
                    }
                    Err(status) => return Err(status),
                };
                Ok::<_, tonic::Status>((rpc, negotiated))
            };
            let result = tokio::select! {_=cancel.cancelled()=>break,result=tokio::time::timeout(Duration::from_secs(20),connect)=>result};
            if let Ok(Ok(ready)) = result {
                connection = Some(ready);
            } else {
                let reason = match &result {
                    Ok(Err(status)) => RejectionReason::from_status(status)
                        .map(|reason| reason.as_str())
                        .unwrap_or("connection_unavailable"),
                    _ => "connection_unavailable",
                };
                update(
                    &snapshot,
                    json!({"inventory_state":reason,"inventory_backoff_seconds":backoff}),
                );
                tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(client::retry_delay(backoff))=>{}}
                backoff = (backoff * 2).min(30);
                continue;
            }
        }
        let (rpc, negotiated) = connection.as_mut().expect("connected");
        let local = spool.clone();
        let max = negotiated
            .inventory_schema
            .map_or(0, |_| u64::from(negotiated.max_inventory_bytes));
        let pending = tokio::task::spawn_blocking(move || {
            let mut guard = local
                .lock()
                .map_err(|_| anyhow::anyhow!("telemetry spool poisoned"))?;
            let spool = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("telemetry spool unavailable"))?;
            let observation = spool.inventories.first_compatible(max)?;
            if let Some(observation) = &observation {
                spool.inventories.pin(&observation.observation_id);
            }
            Ok::<_, anyhow::Error>((
                observation,
                spool.inventories.waiting(max),
                spool.inventories.conflicts(),
                spool.diagnostics(),
            ))
        })
        .await;
        let (observation, waiting, conflicts, diagnostics) = match pending {
            Ok(Ok(value)) => value,
            _ => {
                update(
                    &snapshot,
                    json!({"inventory_state":"storage_unavailable","inventory_schema_wait_records":null}),
                );
                tokio::select! {_=cancel.cancelled()=>break,_=notify.notified()=>{},_=tokio::time::sleep(Duration::from_secs(2))=>{}}
                continue;
            }
        };
        update(&snapshot, diagnostics);
        update(&snapshot, json!({"inventory_schema_wait_records":waiting}));
        if negotiated.inventory_schema.is_none() {
            update(
                &snapshot,
                json!({"inventory_state":"waiting_inventory_receiver","inventory_backoff_seconds":0}),
            );
            tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(30))=>{}}
            connection = None;
            continue;
        }
        let Some(observation) = observation else {
            let waiting = waiting.is_some_and(|count| count > 0);
            update(
                &snapshot,
                json!({"inventory_state":if waiting {"schema_wait"} else if conflicts.is_some_and(|count| count > 0) {"id_content_conflict"} else {"ready"},"inventory_backoff_seconds":0}),
            );
            if waiting {
                connection = None;
            }
            tokio::select! {_=cancel.cancelled()=>break,_=notify.notified()=>{},_=tokio::time::sleep(Duration::from_secs(2))=>{}}
            continue;
        };
        let started = std::time::Instant::now();
        let result = tokio::select! {_=cancel.cancelled()=>break,result=tokio::time::timeout(Duration::from_secs(20),rpc.upload_inventory(observation.clone()))=>result};
        let (ack, reason) = match result {
            Ok(Ok(reply)) if observation.matches_ack(reply.get_ref()) => {
                (Some(reply.into_inner()), "ready")
            }
            Ok(Err(status)) => (
                None,
                RejectionReason::from_status(&status)
                    .map(|r| r.as_str())
                    .unwrap_or("unclassified_rejection"),
            ),
            _ => (None, "connection_unavailable"),
        };
        let local = spool.clone();
        let successful = ack.is_some();
        let conflict = reason == RejectionReason::IdContentConflict.as_str();
        let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
            let mut guard = local
                .lock()
                .map_err(|_| anyhow::anyhow!("telemetry spool poisoned"))?;
            let spool = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("telemetry spool unavailable"))?;
            let result = if let Some(ack) = ack {
                spool.inventories.ack(&ack).map(|_| ())
            } else if conflict {
                spool.inventories.conflict(&observation)
            } else {
                Ok(())
            };
            spool.inventories.unpin();
            result.map(|_| {
                spool.refresh_storage();
                spool.diagnostics()
            })
        })
        .await;
        let durable = matches!(saved, Ok(Ok(_)));
        if let Ok(Ok(diagnostics)) = saved {
            update(&snapshot, diagnostics);
        }
        if successful && durable {
            backoff = 1;
            update(
                &snapshot,
                json!({"inventory_state":"ready","last_inventory_ack_at":chrono::Utc::now()}),
            );
            lane_update(
                &snapshot,
                "inventory",
                json!({"last_ack_at":chrono::Utc::now(),"last_progress_at":chrono::Utc::now(),"upload_latency_ms":started.elapsed().as_millis() as u64,"backoff_seconds":0}),
            );
            continue;
        }
        if conflict && durable {
            backoff = 1;
            update(
                &snapshot,
                json!({"inventory_state":"id_content_conflict","inventory_backoff_seconds":0}),
            );
            continue;
        }
        update(
            &snapshot,
            json!({"inventory_state":if durable {reason} else {"storage_unavailable"},"inventory_backoff_seconds":backoff}),
        );
        connection = None;
        tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(client::retry_delay(backoff))=>{}}
        backoff = (backoff * 2).min(30);
    }
}

#[cfg(test)]
#[path = "inventory_upload_tests.rs"]
mod tests;
