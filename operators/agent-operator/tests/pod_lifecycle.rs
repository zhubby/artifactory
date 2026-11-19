//! [INPUT]: 依赖隔离 Kubernetes/Runtime API 与普通 Pod 资源。
//! [OUTPUT]: 验证身份、重启、故障、finalizer、容量和三次显式重试。
//! [POS]: Agent Operator 的跨 API 生命周期回归，不连接生产集群。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod support;
use artifactory_agent_operator::{api::AgentRun, pods};
use serde_json::{Value, json};
use support::*;

async fn start_run(peer: &Peer) {
    for _ in 0..4 {
        peer.run_step().await.unwrap();
    }
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
}
async fn start_task(peer: &Peer) {
    for _ in 0..4 {
        peer.task_step().await.unwrap();
    }
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
}
fn source_result(pod: &mut Value, exit: i32) {
    pod["spec"]["nodeName"] = json!("worker");
    pod["status"] = json!({"phase":if exit==0{"Succeeded"}else{"Failed"},"containerStatuses":[{"name":"source","image":"source","imageID":"source","ready":false,"restartCount":0,"state":{"terminated":{"exitCode":exit}}}]});
}

#[tokio::test]
async fn creation_ack_loss_and_restart_reuse_the_same_direct_owned_pod() {
    let peer = Peer::new().await;
    peer.stored.lock().unwrap().lose_create_ack = true;
    start_run(&peer).await;
    let pod = peer.pod();
    assert_eq!(pod["kind"], "Pod");
    assert_eq!(pod["metadata"]["ownerReferences"][0]["kind"], "AgentRun");
    assert_eq!(pod["metadata"]["ownerReferences"][0]["uid"], "run-owner");
    assert_eq!(pod["spec"]["restartPolicy"], "Never");
    let uid = pod["metadata"]["uid"].clone();
    for _ in 0..3 {
        peer.run_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 1);
    assert_eq!(stored.runs[0]["status"]["execution"]["uid"], uid);
    assert_eq!(stored.runtime["execution"]["uid"], uid);
    assert!(
        stored
            .events
            .iter()
            .all(|(path, _)| !path.contains("/jobs"))
    );
}

#[tokio::test]
async fn failed_init_finishes_runtime_and_deletes_only_after_stop_is_saved() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.mutate_pod(|pod|{
        pod["spec"]["nodeName"]=json!("worker");
        pod["status"]=json!({"phase":"Pending","initContainerStatuses":[{"name":"source","image":"source","imageID":"source","ready":false,"restartCount":0,"state":{"terminated":{"exitCode":1,"message":"private source credential"}}}],"containerStatuses":[{"name":"pi","image":"pi","imageID":"pi","ready":false,"restartCount":0,"state":{"waiting":{"reason":"PodInitializing"}}}]});
    });
    for _ in 0..7 {
        peer.run_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.runtime["status"], "failed");
    assert_eq!(stored.runs[0]["status"]["phase"], "failed");
    assert!(stored.runs[0]["status"]["execution"].is_null());
    assert!(stored.pods.is_empty());
    let stop = stored
        .events
        .iter()
        .position(|(path, body)| {
            path.ends_with("agentruns/run/status") && body["status"]["execution"]["stopped"] == true
        })
        .unwrap();
    let delete = stored
        .events
        .iter()
        .position(|(path, _)| path.starts_with("DELETE "))
        .unwrap();
    assert!(stop < delete, "stop proof must persist before deletion");
    let deletion = &stored.events[delete].1;
    assert_eq!(deletion["preconditions"]["uid"], "pod-1");
    let report = stored
        .events
        .iter()
        .find(|(path, _)| path.ends_with("execution-failed"))
        .unwrap();
    assert_eq!(report.1["reason"]["kind"], "source_preparation");
    assert!(!report.1.to_string().contains("private source credential"));
}

#[tokio::test]
async fn same_name_uid_replacement_is_never_adopted_or_deleted() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.mutate_pod(|pod| pod["metadata"]["uid"] = json!("replacement"));
    assert!(peer.run_step().await.is_err());
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 1);
    assert!(
        stored
            .events
            .iter()
            .all(|(path, _)| !path.starts_with("DELETE "))
    );
}

#[tokio::test]
async fn missing_planning_pod_fences_generation_without_recreating_unknown_execution() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.run_step().await.unwrap();
    peer.stored.lock().unwrap().pods.clear();
    peer.run_step().await.unwrap();
    assert_eq!(
        peer.stored.lock().unwrap().runtime["lifecycle"]["generation"],
        2
    );
    for _ in 0..3 {
        peer.run_step().await.unwrap();
    }
    {
        let mut stored = peer.stored.lock().unwrap();
        assert_eq!(stored.creates, 1);
        assert_eq!(stored.runs[0]["status"]["phase"], "Superseded");
        assert_eq!(stored.runs[0]["status"]["execution"]["stopped"], false);
        let mut next = run();
        next["metadata"]["name"] = json!("run-next");
        next["metadata"]["uid"] = json!("next-owner");
        next["spec"]["generation"] = json!(2);
        stored.runs.insert(0, next);
    }
    peer.run_step().await.unwrap();
    peer.run_step().await.unwrap();
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 1);
    assert_eq!(stored.runs[0]["status"]["phase"], "Queued");
}

#[tokio::test]
async fn deleting_cr_notifies_runtime_and_keeps_finalizers_for_unknown_node_execution() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.mutate_pod(|pod| {
        pod["spec"]["nodeName"] = json!("offline-node");
        pod["status"] = json!({"phase":"Unknown"});
    });
    peer.stored.lock().unwrap().runs[0]["metadata"]["deletionTimestamp"] =
        json!(chrono::Utc::now().to_rfc3339());
    for _ in 0..5 {
        peer.run_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.runtime["status"], "failed");
    assert!(
        stored.runs[0]["metadata"]["finalizers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == pods::CR_FINALIZER)
    );
    assert!(
        stored.pods.values().next().unwrap()["metadata"]["finalizers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == pods::POD_FINALIZER)
    );
    assert_eq!(stored.creates, 1);
}

#[tokio::test]
async fn deleting_pod_requeues_generation_but_waits_for_confirmed_old_stop() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.run_step().await.unwrap();
    peer.mutate_pod(|pod| {
        pod["metadata"]["deletionTimestamp"] = json!(chrono::Utc::now().to_rfc3339());
        pod["spec"]["nodeName"] = json!("worker");
        pod["status"] = json!({"phase":"Unknown"});
    });
    for _ in 0..4 {
        peer.run_step().await.unwrap();
    }
    {
        let stored = peer.stored.lock().unwrap();
        assert_eq!(stored.runtime["lifecycle"]["generation"], 2);
        assert_eq!(stored.runtime["status"], "queued");
        assert_eq!(stored.runs[0]["status"]["phase"], "Superseded");
        assert!(!stored.runs[0]["status"]["execution"].is_null());
        assert_eq!(stored.pods.len(), 1);
        assert_eq!(stored.creates, 1);
        let report = stored
            .events
            .iter()
            .find(|(path, _)| path.ends_with("execution-lost"))
            .unwrap();
        assert_eq!(report.1["reason"], "pod_missing");
    }
    peer.mutate_pod(|pod| {
        pod["status"] = json!({"phase":"Failed","initContainerStatuses":[{"name":"source","image":"source","imageID":"source","ready":false,"restartCount":0,"state":{"terminated":{"exitCode":0}}}],"containerStatuses":[{"name":"pi","image":"pi","imageID":"pi","ready":false,"restartCount":0,"state":{"terminated":{"exitCode":143}}}]});
    });
    for _ in 0..4 {
        peer.run_step().await.unwrap();
    }
    {
        let mut stored = peer.stored.lock().unwrap();
        assert!(stored.pods.is_empty());
        assert!(stored.runs[0]["status"]["execution"].is_null());
        let mut next = run();
        next["metadata"]["name"] = json!("run-next");
        next["metadata"]["uid"] = json!("next-owner");
        next["spec"]["generation"] = json!(2);
        stored.runs.insert(0, next);
    }
    for _ in 0..4 {
        peer.run_step().await.unwrap();
    }
    assert_eq!(peer.stored.lock().unwrap().creates, 2);
}

#[tokio::test]
async fn source_retries_three_times_with_one_total_deadline_and_reclaims_every_pod() {
    let peer = Peer::new().await;
    start_task(&peer).await;
    let deadline =
        peer.stored.lock().unwrap().tasks[0]["status"]["execution"]["deadlineMs"].clone();
    for attempt in 1..=3 {
        assert_eq!(
            peer.pod()["metadata"]["ownerReferences"][0]["kind"],
            "SourceTask"
        );
        peer.mutate_pod(|pod| source_result(pod, 1));
        for _ in 0..8 {
            peer.task_step().await.unwrap();
        }
        let stored = peer.stored.lock().unwrap();
        if attempt < 3 {
            assert_eq!(
                stored.tasks[0]["status"]["execution"]["deadlineMs"],
                deadline
            );
            assert_eq!(
                stored.tasks[0]["status"]["execution"]["attempt"],
                attempt + 1
            );
        }
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 3);
    assert!(stored.pods.is_empty());
    assert_eq!(stored.tasks[0]["status"]["phase"], "Failed");
    assert!(stored.tasks[0]["status"]["execution"].is_null());
}

#[tokio::test]
async fn source_deadline_and_missing_pod_never_trigger_another_attempt() {
    for missing in [true, false] {
        let peer = Peer::new().await;
        start_task(&peer).await;
        if missing {
            peer.stored.lock().unwrap().pods.clear();
        } else {
            peer.stored.lock().unwrap().tasks[0]["status"]["execution"]["deadlineMs"] = json!(1);
            peer.mutate_pod(|pod| source_result(pod, 1));
        }
        for _ in 0..7 {
            peer.task_step().await.unwrap();
        }
        let stored = peer.stored.lock().unwrap();
        assert_eq!(stored.creates, 1);
        assert_eq!(stored.tasks[0]["status"]["phase"], "Failed");
        assert_eq!(
            !stored.tasks[0]["status"]["execution"].is_null(),
            missing,
            "missing execution stays charged"
        );
    }
}

#[tokio::test]
async fn source_success_and_external_deletion_remove_finalizer_after_uid_cleanup() {
    let peer = Peer::new().await;
    start_task(&peer).await;
    peer.mutate_pod(|pod| source_result(pod, 0));
    for _ in 0..7 {
        peer.task_step().await.unwrap();
    }
    peer.stored.lock().unwrap().tasks[0]["metadata"]["deletionTimestamp"] =
        json!(chrono::Utc::now().to_rfc3339());
    for _ in 0..2 {
        peer.task_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.tasks[0]["status"]["phase"], "Succeeded");
    assert!(stored.pods.is_empty());
    assert!(
        stored.tasks[0]["metadata"]["finalizers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn cleanup_api_failure_preserves_identity_for_retry() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.stored.lock().unwrap().runtime["status"] = json!("canceled");
    peer.stored.lock().unwrap().fail_delete = true;
    peer.run_step().await.unwrap();
    assert!(peer.run_step().await.is_err());
    let stored = peer.stored.lock().unwrap();
    let run: AgentRun = serde_json::from_value(stored.runs[0].clone()).unwrap();
    assert_eq!(
        run.status.unwrap().execution.unwrap().uid.as_deref(),
        Some("pod-1")
    );
    assert_eq!(stored.pods.len(), 1);
}

#[tokio::test]
async fn cancellation_fences_inflight_creation_but_releases_unsent_reservations() {
    for started in [false, true] {
        let peer = Peer::new().await;
        for _ in 0..if started { 3 } else { 2 } {
            peer.run_step().await.unwrap();
        }
        peer.stored.lock().unwrap().runs[0]["metadata"]["deletionTimestamp"] =
            json!(chrono::Utc::now().to_rfc3339());
        for _ in 0..4 {
            peer.run_step().await.unwrap();
        }
        {
            let stored = peer.stored.lock().unwrap();
            assert_eq!(stored.creates, 0);
            assert_eq!(!stored.runs[0]["status"]["execution"].is_null(), started);
            assert_eq!(stored.runtime["status"], "failed");
            assert_eq!(
                stored.runs[0]["metadata"]["finalizers"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(pods::CR_FINALIZER)),
                started
            );
        }

        let peer = Peer::new().await;
        for _ in 0..if started { 3 } else { 2 } {
            peer.task_step().await.unwrap();
        }
        peer.stored.lock().unwrap().tasks[0]["spec"]["cancelRequested"] = json!(true);
        for _ in 0..3 {
            peer.task_step().await.unwrap();
        }
        let stored = peer.stored.lock().unwrap();
        assert_eq!(stored.creates, 0);
        assert_eq!(!stored.tasks[0]["status"]["execution"].is_null(), started);
        assert_eq!(
            stored.tasks[0]["metadata"]["finalizers"]
                .as_array()
                .unwrap()
                .contains(&json!(pods::CR_FINALIZER)),
            started
        );
    }
}

#[tokio::test]
async fn unknown_execution_keeps_project_capacity_after_runtime_releases_it() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    {
        let mut stored = peer.stored.lock().unwrap();
        stored.max_active = 2;
        stored.pods.clear();
        stored.runs[0]["status"]["phase"] = json!("canceled");
        let mut next = run();
        next["metadata"]["name"] = json!("run-next");
        next["metadata"]["uid"] = json!("next-owner");
        next["spec"]["runId"] = json!(TASK);
        stored.runs.insert(0, next);
    }
    for _ in 0..3 {
        peer.run_step().await.unwrap();
    }
    {
        let stored = peer.stored.lock().unwrap();
        assert_eq!(stored.creates, 1);
        assert_eq!(stored.runs[0]["status"]["phase"], "Queued");
        assert!(stored.runs[0]["status"]["execution"].is_null());
    }

    let peer = Peer::new().await;
    start_task(&peer).await;
    {
        let mut stored = peer.stored.lock().unwrap();
        stored.max_active = 2;
        stored.pods.clear();
        stored.tasks[0]["status"]["phase"] = json!("Failed");
        let mut next = task();
        next["metadata"]["name"] = json!("task-next");
        next["metadata"]["uid"] = json!("next-owner");
        next["spec"]["taskId"] = json!(RUN);
        stored.tasks.insert(0, next);
    }
    for _ in 0..3 {
        peer.task_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 1);
    assert_eq!(stored.tasks[0]["status"]["phase"], "Queued");
}

#[tokio::test]
async fn late_pod_without_matching_ledger_stays_charged_even_with_existing_owner() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    {
        let mut stored = peer.stored.lock().unwrap();
        stored.runs[0]["status"]["execution"] = Value::Null;
        let mut next = run();
        next["metadata"]["name"] = json!("run-next");
        next["metadata"]["uid"] = json!("next-owner");
        next["spec"]["runId"] = json!(TASK);
        next["spec"]["projectId"] = json!(TASK);
        stored.runs.insert(0, next);
    }
    for _ in 0..3 {
        peer.run_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 1);
    assert_eq!(stored.runs[0]["status"]["phase"], "Queued");
    assert!(stored.runs[0]["status"]["execution"].is_null());
}

#[tokio::test]
async fn image_pull_wait_does_not_finish_planning_and_unscheduled_cancel_reclaims_pod() {
    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.mutate_pod(|pod| {
        pod["status"] = json!({"phase":"Pending","containerStatuses":[{"name":"pi","image":"pi","imageID":"","ready":false,"restartCount":0,"state":{"waiting":{"reason":"ImagePullBackOff"}}}]});
    });
    for _ in 0..2 {
        peer.run_step().await.unwrap();
    }
    {
        let mut stored = peer.stored.lock().unwrap();
        assert_eq!(stored.runtime["status"], "provisioning");
        assert_eq!(stored.runs[0]["status"]["message"], "ImagePullBackOff");
        assert!(
            stored
                .events
                .iter()
                .all(|(path, _)| !path.ends_with("execution-failed"))
        );
        stored.runtime["status"] = json!("canceled");
    }
    for _ in 0..7 {
        peer.run_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert!(stored.pods.is_empty());
    assert!(stored.runs[0]["status"]["execution"].is_null());
    assert_eq!(stored.runs[0]["status"]["phase"], "canceled");
}

#[tokio::test]
async fn completed_resources_release_cr_protection_without_reinstalling_it() {
    let peer = Peer::new().await;
    start_task(&peer).await;
    peer.mutate_pod(|pod| source_result(pod, 0));
    for _ in 0..10 {
        peer.task_step().await.unwrap();
    }
    {
        let stored = peer.stored.lock().unwrap();
        assert_eq!(stored.tasks[0]["status"]["phase"], "Succeeded");
        assert!(stored.tasks[0]["status"]["execution"].is_null());
        assert!(
            stored.tasks[0]["metadata"]["finalizers"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(stored.pods.is_empty());
        assert!(stored.tasks[0]["metadata"]["deletionTimestamp"].is_null());
    }
    let before = peer
        .stored
        .lock()
        .unwrap()
        .events
        .iter()
        .filter(|(path, _)| path.starts_with("PATCH "))
        .count();
    for _ in 0..4 {
        peer.task_step().await.unwrap();
    }
    let after = peer
        .stored
        .lock()
        .unwrap()
        .events
        .iter()
        .filter(|(path, _)| path.starts_with("PATCH "))
        .count();
    assert_eq!(
        before, after,
        "clean terminal resources must not reinstall protection"
    );

    let peer = Peer::new().await;
    start_run(&peer).await;
    peer.stored.lock().unwrap().runtime["status"] = json!("succeeded");
    for _ in 0..10 {
        peer.run_step().await.unwrap();
    }
    {
        let stored = peer.stored.lock().unwrap();
        assert_eq!(stored.runs[0]["status"]["phase"], "succeeded");
        assert!(stored.runs[0]["status"]["execution"].is_null());
        assert!(
            stored.runs[0]["metadata"]["finalizers"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(stored.pods.is_empty());
    }
    let before = peer
        .stored
        .lock()
        .unwrap()
        .events
        .iter()
        .filter(|(path, _)| path.starts_with("PATCH "))
        .count();
    for _ in 0..4 {
        peer.run_step().await.unwrap();
    }
    let after = peer
        .stored
        .lock()
        .unwrap()
        .events
        .iter()
        .filter(|(path, _)| path.starts_with("PATCH "))
        .count();
    assert_eq!(before, after);
}

#[tokio::test]
async fn terminal_untracked_owned_pod_keeps_protection_even_without_pool_labels() {
    let peer = Peer::new().await;
    start_task(&peer).await;
    {
        let mut stored = peer.stored.lock().unwrap();
        stored.tasks[0]["status"]["phase"] = json!("Succeeded");
        stored.tasks[0]["status"]["execution"] = Value::Null;
        stored.tasks[0]["metadata"]["finalizers"] = json!([]);
        stored.pods.values_mut().next().unwrap()["metadata"]["labels"] = json!({});
    }
    for _ in 0..4 {
        peer.task_step().await.unwrap();
    }
    let stored = peer.stored.lock().unwrap();
    assert!(
        stored.tasks[0]["metadata"]["finalizers"]
            .as_array()
            .unwrap()
            .contains(&json!(pods::CR_FINALIZER))
    );
    assert!(
        stored.tasks[0]["status"]["message"]
            .as_str()
            .unwrap()
            .starts_with("CleanupPending:")
    );
    assert_eq!(stored.creates, 1);
    assert_eq!(stored.pods.len(), 1);
}

#[tokio::test]
async fn stale_source_creation_snapshot_cannot_recreate_after_terminal_cleanup() {
    let peer = Peer::new().await;
    for _ in 0..3 {
        peer.task_step().await.unwrap();
    }
    let stale = peer.stored.lock().unwrap().tasks[0].clone();
    peer.task_step().await.unwrap();
    peer.mutate_pod(|pod| source_result(pod, 0));
    for _ in 0..10 {
        peer.task_step().await.unwrap();
    }
    let result = artifactory_agent_operator::source_tasks::reconcile_task(
        std::sync::Arc::new(serde_json::from_value(stale).unwrap()),
        peer.ctx.clone(),
    )
    .await;
    assert!(
        result.is_err(),
        "old creation intent must be fenced by current owner version"
    );
    let stored = peer.stored.lock().unwrap();
    assert_eq!(stored.creates, 1);
    assert!(stored.pods.is_empty());
    assert_eq!(stored.tasks[0]["status"]["phase"], "Succeeded");
    assert!(
        stored.tasks[0]["metadata"]["finalizers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
