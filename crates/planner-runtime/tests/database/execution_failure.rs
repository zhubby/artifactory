//! [INPUT]: 依赖隔离 PostgreSQL、Runtime HTTP 与规划队列。
//! [OUTPUT]: 验证 Pod 故障释放槽位、事件幂等与身份隔离。
//! [POS]: Operator 故障上报到 Runtime 终态的持久回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_runtime_protocol::{
    ExecutionFailure, ExecutionFailureReason, ExecutionLost, ExecutionLostReason,
};

async fn admitted(s: &TestRuntime, run: Uuid) -> Execution {
    let input = intent(run);
    server::put_intent(s, serde_json::to_value(&input).unwrap())
        .await
        .unwrap();
    let result = s.store.queue("default", 1, 1).await.unwrap();
    assert_eq!(result["runs"][0]["run_id"], json!(run));
    assert_eq!(result["runs"][0]["project_id"], json!(input.project_id));
    let replay = s.store.queue("default", 1, 1).await.unwrap();
    assert_eq!(replay["runs"][0]["project_id"], json!(input.project_id));
    let execution = Execution {
        run_id: run,
        generation: 1,
        uid: format!("pod-{run}"),
        name: format!("pi-{run}"),
        namespace: "test".into(),
    };
    s.store.register_execution(execution.clone()).await.unwrap();
    execution
}

fn failure(execution: Execution) -> ExecutionFailure {
    ExecutionFailure {
        execution,
        reason: ExecutionFailureReason::SourcePreparation { exit_code: 1 },
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn init_failure_reaches_outbox_once_and_releases_the_only_planning_slot() {
    let s = state().await;
    let run = Uuid::new_v4();
    let execution = admitted(&s, run).await;
    let next = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(next)).unwrap())
        .await
        .unwrap();
    assert_eq!(s.store.queue("default", 1, 1).await.unwrap()["queued"], 1);
    let report = failure(execution.clone());
    let (a, b) = tokio::join!(
        s.store.execution_failed(report.clone()),
        s.store.execution_failed(report),
    );
    assert_eq!(a.unwrap()["status"], "failed");
    assert_eq!(b.unwrap()["status"], "failed");
    let current = s.store.read_run(run).await.unwrap();
    assert_eq!(current["status"], "failed");
    assert!(current["lifecycle"]["deadline_ms"].is_null());
    assert_eq!(current["lifecycle"]["calls"], 0);
    assert_eq!(
        s.store.prepare_execution(&execution).await.unwrap(),
        artifactory_planner_runtime::store::PreparationState::Denied
    );
    let outbox = s.store.deliveries().await.unwrap();
    assert_eq!(
        outbox.len(),
        1,
        "concurrent reconciles must create one terminal event"
    );
    assert_eq!(outbox[0].kind, "failed");
    assert_eq!(
        outbox[0].payload["error"],
        "Source preparation exited with code 1"
    );
    assert_eq!(
        s.store.queue("default", 1, 1).await.unwrap()["runs"][0]["run_id"],
        json!(next)
    );
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn failure_reports_fence_every_execution_field_and_preserve_accepted_work() {
    let s = state().await;
    let execution = admitted(&s, Uuid::new_v4()).await;
    for field in ["generation", "uid", "name", "namespace"] {
        let mut changed = serde_json::to_value(&execution).unwrap();
        changed[field] = if field == "generation" {
            json!(2)
        } else {
            json!("other")
        };
        let changed = serde_json::from_value(changed).unwrap();
        assert!(
            s.store.execution_failed(failure(changed)).await.is_err(),
            "{field} must be fenced"
        );
    }
    let acquired = s
        .store
        .acquire_execution(execution.clone(), "https://runtime.invalid")
        .await
        .unwrap();
    let lease: Lease = serde_json::from_value(acquired["lease"].clone()).unwrap();
    s.store
        .tool_call(
            lease.clone(),
            "submit_plan",
            Uuid::new_v4(),
            &json!({"arguments":{"steps":[]}}),
        )
        .await
        .unwrap();
    assert_eq!(
        s.store
            .execution_failed(failure(execution.clone()))
            .await
            .unwrap()["status"],
        "running"
    );
    assert_eq!(
        s.store
            .execution_failed(ExecutionFailure {
                execution: execution.clone(),
                reason: ExecutionFailureReason::WorkerExit { exit_code: 75 }
            })
            .await
            .unwrap()["pending"],
        true
    );
    let events = s.store.deliveries().await.unwrap();
    assert_eq!(events.len(), 1);
    let proposal = &events[0];
    assert_eq!(proposal.kind, "proposal");
    s.store
        .acknowledge_delivery(
            proposal.id,
            proposal.run_id,
            &proposal.kind,
            &proposal.payload,
            &json!({"accepted":true}),
        )
        .await
        .unwrap();
    assert_eq!(
        s.store.execution_failed(failure(execution)).await.unwrap()["status"],
        "succeeded"
    );
    assert!(s.store.deliveries().await.unwrap().is_empty());
    assert!(server::heartbeat_run(&s, lease).await.is_err());
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn a_late_failure_cannot_end_a_new_generation_and_canceled_work_stays_canceled() {
    let s = state().await;
    let old = admitted(&s, Uuid::new_v4()).await;
    sql::execute(
        &mut s.pool.clone(),
        "UPDATE runtime_runs SET dispatched_at=now()-interval '901 seconds' WHERE id=$1",
        params![old.run_id],
    )
    .await
    .unwrap();
    s.store.maintain().await.unwrap();
    assert!(
        s.store
            .execution_failed(failure(old.clone()))
            .await
            .is_err()
    );
    let result = s.store.queue("default", 1, 1).await.unwrap();
    assert_eq!(result["runs"][0]["generation"], 2);
    let current = Execution {
        generation: 2,
        uid: "new-pod".into(),
        name: "new-pod".into(),
        ..old.clone()
    };
    s.store.register_execution(current.clone()).await.unwrap();
    assert!(s.store.execution_failed(failure(old)).await.is_err());
    let invalid = ExecutionFailure {
        execution: current.clone(),
        reason: ExecutionFailureReason::SourcePreparation { exit_code: 0 },
    };
    assert!(s.store.execution_failed(invalid).await.is_err());
    server::put_intent(
        &s,
        json!({"run_id":current.run_id,"version":2,"canceled":true}),
    )
    .await
    .unwrap();
    assert_eq!(
        s.store
            .execution_failed(ExecutionFailure {
                execution: current.clone(),
                reason: ExecutionFailureReason::WorkerExit { exit_code: 75 }
            })
            .await
            .unwrap()["status"],
        "canceled"
    );
    assert_eq!(
        s.store.execution_failed(failure(current)).await.unwrap()["status"],
        "canceled"
    );
    assert!(s.store.deliveries().await.unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn operator_failure_endpoint_requires_service_identity_and_invalidates_worker_grant() {
    let s = state().await;
    let execution = admitted(&s, Uuid::new_v4()).await;
    let acquired = s
        .store
        .acquire_execution(execution.clone(), "https://runtime.invalid")
        .await
        .unwrap();
    let lease: Lease = serde_json::from_value(acquired["lease"].clone()).unwrap();
    let app = server::router(s.clone(), "x".repeat(64));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/v1/execution-failed",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = reqwest::Client::new();
    let report = ExecutionFailure {
        execution: execution.clone(),
        reason: ExecutionFailureReason::WorkerExit { exit_code: 137 },
    };
    assert_eq!(
        http.post(&url).json(&report).send().await.unwrap().status(),
        401
    );
    assert_eq!(
        s.store.read_run(execution.run_id).await.unwrap()["status"],
        "running"
    );
    let mut invalid = serde_json::to_value(&report).unwrap();
    invalid["reason"]["message"] = json!("private source content");
    assert_eq!(
        http.post(&url)
            .bearer_auth("x".repeat(64))
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .status(),
        422
    );
    let response: Value = http
        .post(&url)
        .bearer_auth("x".repeat(64))
        .json(&report)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["status"], "failed");
    assert!(server::heartbeat_run(&s, lease).await.is_err());
    let status: String = sql::scalar(
        &mut s.pool.clone(),
        "SELECT status FROM runtime_conversations WHERE run_id=$1",
        params![execution.run_id],
    )
    .await
    .unwrap();
    assert_eq!(status, "failed");
    task.abort();
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn missing_pod_fences_authorization_once_and_retains_the_original_budget() {
    let s = state().await;
    let execution = admitted(&s, Uuid::new_v4()).await;
    let acquired = s
        .store
        .acquire_execution(execution.clone(), "https://runtime.invalid")
        .await
        .unwrap();
    let lease: Lease = serde_json::from_value(acquired["lease"].clone()).unwrap();
    let before = s.store.read_run(execution.run_id).await.unwrap();
    let lost = ExecutionLost {
        run_id: execution.run_id,
        generation: 1,
        execution: Some(execution.clone()),
        reason: ExecutionLostReason::PodMissing,
    };
    let (first, duplicate) = tokio::join!(
        s.store.execution_lost(lost.clone()),
        s.store.execution_lost(lost.clone())
    );
    assert_eq!(
        usize::from(first.is_ok()) + usize::from(duplicate.is_ok()),
        1,
        "duplicate observation advances exactly once"
    );
    let after = s.store.read_run(execution.run_id).await.unwrap();
    assert_eq!(after["status"], "queued");
    assert_eq!(after["lifecycle"]["generation"], 2);
    assert_eq!(
        after["lifecycle"]["deadline_ms"],
        before["lifecycle"]["deadline_ms"]
    );
    assert_eq!(after["lifecycle"]["calls"], before["lifecycle"]["calls"]);
    assert!(after["execution"].is_null());
    assert!(server::heartbeat_run(&s, lease).await.is_err());
    assert!(s.store.execution_lost(lost).await.is_err());
    assert!(
        s.store.deliveries().await.unwrap().is_empty(),
        "recoverable disappearance does not fail the business build"
    );
    let next = s.store.queue("default", 1, 1).await.unwrap();
    assert_eq!(next["runs"][0]["generation"], 2);
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn interrupted_worker_retries_with_fenced_identity_and_original_budget() {
    let s = state().await;
    let execution = admitted(&s, Uuid::new_v4()).await;
    let acquired = s
        .store
        .acquire_execution(execution.clone(), "https://runtime.invalid")
        .await
        .unwrap();
    let lease: Lease = serde_json::from_value(acquired["lease"].clone()).unwrap();
    let current = s.store.read_run(execution.run_id).await.unwrap();
    let mut life: Lifecycle = serde_json::from_value(current["lifecycle"].clone()).unwrap();
    life.calls = 1;
    sql::execute(
        &mut s.pool.clone(),
        "UPDATE runtime_runs SET lifecycle=$2 WHERE id=$1",
        params![execution.run_id, serde_json::to_value(life).unwrap()],
    )
    .await
    .unwrap();
    let before = s.store.read_run(execution.run_id).await.unwrap();
    let report = ExecutionFailure {
        execution: execution.clone(),
        reason: ExecutionFailureReason::WorkerExit { exit_code: 75 },
    };
    let mut foreign = report.clone();
    foreign.execution.uid = "foreign".into();
    assert!(s.store.execution_failed(foreign).await.is_err());
    let result = s.store.execution_failed(report.clone()).await.unwrap();
    assert_eq!(result["status"], "queued");
    assert_eq!(result["generation"], 2);
    let after = s.store.read_run(execution.run_id).await.unwrap();
    assert_eq!(after["lifecycle"]["calls"], before["lifecycle"]["calls"]);
    assert_eq!(
        after["lifecycle"]["deadline_ms"],
        before["lifecycle"]["deadline_ms"]
    );
    assert!(after["execution"].is_null());
    assert!(server::heartbeat_run(&s, lease).await.is_err());
    assert!(s.store.execution_failed(report).await.is_err());
    assert!(s.store.deliveries().await.unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn missing_pod_exhausted_generation_calls_or_deadline_finishes_without_readmission() {
    for exhausted in ["generation", "calls", "deadline"] {
        let s = state().await;
        let mut execution = admitted(&s, Uuid::new_v4()).await;
        let acquired = s
            .store
            .acquire_execution(execution.clone(), "https://runtime.invalid")
            .await
            .unwrap();
        let lease: Lease = serde_json::from_value(acquired["lease"].clone()).unwrap();
        let current = s.store.read_run(execution.run_id).await.unwrap();
        let mut life: Lifecycle = serde_json::from_value(current["lifecycle"].clone()).unwrap();
        match exhausted {
            "generation" => {
                life.generation = 3;
                execution.generation = 3;
            }
            "calls" => life.calls = life.max_calls,
            _ => life.deadline_ms = Some(chrono::Utc::now().timestamp_millis() - 1),
        }
        sql::execute(
            &mut s.pool.clone(),
            "UPDATE runtime_runs SET lifecycle=$2,execution=$3 WHERE id=$1",
            params![
                execution.run_id,
                serde_json::to_value(&life).unwrap(),
                serde_json::to_value(&execution).unwrap()
            ],
        )
        .await
        .unwrap();
        let result = s
            .store
            .execution_lost(ExecutionLost {
                run_id: execution.run_id,
                generation: execution.generation,
                execution: Some(execution.clone()),
                reason: ExecutionLostReason::PodMissing,
            })
            .await
            .unwrap();
        assert_eq!(result["status"], "failed", "{exhausted}");
        assert_eq!(result["generation"], life.generation);
        assert!(
            s.store.queue("default", 1, 1).await.unwrap()["runs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(s.store.deliveries().await.unwrap().len(), 1);
        assert!(server::heartbeat_run(&s, lease).await.is_err());
        let after = s.store.read_run(execution.run_id).await.unwrap();
        assert_eq!(after["lifecycle"]["calls"], life.calls);
        assert_eq!(after["lifecycle"]["deadline_ms"], json!(life.deadline_ms));
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn resource_loss_fences_identity_and_preserves_pending_proposals() {
    let s = state().await;
    let execution = admitted(&s, Uuid::new_v4()).await;
    let report = ExecutionLost {
        run_id: execution.run_id,
        generation: 1,
        execution: Some(execution.clone()),
        reason: ExecutionLostReason::PodMissing,
    };
    for field in ["uid", "name", "namespace", "generation", "run_id"] {
        let mut bad = serde_json::to_value(&report).unwrap();
        bad["execution"][field] = match field {
            "generation" => json!(2),
            "run_id" => json!(Uuid::new_v4()),
            _ => json!("foreign"),
        };
        assert!(
            s.store
                .execution_lost(serde_json::from_value(bad).unwrap())
                .await
                .is_err(),
            "fence {field}"
        );
    }
    let acquired = s
        .store
        .acquire_execution(execution.clone(), "https://runtime.invalid")
        .await
        .unwrap();
    let lease: Lease = serde_json::from_value(acquired["lease"].clone()).unwrap();
    s.store
        .tool_call(
            lease,
            "submit_plan",
            Uuid::new_v4(),
            &json!({"arguments":{}}),
        )
        .await
        .unwrap();
    assert_eq!(
        s.store.execution_lost(report.clone()).await.unwrap()["pending"],
        true
    );
    let events = s.store.deliveries().await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "proposal");
    let mut deleted = report;
    deleted.reason = ExecutionLostReason::ResourceDeleted;
    assert_eq!(
        s.store.execution_lost(deleted).await.unwrap()["pending"],
        true
    );
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn resource_deleted_before_pod_registration_finishes_business_through_one_outbox_event() {
    let s = state().await;
    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    s.store.queue("default", 1, 1).await.unwrap();
    let report = ExecutionLost {
        run_id: run,
        generation: 1,
        execution: None,
        reason: ExecutionLostReason::ResourceDeleted,
    };
    let app = server::router(s.clone(), "x".repeat(64));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/v1/execution-lost",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = reqwest::Client::new();
    assert_eq!(
        http.post(&url).json(&report).send().await.unwrap().status(),
        401
    );
    let result: Value = http
        .post(&url)
        .bearer_auth("x".repeat(64))
        .json(&report)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["status"], "failed");
    assert_eq!(s.store.deliveries().await.unwrap().len(), 1);
    task.abort();
}
