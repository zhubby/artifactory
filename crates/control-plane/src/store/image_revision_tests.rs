//! [INPUT]: 依赖隔离数据库、真实计划提交与镜像报告。
//! [OUTPUT]: 验证镜像失败修订、固定源码、成功制品和修订预算。
//! [POS]: 两类执行器混合构建的自动修订回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Database, PlanningEventCommit, Result, Uuid, Value, json, params, sql};

async fn mixed_image_fixture(db: &Database) -> Result<(Uuid, Uuid, Uuid, Uuid, Value)> {
    let (_, build, run, source) = db.planning_fixture().await?;
    let plan = json!({"summary":"build both targets","evidence":[{"path":"Dockerfile","reason":"image definition"}],"recipe":{"version":1,"targets":{
        "native":{"os":"linux","arch":"amd64","application_platform":null,"steps":[{"name":"compile","run":"make"}],"artifacts":["dist/app"]},
        "image":{"executor":"buildkit","os":"linux","arch":"amd64","application_platform":null,"image":{"name":"image","platforms":["linux/amd64"]}}
    }}});
    let payload =
        json!({"kind":"submit_plan","deadline_ms":chrono::Utc::now().timestamp_millis()+60_000});
    let accepted = db
        .store
        .commit_planning_event(
            PlanningEventCommit {
                event: Uuid::new_v4(),
                run,
                build,
                hash: "mixed-initial".into(),
                kind: "proposal",
                payload: &payload,
                validation: None,
            },
            |_| async { crate::execution_plan::ExecutionPlan::parse(plan.clone()) },
        )
        .await?;
    anyhow::ensure!(
        accepted["accepted"] == true,
        "initial plan rejected: {accepted}"
    );
    let native: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM jobs WHERE build_id=$1 AND target='native'",
        params![build],
    )
    .await?;
    let image: Uuid = sql::scalar(
        &mut db.pool.clone(),
        "SELECT id FROM jobs WHERE build_id=$1 AND target='image'",
        params![build],
    )
    .await?;
    let node = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let artifact = Uuid::new_v4();
    sql::execute(&mut db.pool.clone(),"INSERT INTO nodes(id,name,os,arch,pool_id,certificate_hash,certificate_expires_at) VALUES($1,'completed-native','linux','amd64','00000000-0000-0000-0000-000000000001',$2,now()+interval '1 day')",params![node,node.to_string()]).await?;
    sql::execute(&mut db.pool.clone(),"INSERT INTO attempts(id,job_id,node_id,lease_token,lease_expires_at,status,command_id,plan_revision_id,source_manifest_id) SELECT $1,$2,$3,'fixture',now(),'succeeded',gen_random_uuid(),plan_revision_id,$4 FROM jobs WHERE id=$2",params![attempt,native,node,source]).await?;
    sql::execute(&mut db.pool.clone(),"INSERT INTO uploads(id,attempt_id,path,object_key,expected_size,expected_sha256,expires_at) VALUES($1,$2,'dist/app',$4,42,$3,now()+interval '1 hour')",params![artifact,attempt,"b".repeat(64),format!("uploads/{artifact}")]).await?;
    sql::execute(&mut db.pool.clone(),"INSERT INTO artifacts(id,attempt_id,path,object_key,size,sha256) VALUES($1,$2,'dist/app',$4,42,$3)",params![artifact,attempt,"b".repeat(64),format!("artifacts/{artifact}")]).await?;
    sql::execute(
        &mut db.pool.clone(),
        "UPDATE jobs SET status='succeeded' WHERE id=$1",
        params![native],
    )
    .await?;
    Ok((build, source, native, image, plan))
}

async fn successful_target(db: &Database, job: Uuid) -> Result<Value> {
    Ok(sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('job',to_jsonb(j),'attempts',(SELECT jsonb_agg(a ORDER BY a.id) FROM attempts a WHERE a.job_id=j.id),'artifacts',(SELECT jsonb_agg(x ORDER BY x.id) FROM artifacts x JOIN attempts a ON a.id=x.attempt_id WHERE a.job_id=j.id)) FROM jobs j WHERE j.id=$1",params![job]).await?)
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_execution_failures_revise_with_logs_and_preserve_successful_targets() -> Result<()> {
    use artifactory_image_protocol::Report;
    let db = Database::new().await?;
    let result=async {
        let (build,source,native,image,plan)=mixed_image_fixture(&db).await?;
        let successful=successful_target(&db,native).await?;
        for revision in 1..=2_i32 {
            let execution=Uuid::new_v4();
            anyhow::ensure!(db.store.claim_image(image,execution,"build","publish").await?,"image was not claimed");
            db.store.save_image_task(execution,"sealed").await?;
            let message=format!("Dockerfile RUN failed in revision {revision}");
            let failure:Report=serde_json::from_value(json!({"kind":"failed","message":message,"failure_kind":"execution"}))?;
            let log_key=format!("image-log-{revision}");
            let log_hash=format!("hash-{revision}");
            db.store.image_report(execution,false,&Report::Log {platform:"linux/amd64".into(),sequence:1,text:message.clone()},Some((&log_key,&log_hash))).await?;
            db.store.image_report(execution,false,&failure,None).await?;
            db.store.refresh_builds().await?;
            db.store.queue_revisions().await?;
            let state:Value=sql::scalar(&mut db.pool.clone(),"SELECT jsonb_build_object('status',status,'revision',revision_count,'source',source_manifest_id) FROM builds WHERE id=$1",params![build]).await?;
            anyhow::ensure!(state["status"]=="replanning" && state["revision"]==revision && state["source"]==source.to_string(),"image failure did not queue a fixed-source revision: {state}");
            let run:Uuid=sql::scalar(&mut db.pool.clone(),"SELECT id FROM planning_runs WHERE build_id=$1 ORDER BY created_at DESC,id DESC LIMIT 1",params![build]).await?;
            let context=db.store.planning_intent_context(run).await?.context;
            anyhow::ensure!(context["failed_targets"].as_array().is_some_and(|rows|rows.len()==1 && rows[0]["target"]=="image" && rows[0]["error"]==message && rows[0]["image_execution_id"]==execution.to_string()),"revision lost the image execution diagnostic: {}",context["failed_targets"]);
            let logs=db.store.planning_logs(build,"image").await?;
            anyhow::ensure!(logs.iter().any(|row|row.object_key==format!("image-log-{revision}")),"planning tool cannot read image failure logs");
            let payload=json!({"kind":"submit_plan","deadline_ms":chrono::Utc::now().timestamp_millis()+60_000});
            let accepted=db.store.commit_planning_event(PlanningEventCommit {event:Uuid::new_v4(),run,build,hash:format!("image-revision-{revision}"),kind:"proposal",payload:&payload,validation:None},|_|async {crate::execution_plan::ExecutionPlan::parse(plan.clone())}).await?;
            anyhow::ensure!(accepted["accepted"]==true,"image revision was rejected: {accepted}");
            let after=successful_target(&db,native).await?;
            anyhow::ensure!(after==successful,"revision changed the successful native target, execution or artifact");
        }
        let execution=Uuid::new_v4();
        anyhow::ensure!(db.store.claim_image(image,execution,"build","publish").await?,"third attempt was not claimed");
        db.store.save_image_task(execution,"sealed").await?;
        let failure:Report=serde_json::from_value(json!({"kind":"failed","message":"third Dockerfile failure","failure_kind":"execution"}))?;
        db.store.image_report(execution,false,&failure,None).await?;
        db.store.refresh_builds().await?;
        db.store.queue_revisions().await?;
        let state:(String,i32)=sql::tuple(&mut db.pool.clone(),"SELECT status,revision_count FROM builds WHERE id=$1",params![build]).await?;
        anyhow::ensure!(state==("failed".into(),2),"image exceeded the two-revision budget");
        Ok(())
    }.await;
    db.close().await?;
    result
}

#[tokio::test]
#[ignore = "requires ARTIFACTORY_TEST_DATABASE_URL"]
async fn image_external_unknown_canceled_and_stale_failures_do_not_revise() -> Result<()> {
    use artifactory_image_protocol::{FailureKind, Report};
    let db = Database::new().await?;
    let result=async {
        for scenario in ["legacy","source","authorization","artifact_transport","infrastructure","build_canceled","execution_canceled","old_source","old_plan","superseded"] {
            let (build,source,_,job,_)=mixed_image_fixture(&db).await?;
            let id=Uuid::new_v4();
            anyhow::ensure!(db.store.claim_image(job,id,"build","publish").await?);
            db.store.save_image_task(id,"sealed").await?;
            let kind=match scenario {
                "legacy"=>None,
                "source"=>Some(FailureKind::Source),
                "authorization"=>Some(FailureKind::Authorization),
                "artifact_transport"=>Some(FailureKind::ArtifactTransport),
                "infrastructure"=>Some(FailureKind::Infrastructure),
                _=>Some(FailureKind::Execution),
            };
            db.store.image_report(id,false,&Report::Failed {message:format!("controlled {scenario} failure"),failure_kind:kind},None).await?;
            db.store.refresh_builds().await?;
            match scenario {
                "build_canceled"=> {sql::execute(&mut db.pool.clone(),"UPDATE builds SET cancel_requested=true WHERE id=$1",params![build]).await?;}
                "execution_canceled"=> {sql::execute(&mut db.pool.clone(),"UPDATE image_executions SET cancel_requested=true WHERE id=$1",params![id]).await?;}
                "old_source"=> {
                    let other=Uuid::new_v4();
                    sql::execute(&mut db.pool.clone(),"INSERT INTO source_manifests(id,project_id,commit_sha,manifest,digest) SELECT $1,project_id,$2,'{}',$3 FROM source_manifests WHERE id=$4",params![other,"1".repeat(40),"c".repeat(64),source]).await?;
                    sql::execute(&mut db.pool.clone(),"UPDATE image_executions SET source_manifest_id=$2 WHERE id=$1",params![id,other]).await?;
                }
                "old_plan"=> {sql::execute(&mut db.pool.clone(),"UPDATE image_executions SET plan_revision_id=NULL WHERE id=$1",params![id]).await?;}
                "superseded"=> {
                    sql::execute(&mut db.pool.clone(),"UPDATE jobs SET status='queued' WHERE id=$1",params![job]).await?;
                    let next=Uuid::new_v4();
                    anyhow::ensure!(db.store.claim_image(job,next,"build","publish").await?);
                    db.store.save_image_task(next,"sealed").await?;
                    db.store.image_report(next,false,&Report::Failed {message:"later infrastructure failure".into(),failure_kind:Some(FailureKind::Infrastructure)},None).await?;
                }
                _=>{}
            }
            db.store.queue_revisions().await?;
            let count:i32=sql::scalar(&mut db.pool.clone(),"SELECT revision_count FROM builds WHERE id=$1",params![build]).await?;
            anyhow::ensure!(count==0,"{scenario} consumed a revision");
        }
        Ok(())
    }.await;
    db.close().await?;
    result
}
