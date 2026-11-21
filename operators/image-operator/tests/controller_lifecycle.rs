//! [INPUT]: 依赖真实 kube 客户端与隔离 HTTP Kubernetes peer。
//! [OUTPUT]: 验证缺池终态、查池重试、创建意图、UID 围栏和收尾。
//! [POS]: Peer 保留收到 DELETE 的活 Pod，模拟 Finalizer 与失联节点。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(feature = "operator")]
mod support;
use artifactory_image_operator::resources;
use serde_json::json;
use support::*;

#[tokio::test]
async fn missing_builder_pool_fails_without_creating_pods_and_releases_finalizer() {
    let peer = Peer::new(
        build("build", "00000000-0000-0000-0000-000000000001"),
        vec![],
    )
    .await;
    peer.stored.lock().unwrap().pool_status = Some(404);
    peer.reconcile().await.unwrap();
    let current = peer.current();
    assert_eq!(current["status"]["phase"], "Failed");
    assert_eq!(current["status"]["observedGeneration"], 1);
    assert_eq!(current["status"]["podOwnership"]["pods"], json!([]));
    assert!(
        current["status"]["message"]
            .as_str()
            .unwrap()
            .contains("Image failure: reason=BuilderPoolNotFound")
    );
    peer.repeat(3).await;
    let current = peer.current();
    assert_eq!(
        current["status"]["message"],
        "Image failure: reason=BuilderPoolNotFound"
    );
    assert_eq!(current["metadata"]["finalizers"], json!([]));
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
}

#[tokio::test]
async fn denied_or_unavailable_builder_pool_remains_retryable() {
    for code in [403, 503] {
        let peer = Peer::new(
            build("build", "00000000-0000-0000-0000-000000000001"),
            vec![],
        )
        .await;
        peer.stored.lock().unwrap().pool_status = Some(code);
        assert!(peer.reconcile().await.is_err());
        assert!(peer.current()["status"].is_null());
        assert_eq!(peer.stored.lock().unwrap().creates, 0);
        peer.stored.lock().unwrap().pool_status = None;
        peer.repeat(2).await;
        assert_eq!(peer.stored.lock().unwrap().creates, 1);
    }
}

#[tokio::test]
async fn finalizer_precedes_creation_and_pod_is_bound_without_batch_api() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["metadata"].as_object_mut().unwrap().remove("finalizers");
    let peer = Peer::new(b, vec![]).await;
    peer.stored.lock().unwrap().fail_metadata = true;
    assert!(peer.reconcile().await.is_err());
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.stored.lock().unwrap().fail_metadata = false;
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert!(peer.current()["status"]["podOwnership"]["creating"].is_string());
    peer.reconcile().await.unwrap();
    let s = peer.stored.lock().unwrap();
    assert_eq!(s.creates, 1);
    assert_eq!(
        s.create_intents,
        vec![s.pods[0]["metadata"]["name"].clone()]
    );
    assert_eq!(
        s.pods[0]["metadata"]["ownerReferences"][0]["kind"],
        "ImageBuild"
    );
    assert_eq!(
        s.pods[0]["metadata"]["finalizers"],
        json!([resources::POD_FINALIZER])
    );
    assert_eq!(
        s.builds[0]["status"]["podOwnership"]["pods"][0]["uid"],
        s.pods[0]["metadata"]["uid"]
    );
    assert_eq!(
        s.builds[0]["status"]["podOwnership"]["pods"][0]["platform"],
        "linux/amd64"
    );
    assert!(
        s.requests
            .iter()
            .all(|(_, path)| !path.contains("/batch/") && !path.contains("/jobs"))
    );
}
#[tokio::test]
async fn create_ack_survives_status_failure_without_a_second_pod() {
    let peer = Peer::new(
        build("build", "00000000-0000-0000-0000-000000000001"),
        vec![],
    )
    .await;
    peer.reconcile().await.unwrap();
    peer.stored.lock().unwrap().fail_status = true;
    assert!(peer.reconcile().await.is_err());
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
    peer.stored.lock().unwrap().fail_status = false;
    peer.repeat(2).await;
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
    assert_eq!(
        peer.current()["status"]["podOwnership"]["pods"][0]["uid"],
        "created-0"
    );
}
#[tokio::test]
async fn reservation_status_failure_never_sends_a_creation_request() {
    let peer = Peer::new(
        build("build", "00000000-0000-0000-0000-000000000001"),
        vec![],
    )
    .await;
    peer.stored.lock().unwrap().fail_status = true;
    assert!(peer.reconcile().await.is_err());
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert!(peer.current()["status"].is_null());
    peer.stored.lock().unwrap().fail_status = false;
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert!(peer.current()["status"]["podOwnership"]["creating"].is_string());
}
#[tokio::test]
async fn canceled_or_deleted_creation_intent_keeps_capacity_and_cr_protection() {
    for deleting in [false, true] {
        let peer = Peer::new(
            build("build", "00000000-0000-0000-0000-000000000001"),
            vec![],
        )
        .await;
        peer.reconcile().await.unwrap();
        let pending = peer.current()["status"]["podOwnership"]["creating"].clone();
        {
            let mut s = peer.stored.lock().unwrap();
            if deleting {
                s.builds[0]["metadata"]["deletionTimestamp"] = json!("2026-11-19T00:00:00Z");
            } else {
                s.builds[0]["spec"]["cancelRequested"] = json!(true);
            }
            bump(&mut s.builds[0]);
        }
        peer.repeat(4).await;
        let current = peer.current();
        assert_eq!(current["status"]["phase"], "Canceled");
        assert_eq!(current["status"]["podOwnership"]["creating"], pending);
        assert!(
            current["status"]["message"]
                .as_str()
                .unwrap()
                .starts_with("CleanupPending:")
        );
        assert!(
            current["metadata"]["finalizers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "images.artifactory.dev/cleanup")
        );
        assert_eq!(peer.stored.lock().unwrap().creates, 0);
    }
}
#[tokio::test]
async fn late_pod_binds_canceled_intent_and_cleans_only_after_stop_is_saved() {
    let peer = Peer::new(
        build("build", "00000000-0000-0000-0000-000000000001"),
        vec![],
    )
    .await;
    peer.reconcile().await.unwrap();
    peer.stored.lock().unwrap().builds[0]["spec"]["cancelRequested"] = json!(true);
    peer.repeat(2).await;
    let late = pod(
        &peer.current(),
        Some("linux/amd64"),
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    let uid = late["metadata"]["uid"].clone();
    peer.stored.lock().unwrap().pods.push(late);
    peer.reconcile().await.unwrap();
    assert!(peer.current()["status"]["podOwnership"]["creating"].is_null());
    assert_eq!(
        peer.current()["status"]["podOwnership"]["pods"][0]["uid"],
        uid
    );
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().pods.len(), 1);
    {
        let mut s = peer.stored.lock().unwrap();
        let mut stopped = complete(&s.builds[0], Some("linux/amd64"));
        stopped["metadata"] = s.pods[0]["metadata"].clone();
        s.pods[0] = stopped;
    }
    peer.reconcile().await.unwrap();
    assert_eq!(
        peer.current()["status"]["podOwnership"]["pods"][0]["stopped"],
        true
    );
    assert_eq!(peer.stored.lock().unwrap().pods.len(), 1);
    peer.repeat(4).await;
    assert!(peer.stored.lock().unwrap().pods.is_empty());
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert_eq!(peer.current()["metadata"]["finalizers"], json!([]));
}
#[tokio::test]
async fn creation_intents_count_pool_and_project_slots_without_double_charging_their_owner() {
    let peer = Peer::new(
        build("first", "00000000-0000-0000-0000-000000000001"),
        vec![],
    )
    .await;
    peer.reconcile().await.unwrap();
    {
        let mut s = peer.stored.lock().unwrap();
        s.pool["spec"]["maxActive"] = json!(2);
        s.pool["spec"]["maxPerProject"] = json!(1);
        s.builds
            .push(build("same", "00000000-0000-0000-0000-000000000002"));
        let mut other = build("other", "00000000-0000-0000-0000-000000000003");
        other["spec"]["projectId"] = json!("other-project");
        s.builds.push(other);
    }
    peer.reconcile_index(1).await.unwrap();
    assert!(peer.stored.lock().unwrap().builds[1]["status"]["podOwnership"]["creating"].is_null());
    peer.reconcile_index(2).await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.reconcile().await.unwrap();
    peer.reconcile_index(2).await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 2);
}
#[tokio::test]
async fn stale_reconcile_snapshot_cannot_create_after_uid_version_cancel_or_terminal_changes() {
    for change in ["uid", "version", "cancel", "terminal"] {
        let peer = Peer::new(
            build("build", "00000000-0000-0000-0000-000000000001"),
            vec![],
        )
        .await;
        peer.reconcile().await.unwrap();
        let snapshot = peer.current();
        {
            let mut s = peer.stored.lock().unwrap();
            match change {
                "uid" => s.builds[0]["metadata"]["uid"] = json!("replacement"),
                "cancel" => s.builds[0]["spec"]["cancelRequested"] = json!(true),
                "terminal" => s.builds[0]["status"]["phase"] = json!("Canceled"),
                _ => (),
            }
            bump(&mut s.builds[0]);
        }
        assert!(peer.reconcile_value(snapshot).await.is_err(), "{change}");
        assert_eq!(peer.stored.lock().unwrap().creates, 0, "{change}");
    }
}
#[tokio::test]
async fn only_one_platform_creation_is_reserved_at_a_time() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["platforms"] = json!(["linux/amd64", "linux/arm64"]);
    let peer = Peer::new(b, vec![]).await;
    peer.reconcile().await.unwrap();
    let first = peer.current()["status"]["podOwnership"]["creating"].clone();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.reconcile().await.unwrap();
    assert_eq!(
        peer.stored.lock().unwrap().create_intents,
        vec![first.clone()]
    );
    assert!(peer.current()["status"]["podOwnership"]["creating"].is_null());
    peer.reconcile().await.unwrap();
    let second = peer.current()["status"]["podOwnership"]["creating"].clone();
    assert_ne!(first, second);
    peer.reconcile().await.unwrap();
    assert_eq!(
        peer.stored.lock().unwrap().create_intents,
        vec![first, second]
    );
}
#[tokio::test]
async fn any_failed_container_persists_safe_reason_before_cleanup_and_preserves_it() {
    for failure in ["source", "worker", "buildkit", "Evicted"] {
        let b = build("build", "00000000-0000-0000-0000-000000000001");
        let mut p = pod(
            &b,
            Some("linux/amd64"),
            &[("worker", running()), ("buildkit", running())],
            Some(ended(0)),
        );
        if failure == "Evicted" {
            p["status"]["phase"] = json!("Failed");
            p["status"]["reason"] = json!("Evicted");
        } else if failure == "source" {
            p["status"]["initContainerStatuses"][0]["state"] = ended(1);
        } else {
            let index = usize::from(failure == "buildkit");
            p["status"]["containerStatuses"][index]["state"] = ended(137);
        }
        p["status"]["message"] = json!("private-token-in-message");
        let peer = Peer::new(b, vec![p]).await;
        peer.reconcile().await.unwrap();
        let message = peer.current()["status"]["message"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(message.contains(if failure == "Evicted" {
            "pod=Evicted"
        } else if failure == "source" {
            "container=source"
        } else {
            "OOMKilled"
        }));
        assert!(!message.contains("private-token"));
        assert!(peer.stored.lock().unwrap().deletions.is_empty());
        peer.reconcile().await.unwrap();
        assert_eq!(
            peer.stored.lock().unwrap().pods.len(),
            1,
            "DELETE does not stop a running Pod"
        );
        {
            let mut s = peer.stored.lock().unwrap();
            s.builds[0]["spec"]["cancelRequested"] = json!(true);
            let p = &mut s.pods[0];
            p["status"]["phase"] = json!("Failed");
            for state in p["status"]["containerStatuses"].as_array_mut().unwrap() {
                state["state"] = ended(0);
            }
            p["status"]["initContainerStatuses"][0]["state"] = ended(0);
        }
        peer.reconcile().await.unwrap();
        assert!(
            peer.current()["status"]["podOwnership"]["pods"][0]["stopped"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(
            peer.stored.lock().unwrap().pods.len(),
            1,
            "stop evidence is durable before finalizer removal"
        );
        peer.repeat(2).await;
        assert!(peer.stored.lock().unwrap().pods.is_empty());
        assert_eq!(peer.current()["status"]["phase"], "Failed");
        assert_eq!(
            peer.current()["status"]["message"],
            message.lines().nth(1).unwrap()
        );
    }
}
#[tokio::test]
async fn source_init_failure_can_finish_without_nonexistent_main_exit_statuses() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let mut p = pod(&b, Some("linux/amd64"), &[], Some(ended(1)));
    p["status"]["phase"] = json!("Failed");
    let peer = Peer::new(b, vec![p]).await;
    peer.repeat(4).await;
    assert!(peer.stored.lock().unwrap().pods.is_empty());
    assert_eq!(
        peer.current()["status"]["message"],
        "Image failure: platform=linux/amd64 container=source reason=Error exit=1"
    );
}
#[tokio::test]
async fn deleting_unknown_pod_keeps_cr_finalizer_and_pool_capacity_until_confirmed_stop() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["metadata"]["deletionTimestamp"] = json!("2026-11-19T00:00:00Z");
    let mut p = pod(
        &b,
        Some("linux/amd64"),
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    p["status"]["phase"] = json!("Unknown");
    let peer = Peer::new(b, vec![p]).await;
    peer.repeat(3).await;
    {
        let mut s = peer.stored.lock().unwrap();
        assert_eq!(s.deletions.len(), 1);
        assert!(s.pods[0]["metadata"]["deletionTimestamp"].is_string());
        assert_eq!(
            s.pods[0]["metadata"]["finalizers"],
            json!([resources::POD_FINALIZER])
        );
        s.pool["spec"]["maxActive"] = json!(1);
        s.builds
            .push(build("waiting", "00000000-0000-0000-0000-000000000002"));
    }
    peer.reconcile_index(1).await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.stored.lock().unwrap().fail_pods = true;
    assert!(peer.reconcile().await.is_err());
    peer.stored.lock().unwrap().fail_pods = false;
    {
        let mut s = peer.stored.lock().unwrap();
        s.pods[0]["status"]["phase"] = json!("Failed");
        for state in s.pods[0]["status"]["containerStatuses"]
            .as_array_mut()
            .unwrap()
        {
            state["state"] = ended(0);
        }
    }
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().pods.len(), 1);
    peer.repeat(3).await;
    assert!(
        peer.stored
            .lock()
            .unwrap()
            .builds
            .iter()
            .all(|b| b["metadata"]["name"] != "build")
    );
    peer.repeat(2).await;
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
}
#[tokio::test]
async fn missing_or_replaced_recorded_pods_fail_without_recreation_or_capacity_release() {
    for replaced in [false, true] {
        let mut b = build("build", "00000000-0000-0000-0000-000000000001");
        let original = pod(
            &b,
            Some("linux/amd64"),
            &[("worker", running()), ("buildkit", running())],
            Some(ended(0)),
        );
        ledger(&mut b, std::slice::from_ref(&original), false);
        let pods = if replaced {
            let mut p = original;
            p["metadata"]["uid"] = json!("replacement");
            vec![p]
        } else {
            vec![]
        };
        let peer = Peer::new(b, pods).await;
        peer.repeat(3).await;
        let current = peer.current();
        assert_eq!(current["status"]["phase"], "Failed");
        assert!(
            current["status"]["message"]
                .as_str()
                .unwrap()
                .contains(if replaced {
                    "pod=IdentityChanged"
                } else {
                    "pod=Missing"
                })
        );
        {
            let mut s = peer.stored.lock().unwrap();
            assert_eq!(s.creates, 0);
            assert!(s.deletions.is_empty(), "never delete a replacement UID");
            s.pool["spec"]["maxActive"] = json!(1);
            s.builds
                .push(build("waiting", "00000000-0000-0000-0000-000000000002"));
        }
        peer.reconcile_index(1).await.unwrap();
        assert_eq!(
            peer.stored.lock().unwrap().creates,
            0,
            "lost unconfirmed Pod reserves capacity"
        );
    }
}
#[tokio::test]
async fn completed_platforms_release_slots_for_publisher_but_remain_owned_until_final_cleanup() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["platforms"] = json!(["linux/amd64", "linux/arm64"]);
    let first = complete(&b, Some("linux/amd64"));
    let second = complete(&b, Some("linux/arm64"));
    let peer = Peer::new(b, vec![first, second]).await;
    peer.stored.lock().unwrap().pool["spec"]["maxActive"] = json!(1);
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Publishing");
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.reconcile().await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
    let publisher = peer.stored.lock().unwrap().pods[2].clone();
    assert_eq!(
        publisher["metadata"]["ownerReferences"][0]["kind"],
        "ImageBuild"
    );
    assert!(publisher["spec"]["initContainers"].is_null());
    {
        let mut s = peer.stored.lock().unwrap();
        let mut done = complete(&s.builds[0], None);
        done["metadata"] = s.pods[2]["metadata"].clone();
        s.pods[2] = done;
    }
    peer.reconcile().await.unwrap();
    assert_eq!(peer.current()["status"]["phase"], "Succeeded");
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
    peer.repeat(2).await;
    assert!(peer.stored.lock().unwrap().pods.is_empty());
    assert!(peer.current()["status"]["message"].is_null());
    assert_eq!(
        peer.current()["status"]["podOwnership"]["pods"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}
#[tokio::test]
async fn succeeded_phase_without_all_container_exit_receipts_cannot_start_publisher() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let mut p = complete(&b, Some("linux/amd64"));
    p["status"]["containerStatuses"][1]["state"] = running();
    let peer = Peer::new(b, vec![p]).await;
    peer.repeat(2).await;
    assert_eq!(peer.current()["status"]["phase"], "Building");
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
}
#[tokio::test]
async fn unscheduled_cancelled_pod_can_be_cleaned_and_foreign_finalizers_are_preserved() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["cancelRequested"] = json!(true);
    let mut p = pod(&b, Some("linux/amd64"), &[], None);
    p["spec"].as_object_mut().unwrap().remove("nodeName");
    p["status"] = json!({"phase":"Pending"});
    p["metadata"]["finalizers"] = json!([resources::POD_FINALIZER, "example.com/foreign"]);
    let peer = Peer::new(b, vec![p]).await;
    peer.repeat(4).await;
    assert_eq!(
        peer.stored.lock().unwrap().pods[0]["metadata"]["finalizers"],
        json!(["example.com/foreign"])
    );
    assert_eq!(
        peer.current()["status"]["podOwnership"]["pods"][0]["stopped"],
        true
    );
    assert!(
        peer.current()["status"]["message"]
            .as_str()
            .unwrap()
            .starts_with("CleanupPending:")
    );
}
#[tokio::test]
async fn foreign_owner_or_ledger_uid_cannot_be_adopted_or_deleted() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    let mut p = pod(
        &b,
        Some("linux/amd64"),
        &[("worker", ended(137)), ("buildkit", running())],
        Some(ended(0)),
    );
    p["metadata"]["ownerReferences"][0]["uid"] = json!("other-build");
    let peer = Peer::new(b.clone(), vec![p]).await;
    assert!(peer.reconcile().await.is_err());
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
    b["status"] = json!({"phase":"Failed","observedGeneration":1,"podOwnership":{"imageBuildUid":"foreign","pods":[]}});
    peer.stored.lock().unwrap().builds[0] = b;
    assert!(peer.reconcile().await.is_err());
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
}

#[tokio::test]
async fn failed_status_write_never_deletes_a_pod_before_durable_uid_and_terminal_state() {
    let b = build("build", "00000000-0000-0000-0000-000000000001");
    let p = pod(
        &b,
        Some("linux/amd64"),
        &[("worker", ended(1)), ("buildkit", running())],
        Some(ended(0)),
    );
    let peer = Peer::new(b, vec![p]).await;
    peer.stored.lock().unwrap().fail_status = true;
    assert!(peer.reconcile().await.is_err());
    assert!(peer.stored.lock().unwrap().deletions.is_empty());
    peer.stored.lock().unwrap().fail_status = false;
    peer.repeat(2).await;
    let state = peer.stored.lock().unwrap();
    assert_eq!(state.deletions.len(), 1);
    assert_eq!(state.deletions[0].1["status"]["phase"], "Failed");
    assert_eq!(
        state.deletions[0].0["preconditions"]["uid"],
        state.deletions[0].1["status"]["podOwnership"]["pods"][0]["uid"]
    );
    assert!(
        state.deletions[0].0.get("gracePeriodSeconds").is_none(),
        "never force-delete running containers"
    );
}

#[tokio::test]
async fn unexpected_deletion_fails_once_and_keeps_the_other_platform_live_until_stopped() {
    let mut b = build("build", "00000000-0000-0000-0000-000000000001");
    b["spec"]["platforms"] = json!(["linux/amd64", "linux/arm64"]);
    let mut first = pod(
        &b,
        Some("linux/amd64"),
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    first["metadata"]["deletionTimestamp"] = json!("2026-11-19T00:00:00Z");
    let second = pod(
        &b,
        Some("linux/arm64"),
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    let peer = Peer::new(b, vec![first, second]).await;
    peer.repeat(3).await;
    assert_eq!(peer.current()["status"]["phase"], "Failed");
    assert!(
        peer.current()["status"]["message"]
            .as_str()
            .unwrap()
            .contains("pod=Deleting")
    );
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert_eq!(peer.stored.lock().unwrap().pods.len(), 2);
    assert_eq!(
        peer.stored.lock().unwrap().deletions.len(),
        1,
        "already deleting platform is not deleted again"
    );
}

#[tokio::test]
async fn project_capacity_counts_deleting_unknown_pods_even_when_pool_has_free_slots() {
    let mut active = build("active", "00000000-0000-0000-0000-000000000001");
    active["spec"]["cancelRequested"] = json!(true);
    let mut p = pod(
        &active,
        Some("linux/amd64"),
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    p["status"]["phase"] = json!("Unknown");
    let peer = Peer::new(active, vec![p]).await;
    peer.repeat(2).await;
    {
        let mut state = peer.stored.lock().unwrap();
        state.pool["spec"]["maxActive"] = json!(8);
        state.pool["spec"]["maxPerProject"] = json!(1);
        state
            .builds
            .push(build("same", "00000000-0000-0000-0000-000000000002"));
        let mut other = build("other", "00000000-0000-0000-0000-000000000003");
        other["spec"]["projectId"] = json!("other-project");
        state.builds.push(other);
    }
    peer.reconcile_index(1).await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    peer.reconcile_index(2).await.unwrap();
    peer.reconcile_index(2).await.unwrap();
    assert_eq!(peer.stored.lock().unwrap().creates, 1);
}

#[tokio::test]
async fn confirmed_terminal_cleanup_releases_cr_finalizer_without_reinstalling_it() {
    for phase in ["Succeeded", "Failed", "Canceled"] {
        let mut b = build("done", "00000000-0000-0000-0000-000000000001");
        let p = complete(&b, Some("linux/amd64"));
        ledger(&mut b, &[p], true);
        b["status"]["phase"] = json!(phase);
        b["metadata"]["finalizers"] =
            json!(["images.artifactory.dev/cleanup", "example.com/foreign"]);
        let peer = Peer::new(b, vec![]).await;
        peer.repeat(4).await;
        let s = peer.stored.lock().unwrap();
        assert_eq!(
            s.builds[0]["metadata"]["finalizers"],
            json!(["example.com/foreign"])
        );
        assert_eq!(s.builds[0]["status"]["phase"], phase);
        assert_eq!(s.creates, 0);
    }
}

#[tokio::test]
async fn protected_orphan_pod_retains_capacity_after_its_cr_is_forcibly_removed() {
    let removed = build("removed", "00000000-0000-0000-0000-000000000001");
    let mut p = pod(
        &removed,
        Some("linux/amd64"),
        &[("worker", running()), ("buildkit", running())],
        Some(ended(0)),
    );
    p["status"]["phase"] = json!("Unknown");
    p["metadata"]["deletionTimestamp"] = json!("2026-11-19T00:00:00Z");
    let waiting = build("waiting", "00000000-0000-0000-0000-000000000002");
    let peer = Peer::new(waiting, vec![p]).await;
    peer.stored.lock().unwrap().pool["spec"]["maxActive"] = json!(1);
    peer.repeat(2).await;
    assert_eq!(peer.stored.lock().unwrap().creates, 0);
    assert!(
        peer.stored.lock().unwrap().deletions.is_empty(),
        "a missing CR is not permission to delete its unknown Pod"
    );
}
