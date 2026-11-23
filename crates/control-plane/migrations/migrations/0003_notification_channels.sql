-- [INPUT]: 依赖通知事件、个人偏好和账户身份。
-- [OUTPUT]: 提供地址绑定、验证、Webhook 与持久投递队列。
-- [POS]: 外部通知隔离层；绑定代数阻止旧任务投向新地址。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
ALTER TABLE notification_events ADD COLUMN channels_routed_at timestamptz;
CREATE INDEX notification_events_channels_pending ON notification_events(occurred_at) WHERE channels_routed_at IS NULL;
CREATE TABLE notification_email_contacts (
 user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 address text NOT NULL, generation bigint NOT NULL DEFAULT 1,
 verified_at timestamptz, token_hash text UNIQUE, token_expires_at timestamptz,
 last_requested_at timestamptz NOT NULL DEFAULT now(), request_window_at timestamptz NOT NULL DEFAULT now(), request_count integer NOT NULL DEFAULT 1,
 unsubscribe_hash text UNIQUE NOT NULL
);
CREATE TABLE notification_webhooks (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
 project_id uuid REFERENCES projects(id) ON DELETE CASCADE, created_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 100), url text NOT NULL,
 enabled boolean NOT NULL DEFAULT false, event_types text[] NOT NULL DEFAULT '{}',
 generation bigint NOT NULL DEFAULT 1, secret_encrypted text NOT NULL,
 last_status text, binding_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 consecutive_failures integer NOT NULL DEFAULT 0, paused_until timestamptz, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notification_webhook_scope ON notification_webhooks(tenant_id,project_id);
CREATE TABLE notification_deliveries (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), channel text NOT NULL CHECK(channel IN ('email','verification','webhook')),
 event_id uuid, user_id uuid REFERENCES users(id) ON DELETE CASCADE,
 endpoint_id uuid REFERENCES notification_webhooks(id) ON DELETE CASCADE,
 generation bigint NOT NULL, event_type text NOT NULL, payload jsonb NOT NULL DEFAULT '{}',
 status text NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','sending','succeeded','failed','canceled')),
 attempts integer NOT NULL DEFAULT 0, next_attempt_at timestamptz NOT NULL DEFAULT now(),
 lease_until timestamptz, lease_id uuid, last_error text, created_at timestamptz NOT NULL DEFAULT now(),
 completed_at timestamptz,
 CONSTRAINT notification_delivery_owner CHECK((channel='webhook' AND endpoint_id IS NOT NULL AND user_id IS NULL) OR (channel!='webhook' AND user_id IS NOT NULL AND endpoint_id IS NULL))
);
CREATE UNIQUE INDEX notification_delivery_user_event ON notification_deliveries(event_id,user_id,generation) WHERE channel='email';
CREATE UNIQUE INDEX notification_delivery_endpoint_event ON notification_deliveries(event_id,endpoint_id,generation) WHERE channel='webhook';
CREATE INDEX notification_delivery_due ON notification_deliveries(next_attempt_at) WHERE status IN ('queued','sending');
