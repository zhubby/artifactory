-- [INPUT]: 依赖当前租户、账户会话与设备启动身份。
-- [OUTPUT]: 创建只读操作命令、终态结果和幂等审计状态。
-- [POS]: 独立于构建 attempt 的有限操作持久化边界。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE node_operations (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
 node_id uuid NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
 actor_id uuid NOT NULL, credential_hash text NOT NULL, request_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN ('collect_diagnostics','refresh_inventory')),
 schema integer NOT NULL CHECK(schema=1), input jsonb NOT NULL CHECK(octet_length(input::text)<=8192),
 input_digest text NOT NULL CHECK(length(input_digest)=64),
 startup_id uuid NOT NULL, control_session uuid NOT NULL,
 state text NOT NULL DEFAULT 'requested' CHECK(state IN ('requested','accepted','running','succeeded','failed','canceled','expired','interrupted','rejected')),
 sequence bigint NOT NULL DEFAULT 0 CHECK(sequence>=0), reason text,
 delivery_state text NOT NULL DEFAULT 'pending' CHECK(delivery_state IN ('pending','awaiting_result','confirmed','outcome_unknown')),
 admission_held boolean NOT NULL DEFAULT true,
 cancel_requested_at timestamptz, created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 issued_at timestamptz NOT NULL, expires_at timestamptz NOT NULL,
 execution_cutoff timestamptz NOT NULL, next_delivery_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 finished_at timestamptz, result jsonb CHECK(result IS NULL OR octet_length(result::text)<=65536),
 result_digest text CHECK(result_digest IS NULL OR length(result_digest)=64),
 result_expires_at timestamptz, UNIQUE(tenant_id,node_id,request_id),
 CHECK(expires_at=issued_at+interval '30 seconds')
);
CREATE UNIQUE INDEX node_operations_one_active ON node_operations(node_id) WHERE admission_held;
CREATE INDEX node_operations_history ON node_operations(tenant_id,node_id,created_at DESC,id DESC);
CREATE INDEX node_operations_pending ON node_operations(next_delivery_at) WHERE finished_at IS NULL;
