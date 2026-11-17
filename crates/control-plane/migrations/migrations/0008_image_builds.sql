-- [INPUT]: 依赖租户、项目、Job 与固定源码快照。
-- [OUTPUT]: 增加镜像执行、凭据、发布来源快照与源码授权。
-- [POS]: 镜像执行使用独立身份，不占用设备 attempt。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE image_credentials (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id),
 project_id uuid NOT NULL REFERENCES projects(id), name text NOT NULL,
 kind text NOT NULL CHECK(kind IN ('registry','secret')), envelope text NOT NULL,
 registry_origin text, CHECK((kind='registry')=(registry_origin IS NOT NULL)),
 enabled boolean NOT NULL DEFAULT true, created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(project_id,name)
);
CREATE TABLE image_executions (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id),
 project_id uuid NOT NULL REFERENCES projects(id), job_id uuid NOT NULL REFERENCES jobs(id),
 snapshot_id uuid NOT NULL REFERENCES source_snapshots(id), plan_revision_id uuid REFERENCES plan_revisions(id),
 parent_id uuid REFERENCES image_executions(id), operation text NOT NULL DEFAULT 'build' CHECK(operation IN ('build','publish')),
 status text NOT NULL DEFAULT 'preparing' CHECK(status IN ('preparing','running','publish_failed','succeeded','failed','canceled')),
 build_token_hash text NOT NULL, publish_token_hash text NOT NULL,
 task_envelope text, spec jsonb NOT NULL, results jsonb NOT NULL DEFAULT '[]',
 candidate_repository text NOT NULL, destination_repository text NOT NULL,
 destination_credential uuid REFERENCES image_credentials(id), destination_registry text, tags text[] NOT NULL,
 root_digest text, error text, deadline_at timestamptz NOT NULL,
 cancel_requested boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now(), finished_at timestamptz,
 last_dispatched_at timestamptz
);
CREATE UNIQUE INDEX image_one_active_build ON image_executions(job_id) WHERE operation='build' AND status IN ('preparing','running');
CREATE UNIQUE INDEX image_one_active_publication ON image_executions(parent_id) WHERE operation='publish' AND destination_credential IS NULL AND status IN ('preparing','running');
CREATE INDEX image_dispatch ON image_executions(created_at) WHERE status IN ('preparing','running');
CREATE TABLE image_platforms (
 execution_id uuid NOT NULL REFERENCES image_executions(id) ON DELETE CASCADE,
 platform text NOT NULL CHECK(platform IN ('linux/amd64','linux/arm64','publish')),
 pod_uid uuid NOT NULL, heartbeat_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(execution_id,platform)
);
CREATE TABLE image_logs (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 execution_id uuid NOT NULL REFERENCES image_executions(id) ON DELETE CASCADE,
 platform text NOT NULL, sequence bigint NOT NULL CHECK(sequence>0),
 object_key text NOT NULL, content_hash text NOT NULL,
 UNIQUE(execution_id,platform,sequence)
);
CREATE TABLE image_artifacts (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id),
 project_id uuid NOT NULL REFERENCES projects(id), job_id uuid NOT NULL REFERENCES jobs(id),
 execution_id uuid NOT NULL UNIQUE REFERENCES image_executions(id),
 repository text NOT NULL, digest text NOT NULL CHECK(digest ~ '^sha256:[0-9a-f]{64}$'),
 platforms jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE release_images (
 release_id uuid NOT NULL REFERENCES releases(id), image_id uuid NOT NULL REFERENCES image_artifacts(id),
 PRIMARY KEY(release_id,image_id)
);
CREATE TABLE image_source_tokens (
 token_hash text PRIMARY KEY, execution_id uuid NOT NULL UNIQUE REFERENCES image_executions(id) ON DELETE CASCADE
);
CREATE FUNCTION image_tenant_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE p uuid; t uuid;
BEGIN
 SELECT tenant_id INTO t FROM projects WHERE id=NEW.project_id;
 IF t IS DISTINCT FROM NEW.tenant_id THEN RAISE EXCEPTION 'image tenant mismatch'; END IF;
 IF TG_TABLE_NAME='image_executions' THEN
  SELECT b.project_id INTO p FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=NEW.job_id;
  IF p IS DISTINCT FROM NEW.project_id OR NOT EXISTS(SELECT 1 FROM source_snapshots s WHERE s.id=NEW.snapshot_id AND s.project_id=p AND s.tenant_id=t) THEN RAISE EXCEPTION 'image source mismatch'; END IF;
  IF NEW.destination_credential IS NOT NULL AND NOT EXISTS(SELECT 1 FROM image_credentials c WHERE c.id=NEW.destination_credential AND c.project_id=p AND c.tenant_id=t AND c.kind='registry') THEN RAISE EXCEPTION 'image credential mismatch'; END IF;
  IF NEW.parent_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM image_executions e WHERE e.id=NEW.parent_id AND e.project_id=p AND e.tenant_id=t AND e.job_id=NEW.job_id AND e.operation='build') THEN RAISE EXCEPTION 'image parent mismatch'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER image_tenant_guard BEFORE INSERT OR UPDATE ON image_credentials FOR EACH ROW EXECUTE FUNCTION image_tenant_guard();
CREATE TRIGGER image_tenant_guard BEFORE INSERT OR UPDATE ON image_executions FOR EACH ROW EXECUTE FUNCTION image_tenant_guard();
