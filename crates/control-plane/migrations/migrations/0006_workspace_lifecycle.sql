SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE tenants ADD COLUMN status text NOT NULL DEFAULT 'active' CHECK(status IN ('active','draining','archived','deleting'));
ALTER TABLE tenants ADD COLUMN archived_at timestamptz;
ALTER TABLE projects ADD COLUMN status text NOT NULL DEFAULT 'active' CHECK(status IN ('active','draining','archived','deleting'));
ALTER TABLE projects ADD COLUMN archived_at timestamptz;

-- These identities survive purging business rows; old links never change owners.
CREATE TABLE deleted_tenants(id uuid PRIMARY KEY,slug text NOT NULL UNIQUE,deleted_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE deleted_projects(id uuid PRIMARY KEY,tenant_id uuid NOT NULL,slug text NOT NULL,deleted_at timestamptz NOT NULL DEFAULT now(),UNIQUE(tenant_id,slug));
CREATE TABLE source_deleted_projects(project_id uuid PRIMARY KEY,inventory jsonb NOT NULL DEFAULT '{}',completed boolean NOT NULL DEFAULT false);
CREATE TABLE workspace_tasks(id uuid PRIMARY KEY,tenant_id uuid NOT NULL,project_id uuid,kind text NOT NULL,object_hash text,expires_at timestamptz NOT NULL);
CREATE TABLE shared_object_deletions(hash text PRIMARY KEY,operation_id uuid NOT NULL);

CREATE TABLE lifecycle_operations (
 id uuid PRIMARY KEY, actor_id uuid NOT NULL, tenant_id uuid NOT NULL, project_id uuid,
 slug text NOT NULL, action text NOT NULL CHECK(action IN ('archive','restore','delete')),
 state text NOT NULL CHECK(state IN ('waiting','running','succeeded','failed','canceled')),
 phase text NOT NULL DEFAULT 'draining', previous_status text NOT NULL,
 idempotency_key uuid NOT NULL, blockers jsonb NOT NULL DEFAULT '[]', counts jsonb NOT NULL DEFAULT '{}',
 manifest jsonb NOT NULL DEFAULT '{}', object_cursor bigint NOT NULL DEFAULT 0,
 error text, lease_owner uuid, lease_until timestamptz, generation bigint NOT NULL DEFAULT 0,
 next_run_at timestamptz NOT NULL DEFAULT now(), created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(actor_id,idempotency_key)
);
CREATE UNIQUE INDEX lifecycle_one_active_resource ON lifecycle_operations(tenant_id,coalesce(project_id,'00000000-0000-0000-0000-000000000000')) WHERE state IN ('waiting','running','failed');
CREATE INDEX lifecycle_pending ON lifecycle_operations(next_run_at,created_at) WHERE state IN ('waiting','running');

CREATE FUNCTION require_active_tenant(t uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE current_status text;
BEGIN
 SELECT status INTO current_status FROM tenants WHERE id=t FOR SHARE;
 IF current_status IS DISTINCT FROM 'active' THEN RAISE EXCEPTION 'workspace is read only' USING ERRCODE='55000'; END IF;
END $$;

CREATE FUNCTION require_active_project(p uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE t uuid; s text;
BEGIN
 SELECT tenant_id INTO t FROM projects WHERE id=p;
 PERFORM require_active_tenant(t);
 SELECT status INTO s FROM projects WHERE id=p FOR SHARE;
 IF s IS DISTINCT FROM 'active' THEN RAISE EXCEPTION 'workspace is read only' USING ERRCODE='55000'; END IF;
END $$;
CREATE FUNCTION project_configuration_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM require_active_project(NEW.id);
 RETURN NEW;
END $$;
CREATE TRIGGER workspace_project_configuration BEFORE UPDATE OF name,repository_url,credential_id,pool_ids,node_ids,node_access_mode,signing_profiles,model_profile_id,build_context,source_credentials,planner_backend ON projects FOR EACH ROW EXECUTE FUNCTION project_configuration_admission();
CREATE FUNCTION project_membership_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM require_active_project(NEW.project_id);
 RETURN NEW;
END $$;
CREATE TRIGGER workspace_project_members BEFORE INSERT OR UPDATE ON project_members FOR EACH ROW EXECUTE FUNCTION project_membership_admission();
CREATE TRIGGER workspace_project_nodes BEFORE INSERT OR UPDATE ON project_node_grants FOR EACH ROW EXECUTE FUNCTION project_membership_admission();
CREATE TRIGGER workspace_project_pools BEFORE INSERT OR UPDATE ON project_pool_grants FOR EACH ROW EXECUTE FUNCTION project_membership_admission();

CREATE FUNCTION workspace_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM require_active_tenant(NEW.tenant_id);
 RETURN NEW;
END $$;
CREATE TRIGGER workspace_project_admission BEFORE INSERT ON projects FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_node_admission BEFORE INSERT ON nodes FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_worker_admission BEFORE INSERT ON ai_workers FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_node_configuration BEFORE UPDATE OF name,pool_id,slots,draining,emissary_target_release ON nodes FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_worker_configuration BEFORE UPDATE OF name,draining ON ai_workers FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_pool_admission BEFORE INSERT OR UPDATE OF name,emissary_target_release,emissary_update_concurrency,emissary_updates_paused ON node_pools FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_credentials_admission BEFORE INSERT OR UPDATE ON credentials FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_tokens_admission BEFORE INSERT ON api_tokens FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_enrollment_admission BEFORE INSERT ON enrollment_tokens FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_ai_enrollment_admission BEFORE INSERT ON ai_enrollment_tokens FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_profiles_admission BEFORE INSERT OR UPDATE OF name,api,base_url,model,parameters,enabled,key_ciphertext,is_default,max_calls,timeout_seconds ON model_profiles FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_invitations_admission BEFORE INSERT ON tenant_invitations FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_members_admission BEFORE INSERT OR UPDATE ON tenant_members FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_ota_admission BEFORE INSERT ON emissary_updates FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE FUNCTION ota_release_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM workspace_tasks WHERE tenant_id=NEW.tenant_id AND kind='release_import' AND object_hash=NEW.manifest_hash AND expires_at>clock_timestamp()) THEN PERFORM require_active_tenant(NEW.tenant_id); END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER workspace_ota_release_admission BEFORE INSERT ON emissary_releases FOR EACH ROW EXECUTE FUNCTION ota_release_admission();

-- Allocation and admission share the same transaction and lock parent before child.
CREATE OR REPLACE FUNCTION assign_build_number() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE tenant uuid;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.number IS DISTINCT FROM OLD.number OR NEW.project_id IS DISTINCT FROM OLD.project_id THEN
   RAISE EXCEPTION 'build number and project are immutable' USING ERRCODE='23514';
  END IF;
 ELSE
  SELECT tenant_id INTO tenant FROM projects WHERE id=NEW.project_id;
  PERFORM require_active_tenant(tenant);
  UPDATE projects SET next_build_number=next_build_number+1 WHERE id=NEW.project_id AND status='active'
   RETURNING next_build_number-1 INTO NEW.number;
  IF NOT FOUND THEN RAISE EXCEPTION 'workspace is read only' USING ERRCODE='55000'; END IF;
 END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION assign_route_slug() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE base text; candidate text; suffix text; sequence bigint := 1; occupied boolean;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF OLD.slug IS NOT NULL AND NEW.slug IS DISTINCT FROM OLD.slug THEN RAISE EXCEPTION 'route slug is immutable' USING ERRCODE='23514'; END IF;
  IF TG_TABLE_NAME='projects' THEN
   IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id THEN RAISE EXCEPTION 'project tenant is immutable' USING ERRCODE='23514'; END IF;
  END IF;
 END IF;
 base := trim(both '-' from left(regexp_replace(lower(NEW.name COLLATE "C"),'[^a-z0-9]+','-','g'),63));
 IF TG_TABLE_NAME='tenants' THEN
  base:=coalesce(nullif(base,''),'tenant');
  PERFORM pg_advisory_xact_lock(hashtextextended('tenant-route-slugs',93));
 ELSE
  base:=coalesce(nullif(base,''),'project');
  PERFORM pg_advisory_xact_lock(hashtextextended('project-route-slugs:'||NEW.tenant_id::text,93));
 END IF;
 IF NEW.slug IS NOT NULL THEN
  IF TG_OP='INSERT' THEN
   IF TG_TABLE_NAME='tenants' THEN SELECT EXISTS(SELECT 1 FROM deleted_tenants WHERE slug=NEW.slug) INTO occupied;
   ELSE SELECT EXISTS(SELECT 1 FROM deleted_projects WHERE tenant_id=NEW.tenant_id AND slug=NEW.slug) INTO occupied; END IF;
   IF occupied THEN RAISE EXCEPTION 'route slug is retired' USING ERRCODE='23505'; END IF;
  END IF;
  RETURN NEW;
 END IF;
 LOOP
  suffix:=CASE WHEN sequence=1 THEN '' ELSE '-'||sequence::text END;
  candidate:=rtrim(left(base,63-length(suffix)),'-')||suffix;
  IF TG_TABLE_NAME='tenants' THEN
   SELECT EXISTS(SELECT 1 FROM tenants WHERE slug=candidate UNION ALL SELECT 1 FROM deleted_tenants WHERE slug=candidate) INTO occupied;
  ELSE
   SELECT EXISTS(SELECT 1 FROM projects WHERE tenant_id=NEW.tenant_id AND slug=candidate UNION ALL SELECT 1 FROM deleted_projects WHERE tenant_id=NEW.tenant_id AND slug=candidate) INTO occupied;
  END IF;
  EXIT WHEN NOT occupied;
  sequence:=sequence+1;
 END LOOP;
 NEW.slug:=candidate;
 RETURN NEW;
END $$;

-- Embedded Source shares the same project retirement fence as standalone Source.
CREATE FUNCTION source_workspace_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_advisory_xact_lock_shared(hashtextextended(NEW.project_id::text,1));
 IF EXISTS(SELECT 1 FROM source_deleted_projects WHERE project_id=NEW.project_id) THEN RAISE EXCEPTION 'project retired' USING ERRCODE='55000'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER source_repository_admission BEFORE INSERT ON source_repositories FOR EACH ROW EXECUTE FUNCTION source_workspace_admission();
CREATE TRIGGER source_snapshot_admission BEFORE INSERT ON source_snapshots FOR EACH ROW EXECUTE FUNCTION source_workspace_admission();
