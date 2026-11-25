//! [INPUT]: 依赖真实业务数据库、轻量解析和 Coordinator 持久事件。
//! [OUTPUT]: 验证清单事务、迟到历史、乱序重放与锁等待取消围栏。
//! [POS]: 两阶段源码接纳的数据库回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use serde_json::Value;

async fn pending(db: &Database) -> Result<(Uuid, Uuid, SourceManifest)> {
    let (build, manifest) = db.task().await?;
    db.pin(build).await?;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE builds SET source_preparation_version=2 WHERE id=$1",
        params![build],
    )
    .await?;
    let result = json!({"commit_sha":"a".repeat(40)});
    db.store
        .commit_source_task(build, &execution(), &result)
        .await?;
    db.store
        .commit_source_task(build, &execution(), &result)
        .await?;
    ensure!(
        db.counts(build).await? == (0, 1),
        "lightweight resolve created source or duplicate run"
    );
    let status: String = sql::scalar(
        &mut db.pool.clone(),
        "SELECT status FROM builds WHERE id=$1",
        params![build],
    )
    .await?;
    ensure!(
        status == "resolving",
        "AI deadline consumed before source ready"
    );
    let run: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM planning_runs WHERE build_id=$1",
        params![build],
    )
    .await?;
    let material = db.store.build_source_material(&[7; 32], build).await?;
    ensure!(
        material.manifest.is_none() && material.pinned_ref_oid == Some("a".repeat(40)),
        "pending material changed root"
    );
    let intent = db.store.planning_intent_context(run).await?;
    ensure!(
        intent.source_resolution.is_some() && intent.source_manifest_id.is_none(),
        "pending initial run lost resolution"
    );
    Ok((build, run, manifest))
}
fn pod(run: Uuid, generation: i64) -> Value {
    json!({"run_id":run,"generation":generation,"uid":format!("pod-{generation}"),"name":format!("pod-{generation}"),"namespace":"test"})
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn preparation_manifest_is_atomic_replayable_and_progress_survives_reordered_delivery()
-> Result<()> {
    let db = Database::new().await?;
    let result=async {
  let (build,run,manifest)=pending(&db).await?;let ex=pod(run,1);let event=Uuid::new_v4();let payload=json!({"manifest":manifest,"execution":ex});
  let commit=|| db.store.commit_preparation_event(event,run,"fixed-event","source_ready",&payload);
  let (a,b)=tokio::join!(commit(),commit());ensure!(a?==b?,"concurrent receipt changed");ensure!(db.counts(build).await?==(1,1),"source ready duplicated manifest or run");
  ensure!(db.store.commit_preparation_event(event,run,"changed-event","source_ready",&payload).await.is_err(),"event ID rebound");
  let state:String=sql::scalar(&mut db.pool.clone(),"SELECT status FROM builds WHERE id=$1",params![build]).await?;ensure!(state=="planning","source binding did not advance build");
  let at=chrono::Utc::now();
  db.store.commit_preparation_event(Uuid::new_v4(),run,"ready","source_local_ready",&json!({"execution":ex,"source_manifest_id":manifest.id,"source_digest":manifest.digest,"ready_at":at})).await?;
  db.store.commit_preparation_event(Uuid::new_v4(),run,"started","planning_started",&json!({"execution":ex,"started_at":at})).await?;
  db.store.commit_preparation_event(Uuid::new_v4(),run,"progress","source_progress",&json!({"execution":ex,"created_at":at,"progress":{"sequence":1,"stage":"verify","state":"started"}})).await?;
  let summary:Value=sql::scalar(&mut db.pool.clone(),"SELECT source_preparation FROM planning_runs WHERE id=$1",params![run]).await?;ensure!(summary["current_stage"]=="ready"&&summary["ready_at"].is_string()&&summary["started_at"].is_string(),"late progress erased readiness");
  db.store.commit_preparation_event(Uuid::new_v4(),run,"new","source_progress",&json!({"execution":pod(run,2),"created_at":at,"progress":{"sequence":1,"stage":"fetch","state":"started"}})).await?;
  let stale=db.store.commit_preparation_event(Uuid::new_v4(),run,"stale","source_local_ready",&json!({"execution":ex,"source_manifest_id":manifest.id,"source_digest":manifest.digest,"ready_at":at})).await?;ensure!(stale["accepted"]==true&&stale["historical"]==true,"old workspace fact was not retained as history");
  let summary:Value=sql::scalar(&mut db.pool.clone(),"SELECT source_preparation FROM planning_runs WHERE id=$1",params![run]).await?;ensure!(summary["generation"]==2&&summary["ready_at"].is_null(),"new generation inherited readiness");
  Ok(())
 }.await;
    db.close().await?;
    result
}
#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn canceled_preparation_cannot_bind_manifest_and_resolving_failure_finishes_build()
-> Result<()> {
    let db = Database::new().await?;
    let result=async {
  for canceled in [false,true] {
   let (build,run,manifest)=pending(&db).await?;
   if canceled {db.store.cancel_build(build).await?;let receipt=db.store.commit_preparation_event(Uuid::new_v4(),run,"canceled","source_ready",&json!({"manifest":manifest,"execution":pod(run,1)})).await?;ensure!(receipt["accepted"]==false,"canceled build revived");}
   else {let payload=json!({"generation":1,"error":"Source fetch failed","diagnostic":{"stage":"fetch","code":"network"},"execution":pod(run,1)});
    // Use the fixed diagnostic vocabulary rather than raw Git messages.
    let mut payload=payload;payload["diagnostic"]["code"]=json!(artifactory_source_checkout::GitFailureKind::Connection);
    db.store.commit_planning_event(super::super::planning_bridge::PlanningEventCommit{event:Uuid::new_v4(),run,build,hash:"preparation-failure".into(),kind:"failed",payload:&payload,validation:None}, |_|async{panic!("source failure cannot create plan")}).await?;
    let row:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',b.status,'diagnostic',r.source_preparation->'diagnostic') FROM builds b JOIN planning_runs r ON r.build_id=b.id WHERE b.id=$1",params![build]).await?;ensure!(row["status"]=="failed"&&row["diagnostic"]["stage"]=="fetch","preparation failure lost safe diagnosis");
   }
   ensure!(db.counts(build).await?.0==0,"stopped preparation wrote source");
  }Ok(())
 }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn preparation_retains_out_of_order_progress_and_rejects_changed_replays() -> Result<()> {
    let db = Database::new().await?;
    let result=async {
        let (_,run,_)=pending(&db).await?;
        let at=chrono::Utc::now();
        let first=json!({"execution":pod(run,1),"created_at":at,"progress":{"sequence":1,"stage":"fetch","state":"started"}});
        let second=json!({"execution":pod(run,1),"created_at":at,"progress":{"sequence":2,"stage":"fetch","state":"completed"}});
        for payload in [&second,&first,&first] {
            assert_eq!(db.store.commit_preparation_event(Uuid::new_v4(),run,"ordered-progress","source_progress",payload).await?["accepted"],true);
        }
        let row:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('summary',source_preparation,'events',source_preparation_events) FROM planning_runs WHERE id=$1",params![run]).await?;
        assert_eq!(row["summary"]["sequence"],2);
        assert_eq!(row["summary"]["state"],"completed");
        assert_eq!(row["events"].as_array().unwrap().len(),2);
        assert_eq!(row["events"][0]["progress"]["sequence"],1);
        assert_eq!(row["events"][1]["progress"]["sequence"],2);
        let mut changed=first.clone();changed["progress"]["state"]=json!("completed");
        assert!(db.store.commit_preparation_event(Uuid::new_v4(),run,"conflict","source_progress",&changed).await.is_err());
        for field in ["uid","name","namespace"] {
            let mut changed=first.clone();changed["execution"][field]=json!("replacement");
            changed["progress"]["sequence"]=json!(3);
            assert!(db.store.commit_preparation_event(Uuid::new_v4(),run,"wrong-pod","source_progress",&changed).await.is_err(),"{field}");
        }
        let next=json!({"execution":pod(run,2),"created_at":at,"progress":{"sequence":1,"stage":"fetch","state":"started"}});
        db.store.commit_preparation_event(Uuid::new_v4(),run,"next-generation","source_progress",&next).await?;
        let previous=json!({"execution":pod(run,1),"created_at":at,"progress":{"sequence":3,"stage":"submodules","state":"started"}});
        let receipt=db.store.commit_preparation_event(Uuid::new_v4(),run,"previous-history","source_progress",&previous).await?;
        assert_eq!(receipt["historical"],true);
        let row:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('summary',source_preparation,'events',source_preparation_events) FROM planning_runs WHERE id=$1",params![run]).await?;
        assert_eq!(row["summary"]["generation"],2);
        assert_eq!(row["summary"]["sequence"],1);
        assert_eq!(row["events"].as_array().unwrap().len(),4);
        Ok(())
    }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn preparation_terminal_receipts_keep_earlier_facts_without_changing_business_state()
-> Result<()> {
    let db = Database::new().await?;
    let result=async {
        for terminal in ["failed","canceled"] {
            let (build,run,manifest)=pending(&db).await?;
            let ex=pod(run,1);
            db.store.commit_preparation_event(Uuid::new_v4(),run,"fixed-manifest","source_ready",&json!({"execution":ex,"manifest":manifest})).await?;
            let at=chrono::Utc::now();
            sql::execute(&mut db.pool.clone(),"UPDATE builds SET status=$2,cancel_requested=$3,finished_at=clock_timestamp() WHERE id=$1",params![build,terminal,terminal=="canceled"]).await?;
            sql::execute(&mut db.pool.clone(),"UPDATE planning_runs SET state=$2,finished_at=clock_timestamp() WHERE id=$1",params![run,terminal]).await?;
            for (kind,payload) in [
                ("planning_started",json!({"execution":ex,"started_at":at})),
                ("source_local_ready",json!({"execution":ex,"ready_at":at,"source_manifest_id":manifest.id,"source_digest":manifest.digest})),
                ("source_progress",json!({"execution":ex,"created_at":at,"progress":{"sequence":1,"stage":"verify","state":"completed"}})),
            ] {
                assert_eq!(db.store.commit_preparation_event(Uuid::new_v4(),run,"late-fact",kind,&payload).await?["accepted"],true);
            }
            let row:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('build',b.status,'run',r.state,'summary',r.source_preparation,'events',r.source_preparation_events) FROM builds b JOIN planning_runs r ON r.build_id=b.id WHERE r.id=$1",params![run]).await?;
            assert_eq!(row["build"],terminal);assert_eq!(row["run"],terminal);
            assert!(row["summary"]["ready_at"].is_string());assert!(row["summary"]["started_at"].is_string());
            assert_eq!(row["events"].as_array().unwrap().len(),3);
            let receipt=db.store.commit_preparation_event(Uuid::new_v4(),run,"late-bind","source_ready",&json!({"execution":ex,"manifest":manifest})).await?;
            assert_eq!(receipt["accepted"],false);
            let future=chrono::Utc::now()+chrono::Duration::seconds(1);
            assert!(db.store.commit_preparation_event(Uuid::new_v4(),run,"after-completion","source_progress",&json!({"execution":ex,"created_at":future,"progress":{"sequence":2,"stage":"ready","state":"completed"}})).await.is_err());
            let wrong_digest=json!({"execution":ex,"ready_at":at,"source_manifest_id":manifest.id,"source_digest":"b".repeat(64)});
            assert!(db.store.commit_preparation_event(Uuid::new_v4(),run,"wrong-fixed-source","source_local_ready",&wrong_digest).await.is_err());
        }
        Ok(())
    }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; verifies terminal observation fences after lock waits"]
async fn preparation_facts_during_cancel_lock_wait_remain_observable() -> Result<()> {
    let db = Database::new().await?;
    let result = async {
        let (build, run, manifest) = pending(&db).await?;
        let ex = pod(run, 1);
        db.store.commit_preparation_event(Uuid::new_v4(), run, "manifest-before-cancel", "source_ready", &json!({"execution":ex,"manifest":manifest})).await?;
        let mut connection = db.pool.clone();
        let mut holder = connection.transaction().await?;
        let pid: i32 = sql::scalar(&mut holder, "SELECT pg_backend_pid()", params![]).await?;
        sql::execute(&mut holder, "SELECT id FROM builds WHERE id=$1 FOR UPDATE", params![build]).await?;
        let store = db.store.clone();
        let cancel = tokio::spawn(async move { store.cancel_build(build).await });
        let waiting = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let blocked: bool = sql::scalar(&mut db.pool.clone(), "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))", params![pid]).await?;
                if blocked {
                    return Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await;
        let at: chrono::DateTime<chrono::Utc> = sql::scalar(&mut db.pool.clone(), "SELECT clock_timestamp()", params![]).await?;
        holder.commit().await?;
        waiting.context("cancel did not wait on Build lock")??;
        tokio::time::timeout(std::time::Duration::from_secs(5), cancel).await???;
        for (kind, payload) in [
            ("source_progress", json!({"execution":ex,"created_at":at,"progress":{"sequence":1,"stage":"verify","state":"completed"}})),
            ("source_local_ready", json!({"execution":ex,"ready_at":at,"source_manifest_id":manifest.id,"source_digest":manifest.digest})),
            ("planning_started", json!({"execution":ex,"started_at":at})),
        ] {
            let receipt = db.store.commit_preparation_event(Uuid::new_v4(), run, "fact-during-lock-wait", kind, &payload).await?;
            assert_eq!(receipt["accepted"], true);
        }
        let state: Value = sql::scalar(&mut db.pool.clone(), "SELECT jsonb_build_object('build',b.status,'run',r.state,'cancel_requested',b.cancel_requested,'events',jsonb_array_length(r.source_preparation_events)) FROM builds b JOIN planning_runs r ON r.build_id=b.id WHERE r.id=$1", params![run]).await?;
        assert_eq!(state, json!({"build":"canceled","run":"canceled","cancel_requested":true,"events":3}));
        Ok(())
    }.await;
    let cleanup = db.close().await;
    result.and(cleanup)
}
