//! [INPUT]: 依赖固定源码元数据、执行计划和业务数据库。
//! [OUTPUT]: 保存计划并查询授权节点与构建日志。
//! [POS]: Runtime 提案的事务持久化边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::ControlStore;
use crate::{crypto, execution_plan::ExecutionPlan};
use anyhow::{Result, ensure};
use artifactory_database::{params, sql};
use serde_json::{Value, json};
use toasty::stmt::Type;
use uuid::Uuid;
pub(crate) struct PlanningLog {
    pub object_key: String,
    pub step: String,
}
pub(crate) struct SubmissionContext {
    pub source_manifest_id: Uuid,
    pub parameters: Value,
    pub target_requirements: Value,
    pub planning_context: Value,
    pub selected_targets: Vec<String>,
    pub current_plan_id: Option<Uuid>,
    pub failed_targets: Vec<String>,
    pub previous: Option<Value>,
}
pub(super) async fn submission_context(
    tx: &mut toasty::db::Transaction<'_>,
    row: &sql::Record,
) -> Result<SubmissionContext> {
    let current_plan_id: Option<Uuid> = row.decode("current_plan_id")?;
    let previous = if let Some(id) = current_plan_id {
        Some(
            sql::scalar(
                tx,
                "SELECT contents FROM plan_revisions WHERE id=$1",
                params![id],
            )
            .await?,
        )
    } else {
        None
    };
    Ok(SubmissionContext {
        source_manifest_id: row.decode("source_manifest_id")?,
        parameters: row.decode("parameters")?,
        target_requirements: row.decode("target_requirements")?,
        planning_context: row.decode("planning_context")?,
        selected_targets: row.decode("selected_targets")?,
        current_plan_id,
        failed_targets: row.decode("failed_targets")?,
        previous,
    })
}
pub(super) async fn persist_plan(
    tx: &mut toasty::db::Transaction<'_>,
    row: &sql::Record,
    plan: ExecutionPlan,
) -> Result<Value> {
    let build: Uuid = row.decode("build_id")?;
    let snapshot: Uuid = row.decode("source_manifest_id")?;
    let current: Option<Uuid> = row.decode("current_plan_id")?;
    let failed: Vec<String> = row.decode("failed_targets")?;
    let plan_id = Uuid::new_v4();
    let contents = serde_json::to_value(&plan)?;
    super::models::PlanRevision::create()
        .id(plan_id)
        .build_id(build)
        .run_id(row.decode::<Uuid>("id")?)
        .revision(row.decode::<i32>("revision_count")?)
        .source_manifest_id(snapshot)
        .contents(toasty::Json(contents.clone()))
        .sha256(crypto::hash(serde_json::to_vec(&contents)?))
        .exec(tx)
        .await?;
    ensure!(
        row.decode::<String>("purpose")? != "placement",
        "placement rounds cannot change execution plans"
    );
    for (name, target) in &plan.recipe.targets {
        if current.is_none() {
            super::models::Job::create()
                .id(Uuid::new_v4())
                .build_id(build)
                .target(name)
                .target_config(toasty::Json((serde_json::to_value(target)?).clone()))
                .plan_revision_id(plan_id)
                .exec(tx)
                .await?;
        } else if failed.contains(name) {
            let changed=sql::execute(tx, "UPDATE jobs SET target_config=$3,plan_revision_id=$4,status='queued' WHERE build_id=$1 AND target=$2 AND status IN ('failed','lost')", params![build, name, serde_json::to_value(target)?, plan_id]).await?;
            ensure!(changed == 1, "target is no longer eligible for revision");
        }
    }
    for (name, placement) in &plan.placements {
        sql::execute(tx,"INSERT INTO job_placements(job_id,placement) SELECT id,$3 FROM jobs WHERE build_id=$1 AND target=$2 AND status='queued' ON CONFLICT(job_id) DO UPDATE SET placement=excluded.placement,version=job_placements.version+1,waiting_since=NULL,reason=NULL",params![build,name,serde_json::to_value(placement)?]).await?;
    }
    sql::execute(tx, "UPDATE builds SET current_plan_id=$2,config_snapshot=$3,status='queued',error=NULL,finished_at=NULL WHERE id=$1", params![build, plan_id, serde_json::to_value(&plan.recipe)?]).await?;
    sql::execute(
        tx,
        "UPDATE planning_runs SET state='succeeded',finished_at=now() WHERE id=$1",
        params![row.decode::<Uuid>("id")?],
    )
    .await?;
    Ok(json!({"accepted":true,"plan_revision_id":plan_id}))
}
impl ControlStore {
    pub(crate) async fn planning_nodes(&self, build: Uuid) -> Result<Vec<Value>> {
        let groups: Vec<Value> = sql::scalars(
            &mut self.pool.clone(),
            r#"WITH authorized AS (
                SELECT n.pool_id,n.os,n.arch,n.labels,n.capabilities,n.name,n.slots,
                    ARRAY(SELECT s FROM unnest(n.signing_profiles) s
                          WHERE s=ANY(p.signing_profiles)
                            AND b.planning_context->'signing_profiles' ? s) AS signing_profiles,
                    CASE WHEN n.startup_id IS NULL THEN n.diagnostics->'tools'
                         ELSE (SELECT coalesce(jsonb_object_agg(substr(key,7),value),'{}')
                               FROM jsonb_each(n.metadata) WHERE starts_with(key,'tools.'))
                    END AS tools,
                    coalesce(n.last_seen>clock_timestamp()-interval '30 seconds'
                        AND n.active_session IS NOT NULL AND n.connection_owner IS NOT NULL
                        AND n.certificate_expires_at>clock_timestamp(),false) AS online,
                    n.draining OR n.ota_draining AS draining,
                    (SELECT count(*) FROM attempts a WHERE a.node_id=n.id
                        AND a.status IN ('leased','running')
                        AND a.lease_expires_at>clock_timestamp()) AS active_tasks
                FROM builds b JOIN projects p ON p.id=b.project_id
                JOIN nodes n ON build_node_allowed(b.id,n.id)
                WHERE b.id=$1 AND NOT n.revoked
            ), groups AS (
                SELECT pool_id,os,arch,labels,capabilities,signing_profiles,tools,
                    min(name) AS name,count(*) AS nodes,sum(slots) AS task_slots,
                    sum(active_tasks) AS active_tasks,
                    coalesce(sum(greatest(slots-active_tasks,0))
                        FILTER (WHERE online AND NOT draining),0) AS available_slots,
                    bool_or(online) AS online,count(*) FILTER (WHERE online) AS online_count,
                    count(*) FILTER (WHERE draining) AS draining_count
                FROM authorized
                GROUP BY pool_id,os,arch,labels,capabilities,signing_profiles,tools
            ) SELECT jsonb_build_object(
                'name',name,'pool_id',pool_id,'os',os,'arch',arch,'labels',labels,
                'capabilities',capabilities,'signing_profiles',signing_profiles,'tools',tools,
                'count',nodes,'task_slots',task_slots,'active_tasks',active_tasks,
                'available_slots',available_slots,'online',online,'online_count',online_count,
                'draining_count',draining_count,'summary',true,
                'total_groups',count(*) OVER (),'truncated',count(*) OVER ()>128)
            FROM groups ORDER BY nodes DESC,pool_id,os,arch,labels::text,
                capabilities::text,signing_profiles,tools::text LIMIT 128"#,
            params![build],
        )
        .await?;
        // Keep room for source evidence and tool history in both planning protocols.
        const MAX_SUMMARY_BYTES: usize = 128 * 1024 - 4096;
        const MAX_GROUP_BYTES: usize = 8 * 1024;
        let mut summary = Vec::with_capacity(groups.len());
        let mut bytes = 2;
        let mut truncated = false;
        for mut group in groups {
            if serde_json::to_vec(&group)?.len() > MAX_GROUP_BYTES {
                for field in ["labels", "capabilities", "signing_profiles", "tools"] {
                    group
                        .as_object_mut()
                        .expect("database returns a JSON object")
                        .remove(field);
                }
                group["details_omitted"] = json!(true);
                truncated = true;
            }
            let size = serde_json::to_vec(&group)?.len() + 1;
            if bytes + size > MAX_SUMMARY_BYTES {
                truncated = true;
                break;
            }
            bytes += size;
            summary.push(group);
        }
        if truncated {
            for group in &mut summary {
                group["truncated"] = json!(true);
            }
        }
        Ok(summary)
    }

    pub(crate) async fn planning_logs(
        &self,
        build: Uuid,
        target: &str,
    ) -> Result<Vec<PlanningLog>> {
        let rows=sql::records(&mut self.pool.clone(), "SELECT l.object_key,l.step FROM log_chunks l JOIN attempts a ON a.id=l.attempt_id JOIN jobs j ON j.id=a.job_id WHERE j.build_id=$1 AND ($2='' OR j.target=$2) ORDER BY l.id DESC LIMIT 24", params![build, target], &[("object_key", Type::String),("step", Type::String)]).await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                Ok::<_, artifactory_database::Error>(PlanningLog {
                    object_key: row.decode("object_key")?,
                    step: row.decode("step")?,
                })
            })
            .collect::<artifactory_database::Result<_>>()?)
    }
}
