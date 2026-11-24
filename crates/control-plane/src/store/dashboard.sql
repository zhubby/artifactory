-- [INPUT]: 依赖租户、可见项目、可选项目、天数、时区与统一读取时刻。
-- [OUTPUT]: 生成完整窗口摘要、自然日日期桶和去重的近期构建活动。
-- [POS]: 概览以窄投影聚合；物化窗口不保留构建配置与大 JSON。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
WITH clock AS (
    SELECT coalesce($6::timestamptz, now()) AS at
), bounds AS (
    SELECT at,
           (at AT TIME ZONE $5)::date AS today,
           ((at AT TIME ZONE $5)::date - ($4::int - 1)) AS first_day,
           (((at AT TIME ZONE $5)::date - ($4::int - 1))::timestamp AT TIME ZONE $5) AS start
    FROM clock
), visible_projects AS MATERIALIZED (
    SELECT p.id, p.name, p.slug, p.status
    FROM projects p
    WHERE p.tenant_id=$1
      AND ($2::uuid[] IS NULL OR p.id=ANY($2))
      AND ($3::uuid IS NULL OR p.id=$3)
), scoped_builds AS NOT MATERIALIZED (
    SELECT b.id, b.project_id, b.number, b.status, b.release_id, b.release_version,
           b.created_at, b.finished_at, p.name AS project_name, p.slug AS project_slug,
           greatest(b.created_at, b.finished_at) AS activity_at
    FROM builds b JOIN visible_projects p ON p.id=b.project_id
    WHERE b.tenant_id=$1
), cohort AS MATERIALIZED (
    SELECT b.status,
           (b.created_at AT TIME ZONE $5)::date AS day,
           CASE WHEN b.status='succeeded' AND b.finished_at>=b.created_at
                THEN extract(epoch FROM b.finished_at-b.created_at)::double precision END AS turnaround
    FROM scoped_builds b CROSS JOIN bounds w
    WHERE b.created_at>=w.start AND b.created_at<=w.at
), activity AS MATERIALIZED (
    SELECT b.* FROM scoped_builds b CROSS JOIN bounds w
    WHERE (b.created_at>=w.start AND b.created_at<=w.at)
       OR (b.finished_at>=w.start AND b.finished_at<=w.at)
), totals AS (
    SELECT count(*) AS total,
           count(*) FILTER (WHERE status='succeeded') AS succeeded,
           count(*) FILTER (WHERE status IN ('failed','lost')) AS failed,
           count(*) FILTER (WHERE status='canceled') AS canceled,
           count(*) FILTER (WHERE status NOT IN ('succeeded','failed','lost','canceled')) AS ongoing,
           percentile_cont(0.5) WITHIN GROUP (ORDER BY turnaround) AS turnaround
    FROM cohort
), daily AS (
    SELECT day, count(*) AS total,
           count(*) FILTER (WHERE status='succeeded') AS succeeded,
           count(*) FILTER (WHERE status IN ('failed','lost')) AS failed,
           count(*) FILTER (WHERE status='canceled') AS canceled,
           count(*) FILTER (WHERE status NOT IN ('succeeded','failed','lost','canceled')) AS ongoing,
           percentile_cont(0.5) WITHIN GROUP (ORDER BY turnaround) AS turnaround
    FROM cohort GROUP BY day
), last_project_build AS (
    SELECT DISTINCT ON (project_id) *
    FROM activity ORDER BY project_id, activity_at DESC, id DESC
), recent_projects AS (
    SELECT a.*, p.status AS project_status
    FROM last_project_build a JOIN visible_projects p ON p.id=a.project_id
    ORDER BY activity_at DESC, a.id DESC LIMIT 5
), recent_builds AS (
    SELECT * FROM activity ORDER BY activity_at DESC, id DESC LIMIT 5
), build_documents AS (
    SELECT b.id, jsonb_build_object(
        'id',b.id,'number',b.number,'status',b.status,
        'release_id',b.release_id,'release_version',b.release_version,
        'project_id',b.project_id,'project_name',b.project_name,'project_slug',b.project_slug,
        'created_at',b.created_at,'finished_at',b.finished_at,'activity_at',b.activity_at
    ) AS document
    FROM scoped_builds b
    WHERE b.id IN (SELECT id FROM recent_projects UNION SELECT id FROM recent_builds)
)
SELECT jsonb_build_object(
    'window',jsonb_build_object('days',$4,'timezone',$5,'start',w.start,'end',w.at,'generated_at',w.at),
    'summary',jsonb_build_object(
        'total_projects',(SELECT count(*) FROM visible_projects),
        'active_projects',(SELECT count(DISTINCT project_id) FROM activity),
        'builds_created',t.total,'succeeded',t.succeeded,'failed',t.failed,
        'canceled',t.canceled,'ongoing',t.ongoing,
        'current_ongoing',(SELECT count(*) FROM scoped_builds WHERE status NOT IN ('succeeded','failed','lost','canceled')),
        'success_rate',t.succeeded::double precision/nullif(t.succeeded+t.failed,0),
        'median_turnaround_seconds',t.turnaround
    ),
    'daily',(SELECT jsonb_agg(jsonb_build_object(
        'date',to_char(w.first_day+bucket.day,'YYYY-MM-DD'),
        'total',coalesce(d.total,0),'succeeded',coalesce(d.succeeded,0),
        'failed',coalesce(d.failed,0),'canceled',coalesce(d.canceled,0),'ongoing',coalesce(d.ongoing,0),
        'median_turnaround_seconds',d.turnaround
    ) ORDER BY bucket.day)
        FROM generate_series(0,$4::int-1) AS bucket(day)
        LEFT JOIN daily d ON d.day=w.first_day+bucket.day),
    'recent_projects',coalesce((SELECT jsonb_agg(jsonb_build_object(
        'id',p.project_id,'name',p.project_name,'slug',p.project_slug,'status',p.project_status,
        'activity_at',p.activity_at,'last_build',d.document
    ) ORDER BY p.activity_at DESC,p.id DESC)
        FROM recent_projects p JOIN build_documents d ON d.id=p.id),'[]'::jsonb),
    'recent_builds',coalesce((SELECT jsonb_agg(d.document ORDER BY b.activity_at DESC,b.id DESC)
        FROM recent_builds b JOIN build_documents d ON d.id=b.id),'[]'::jsonb)
) FROM bounds w CROSS JOIN totals t
