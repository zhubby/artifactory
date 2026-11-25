//! [INPUT]: 依赖真实隔离数据库和完整执行身份。
//! [OUTPUT]: 验证未绑定清单与旧代本地就绪不能领取模型租约。
//! [POS]: 源码准备授权的回归入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn pending_source_requires_binding_and_local_readiness_before_model_acquire() {
    let s = state().await;
    let mut value = serde_json::to_value(intent(Uuid::new_v4())).unwrap();
    value["context"] = json!({"purpose":"initial"});
    value["source_manifest_id"] = Value::Null;
    value["source_digest"] = Value::Null;
    value["source_resolution"] = json!({"source_id":value["build_id"],"repository_url":"https://git.example/repo","commit_sha":"b".repeat(40)});
    let run = Uuid::parse_str(value["run_id"].as_str().unwrap()).unwrap();
    server::put_intent(&s, value).await.unwrap();
    s.store.queue("default", 1, 1).await.unwrap();
    let ex = Execution {
        run_id: run,
        generation: 1,
        uid: "pod-1".into(),
        name: "pod-1".into(),
        namespace: "test".into(),
    };
    s.store.register_execution(ex.clone()).await.unwrap();
    assert!(
        s.store
            .acquire_execution(ex.clone(), "https://planning.invalid")
            .await
            .is_err()
    );
    let read = s.store.read_run(run).await.unwrap();
    assert!(read["lifecycle"]["deadline_ms"].is_null());
    assert_eq!(read["lifecycle"]["calls"], 0);
}

fn pending(run: Uuid) -> RunIntent {
    let mut input = intent(run);
    input.context = json!({"purpose":"initial"});
    input.source_manifest_id = None;
    input.source_digest = None;
    input.source_resolution = Some(artifactory_planning_protocol::SourceResolution {
        source_id: input.build_id,
        repository_url: "https://git.example/repo".into(),
        commit_sha: "b".repeat(40),
    });
    input
}
fn manifest(input: &RunIntent) -> artifactory_source_checkout::SourceManifest {
    artifactory_source_checkout::SourceManifest::new(
        input.build_id,
        vec![artifactory_source_checkout::SourceRepository {
            path: String::new(),
            upstream_url: "https://git.example/repo".into(),
            commit_sha: "b".repeat(40),
            tags: Default::default(),
            lfs_objects: vec![],
        }],
    )
    .unwrap()
}
fn execution(run: Uuid, generation: i64) -> Execution {
    Execution {
        run_id: run,
        generation,
        uid: format!("pod-{generation}"),
        name: format!("pod-{generation}"),
        namespace: "test".into(),
    }
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn source_candidate_receipt_replay_and_current_pod_readiness_fence_model_acquire() {
    use artifactory_source_checkout::{SourceProgress, SourceProgressState, SourceStage};
    let s = state().await;
    let input = pending(Uuid::new_v4());
    let run = input.run_id;
    let source = manifest(&input);
    let value = serde_json::to_value(&source).unwrap();
    server::put_intent(&s, serde_json::to_value(input).unwrap())
        .await
        .unwrap();
    s.store.queue("default", 1, 1).await.unwrap();
    let first = execution(run, 1);
    s.store.register_execution(first.clone()).await.unwrap();
    s.store
        .preparation_pin(&first, &json!({"kind":"tags","path":"","tags":{}}))
        .await
        .unwrap();
    assert!(
        s.store
            .preparation_ready(&first, source.id, &source.digest)
            .await
            .is_err()
    );
    assert_eq!(
        s.store.preparation_commit(&first, &value).await.unwrap()["pending"],
        true
    );
    assert_eq!(
        s.store.preparation_commit(&first, &value).await.unwrap()["pending"],
        true
    );
    let events = s.store.deliveries().await.unwrap();
    let ready = events.iter().find(|v| v.kind == "source_ready").unwrap();
    assert_eq!(
        events.iter().filter(|v| v.kind == "source_ready").count(),
        1
    );
    let mut wrong = value.clone();
    wrong["digest"] = json!("c".repeat(64));
    assert!(s.store.preparation_commit(&first, &wrong).await.is_err());
    // The durable candidate survives a lost Pod before the cross-database receipt arrives.
    s.store
        .execution_lost(artifactory_planning_protocol::ExecutionLost {
            run_id: run,
            generation: 1,
            execution: Some(first.clone()),
            reason: artifactory_planning_protocol::ExecutionLostReason::PodMissing,
        })
        .await
        .unwrap();
    s.store
        .acknowledge_delivery(
            ready.id,
            run,
            &ready.kind,
            &ready.payload,
            &json!({"accepted":true,"source_manifest_id":source.id,"source_digest":source.digest}),
        )
        .await
        .unwrap();
    s.store
        .acknowledge_delivery(
            ready.id,
            run,
            &ready.kind,
            &ready.payload,
            &json!({"accepted":true,"source_manifest_id":source.id,"source_digest":source.digest}),
        )
        .await
        .unwrap();
    s.store.queue("default", 1, 1).await.unwrap();
    let second = execution(run, 2);
    s.store.register_execution(second.clone()).await.unwrap();
    assert!(
        s.store
            .preparation_ready(&first, source.id, &source.digest)
            .await
            .is_err()
    );
    assert!(
        s.store
            .acquire_execution(second.clone(), "https://planning.invalid")
            .await
            .is_err()
    );
    assert_eq!(
        s.store.preparation_commit(&second, &value).await.unwrap()["accepted"],
        true
    );
    let progress = SourceProgress {
        sequence: 1,
        stage: SourceStage::Verify,
        state: SourceProgressState::Started,
    };
    s.store
        .preparation_progress(&second, &progress)
        .await
        .unwrap();
    s.store
        .preparation_ready(&second, source.id, &source.digest)
        .await
        .unwrap();
    let completed = SourceProgress {
        sequence: 2,
        stage: SourceStage::Ready,
        state: SourceProgressState::Completed,
    };
    s.store
        .preparation_progress(&second, &completed)
        .await
        .unwrap();
    s.store
        .preparation_progress(&second, &progress)
        .await
        .unwrap();
    let conflict = SourceProgress {
        stage: SourceStage::Fetch,
        ..progress
    };
    assert!(
        s.store
            .preparation_progress(&second, &conflict)
            .await
            .is_err()
    );
    assert!(s.store.read_run(run).await.unwrap()["preparation"]["ready_at"].is_string());
    let acquired = s
        .store
        .acquire_execution(second.clone(), "https://planning.invalid")
        .await
        .unwrap();
    assert_eq!(acquired["task"]["context"]["source_manifest"], value);
    assert!(s.store.read_run(run).await.unwrap()["lifecycle"]["deadline_ms"].is_number());
    server::put_intent(&s, json!({"run_id":run,"version":2,"canceled":true}))
        .await
        .unwrap();
    assert!(
        s.store
            .acquire_execution(second, "https://planning.invalid")
            .await
            .is_err()
    );
}
