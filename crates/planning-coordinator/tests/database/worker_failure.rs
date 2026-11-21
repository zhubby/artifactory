//! [INPUT]: 依赖隔离 PostgreSQL、有效 Worker 租约和终态 outbox。
//! [OUTPUT]: 验证失败原因持久化、秘密脱敏和提案终态围栏。
//! [POS]: Worker 错误报告到控制平面事件的持久回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

fn failure(lease: &Lease, error: Value, sequence: i64) -> Value {
    json!({
        "lease":lease,"event_id":Uuid::new_v4(),"sequence":sequence,
        "kind":"failed","payload":{"error":error}
    })
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn worker_failure_preserves_the_actionable_reason_in_the_durable_event() {
    let s = state().await;
    for error in [
        "model authentication rejected; check the platform model profile",
        "model call limit reached",
        "planning deadline reached",
        "400 unsupported system role: developer",
    ] {
        let run = Uuid::new_v4();
        server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
            .await
            .unwrap();
        let lease = owner(&s, run).await;
        server::report_event(&s, failure(&lease, json!(error), 1))
            .await
            .unwrap();
        assert_eq!(s.store.read_run(run).await.unwrap()["status"], "failed");
        let events = s.store.deliveries().await.unwrap();
        let event = events.iter().find(|event| event.run_id == run).unwrap();
        assert_eq!(event.kind, "failed");
        assert_eq!(event.payload["error"], error);
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn worker_failure_redacts_credentials_before_utf8_truncation() {
    let s = state().await;
    let run = Uuid::new_v4();
    let mut input = intent(run);
    input.model.api_key = "private\n\"provider-key".into();
    server::put_intent(&s, serde_json::to_value(&input).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    let error = format!(
        "400 参数不支持 {} private\\n\\\"provider-key {}\0 {}",
        input.model.api_key,
        lease.token,
        "构建失败".repeat(1000)
    );
    server::report_event(&s, failure(&lease, json!(error), 1))
        .await
        .unwrap();
    let events = s.store.deliveries().await.unwrap();
    let error = events[0].payload["error"].as_str().unwrap();
    assert!(error.starts_with("400 参数不支持"));
    assert!(!error.contains("private"));
    assert!(!error.contains(&lease.token));
    assert!(!error.contains('\0'));
    assert!(error.len() <= 2000);
    assert!(error.len() > 1900, "useful details must survive truncation");
    assert!(std::str::from_utf8(error.as_bytes()).is_ok());
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn malformed_worker_errors_use_the_safe_fallback() {
    let s = state().await;
    for error in [
        Value::Null,
        json!(17),
        json!({"error":"nested"}),
        json!(" \0\n "),
    ] {
        let run = Uuid::new_v4();
        server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
            .await
            .unwrap();
        let lease = owner(&s, run).await;
        server::report_event(&s, failure(&lease, error, 1))
            .await
            .unwrap();
        let events = s.store.deliveries().await.unwrap();
        let event = events.iter().find(|event| event.run_id == run).unwrap();
        assert_eq!(event.payload["error"], "Pi planning failed");
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn worker_failure_cannot_replace_a_pending_or_accepted_proposal() {
    let s = state().await;
    for name in ["submit_plan", "request_input"] {
        let run = Uuid::new_v4();
        server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
            .await
            .unwrap();
        let lease = owner(&s, run).await;
        s.store
            .tool_call(
                lease.clone(),
                name,
                Uuid::new_v4(),
                &json!({"arguments":{}}),
            )
            .await
            .unwrap();
        assert!(
            server::report_event(&s, failure(&lease, json!("late failure"), 1))
                .await
                .is_err()
        );
        let current = s.store.read_run(run).await.unwrap();
        assert_eq!(current["status"], "running");
        assert_eq!(current["lifecycle"]["pending"], true);
        let events = s.store.deliveries().await.unwrap();
        let proposal = events.iter().find(|event| event.run_id == run).unwrap();
        assert_eq!(proposal.kind, "proposal");
        assert_eq!(events.iter().filter(|event| event.run_id == run).count(), 1);
        s.store
            .acknowledge_delivery(
                proposal.id,
                run,
                "proposal",
                &proposal.payload,
                &json!({"accepted":true}),
            )
            .await
            .unwrap();
        assert!(
            server::report_event(&s, failure(&lease, json!("late failure"), 2))
                .await
                .is_err()
        );
        assert_eq!(
            s.store.read_run(run).await.unwrap()["status"],
            if name == "request_input" {
                "needs_input"
            } else {
                "succeeded"
            }
        );
        assert!(
            s.store
                .deliveries()
                .await
                .unwrap()
                .iter()
                .all(|event| event.run_id != run)
        );
    }
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn worker_failure_cannot_replace_cancellation_or_an_earlier_failure() {
    let s = state().await;
    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
        .await
        .unwrap();
    assert!(
        server::report_event(&s, failure(&lease, json!("late failure"), 1))
            .await
            .is_err()
    );
    assert_eq!(s.store.read_run(run).await.unwrap()["status"], "canceled");
    assert!(s.store.deliveries().await.unwrap().is_empty());

    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    server::report_event(&s, failure(&lease, json!("first failure"), 1))
        .await
        .unwrap();
    assert!(
        server::report_event(&s, failure(&lease, json!("later failure"), 2))
            .await
            .is_err()
    );
    let events = s.store.deliveries().await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["error"], "first failure");
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn stale_worker_failure_does_not_replace_newer_usage_or_generation() {
    let s = state().await;
    let run = Uuid::new_v4();
    server::put_intent(&s, serde_json::to_value(intent(run)).unwrap())
        .await
        .unwrap();
    let lease = owner(&s, run).await;
    server::report_event(&s, json!({"lease":lease,"event_id":Uuid::new_v4(),"sequence":2,"kind":"usage","payload":{"calls":1}})).await.unwrap();
    server::report_event(&s, failure(&lease, json!("stale failure"), 1))
        .await
        .unwrap();
    assert_eq!(s.store.read_run(run).await.unwrap()["status"], "running");
    assert!(s.store.deliveries().await.unwrap().is_empty());
    let mut stale = lease.clone();
    stale.generation = 0;
    assert!(
        server::report_event(&s, failure(&stale, json!("wrong generation"), 3))
            .await
            .is_err()
    );
    assert_eq!(s.store.read_run(run).await.unwrap()["status"], "running");
}
