-- [INPUT]: 独立清单观察、startup与内容版本摘要。
-- [OUTPUT]: 保存事务回执、startup最新摘要与跨副本准入预算。
-- [POS]: 遥测增量迁移，不改动旧采样清单表。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE inventory_deliveries (
    node_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    startup_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision>=0),
    source_batch_id uuid,
    observed_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL,
    delivery_digest text NOT NULL CHECK (length(delivery_digest)=64),
    inventory_digest text NOT NULL CHECK (length(inventory_digest)=64),
    PRIMARY KEY(node_id,observation_id)
);
CREATE UNIQUE INDEX inventory_native_revision ON inventory_deliveries(node_id,startup_id,revision) WHERE source_batch_id IS NULL;
CREATE UNIQUE INDEX inventory_legacy_source ON inventory_deliveries(node_id,source_batch_id) WHERE source_batch_id IS NOT NULL;
CREATE INDEX inventory_delivery_history ON inventory_deliveries(node_id,observed_at DESC,observation_id);
CREATE INDEX inventory_delivery_retention ON inventory_deliveries(received_at);
CREATE TABLE startup_inventories (
    node_id uuid NOT NULL,
    startup_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    revision bigint NOT NULL,
    source_batch_id uuid,
    observed_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL,
    inventory_digest text NOT NULL,
    PRIMARY KEY(node_id,startup_id)
);
CREATE TABLE inventory_ingress (
    node_id uuid PRIMARY KEY,
    tokens double precision NOT NULL CHECK(tokens>=0 AND tokens<=4),
    updated_at timestamptz NOT NULL
);
CREATE TABLE startup_hosts (
    node_id uuid NOT NULL,
    startup_id uuid NOT NULL,
    sampled_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL,
    monotonic_ms bigint NOT NULL,
    dropped_batches bigint NOT NULL,
    sample jsonb NOT NULL,
    PRIMARY KEY(node_id,startup_id)
);
INSERT INTO startup_hosts SELECT node_id,session_id,sampled_at,received_at,monotonic_ms,dropped_batches,sample FROM latest_hosts;
