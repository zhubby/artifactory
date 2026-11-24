-- [INPUT]: 依赖现有账户会话、租户和设备身份。
-- [OUTPUT]: 追加终端会话、一次性票据与带租约的实例目录。
-- [POS]: 终端控制状态持久化；不保存输入输出字节。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE terminal_instances (
 id uuid PRIMARY KEY, endpoint text NOT NULL, certificate text NOT NULL,
 public_key text NOT NULL, expires_at timestamptz NOT NULL
);
CREATE TABLE terminal_sessions (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL, node_id uuid NOT NULL,
 user_id uuid NOT NULL, credential_hash text NOT NULL,
 idempotency_key uuid NOT NULL, renderer_id uuid NOT NULL,
 owner uuid NOT NULL, node_session uuid NOT NULL, startup_id uuid NOT NULL,
 profile text NOT NULL, term text NOT NULL, columns integer NOT NULL,
 rows integer NOT NULL, state text NOT NULL DEFAULT 'pending'
 CHECK(state IN ('pending','opening','active','closing','closed','failed')),
 generation bigint NOT NULL DEFAULT 0, ticket_hash text,
 ticket_expires_at timestamptz, created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL DEFAULT now()+interval '2 hours',
 last_input_at timestamptz NOT NULL DEFAULT now(),
 attached_at timestamptz, finished_at timestamptz,
 exit_code integer, exit_signal text, reason text,
 UNIQUE(credential_hash,idempotency_key),
 CHECK(columns BETWEEN 1 AND 500), CHECK(rows BETWEEN 1 AND 500)
);
CREATE INDEX terminal_sessions_node_active ON terminal_sessions(node_id,state)
 WHERE finished_at IS NULL;
CREATE INDEX terminal_sessions_user_active ON terminal_sessions(user_id,state)
 WHERE finished_at IS NULL;
CREATE INDEX terminal_sessions_owner_active ON terminal_sessions(owner)
 WHERE finished_at IS NULL;
CREATE INDEX terminal_sessions_expiration ON terminal_sessions(expires_at)
 WHERE finished_at IS NULL;
