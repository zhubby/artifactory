//! [INPUT]: 依赖构建目标、最新原生尝试与当前镜像执行。
//! [OUTPUT]: 提供调度和规划共用的失败诊断查询。
//! [POS]: 两类执行器进入自动修订的统一读取边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md

// 镜像先取最新执行，再核对固定源码与计划；旧执行不能触发新一轮修订。
pub(super) const LATEST_FAILURES: &str = "
    SELECT j.build_id,j.target,j.status AS job_status,a.status,a.error,a.failure_kind,
        a.id AS attempt_id,NULL::uuid AS image_execution_id
    FROM jobs j
    JOIN LATERAL (
        SELECT id,status,error,failure_kind FROM attempts WHERE job_id=j.id
        ORDER BY created_at DESC,id DESC LIMIT 1
    ) a ON true
    WHERE j.status IN ('failed','lost') AND j.target_config->>'executor' IS DISTINCT FROM 'buildkit'
    UNION ALL
    SELECT j.build_id,j.target,j.status,e.status,e.error,e.failure_kind,
        NULL::uuid,e.id
    FROM jobs j JOIN builds b ON b.id=j.build_id
    JOIN LATERAL (
        SELECT id,status,error,failure_kind,source_manifest_id,plan_revision_id,cancel_requested
        FROM image_executions WHERE job_id=j.id AND operation='build'
        ORDER BY created_at DESC,id DESC LIMIT 1
    ) e ON true
    WHERE j.status='failed' AND j.target_config->>'executor'='buildkit'
        AND e.status='failed' AND NOT e.cancel_requested
        AND e.source_manifest_id=b.source_manifest_id
        AND e.plan_revision_id IS NOT DISTINCT FROM j.plan_revision_id";
