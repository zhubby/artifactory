-- [INPUT]: 依赖账户、租户成员和项目授权。
-- [OUTPUT]: 提供持久事件、首次接收版本和当前授权函数。
-- [POS]: 通知核心的追加迁移；事件不引用可删除资源。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE notification_events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), event_key text UNIQUE NOT NULL,
 event_type text NOT NULL, schema_version integer NOT NULL DEFAULT 1,
 category text NOT NULL CHECK(category IN ('build','release','permission','device','credential','account','workspace','delivery')),
 severity text NOT NULL CHECK(severity IN ('info','success','warning','error')),
 tenant_id uuid, project_id uuid, resource_type text NOT NULL, resource_id uuid,
 aggregate_key text NOT NULL, aggregate_version bigint NOT NULL CHECK(aggregate_version>0),
 actor_id uuid, occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 payload jsonb NOT NULL DEFAULT '{}', attention boolean NOT NULL DEFAULT false,
 needs_action boolean NOT NULL DEFAULT false, candidate_user_ids uuid[] NOT NULL DEFAULT '{}',
 projected_at timestamptz, attempts integer NOT NULL DEFAULT 0,
 next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp(), last_error text
);
CREATE INDEX notification_events_pending ON notification_events(next_attempt_at,occurred_at,id) WHERE projected_at IS NULL;
CREATE INDEX notification_events_aggregate ON notification_events(aggregate_key,aggregate_version,id);
CREATE TABLE notification_preferences (
 user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 preferences jsonb NOT NULL DEFAULT '{}'
);
CREATE TABLE notification_project_subscriptions (
 user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(user_id,project_id)
);
CREATE TABLE notification_accounts (
 user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 watermark bigint NOT NULL DEFAULT 0
);
CREATE TABLE notification_inbox (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 aggregate_key text NOT NULL, event_id uuid NOT NULL REFERENCES notification_events(id),
 category text NOT NULL, event_type text NOT NULL, severity text NOT NULL,
 tenant_id uuid, project_id uuid, resource_type text NOT NULL, resource_id uuid,
 payload jsonb NOT NULL, occurred_at timestamptz NOT NULL, updated_at timestamptz NOT NULL,
 content_revision bigint NOT NULL, first_received_revision bigint NOT NULL,
 attention_revision bigint NOT NULL DEFAULT 0,
 read_revision bigint NOT NULL DEFAULT 0, attention_watermark bigint NOT NULL DEFAULT 0,
 needs_action boolean NOT NULL DEFAULT false, UNIQUE(user_id,aggregate_key)
);
CREATE INDEX notification_inbox_page ON notification_inbox(user_id,updated_at DESC,id DESC);
CREATE INDEX notification_inbox_aggregate ON notification_inbox(aggregate_key,user_id);
CREATE FUNCTION notification_user_can_read(u uuid,t uuid,p uuid,kind text,r uuid) RETURNS boolean
LANGUAGE sql STABLE AS $$
 SELECT EXISTS(SELECT 1 FROM users WHERE id=u) AND CASE
 WHEN kind='account_safe' THEN r=u
 WHEN t IS NULL THEN p IS NULL AND kind IN ('account','delivery') AND r=u
 ELSE EXISTS(SELECT 1 FROM tenants tenant JOIN tenant_members tm ON tm.tenant_id=tenant.id AND tm.user_id=u
 WHERE tenant.id=t AND tenant.status<>'deleting' AND
 CASE WHEN kind IN ('tenant_admin','credential','credentials','device','node','nodes','pool') THEN tm.role IN ('owner','admin')
 WHEN p IS NOT NULL THEN EXISTS(SELECT 1 FROM projects pr WHERE pr.id=p AND pr.tenant_id=t AND pr.status<>'deleting'
 AND (tm.role IN ('owner','admin') OR EXISTS(SELECT 1 FROM project_members pm WHERE pm.project_id=p AND pm.user_id=u AND (kind<>'project_admin' OR pm.role='admin'))))
 WHEN kind='project_admin' THEN false
 ELSE true END) END
$$;
