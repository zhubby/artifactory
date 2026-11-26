//! [INPUT]: 依赖固定探测目录、取消令牌和独立清单耐久回调。
//! [OUTPUT]: 生成白名单诊断或完整刷新结果，显式保留部分失败。
//! [POS]: 有限操作执行器；不修改启动调度快照。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::InventorySink;
use crate::{config::Config, doctor};
use artifactory_emissary_protocol::operations::*;
use futures::{StreamExt, stream};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(super) async fn execute(
    cfg: &Config,
    envelope: &OperationEnvelope,
    cancel: CancellationToken,
    sink: Option<InventorySink>,
    deadline: tokio::time::Instant,
) -> (OperationState, Option<OperationReason>, OperationResult) {
    let input = match envelope.validate() {
        Ok(input) => input,
        Err(_) => {
            return (
                OperationState::Rejected,
                Some(OperationReason::InvalidInput),
                OperationResult::default(),
            );
        }
    };
    let cancel = cancel.child_token();
    let deadline_cancel = cancel.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep_until(deadline).await;
        deadline_cancel.cancel();
    });
    let mut result = OperationResult::default();
    let mut probes = stream::iter(input.probes())
        .map(|id| {
            let cancel = &cancel;
            async move {
                if cancel.is_cancelled() {
                    return (id, None);
                }
                let timeout = deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .min(Duration::from_secs(3));
                if timeout.is_zero() {
                    return (id, None);
                }
                let report = doctor::probe_report(cfg, &id, cancel, timeout).await;
                (id, Some(report))
            }
        })
        .buffered(8);
    while let Some((id, report)) = probes.next().await {
        if let Some(report) = report {
            result.probes.insert(id, report);
        } else {
            result.skipped.insert(
                id,
                if tokio::time::Instant::now() >= deadline {
                    SkippedReason::BudgetExhausted
                } else {
                    SkippedReason::Canceled
                },
            );
        }
    }
    let mut reason = None;
    if envelope.kind == OperationKind::RefreshInventory
        && result.successful()
        && !cancel.is_cancelled()
    {
        if let Some(sink) = sink {
            let inventory =
                crate::telemetry::inventory::refresh(cfg, &result.probes, &cancel, deadline).await;
            if cancel.is_cancelled() {
                reason = Some(OperationReason::InventoryUnavailable);
            } else {
                match tokio::time::timeout_at(deadline, sink(inventory)).await {
                    Ok(Ok(refresh)) => result.inventory = Some(refresh),
                    _ => reason = Some(OperationReason::InventoryUnavailable),
                }
            }
        } else {
            reason = Some(OperationReason::InventoryUnavailable)
        }
    }
    timer.abort();
    if tokio::time::Instant::now() >= deadline {
        reason = Some(OperationReason::ExecutionBudgetExhausted);
    }
    let state = if reason.is_none() && result.successful() && !cancel.is_cancelled() {
        OperationState::Succeeded
    } else {
        OperationState::Failed
    };
    (state, reason, result)
}
