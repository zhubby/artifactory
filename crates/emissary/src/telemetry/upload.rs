//! [INPUT]: 单写者spool、独立Telemetry协商与mTLS连接。
//! [OUTPUT]: 原字节重放、幂等拒收删除与分队列进展。
//! [POS]: 遥测后台发送器；失败不进入构建租约路径。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Snapshot, spool::Spool};
use crate::{client, config::Config};
use artifactory_emissary_protocol::{
    TelemetryBatch,
    telemetry::Batch,
    telemetry_client::TelemetryClient,
    telemetry_negotiation::{self, Negotiated, RejectionReason},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tonic::{Status, transport::Channel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposition {
    Retry,
    Drop(&'static str),
}

fn disposition(status: &Status, bytes: &[u8], negotiated: Negotiated) -> Disposition {
    match RejectionReason::from_status(status) {
        Some(RejectionReason::ExpiredSample) => Disposition::Drop("expired"),
        Some(RejectionReason::Malformed) => Disposition::Drop("malformed"),
        Some(RejectionReason::BoundsExceeded) => Disposition::Drop("bounds_exceeded"),
        Some(RejectionReason::IdContentConflict) => Disposition::Drop("id_content_conflict"),
        Some(RejectionReason::IdentityRetired) => Disposition::Drop("identity_retired"),
        None if !negotiated.digest_ack
            && status.code() == tonic::Code::OutOfRange
            && serde_json::from_slice::<Batch>(bytes)
                .is_ok_and(|b| b.sampled_at < chrono::Utc::now() - chrono::Duration::hours(48)) =>
        {
            Disposition::Drop("expired")
        }
        _ => Disposition::Retry,
    }
}

pub(super) fn update(snapshot: &RwLock<Snapshot>, fields: Value) {
    if let Ok(mut snapshot) = snapshot.write() {
        if !snapshot.metrics["telemetry"].is_object() {
            snapshot.metrics["telemetry"] = json!({});
        }
        if let Some(values) = fields.as_object() {
            snapshot.metrics["telemetry"]
                .as_object_mut()
                .unwrap()
                .extend(values.clone());
        }
    }
}

pub(super) fn lane_update(snapshot: &RwLock<Snapshot>, lane: &str, fields: Value) {
    let fields = fields
        .as_object()
        .map(|fields| {
            fields
                .iter()
                .map(|(key, value)| (format!("{lane}_{key}"), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    update(snapshot, Value::Object(fields));
}

fn temporary_negotiation(status: &Status) -> bool {
    matches!(
        status.code(),
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::ResourceExhausted
    ) && RejectionReason::from_status(status).is_none_or(|reason| {
        matches!(
            reason,
            RejectionReason::Overloaded | RejectionReason::DatabaseUnavailable
        )
    })
}

pub(super) async fn run(
    mut cfg: Config,
    spool: Arc<Mutex<Option<Spool>>>,
    snapshot: Arc<RwLock<Snapshot>>,
    notify: Arc<Notify>,
    cancel: CancellationToken,
) {
    cfg.endpoint = cfg.telemetry_endpoint.clone();
    let mut connection: Option<(TelemetryClient<Channel>, Negotiated, bool)> = None;
    let mut last_safe = None::<Negotiated>;
    let mut negotiation_backoff = 1;
    let mut negotiation_retry_at = std::time::Instant::now();
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
                        .map_err(|_| Status::unavailable("connection_unavailable"))?,
                );
                let attempt = tokio::time::timeout(
                    Duration::from_secs(20),
                    rpc.negotiate(telemetry_negotiation::offer()),
                )
                .await
                .unwrap_or_else(|_| Err(Status::deadline_exceeded("negotiation_timeout")));
                let (mut negotiated, retry) = match attempt {
                    Ok(reply) => (Negotiated::from_reply(&reply.into_inner())?, false),
                    Err(status) if status.code() == tonic::Code::Unimplemented => {
                        (Negotiated::legacy(), false)
                    }
                    Err(status) if temporary_negotiation(&status) => {
                        (last_safe.unwrap_or_else(Negotiated::legacy), true)
                    }
                    Err(status) => return Err(status),
                };
                // 已采用摘要确认后，不因后端切换降低确认强度。
                if last_safe.is_some_and(|safe| safe.digest_ack) {
                    negotiated.digest_ack = true;
                }
                Ok::<_, Status>((rpc, negotiated, retry))
            };
            match tokio::select! { _=cancel.cancelled()=>break, r=tokio::time::timeout(Duration::from_secs(25), connect)=>r }
            {
                Ok(Ok(ready)) => {
                    if !ready.2 {
                        last_safe = Some(ready.1);
                    }
                    if ready.2 {
                        negotiation_retry_at =
                            std::time::Instant::now() + client::retry_delay(negotiation_backoff);
                        negotiation_backoff = (negotiation_backoff * 2).min(30);
                    } else {
                        negotiation_backoff = 1;
                    }
                    update(
                        &snapshot,
                        json!({"negotiation_state":if ready.2 {"retrying_safe_format"} else {"ready"}}),
                    );
                    connection = Some(ready);
                }
                failed => {
                    let reason = match &failed {
                        Ok(Err(status)) => RejectionReason::from_status(status)
                            .map(|reason| reason.as_str())
                            .unwrap_or("connection_unavailable"),
                        _ => "connection_unavailable",
                    };
                    update(&snapshot, json!({"state":reason,"backoff_seconds":backoff}));
                    tokio::select! { _=cancel.cancelled()=>break, _=tokio::time::sleep(client::retry_delay(backoff))=>{} }
                    backoff = (backoff * 2).min(30);
                    continue;
                }
            }
        }
        let (rpc, negotiated, retry_negotiation) = connection.as_mut().unwrap();
        let retry_negotiation = *retry_negotiation;
        let local = spool.clone();
        let max = if negotiated.batch_schema.is_some() {
            u64::from(negotiated.max_batch_bytes)
        } else {
            0
        };
        let pending = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let mut guard = local
                .lock()
                .map_err(|_| anyhow::anyhow!("telemetry spool poisoned"))?;
            let Some(spool) = guard.as_mut() else {
                return Ok(None);
            };
            let record = spool.next_compatible(max)?;
            let waiting = spool.schema_waiting(max);
            if let Some((path, _)) = &record {
                spool.pin(path);
            }
            Ok(Some((record, waiting)))
        })
        .await;
        let pending = match pending {
            Ok(Ok(Some(value))) => Some(value),
            Ok(Ok(None)) => None,
            _ => {
                update(&snapshot, json!({"state":"storage_unavailable"}));
                None
            }
        };
        let Some((record, waiting)) = pending else {
            tokio::select! { _=cancel.cancelled()=>break, _=notify.notified()=>{}, _=tokio::time::sleep(Duration::from_secs(10))=>{} }
            continue;
        };
        update(&snapshot, json!({"schema_wait_records":waiting}));
        let Some((path, bytes)) = record else {
            if waiting.is_some_and(|waiting| waiting > 0) {
                update(&snapshot, json!({"state":"schema_wait"}));
            }
            connection = None;
            tokio::select! { _=cancel.cancelled()=>break, _=notify.notified()=>{}, _=tokio::time::sleep(Duration::from_secs(10))=>{} }
            continue;
        };
        let lane = if path
            .parent()
            .is_some_and(|parent| parent.ends_with("fresh"))
        {
            "fresh"
        } else {
            "history"
        };
        let expected = serde_json::from_slice::<Batch>(&bytes).map(|b| b.id.to_string());
        let started = std::time::Instant::now();
        let result = if negotiated.batch_schema.is_some()
            && bytes.len() <= negotiated.max_batch_bytes as usize
        {
            tokio::select! { _=cancel.cancelled()=>break, r=tokio::time::timeout(Duration::from_secs(20),rpc.upload(TelemetryBatch { json:bytes.clone(),schema_version:if negotiated.digest_ack { 1 } else { 0 } }))=>r }
        } else {
            Ok(Err(RejectionReason::SchemaUnsupported.status()))
        };
        let (remove, dropped, reason) = match result {
            Ok(Ok(ack))
                if expected.as_ref().is_ok_and(|id| {
                    negotiated
                        .ack_requirement()
                        .matches_ack(ack.get_ref(), id, &bytes)
                }) =>
            {
                (true, false, "ready")
            }
            Ok(Err(status)) => {
                let disposition = disposition(&status, &bytes, *negotiated);
                let reason = RejectionReason::from_status(&status)
                    .map(|r| r.as_str())
                    .unwrap_or(match disposition {
                        Disposition::Drop(reason) => reason,
                        Disposition::Retry => "unclassified_rejection",
                    });
                (matches!(disposition, Disposition::Drop(_)), true, reason)
            }
            _ => (false, false, "connection_unavailable"),
        };
        let local = spool.clone();
        let stored = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
            let mut guard = local
                .lock()
                .map_err(|_| anyhow::anyhow!("telemetry spool poisoned"))?;
            if let Some(spool) = guard.as_mut() {
                let result = (|| {
                    if remove {
                        if dropped {
                            spool.reject(&path, &bytes, reason)?;
                        } else {
                            spool.ack(&path)?;
                        }
                    }
                    spool.refresh_storage();
                    Ok(spool.diagnostics())
                })();
                spool.unpin();
                result
            } else {
                anyhow::bail!("telemetry spool unavailable")
            }
        })
        .await;
        let saved = matches!(stored, Ok(Ok(_)));
        if let Ok(Ok(diagnostics)) = stored {
            update(&snapshot, diagnostics);
        }
        if !saved {
            update(&snapshot, json!({"state":"storage_unavailable"}));
        }
        if remove && saved {
            lane_update(
                &snapshot,
                lane,
                json!({"state":if dropped {reason} else {"ready"},"last_progress_at":chrono::Utc::now(),"backoff_seconds":0}),
            );
            if !dropped {
                update(
                    &snapshot,
                    json!({"last_successful_ack_at":chrono::Utc::now(),"upload_latency_ms":started.elapsed().as_millis() as u64,"state":"ready","backoff_seconds":0}),
                );
                lane_update(
                    &snapshot,
                    lane,
                    json!({"last_ack_at":chrono::Utc::now(),"upload_latency_ms":started.elapsed().as_millis() as u64}),
                );
            } else {
                update(
                    &snapshot,
                    json!({"state":reason,"backoff_seconds":0,"last_rejection_reason":reason,"last_rejection_at":chrono::Utc::now()}),
                );
            }
            backoff = 1;
            if retry_negotiation && std::time::Instant::now() >= negotiation_retry_at {
                connection = None;
            }
        } else {
            update(
                &snapshot,
                json!({"state":if saved {reason} else {"storage_unavailable"},"backoff_seconds":backoff}),
            );
            lane_update(
                &snapshot,
                lane,
                json!({"state":if saved {reason} else {"storage_unavailable"},"backoff_seconds":backoff}),
            );
            connection = None;
            tokio::select! { _=cancel.cancelled()=>break, _=tokio::time::sleep(client::retry_delay(backoff))=>{} }
            backoff = (backoff * 2).min(30);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_or_authorization_rejection_never_deletes_offline_bytes() {
        let raw = include_bytes!("../../../emissary-protocol/tests/fixtures/telemetry-batch.json");
        for status in [
            Status::invalid_argument("old strict decoder"),
            Status::permission_denied("no permission"),
            RejectionReason::SchemaUnsupported.status(),
            RejectionReason::ClockFuture.status(),
        ] {
            assert_eq!(
                disposition(&status, raw, Negotiated::legacy()),
                Disposition::Retry
            );
        }
        assert_eq!(
            disposition(
                &RejectionReason::ExpiredSample.status(),
                raw,
                Negotiated::legacy()
            ),
            Disposition::Drop("expired")
        );
    }
}
