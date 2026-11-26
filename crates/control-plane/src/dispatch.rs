//! [INPUT]: 依赖当前连接归属和持久命令、有限操作记录。
//! [OUTPUT]: 有界重放构建命令、升级和再授权只读操作。
//! [POS]: 多副本连接分发边界；传输成功不等于业务接受。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{protocol::Command, scheduler, state::AppState};
use anyhow::Result;
use futures::{StreamExt, stream};

/// Deliver a bounded batch; a successful socket write is never treated as execution ACK.
///
/// # Errors
/// Returns database claim errors. Individual delivery failures remain pending for replay.
pub async fn deliver(state: &AppState) -> Result<()> {
    deliver_cancellations(state).await?;
    deliver_node_operations(state).await?;
    deliver_assignments(state).await
}

pub(crate) async fn deliver_cancellations(state: &AppState) -> Result<()> {
    deliver_kind(state, "cancel").await
}

pub(crate) async fn deliver_assignments(state: &AppState) -> Result<()> {
    deliver_kind(state, "assign").await
}

async fn deliver_kind(state: &AppState, kind: &str) -> Result<()> {
    if state.coordination.is_draining() {
        return Ok(());
    }
    let rows = state
        .store
        .pending_commands(
            state.coordination.id,
            state.settings.control.scheduler_batch,
            kind,
        )
        .await?;
    // Descriptor preparation may contact Source. Bound it independently of the
    // command channel so one unavailable source cannot block unrelated devices.
    stream::iter(rows)
        .for_each_concurrent(8, |row| async move {
            let node = row.node;
            let session = row.session;
            let connection = state.connections.read().await.get(&node).cloned();
            let Some(connection) = connection.filter(|c| c.session == session) else {
                return;
            };
            let attempt = row.attempt;
            let kind = row.kind.clone();
            let assignment = if kind == "assign" {
                let result = async {
                    let job = state.store.assignment_job(attempt).await?;
                    scheduler::assignment(state, job, attempt).await
                }
                .await;
                match result {
                    Ok(assignment) => Some(assignment),
                    Err(error) => {
                        tracing::warn!(%attempt,%error,"device assignment delivery deferred");
                        return;
                    }
                }
            } else {
                None
            };
            if state.coordination.is_draining() {
                return;
            }
            // Source preparation can outlive a reconnect, cancellation or node
            // revocation. Recheck immediately before exposing the assignment.
            if !matches!(
                state
                    .store
                    .command_authorized(&row, state.coordination.id)
                    .await,
                Ok(true)
            ) {
                return;
            }
            let command = Command {
                command_id: row.id.to_string(),
                kind,
                attempt_id: attempt.to_string(),
                lease_token: row.lease_token,
                lease_seconds: 90,
                assignment,
                ..Default::default()
            };
            // Full/closed queues retain the durable command for a later replay.
            let _ = connection.sender.try_send(Ok(command));
        })
        .await;
    Ok(())
}

pub(crate) async fn deliver_ota(state: &AppState) -> Result<()> {
    let rows = state
        .store
        .ota_candidates(
            state.coordination.id,
            state.settings.control.scheduler_batch,
        )
        .await?;
    for (node, session) in rows {
        let connection = state.connections.read().await.get(&node).cloned();
        if let Some(connection) = connection.filter(|c| c.session == session)
            && let Some(command) = crate::ota::next(state, node, session).await?
        {
            let _ = connection.sender.try_send(Ok(command));
        }
    }
    Ok(())
}

pub(crate) async fn deliver_node_operations(state: &AppState) -> Result<()> {
    if state.coordination.is_draining() {
        return Ok(());
    }
    let rows = state
        .store
        .pending_node_operations(
            state.coordination.id(),
            state.settings.control.scheduler_batch.clamp(1, 100) as usize,
        )
        .await?;
    for row in rows {
        let envelope = &row.envelope;
        let connection = state
            .connections
            .read()
            .await
            .get(&envelope.node_id)
            .cloned();
        let Some(connection) =
            connection.filter(|connection| connection.session == envelope.control_session)
        else {
            continue;
        };
        let Some(cancel_requested) = state
            .store
            .node_operation_delivery_authorized(envelope, state.coordination.id())
            .await?
        else {
            continue;
        };
        // 新会话先重授权状态；startup变化后只重放interrupted结果。
        let resume = Command {
            command_id: envelope.operation_id.to_string(),
            kind: "node_operation_resume".into(),
            signing_session_id: envelope.control_session.to_string(),
            payload_json: serde_json::to_string(&row.resume)?,
            ..Default::default()
        };
        if connection.sender.try_send(Ok(resume)).is_err() {
            continue;
        }
        if row.resume.current_startup_id != envelope.startup_id || !row.execute_allowed {
            continue;
        }
        if cancel_requested {
            let cancel = artifactory_emissary_protocol::operations::OperationCancel {
                operation_id: envelope.operation_id,
                startup_id: envelope.startup_id,
                control_session: envelope.control_session,
            };
            if connection
                .sender
                .try_send(Ok(Command {
                    command_id: envelope.operation_id.to_string(),
                    kind: "node_operation_cancel".into(),
                    signing_session_id: envelope.control_session.to_string(),
                    payload_json: serde_json::to_string(&cancel)?,
                    ..Default::default()
                }))
                .is_err()
            {
                continue;
            }
        }
        // 取消先于原投递；设备持久接受后直接取消，不启动探测。
        let command = Command {
            command_id: envelope.operation_id.to_string(),
            kind: "node_operation".into(),
            signing_session_id: envelope.control_session.to_string(),
            payload_json: serde_json::to_string(envelope)?,
            ..Default::default()
        };
        let _ = connection.sender.try_send(Ok(command));
    }
    Ok(())
}
