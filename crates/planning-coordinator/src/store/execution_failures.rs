//! [INPUT]: 依赖 Operator 执行身份、运行行锁与终态 outbox。
//! [OUTPUT]: 原子记录故障和 Pod 丢失，释放槽位并保留提案终态。
//! [POS]: 源码 init 和 Worker 退出的可信服务状态边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{PlanningStore, conversations, enqueue, locked};
use crate::lifecycle::Lifecycle;
use anyhow::{Result, ensure};
use artifactory_database::{params, sql};
use artifactory_planning_protocol::{
    ExecutionFailure, ExecutionFailureReason, ExecutionLost, ExecutionLostReason,
};
use serde_json::{Value, json};

impl PlanningStore {
    /// 以原执行身份撤销授权；旧 generation 重放被拒绝。
    ///
    /// # Errors
    /// 运行、generation 或完整执行身份不匹配时拒绝写入。
    pub async fn execution_lost(&self, lost: ExecutionLost) -> Result<Value> {
        if let Some(execution) = &lost.execution {
            ensure!(
                execution.run_id == lost.run_id && execution.generation == lost.generation,
                "execution scope mismatch"
            );
        } else {
            ensure!(
                lost.reason == ExecutionLostReason::ResourceDeleted,
                "missing execution identity"
            );
        }
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, lost.run_id).await?;
        let mut life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
        ensure!(
            life.generation == lost.generation,
            "execution generation fenced"
        );
        let expected = lost
            .execution
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        ensure!(
            row.decode::<Option<Value>>("execution")? == expected,
            "unregistered execution identity"
        );
        ensure!(
            life.owner.as_deref().is_none_or(|owner| lost
                .execution
                .as_ref()
                .is_some_and(|execution| execution.uid == owner)),
            "execution owner fenced"
        );
        let status: String = row.decode("status")?;
        if matches!(
            status.as_str(),
            "failed" | "canceled" | "succeeded" | "needs_input"
        ) || life.pending
        {
            return Ok(
                json!({"status":status,"pending":life.pending,"generation":life.generation}),
            );
        }
        ensure!(
            matches!(status.as_str(), "provisioning" | "running"),
            "run is not active"
        );
        ensure!(!life.canceled, "run canceled");
        let now = chrono::Utc::now().timestamp_millis();
        life.lease_until_ms = None;
        let fail = lost.reason == ExecutionLostReason::ResourceDeleted
            || life.generation >= 3
            || life.calls >= life.max_calls
            || life.deadline_ms.is_some_and(|deadline| deadline <= now);
        let next_status = if fail { "failed" } else { "queued" };
        conversations::finish(
            &mut tx,
            lost.run_id,
            lost.generation,
            if fail { "failed" } else { "interrupted" },
        )
        .await?;
        if fail {
            let message = if lost.reason == ExecutionLostReason::ResourceDeleted {
                "Planning resource deleted before Coordinator completion"
            } else {
                "Planning execution was interrupted and retry budget or deadline was exhausted"
            };
            enqueue(
                &mut tx,
                lost.run_id,
                "failed",
                json!({"error":message,"calls":life.calls}),
            )
            .await?;
        } else {
            life.retry(now)?;
        }
        sql::execute(&mut tx,
            "UPDATE planning_runs SET status=$2,lifecycle=$3,execution=NULL,source_ready_execution=NULL,source_preparation='{}'::jsonb,grant_hash=NULL,grant_ciphertext=NULL,dispatched_at=NULL,updated_at=now() WHERE id=$1",
            params![lost.run_id,next_status,serde_json::to_value(&life)?]).await?;
        tx.commit().await?;
        Ok(json!({"status":next_status,"pending":false,"generation":life.generation}))
    }

    /// 仅接受当前登记 Pod 的故障；终态重放不生成第二个事件。
    ///
    /// # Errors
    /// 身份过期、退出码无效或存储事务失败时返回错误。
    pub async fn execution_failed(&self, failure: ExecutionFailure) -> Result<Value> {
        // EX_TEMPFAIL 表示已领取租约但未确认结果。沿用丢失执行的完整围栏和预算。
        if failure.reason == (ExecutionFailureReason::WorkerExit { exit_code: 75 }) {
            return self
                .execution_lost(ExecutionLost {
                    run_id: failure.execution.run_id,
                    generation: failure.execution.generation,
                    execution: Some(failure.execution),
                    reason: ExecutionLostReason::WorkerInterrupted,
                })
                .await;
        }
        let execution = &failure.execution;
        let message = match failure.reason {
            ExecutionFailureReason::SourcePreparation { exit_code } => {
                ensure!(exit_code != 0, "source initialization did not fail");
                format!("Source preparation exited with code {exit_code}")
            }
            ExecutionFailureReason::WorkerExit { exit_code } => {
                format!(
                    "Planning worker exited before Coordinator completion with code {exit_code}"
                )
            }
            ExecutionFailureReason::PodFailed => {
                "Planning Pod failed before Coordinator completion".into()
            }
        };
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let row = locked(&mut tx, execution.run_id).await?;
        let mut life: Lifecycle = serde_json::from_value(row.decode("lifecycle")?)?;
        ensure!(
            life.generation == execution.generation,
            "execution generation fenced"
        );
        ensure!(
            row.decode::<Option<Value>>("execution")? == Some(serde_json::to_value(execution)?),
            "unregistered execution identity"
        );
        ensure!(
            life.owner.as_deref().is_none_or(|uid| uid == execution.uid),
            "execution owner fenced"
        );
        let status: String = row.decode("status")?;
        if matches!(
            status.as_str(),
            "failed" | "canceled" | "succeeded" | "needs_input"
        ) || life.pending
        {
            // 已提交提案由其 outbox 决定终态，Pod 退出不能覆盖接受结果。
            return Ok(json!({"status":status,"pending":life.pending}));
        }
        ensure!(
            matches!(status.as_str(), "provisioning" | "running"),
            "run is not active"
        );
        ensure!(!life.canceled, "run canceled");
        life.lease_until_ms = None;
        conversations::finish(&mut tx, execution.run_id, execution.generation, "failed").await?;
        sql::execute(
            &mut tx,
            "UPDATE planning_runs SET status='failed',lifecycle=$2,grant_hash=NULL,grant_ciphertext=NULL,updated_at=now() WHERE id=$1",
            params![execution.run_id, serde_json::to_value(&life)?],
        ).await?;
        enqueue(
            &mut tx,
            execution.run_id,
            "failed",
            json!({
                "error":message,"calls":life.calls,"execution_failure":failure.reason
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(json!({"status":"failed","pending":false}))
    }
}
