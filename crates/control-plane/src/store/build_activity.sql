-- [INPUT]: 依赖受权构建、源码阶段及既有规划、设备和归档历史。
-- [OUTPUT]: 投影有界过程事件与不含秘密的源码摘要。
-- [POS]: 只读取业务事实；不重建已丢失的历史日志。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
WITH build AS MATERIALIZED (
    SELECT id,status,created_at,finished_at,source_manifest_id FROM builds WHERE id=$1 AND tenant_id=$2
), source AS MATERIALIZED (
    SELECT s.id,s.status,s.created_at,s.progress,s.source_execution,s.result,s.pinned_ref_oid,
           coalesce(m.created_at,CASE WHEN s.status IN ('failed','canceled') THEN b.finished_at END) AS finished_at
    FROM source_tasks s JOIN build b ON s.build_id=b.id
    LEFT JOIN source_manifests m ON m.id=b.source_manifest_id
), runs AS MATERIALIZED (
    SELECT r.id,r.state,r.purpose,r.calls,r.created_at,r.finished_at,r.model_config->>'model' AS model
    FROM planning_runs r JOIN build b ON r.build_id=b.id
), jobs AS MATERIALIZED (
    SELECT j.id,j.target,j.created_at FROM jobs j JOIN build b ON j.build_id=b.id
), attempts AS MATERIALIZED (
    SELECT a.id,a.created_at,a.started_at,a.finished_at,a.status,a.plan_revision_id,j.target,n.name AS node
    FROM attempts a JOIN jobs j ON a.job_id=j.id JOIN nodes n ON a.node_id=n.id
), images AS MATERIALIZED (
    SELECT e.id,e.created_at,e.finished_at,e.status,e.plan_revision_id,j.target
    FROM image_executions e JOIN jobs j ON e.job_id=j.id WHERE e.operation='build'
), events AS (
    SELECT 'build/created' AS id,'source' AS stage,'created' AS action,'completed' AS status,b.created_at AS at,
           jsonb_build_object('actor','control') AS details FROM build b
    UNION ALL
    SELECT 'source/queued','source','source_queued','completed',s.created_at,jsonb_build_object('actor','control') FROM source s
    UNION ALL
    SELECT 'source/'||(e->>'attempt')||'/'||(e->>'sequence'),'source','source_'||(e->>'stage'),
           CASE WHEN e->>'state'='completed' THEN 'succeeded' ELSE 'running' END,(e->>'created_at')::timestamptz,
           jsonb_build_object('actor',e->>'pod','attempt',e->'attempt','sequence',e->'sequence')
    FROM source s CROSS JOIN LATERAL jsonb_array_elements(s.progress) e
    UNION ALL
    SELECT 'source/finished','source','source_finished',s.status,s.finished_at,
           jsonb_build_object('actor','source','commit_sha',s.pinned_ref_oid,
               'code',s.result#>'{diagnostic,code}','source_stage',s.result#>'{diagnostic,stage}')
    FROM source s WHERE s.finished_at IS NOT NULL
    UNION ALL
    SELECT 'planning/'||r.id||'/created','planning','planning_queued','queued',r.created_at,
           jsonb_build_object('actor','planner','purpose',r.purpose,'model',r.model,'run_id',r.id) FROM runs r
    UNION ALL
    SELECT 'planning/'||r.id||'/finished','planning','planning_finished',r.state,r.finished_at,
           jsonb_build_object('actor','planner','purpose',r.purpose,'model',r.model,'calls',r.calls,'run_id',r.id)
    FROM runs r WHERE r.finished_at IS NOT NULL
    UNION ALL
    SELECT 'plan/'||p.id,'planning','plan_accepted','succeeded',p.created_at,
           jsonb_build_object('actor','control','revision',p.revision,'plan_id',p.id)
    FROM plan_revisions p JOIN build b ON p.build_id=b.id
    UNION ALL
    SELECT 'job/'||j.id,'dispatch','target_queued','queued',j.created_at,
           jsonb_build_object('actor','scheduler','target',j.target) FROM jobs j
    UNION ALL
    SELECT 'attempt/'||a.id||'/assigned','dispatch','node_assigned','succeeded',a.created_at,
           jsonb_build_object('actor',a.node,'target',a.target,'attempt_id',a.id,'plan_id',a.plan_revision_id) FROM attempts a
    UNION ALL
    SELECT 'attempt/'||a.id||'/started','execution','execution_started','running',a.started_at,
           jsonb_build_object('actor',a.node,'target',a.target,'attempt_id',a.id,'plan_id',a.plan_revision_id)
    FROM attempts a WHERE a.started_at IS NOT NULL
    UNION ALL
    SELECT 'attempt/'||a.id||'/finished','execution','execution_finished',a.status,a.finished_at,
           jsonb_build_object('actor',a.node,'target',a.target,'attempt_id',a.id,'plan_id',a.plan_revision_id)
    FROM attempts a WHERE a.finished_at IS NOT NULL
    UNION ALL
    SELECT 'step/'||a.id||'/'||s.step_index||'/started','execution','step_started','running',s.started_at,
           jsonb_build_object('actor',a.node,'target',a.target,'step',s.name,'phase',s.phase,'attempt_id',a.id,'plan_id',a.plan_revision_id)
    FROM step_runs s JOIN attempts a ON s.attempt_id=a.id
    UNION ALL
    SELECT 'step/'||a.id||'/'||s.step_index||'/finished','execution','step_finished',s.status,s.finished_at,
           jsonb_build_object('actor',a.node,'target',a.target,'step',s.name,'phase',s.phase,'exit_code',s.exit_code,'attempt_id',a.id,'plan_id',a.plan_revision_id)
    FROM step_runs s JOIN attempts a ON s.attempt_id=a.id WHERE s.finished_at IS NOT NULL
    UNION ALL
    SELECT 'image/'||e.id||'/created','dispatch','image_preparing','queued',e.created_at,
           jsonb_build_object('actor','buildkit','target',e.target,'execution_id',e.id,'plan_id',e.plan_revision_id) FROM images e
    UNION ALL
    SELECT 'image/'||e.id||'/finished','execution','image_finished',e.status,e.finished_at,
           jsonb_build_object('actor','buildkit','target',e.target,'execution_id',e.id,'plan_id',e.plan_revision_id)
    FROM images e WHERE e.finished_at IS NOT NULL
    UNION ALL
    SELECT 'artifact/'||f.id,'artifacts','artifact_registered','succeeded',f.created_at,
           jsonb_build_object('actor','storage','target',a.target,'path',f.path,'sha256',f.sha256,'attempt_id',a.id)
    FROM artifacts f JOIN attempts a ON f.attempt_id=a.id
    UNION ALL
    SELECT 'image-artifact/'||f.id,'artifacts','image_registered','succeeded',f.created_at,
           jsonb_build_object('actor','registry','target',e.target,'digest',f.digest,'execution_id',e.id)
    FROM image_artifacts f JOIN images e ON f.execution_id=e.id
    UNION ALL
    SELECT 'build/finished','build','build_finished',b.status,b.finished_at,
           jsonb_build_object('actor','control') FROM build b WHERE b.finished_at IS NOT NULL
), limited AS MATERIALIZED (
    SELECT * FROM events ORDER BY at DESC,id DESC LIMIT 501
), displayed AS (
    SELECT * FROM limited ORDER BY at DESC,id DESC LIMIT 500
)
SELECT jsonb_build_object(
    'build_id',b.id,
    'generated_at',clock_timestamp(),
    'source',(SELECT jsonb_build_object('status',s.status,'created_at',s.created_at,'finished_at',s.finished_at,
        'attempt',s.source_execution->'attempt','pod',s.source_execution->'name',
        'history_available',jsonb_array_length(s.progress)>0,
        'diagnostic',CASE WHEN s.result->'diagnostic' IS NOT NULL THEN jsonb_build_object(
            'stage',s.result#>'{diagnostic,stage}','code',s.result#>'{diagnostic,code}') END) FROM source s),
    'events',coalesce((SELECT jsonb_agg(jsonb_build_object('id',id,'stage',stage,'action',action,'status',status,
        'created_at',at,'details',details) ORDER BY at,id) FROM displayed),'[]'::jsonb),
    'truncated',(SELECT count(*)>500 FROM limited)
) FROM build b
