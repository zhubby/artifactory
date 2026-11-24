//! [INPUT]: 依赖源码任务锁、当前执行围栏和枚举进度合同。
//! [OUTPUT]: 幂等追加带服务端时间的源码动作。
//! [POS]: 内部 Pod 进度持久边界；拒绝取消、终态和旧执行。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{
    ControlStore,
    sources::{
        SourceTaskExecution, current_source_execution, ensure_live_source_task, lock_source_task,
    },
};
use anyhow::{Context, Result, ensure};
use artifactory_database::{params, sql};
use artifactory_source_checkout::SourceProgress;
use serde_json::{Value, json};
use uuid::Uuid;

impl ControlStore {
    pub(crate) async fn record_source_progress(
        &self,
        id: Uuid,
        execution: &SourceTaskExecution,
        progress: &SourceProgress,
    ) -> Result<()> {
        ensure!(
            (1..=256).contains(&progress.sequence),
            "source progress sequence exceeds limit"
        );
        let mut db = self.pool.clone();
        let mut tx = db.transaction().await?;
        let task = lock_source_task(&mut tx, id).await?;
        current_source_execution(&task, execution)?;
        ensure_live_source_task(&mut tx, id, &task).await?;
        let events = task["progress"]
            .as_array()
            .context("source progress must be an array")?;
        if let Some(prior) = events.iter().find(|event| {
            event["attempt"] == execution.attempt && event["sequence"] == progress.sequence
        }) {
            let value = serde_json::to_value(progress)?;
            ensure!(
                prior["stage"] == value["stage"] && prior["state"] == value["state"],
                "source progress replay changed"
            );
            tx.commit().await?;
            return Ok(());
        }
        ensure!(events.len() < 256, "source progress event limit reached");
        ensure!(
            events
                .iter()
                .filter(|event| event["attempt"] == execution.attempt)
                .all(|event| event["sequence"]
                    .as_u64()
                    .is_some_and(|sequence| sequence < u64::from(progress.sequence))),
            "source progress sequence regressed"
        );
        let event: Value = json!({"sequence":progress.sequence,"stage":progress.stage,"state":progress.state,"attempt":execution.attempt,"pod":execution.name});
        sql::execute(&mut tx,
            "UPDATE source_tasks SET progress=progress||jsonb_build_array($2::jsonb||jsonb_build_object('created_at',clock_timestamp())) WHERE id=$1",
            params![id,event]).await?;
        tx.commit().await?;
        Ok(())
    }
}
