-- [INPUT]: 依赖全新 PostgreSQL 数据库。
-- [OUTPUT]: 创建队列、预算、审计与提案表。
-- [POS]: Runtime 唯一初始化基线。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE runtime_project_tenants(project_id uuid PRIMARY KEY,tenant_id uuid NOT NULL,UNIQUE(project_id,tenant_id));
CREATE TABLE runtime_runs (
 tenant_id uuid, id uuid PRIMARY KEY, version bigint NOT NULL, intent_hash text, intent_ciphertext text,
 project_id uuid, pool text, source_manifest_id uuid, source_digest text,
 lifecycle jsonb NOT NULL, status text NOT NULL, execution jsonb,
 grant_hash text, grant_ciphertext text, usage jsonb NOT NULL DEFAULT '{}',
 report_sequence bigint NOT NULL DEFAULT 0, created_at timestamptz NOT NULL DEFAULT now(),
 dispatched_at timestamptz, updated_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(project_id,tenant_id) REFERENCES runtime_project_tenants(project_id,tenant_id)
);
CREATE INDEX runtime_queue ON runtime_runs(status,created_at);
CREATE TABLE runtime_profiles (id uuid PRIMARY KEY, next_call_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE runtime_model_requests (
 run_id uuid NOT NULL REFERENCES runtime_runs(id), request_id uuid NOT NULL,
 input_hash text NOT NULL, generation bigint NOT NULL, state text NOT NULL DEFAULT 'reserved',
 created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(run_id,request_id)
);
CREATE TABLE runtime_reports (
 run_id uuid NOT NULL REFERENCES runtime_runs(id), event_id uuid NOT NULL, input_hash text NOT NULL,
 PRIMARY KEY(run_id,event_id)
);
CREATE TABLE runtime_proposals (
 id uuid PRIMARY KEY, run_id uuid NOT NULL REFERENCES runtime_runs(id), generation bigint NOT NULL,
 input_hash text NOT NULL, kind text NOT NULL, payload jsonb NOT NULL,
 deadline_ms bigint NOT NULL, result jsonb, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX one_pending_proposal ON runtime_proposals(run_id) WHERE result IS NULL;
CREATE TABLE runtime_outbox (
 id uuid PRIMARY KEY, run_id uuid NOT NULL REFERENCES runtime_runs(id), kind text NOT NULL,
 payload jsonb NOT NULL, delivered boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX runtime_runs_tenant ON runtime_runs(tenant_id,status);
CREATE TABLE runtime_conversations (
 run_id uuid NOT NULL REFERENCES runtime_runs(id), generation bigint NOT NULL,
 pod_uid text NOT NULL, status text NOT NULL, model text NOT NULL, source_digest text,
 started_at timestamptz NOT NULL DEFAULT now(), ended_at timestamptz,
 PRIMARY KEY(run_id,generation)
);
CREATE TABLE runtime_messages (
 run_id uuid NOT NULL, generation bigint NOT NULL, sequence bigint NOT NULL, event_id uuid NOT NULL,
 kind text NOT NULL, input_hash text NOT NULL, payload jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(run_id,generation,sequence),
 UNIQUE(run_id,generation,event_id), FOREIGN KEY(run_id,generation) REFERENCES runtime_conversations(run_id,generation)
);
CREATE TABLE runtime_deleted_projects(project_id uuid PRIMARY KEY,deleted_at timestamptz NOT NULL DEFAULT now());
CREATE FUNCTION runtime_workspace_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('runtime-project:'||NEW.project_id::text,96));
 IF EXISTS(SELECT 1 FROM runtime_deleted_projects WHERE project_id=NEW.project_id) THEN RAISE EXCEPTION 'project retired' USING ERRCODE='55000'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER runtime_run_admission BEFORE INSERT ON runtime_runs FOR EACH ROW EXECUTE FUNCTION runtime_workspace_admission();
CREATE TRIGGER runtime_project_admission BEFORE INSERT ON runtime_project_tenants FOR EACH ROW EXECUTE FUNCTION runtime_workspace_admission();
