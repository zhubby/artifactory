//! [INPUT]: 依赖源码隔离夹具、阶段记录与当前执行围栏。
//! [OUTPUT]: 验证进度幂等、旧 Pod 拒绝及终态诊断保留。
//! [POS]: 源码事件的数据库回归；不创建真实 Pod。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_source_checkout::{SourceProgress, SourceProgressState, SourceStage};

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn build_activity_keeps_native_steps_and_artifacts_attached_to_their_attempt() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let (_,attempt,manifest) = db.native().await?;
        let project:Uuid = sql::scalar(&mut db.pool.clone(),"SELECT project_id FROM builds WHERE id=$1",params![manifest.id]).await?;
        sql::execute(&mut db.pool.clone(),"INSERT INTO step_runs(attempt_id,step_index,name,phase,status,finished_at,exit_code) VALUES($1,0,'compile','build','failed',now(),7)",params![attempt]).await?;
        let mut auth = crate::auth::Auth::scoped(Uuid::new_v4(),"viewer",vec![project],vec!["read".into()]);
        auth.tenant_id = Uuid::from_u128(1);
        let data = db.store.for_auth(&auth).build_activity(manifest.id).await?;
        let events=data["events"].as_array().context("events missing")?;
        let step=events.iter().find(|event|event["action"]=="step_finished").context("step missing")?;
        assert_eq!(step["details"]["attempt_id"],attempt.to_string());
        assert_eq!(step["details"]["exit_code"],7);
        assert_eq!(step["details"]["actor"],"source-test");
        assert!(!data.to_string().contains("lease_token"));
        Ok(())
    }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn source_progress_is_idempotent_and_fenced_by_the_current_execution() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let (id, _) = db.task().await?;
        let current = execution();
        db.store
            .source_task_material(&[7; 32], id, &current)
            .await?;
        let progress = SourceProgress {
            sequence: 1,
            stage: SourceStage::ResolveRef,
            state: SourceProgressState::Started,
        };
        db.store
            .record_source_progress(id, &current, &progress)
            .await?;
        db.store
            .record_source_progress(id, &current, &progress)
            .await?;
        let task = db.store.source_task_context(id).await?;
        assert_eq!(task["progress"].as_array().unwrap().len(), 1);
        assert_eq!(task["progress"][0]["attempt"], 1);
        assert!(task["progress"][0]["created_at"].is_string());
        let altered = SourceProgress {
            state: SourceProgressState::Completed,
            ..progress.clone()
        };
        assert!(
            db.store
                .record_source_progress(id, &current, &altered)
                .await
                .is_err()
        );
        let second = SourceTaskExecution {
            attempt: 2,
            uid: "pod-2".into(),
            name: "source-2".into(),
            ..current.clone()
        };
        db.store.source_task_material(&[7; 32], id, &second).await?;
        assert!(
            db.store
                .record_source_progress(id, &current, &progress)
                .await
                .is_err()
        );
        db.store
            .record_source_progress(id, &second, &progress)
            .await?;
        sql::execute(
            &mut db.pool.clone(),
            "UPDATE builds SET cancel_requested=true WHERE id=$1",
            params![id],
        )
        .await?;
        assert!(
            db.store
                .record_source_progress(id, &second, &altered)
                .await
                .is_err()
        );
        Ok(())
    }
    .await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL"]
async fn source_failure_persists_only_a_validated_diagnostic() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let (id, _) = db.task().await?;
        let current = execution();
        db.store.source_task_material(&[7; 32], id, &current).await?;
        let unknown = json!({"error":"safe failure","diagnostic":{"stage":"resolve_ref","code":"timeout","secret":"must-not-persist"}});
        assert!(db.store.commit_source_task(id, &current, &unknown).await.is_err());
        let valid = json!({"error":"safe failure","diagnostic":{"stage":"resolve_ref","code":"timeout"}});
        db.store.commit_source_task(id, &current, &valid).await?;
        let stored = db.store.source_task_result(id).await?;
        assert_eq!(stored["result"]["diagnostic"], valid["diagnostic"]);
        Ok(())
    }.await;
    db.close().await?;
    result
}
