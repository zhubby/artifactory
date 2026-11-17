//! [INPUT]: node_metadata 的节点、资源和规划 Worker 夹具。
//! [OUTPUT]: 验证候选准入、有限重排和不可变执行脚本。
//! [POS]: 在共享维护锁释放后，按持久轮次验证重排边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn superseded_admission_does_not_start_the_unavailable_timer() {
    let mut database = Database::with_tenant().await;
    let certificates = Certificates::new();
    let (node, _, resource) = placement_node(&database).await;
    let (_, build, job) = database.fixture().await;
    rank(&database, build, job, &[node]).await;
    let values = Arc::new(RwLock::new(vec![resource]));
    let requested = Arc::new(tokio::sync::Notify::new());
    let (_directory, server) = resource_server_notifying(
        &mut database,
        &certificates,
        values.clone(),
        Some(requested.clone()),
    )
    .await;
    let resource_guard = values.write().await;
    let state = database.state.clone();
    let scheduler = tokio::spawn(async move { scheduler::schedule(&state).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), requested.notified())
        .await
        .unwrap();
    // A different replica accepts a new list while this replica fetches resources.
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE job_placements SET version=version+1 WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    drop(resource_guard);
    scheduler.await.unwrap().unwrap();
    let untouched: bool = sql::scalar(
        &mut database.pool.clone(),
        "SELECT waiting_since IS NULL AND reason IS NULL AND NOT EXISTS(SELECT 1 FROM attempts WHERE job_id=$1) FROM job_placements WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert!(
        untouched,
        "a stale admission batch is not an unavailable candidate observation"
    );
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE jobs SET dispatch_after=clock_timestamp() WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    scheduler::schedule(&database.state).await.unwrap();
    let selected: Uuid = sql::scalar(
        &mut database.pool.clone(),
        "SELECT node_id FROM attempts WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(
        selected, node,
        "the next fresh batch must still allocate the job"
    );
    server.abort();
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ranked_placement_busy_fallback_resource_changes_and_no_unlisted_devices() {
    let mut database = Database::with_tenant().await;
    let certificates = Certificates::new();
    let (a, _, resource_a) = placement_node(&database).await;
    let (b, _, resource_b) = placement_node(&database).await;
    let (_unlisted, _, resource_c) = placement_node(&database).await;
    let resources = Arc::new(RwLock::new(vec![resource_a, resource_b, resource_c]));
    let (_directory, server) =
        resource_server(&mut database, &certificates, resources.clone()).await;
    let (_, build, job) = database.fixture().await;
    // UUID order is deliberately opposite to the model order.
    let mut order = vec![a, b];
    order.sort();
    order.reverse();
    rank(&database, build, job, &order).await;
    scheduler::schedule(&database.state).await.unwrap();
    let selected: Uuid = sql::scalar(
        &mut database.pool.clone(),
        "SELECT node_id FROM attempts WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(selected, order[0]);
    let (_, build2, job2) = database.fixture().await;
    rank(&database, build2, job2, &order).await;
    scheduler::schedule(&database.state).await.unwrap();
    let selected: Uuid = sql::scalar(
        &mut database.pool.clone(),
        "SELECT node_id FROM attempts WHERE job_id=$1",
        params![job2],
    )
    .await
    .unwrap();
    assert_eq!(
        selected, order[1],
        "busy preferred candidate must fall through in the AI order"
    );
    let (_, build3, job3) = database.fixture().await;
    rank(&database, build3, job3, &order).await;
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE attempts SET status='succeeded'",
        params![],
    )
    .await
    .unwrap();
    for resource in resources.write().await.iter_mut() {
        resource["available_memory_bytes"] = json!(1);
    }
    scheduler::schedule(&database.state).await.unwrap();
    let count: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts WHERE job_id=$1",
        params![job3],
    )
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "allocation must use the current resources and never an unlisted device"
    );
    assert!(
        sql::scalar::<bool>(
            &mut database.pool.clone(),
            "SELECT reason IS NOT NULL FROM job_placements WHERE job_id=$1",
            params![job3]
        )
        .await
        .unwrap()
    );
    server.abort();
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn placement_reselection_is_bounded_and_cannot_modify_scripts() {
    let database = Database::with_tenant().await;
    let certificates = Certificates::new();
    let (_, build, job) = database.fixture().await;
    let (node, _, _) = placement_node(&database).await;
    let replica = Replica::start(database.state.clone(), &certificates).await;
    let (mut client, initial_run, _) = planner(&database, &replica, &certificates, build).await;
    rank(&database, build, job, &[node]).await;
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE builds SET status='queued' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE planning_runs SET state='failed' WHERE id=$1",
        params![initial_run],
    )
    .await
    .unwrap();
    let original: Value = sql::scalar(
        &mut database.pool.clone(),
        "SELECT target_config FROM jobs WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    for round in 1..=2 {
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE job_placements SET waiting_since=now()-interval '31 seconds' WHERE job_id=$1",
            params![job],
        )
        .await
        .unwrap();
        let run = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                scheduler::tick(&database.state).await.unwrap();
                let run: Option<Uuid> = sql::optional_scalar(
                    &mut database.pool.clone(),
                    "SELECT run_id FROM placement_rounds WHERE build_id=$1 AND round=$2",
                    params![build, round],
                )
                .await
                .unwrap();
                if let Some(run) = run {
                    break run;
                }
                // 并行 schema 共享数据库维护锁。未获锁的 tick 不写入轮次。
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("placement round did not persist after maintenance reconciliation");
        let config: Value = sql::scalar(
            &mut database.pool.clone(),
            "SELECT model_config FROM planning_runs WHERE id=$1",
            params![run],
        )
        .await
        .unwrap();
        assert_eq!(config["max_calls"], 5);
        assert_eq!(config["timeout_seconds"], 60);
        sql::execute(&mut database.pool.clone(),"UPDATE planning_runs SET worker_id=(SELECT worker_id FROM planning_runs WHERE id=$2),state='leased',lease_hash=$3,lease_expires_at=now()+interval '1 minute' WHERE id=$1",params![run,initial_run,crypto::hash("lease")]).await.unwrap();
        for _ in 0..5 {
            client
                .report(pb::PlannerReportRequest {
                    run_id: run.to_string(),
                    lease_token: "lease".into(),
                    kind: "model_call".into(),
                    payload_json: "{}".into(),
                })
                .await
                .unwrap();
        }
        assert_eq!(
            client
                .report(pb::PlannerReportRequest {
                    run_id: run.to_string(),
                    lease_token: "lease".into(),
                    kind: "model_call".into(),
                    payload_json: "{}".into()
                })
                .await
                .unwrap_err()
                .code(),
            tonic::Code::ResourceExhausted
        );
        let placement: Value = sql::scalar(
            &mut database.pool.clone(),
            "SELECT placement FROM job_placements WHERE job_id=$1",
            params![job],
        )
        .await
        .unwrap();
        let invalid = node_tool(
            &mut client,
            run,
            "submit_placements",
            json!({"placements":{"app":placement},"scripts":[{"name":"bad","content":"changed"}]}),
        )
        .await;
        assert!(invalid.get("error").is_some(), "{invalid}");
        let mut relaxed = placement.clone();
        relaxed["resources"]["min_available_memory_bytes"] = json!(0);
        let invalid = node_tool(
            &mut client,
            run,
            "submit_placements",
            json!({"placements":{"app":relaxed}}),
        )
        .await;
        assert!(
            invalid.get("error").is_some(),
            "resource thresholds must remain immutable: {invalid}"
        );
        let result = node_tool(
            &mut client,
            run,
            "submit_placements",
            json!({"placements":{"app":placement}}),
        )
        .await;
        assert_eq!(result["accepted"], true, "{result}");
        scheduler::tick(&database.state).await.unwrap();
        let count: i64 = sql::scalar(
            &mut database.pool.clone(),
            "SELECT count(*) FROM placement_rounds WHERE build_id=$1",
            params![build],
        )
        .await
        .unwrap();
        assert_eq!(
            count, round as i64,
            "rounds must be spaced at least 60 seconds apart"
        );
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE placement_rounds SET created_at=now()-interval '61 seconds' WHERE build_id=$1",
            params![build],
        )
        .await
        .unwrap();
    }
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE job_placements SET waiting_since=now()-interval '31 seconds' WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    scheduler::tick(&database.state).await.unwrap();
    assert_eq!(
        sql::scalar::<i64>(
            &mut database.pool.clone(),
            "SELECT count(*) FROM placement_rounds WHERE build_id=$1",
            params![build]
        )
        .await
        .unwrap(),
        2
    );
    assert_eq!(
        sql::scalar::<Value>(
            &mut database.pool.clone(),
            "SELECT target_config FROM jobs WHERE id=$1",
            params![job]
        )
        .await
        .unwrap(),
        original
    );
    replica.stop().await;
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn ranked_admission_rechecks_identity_permissions_health_and_both_resource_clocks() {
    let mut database = Database::with_tenant().await;
    let certificates = Certificates::new();
    let (node, startup, valid) = placement_node(&database).await;
    let resources = Arc::new(RwLock::new(vec![valid.clone()]));
    let (_directory, server) =
        resource_server(&mut database, &certificates, resources.clone()).await;
    for case in [
        "memory", "disk", "cpu", "sampled", "received", "startup", "digest", "project", "drain",
        "ota", "offline", "tools", "cancel",
    ] {
        let (project, build, job) = database.fixture().await;
        rank(&database, build, job, &[node]).await;
        sql::execute(&mut database.pool.clone(),"UPDATE nodes SET draining=false,ota_draining=false,last_seen=clock_timestamp(),metadata_digest=$2,capabilities='[\"git\",\"bash\",\"source-cache-v1\",\"external-build-plan-v1\"]' WHERE id=$1",params![node,"a".repeat(64)]).await.unwrap();
        let mut current = valid.clone();
        match case {
            "memory" => current["available_memory_bytes"] = Value::Null,
            "disk" => current["build_disk_available_bytes"] = Value::Null,
            "cpu" => current["cpu_percent"] = json!(99),
            "sampled" => {
                current["sampled_at"] = json!(chrono::Utc::now() - chrono::Duration::seconds(91))
            }
            "received" => {
                current["received_at"] = json!(chrono::Utc::now() - chrono::Duration::seconds(91))
            }
            "startup" => current["startup_id"] = json!(Uuid::new_v4()),
            "digest" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE nodes SET metadata_digest=$2 WHERE id=$1",
                    params![node, "b".repeat(64)],
                )
                .await
                .unwrap();
            }
            "project" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE projects SET pool_ids='{}' WHERE id=$1",
                    params![project],
                )
                .await
                .unwrap();
            }
            "drain" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE nodes SET draining=true WHERE id=$1",
                    params![node],
                )
                .await
                .unwrap();
            }
            "ota" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE nodes SET ota_draining=true WHERE id=$1",
                    params![node],
                )
                .await
                .unwrap();
            }
            "offline" => {
                sql::execute(&mut database.pool.clone(),"UPDATE nodes SET last_seen=clock_timestamp()-interval '31 seconds' WHERE id=$1",params![node]).await.unwrap();
            }
            "tools" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE nodes SET capabilities='[\"git\",\"bash\"]' WHERE id=$1",
                    params![node],
                )
                .await
                .unwrap();
            }
            "cancel" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE builds SET cancel_requested=true WHERE id=$1",
                    params![build],
                )
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        *resources.write().await = vec![current];
        scheduler::schedule(&database.state).await.unwrap();
        let count: i64 = sql::scalar(
            &mut database.pool.clone(),
            "SELECT count(*) FROM attempts WHERE job_id=$1",
            params![job],
        )
        .await
        .unwrap();
        assert_eq!(count, 0, "must reject {case}");
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE builds SET cancel_requested=true WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
    }
    assert_ne!(startup, Uuid::nil());
    server.abort();
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn three_ranked_schedulers_preserve_the_node_slot_limit() {
    let mut database = Database::with_tenant().await;
    let certificates = Certificates::new();
    let (node, _, resource) = placement_node(&database).await;
    let (_directory, server) = resource_server(
        &mut database,
        &certificates,
        Arc::new(RwLock::new(vec![resource])),
    )
    .await;
    for _ in 0..3 {
        let (_, build, job) = database.fixture().await;
        rank(&database, build, job, &[node]).await;
    }
    let first = database.state.clone();
    let second = AppState {
        coordination: Default::default(),
        ..first.clone()
    };
    let third = AppState {
        coordination: Default::default(),
        ..first.clone()
    };
    let (a, b, c) = tokio::join!(
        scheduler::schedule(&first),
        scheduler::schedule(&second),
        scheduler::schedule(&third)
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    let assigned: i64 = sql::scalar(
        &mut database.pool.clone(),
        "SELECT count(*) FROM attempts WHERE node_id=$1",
        params![node],
    )
    .await
    .unwrap();
    assert_eq!(
        assigned, 1,
        "ranked allocation must hold the node lock through slot revalidation"
    );
    server.abort();
    database.close().await;
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn placement_results_reject_cancellation_expiry_versions_and_running_targets() {
    let database = Database::with_tenant().await;
    let (_, build, job) = database.fixture().await;
    let (node, _, _) = placement_node(&database).await;
    let certificates = Certificates::new();
    let replica = Replica::start(database.state.clone(), &certificates).await;
    let (mut client, initial, _) = planner(&database, &replica, &certificates, build).await;
    rank(&database, build, job, &[node]).await;
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE builds SET status='running' WHERE id=$1",
        params![build],
    )
    .await
    .unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE planning_runs SET state='failed' WHERE id=$1",
        params![initial],
    )
    .await
    .unwrap();
    let run = Uuid::new_v4();
    sql::execute(&mut database.pool.clone(),"INSERT INTO planning_runs(id,build_id,worker_id,purpose,state,model_config,lease_hash,lease_expires_at,deadline_at) SELECT $1,build_id,worker_id,'placement','leased','{}',$3,now()+interval '1 minute',now()+interval '1 minute' FROM planning_runs WHERE id=$2",params![run,initial,crypto::hash("lease")]).await.unwrap();
    sql::execute(
        &mut database.pool.clone(),
        "INSERT INTO placement_rounds(build_id,round,run_id,versions) VALUES($1,1,$2,$3)",
        params![build, run, json!({"app":{"job_id":job,"version":1}})],
    )
    .await
    .unwrap();
    let before: Value = sql::scalar(
        &mut database.pool.clone(),
        "SELECT placement FROM job_placements WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    let args = json!({"placements":{"app":before}});
    for case in ["cancel", "lease", "deadline"] {
        match case {
            "cancel" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE builds SET cancel_requested=true WHERE id=$1",
                    params![build],
                )
                .await
                .unwrap();
            }
            "lease" => {
                sql::execute(&mut database.pool.clone(),"UPDATE planning_runs SET lease_expires_at=now()-interval '1 second' WHERE id=$1",params![run]).await.unwrap();
            }
            "deadline" => {
                sql::execute(
                    &mut database.pool.clone(),
                    "UPDATE planning_runs SET deadline_at=now()-interval '1 second' WHERE id=$1",
                    params![run],
                )
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        let rejected = client
            .tool(pb::PlannerToolRequest {
                run_id: run.to_string(),
                lease_token: "lease".into(),
                name: "submit_placements".into(),
                arguments_json: args.to_string(),
            })
            .await
            .unwrap_err();
        assert_eq!(rejected.code(), tonic::Code::FailedPrecondition, "{case}");
        sql::execute(
            &mut database.pool.clone(),
            "UPDATE builds SET cancel_requested=false WHERE id=$1",
            params![build],
        )
        .await
        .unwrap();
        sql::execute(&mut database.pool.clone(),"UPDATE planning_runs SET lease_expires_at=now()+interval '1 minute',deadline_at=now()+interval '1 minute' WHERE id=$1",params![run]).await.unwrap();
    }
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE job_placements SET version=2 WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    let rejected = node_tool(&mut client, run, "submit_placements", args.clone()).await;
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|s| s.contains("version superseded")),
        "{rejected}"
    );
    sql::execute(
        &mut database.pool.clone(),
        "UPDATE jobs SET status='running' WHERE id=$1",
        params![job],
    )
    .await
    .unwrap();
    let accepted = node_tool(&mut client, run, "submit_placements", args).await;
    assert_eq!(accepted["updated_targets"], 0);
    let after: Value = sql::scalar(
        &mut database.pool.clone(),
        "SELECT placement FROM job_placements WHERE job_id=$1",
        params![job],
    )
    .await
    .unwrap();
    assert_eq!(before, after, "a late result cannot change running targets");
    replica.stop().await;
    database.close().await;
}
