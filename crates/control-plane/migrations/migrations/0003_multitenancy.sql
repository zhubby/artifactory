-- Coordinated maintenance migration. Take a backup and drain all old instances first.
CREATE TABLE tenants (
 id uuid PRIMARY KEY, name text NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
 owner_id uuid NOT NULL REFERENCES users(id), created_at timestamptz NOT NULL DEFAULT now(),
 created_by uuid NOT NULL REFERENCES users(id), creation_key uuid NOT NULL, UNIQUE(created_by,creation_key)
);
CREATE TABLE tenant_members (
 tenant_id uuid NOT NULL REFERENCES tenants(id), user_id uuid NOT NULL REFERENCES users(id),
 role text NOT NULL CHECK(role IN ('owner','admin','member')),
 created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(tenant_id,user_id)
);
CREATE UNIQUE INDEX tenant_one_owner ON tenant_members(tenant_id) WHERE role='owner';
CREATE TABLE tenant_invitations (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id), token_hash text NOT NULL UNIQUE,
 role text NOT NULL CHECK(role IN ('admin','member')), created_by uuid NOT NULL REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT now(), expires_at timestamptz NOT NULL DEFAULT now()+interval '72 hours',
 revoked_at timestamptz, accepted_at timestamptz, accepted_by uuid REFERENCES users(id)
);
CREATE INDEX invitations_tenant ON tenant_invitations(tenant_id,created_at);
-- The empty installation is bootstrapped when its first administrator is created.
INSERT INTO tenants(id,name,owner_id,created_by,creation_key)
 SELECT '00000000-0000-0000-0000-000000000001','Default',id,id,'00000000-0000-0000-0000-000000000001'
 FROM users WHERE role='admin' ORDER BY created_at,id LIMIT 1;
-- A legacy database with resources must have an administrator; fail atomically otherwise.
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM tenants) AND (EXISTS(SELECT 1 FROM projects) OR EXISTS(SELECT 1 FROM nodes)) THEN
  RAISE EXCEPTION 'legacy resources require an administrator before the tenant migration';
 END IF;
END $$;
INSERT INTO tenant_members(tenant_id,user_id,role)
 SELECT t.id,u.id,CASE WHEN u.id=t.owner_id THEN 'owner' ELSE 'admin' END
 FROM users u CROSS JOIN tenants t WHERE u.role='admin';
DELETE FROM node_pools WHERE NOT EXISTS(SELECT 1 FROM tenants);
DELETE FROM sessions;
UPDATE api_tokens SET revoked=true WHERE user_id IN(SELECT id FROM users WHERE role<>'admin');
ALTER TABLE node_pools ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE node_pools SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE node_pools ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX node_pools_tenant ON node_pools(tenant_id);
ALTER TABLE projects ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE projects SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE projects ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX projects_tenant ON projects(tenant_id);
ALTER TABLE credentials ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE credentials SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE credentials ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX credentials_tenant ON credentials(tenant_id);
ALTER TABLE model_profiles ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE model_profiles SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE model_profiles ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX model_profiles_tenant ON model_profiles(tenant_id);
ALTER TABLE ai_workers ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE ai_workers SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE ai_workers ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX ai_workers_tenant ON ai_workers(tenant_id);
ALTER TABLE ai_enrollment_tokens ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE ai_enrollment_tokens SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE ai_enrollment_tokens ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX ai_enrollment_tokens_tenant ON ai_enrollment_tokens(tenant_id);
ALTER TABLE api_tokens ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE api_tokens SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE api_tokens ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX api_tokens_tenant ON api_tokens(tenant_id);
ALTER TABLE emissary_releases ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE emissary_releases SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE emissary_releases ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX emissary_releases_tenant ON emissary_releases(tenant_id);
ALTER TABLE nodes ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE nodes SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE nodes ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX nodes_tenant ON nodes(tenant_id);
ALTER TABLE enrollment_tokens ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE enrollment_tokens SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE enrollment_tokens ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX enrollment_tokens_tenant ON enrollment_tokens(tenant_id);
ALTER TABLE builds ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE builds SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE builds ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX builds_tenant ON builds(tenant_id);
ALTER TABLE jobs ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE jobs SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE jobs ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX jobs_tenant ON jobs(tenant_id);
ALTER TABLE attempts ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE attempts SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE attempts ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX attempts_tenant ON attempts(tenant_id);
ALTER TABLE artifacts ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE artifacts SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE artifacts ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX artifacts_tenant ON artifacts(tenant_id);
ALTER TABLE uploads ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE uploads SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE uploads ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX uploads_tenant ON uploads(tenant_id);
ALTER TABLE log_chunks ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE log_chunks SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE log_chunks ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX log_chunks_tenant ON log_chunks(tenant_id);
ALTER TABLE step_runs ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE step_runs SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE step_runs ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX step_runs_tenant ON step_runs(tenant_id);
ALTER TABLE attempt_events ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE attempt_events SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE attempt_events ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX attempt_events_tenant ON attempt_events(tenant_id);
ALTER TABLE planning_runs ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE planning_runs SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE planning_runs ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX planning_runs_tenant ON planning_runs(tenant_id);
ALTER TABLE plan_revisions ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE plan_revisions SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE plan_revisions ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX plan_revisions_tenant ON plan_revisions(tenant_id);
ALTER TABLE planning_events ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE planning_events SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE planning_events ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX planning_events_tenant ON planning_events(tenant_id);
ALTER TABLE publish_attempts ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE publish_attempts SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE publish_attempts ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX publish_attempts_tenant ON publish_attempts(tenant_id);
ALTER TABLE releases ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE releases SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE releases ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX releases_tenant ON releases(tenant_id);
ALTER TABLE emissary_updates ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE emissary_updates SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE emissary_updates ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX emissary_updates_tenant ON emissary_updates(tenant_id);
ALTER TABLE source_repositories ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE source_repositories SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE source_repositories ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX source_repositories_tenant ON source_repositories(tenant_id);
ALTER TABLE source_snapshots ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE source_snapshots SET tenant_id='00000000-0000-0000-0000-000000000001';
ALTER TABLE source_snapshots ALTER COLUMN tenant_id SET NOT NULL;
CREATE INDEX source_snapshots_tenant ON source_snapshots(tenant_id);
ALTER TABLE audit_events ADD COLUMN tenant_id uuid REFERENCES tenants(id);
UPDATE audit_events SET tenant_id=(SELECT id FROM tenants LIMIT 1);
CREATE INDEX audit_tenant ON audit_events(tenant_id,id DESC);
CREATE FUNCTION immutable_tenant() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id THEN RAISE EXCEPTION 'tenant ownership is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON node_pools FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON projects FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON credentials FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON model_profiles FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON ai_workers FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON api_tokens FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON ai_enrollment_tokens FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON emissary_releases FOR EACH ROW EXECUTE FUNCTION immutable_tenant();
ALTER TABLE jobs ADD COLUMN blocked_reason text;
ALTER TABLE node_pools DROP CONSTRAINT node_pools_name_key;
ALTER TABLE node_pools ADD UNIQUE(tenant_id,name);
ALTER TABLE model_profiles DROP CONSTRAINT model_profiles_name_key;
ALTER TABLE model_profiles ADD UNIQUE(tenant_id,name);
DROP INDEX one_default_model;
CREATE UNIQUE INDEX one_default_model ON model_profiles(tenant_id) WHERE is_default;
ALTER TABLE emissary_releases DROP CONSTRAINT emissary_releases_version_key;
ALTER TABLE emissary_releases DROP CONSTRAINT emissary_releases_manifest_hash_key;
ALTER TABLE emissary_releases ADD UNIQUE(tenant_id,version), ADD UNIQUE(tenant_id,manifest_hash);
ALTER TABLE node_pools ADD COLUMN is_default boolean NOT NULL DEFAULT false;
UPDATE node_pools SET is_default=true WHERE id='00000000-0000-0000-0000-000000000001';
CREATE UNIQUE INDEX tenant_default_pool ON node_pools(tenant_id) WHERE is_default;
ALTER TABLE projects ALTER COLUMN pool_ids SET DEFAULT '{}';
ALTER TABLE projects ADD COLUMN node_access_mode text NOT NULL DEFAULT 'pools' CHECK(node_access_mode IN ('pools','nodes')),
 ADD COLUMN node_ids uuid[] NOT NULL DEFAULT '{}';
CREATE TABLE project_members (
 tenant_id uuid NOT NULL, project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 user_id uuid NOT NULL, role text NOT NULL CHECK(role IN ('admin','operator','viewer')),
 created_at timestamptz NOT NULL DEFAULT now(), PRIMARY KEY(project_id,user_id),
 FOREIGN KEY(tenant_id,user_id) REFERENCES tenant_members(tenant_id,user_id) ON DELETE CASCADE
);
CREATE INDEX project_members_user ON project_members(tenant_id,user_id);
CREATE TABLE project_node_grants (
 tenant_id uuid NOT NULL, project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 node_id uuid NOT NULL REFERENCES nodes(id), PRIMARY KEY(project_id,node_id)
);
CREATE TABLE project_pool_grants (
 tenant_id uuid NOT NULL, project_id uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 pool_id uuid NOT NULL REFERENCES node_pools(id), PRIMARY KEY(project_id,pool_id)
);
-- Avoid re-reading immutable ownership on every heartbeat, sample, and lease renewal.
CREATE FUNCTION derive_tenant() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE parent uuid;
BEGIN
 EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[0]) INTO parent USING (to_jsonb(NEW)->>TG_ARGV[1])::uuid;
 IF parent IS NULL OR (NEW.tenant_id IS NOT NULL AND NEW.tenant_id<>parent) THEN
  RAISE EXCEPTION 'cross-tenant reference' USING ERRCODE='23514';
 END IF;
 IF TG_OP='UPDATE' AND OLD.tenant_id<>parent THEN RAISE EXCEPTION 'tenant ownership is immutable'; END IF;
 NEW.tenant_id=parent; RETURN NEW;
END $$;
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,pool_id ON nodes FOR EACH ROW EXECUTE FUNCTION derive_tenant('node_pools','pool_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,pool_id ON enrollment_tokens FOR EACH ROW EXECUTE FUNCTION derive_tenant('node_pools','pool_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,project_id ON builds FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects','project_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,build_id ON jobs FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds','build_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,job_id ON attempts FOR EACH ROW EXECUTE FUNCTION derive_tenant('jobs','job_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,attempt_id ON artifacts FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts','attempt_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,attempt_id ON uploads FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts','attempt_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,attempt_id ON log_chunks FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts','attempt_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,attempt_id ON step_runs FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts','attempt_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,attempt_id ON attempt_events FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts','attempt_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,build_id ON planning_runs FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds','build_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,build_id ON plan_revisions FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds','build_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,run_id ON planning_events FOR EACH ROW EXECUTE FUNCTION derive_tenant('planning_runs','run_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,build_id ON publish_attempts FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds','build_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,project_id ON releases FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects','project_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,node_id ON emissary_updates FOR EACH ROW EXECUTE FUNCTION derive_tenant('nodes','node_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,project_id ON source_repositories FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects','project_id');
CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id,project_id ON source_snapshots FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects','project_id');
CREATE FUNCTION check_tenant_reference() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE target uuid; ref uuid;
BEGIN
 ref=(to_jsonb(NEW)->>TG_ARGV[1])::uuid;
 IF ref IS NOT NULL THEN
  EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[0]) INTO target USING ref;
  IF target IS NULL OR NEW.tenant_id<>target THEN RAISE EXCEPTION 'cross-tenant reference' USING ERRCODE='23514'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER tenant_ref_credential_id BEFORE INSERT OR UPDATE OF tenant_id,credential_id ON projects FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('credentials','credential_id');
CREATE TRIGGER tenant_ref_model_profile_id BEFORE INSERT OR UPDATE OF tenant_id,model_profile_id ON projects FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('model_profiles','model_profile_id');
CREATE TRIGGER tenant_ref_node_id BEFORE INSERT OR UPDATE OF tenant_id,node_id ON attempts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('nodes','node_id');
CREATE TRIGGER tenant_ref_worker_id BEFORE INSERT OR UPDATE OF tenant_id,worker_id ON planning_runs FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('ai_workers','worker_id');
CREATE TRIGGER tenant_ref_source_credential_id BEFORE INSERT OR UPDATE OF tenant_id,source_credential_id ON builds FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('credentials','source_credential_id');
CREATE TRIGGER tenant_ref_source_snapshot_id BEFORE INSERT OR UPDATE OF tenant_id,source_snapshot_id ON builds FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('source_snapshots','source_snapshot_id');
CREATE TRIGGER tenant_ref_current_plan_id BEFORE INSERT OR UPDATE OF tenant_id,current_plan_id ON builds FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('plan_revisions','current_plan_id');
CREATE TRIGGER tenant_ref_plan_revision_id BEFORE INSERT OR UPDATE OF tenant_id,plan_revision_id ON jobs FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('plan_revisions','plan_revision_id');
CREATE TRIGGER tenant_ref_plan_revision_id BEFORE INSERT OR UPDATE OF tenant_id,plan_revision_id ON attempts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('plan_revisions','plan_revision_id');
CREATE TRIGGER tenant_ref_source_snapshot_id BEFORE INSERT OR UPDATE OF tenant_id,source_snapshot_id ON attempts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('source_snapshots','source_snapshot_id');
CREATE TRIGGER tenant_ref_run_id BEFORE INSERT OR UPDATE OF tenant_id,run_id ON plan_revisions FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('planning_runs','run_id');
CREATE TRIGGER tenant_ref_source_snapshot_id BEFORE INSERT OR UPDATE OF tenant_id,source_snapshot_id ON plan_revisions FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('source_snapshots','source_snapshot_id');
CREATE TRIGGER tenant_ref_root_repository_id BEFORE INSERT OR UPDATE OF tenant_id,root_repository_id ON source_snapshots FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('source_repositories','root_repository_id');
CREATE TRIGGER tenant_ref_pool_id BEFORE INSERT OR UPDATE OF tenant_id,pool_id ON emissary_updates FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('node_pools','pool_id');
CREATE TRIGGER tenant_ref_release_id BEFORE INSERT OR UPDATE OF tenant_id,release_id ON emissary_updates FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_releases','release_id');
CREATE TRIGGER tenant_ref_retry_of BEFORE INSERT OR UPDATE OF tenant_id,retry_of ON emissary_updates FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_updates','retry_of');
CREATE TRIGGER tenant_ref_emissary_target_release BEFORE INSERT OR UPDATE OF tenant_id,emissary_target_release ON nodes FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_releases','emissary_target_release');
CREATE TRIGGER tenant_ref_emissary_target_release BEFORE INSERT OR UPDATE OF tenant_id,emissary_target_release ON node_pools FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_releases','emissary_target_release');
CREATE TRIGGER tenant_ref_project_id BEFORE INSERT OR UPDATE OF tenant_id,project_id ON project_members FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('projects','project_id');
CREATE TRIGGER tenant_ref_project_id BEFORE INSERT OR UPDATE OF tenant_id,project_id ON project_node_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('projects','project_id');
CREATE TRIGGER tenant_ref_node_id BEFORE INSERT OR UPDATE OF tenant_id,node_id ON project_node_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('nodes','node_id');
CREATE TRIGGER tenant_ref_project_id BEFORE INSERT OR UPDATE OF tenant_id,project_id ON project_pool_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('projects','project_id');
CREATE TRIGGER tenant_ref_pool_id BEFORE INSERT OR UPDATE OF tenant_id,pool_id ON project_pool_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('node_pools','pool_id');
CREATE TRIGGER tenant_ref_build_id BEFORE INSERT OR UPDATE OF tenant_id,build_id ON releases FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('builds','build_id');
CREATE TRIGGER tenant_ref_credential_id BEFORE INSERT OR UPDATE OF tenant_id,credential_id ON source_repositories FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('credentials','credential_id');
CREATE TRIGGER tenant_ref_upload BEFORE INSERT OR UPDATE OF tenant_id,id ON artifacts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('uploads','id');
-- These link tables inherit authority from both endpoints rather than accepting a tenant field.
CREATE FUNCTION check_link_tenants() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE left_tenant uuid; right_tenant uuid;
BEGIN
 EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[0]) INTO left_tenant USING (to_jsonb(NEW)->>TG_ARGV[1])::uuid;
 EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[2]) INTO right_tenant USING (to_jsonb(NEW)->>TG_ARGV[3])::uuid;
 IF left_tenant IS NULL OR right_tenant IS NULL OR left_tenant<>right_tenant THEN
  RAISE EXCEPTION 'cross-tenant link' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON release_artifacts FOR EACH ROW EXECUTE FUNCTION check_link_tenants('releases','release_id','artifacts','artifact_id');
CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON source_tokens FOR EACH ROW EXECUTE FUNCTION check_link_tenants('attempts','attempt_id','source_snapshots','snapshot_id');
CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON device_commands FOR EACH ROW EXECUTE FUNCTION check_link_tenants('attempts','attempt_id','nodes','node_id');
CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON placement_rounds FOR EACH ROW EXECUTE FUNCTION check_link_tenants('builds','build_id','planning_runs','run_id');
CREATE FUNCTION sync_project_grants() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM unnest(NEW.pool_ids) grant_id LEFT JOIN node_pools p ON p.id=grant_id WHERE p.tenant_id IS DISTINCT FROM NEW.tenant_id)
 OR EXISTS(SELECT 1 FROM unnest(NEW.node_ids) grant_id LEFT JOIN nodes n ON n.id=grant_id WHERE n.tenant_id IS DISTINCT FROM NEW.tenant_id)
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(NEW.source_credentials) binding LEFT JOIN credentials c ON c.id=(binding->>'credential_id')::uuid WHERE c.tenant_id IS DISTINCT FROM NEW.tenant_id)
 THEN RAISE EXCEPTION 'cross-tenant project resource' USING ERRCODE='23514'; END IF;
 DELETE FROM project_node_grants WHERE project_id=NEW.id;
 INSERT INTO project_node_grants SELECT NEW.tenant_id,NEW.id,id FROM unnest(NEW.node_ids) id;
 DELETE FROM project_pool_grants WHERE project_id=NEW.id;
 INSERT INTO project_pool_grants SELECT NEW.tenant_id,NEW.id,id FROM unnest(NEW.pool_ids) id;
 RETURN NEW;
END $$;
CREATE TRIGGER project_grants AFTER INSERT OR UPDATE OF pool_ids,node_ids,node_access_mode,source_credentials ON projects FOR EACH ROW EXECUTE FUNCTION sync_project_grants();
INSERT INTO project_pool_grants SELECT tenant_id,id,unnest(pool_ids) FROM projects;
CREATE FUNCTION project_node_allowed(project uuid,node uuid) RETURNS boolean LANGUAGE sql STABLE AS $$
 SELECT EXISTS(SELECT 1 FROM projects p JOIN nodes n ON n.tenant_id=p.tenant_id
 WHERE p.id=project AND n.id=node AND
 ((p.node_access_mode='pools' AND n.pool_id=ANY(p.pool_ids)) OR (p.node_access_mode='nodes' AND n.id=ANY(p.node_ids))))
$$;
CREATE FUNCTION build_node_allowed(build uuid,node uuid) RETURNS boolean LANGUAGE sql STABLE AS $$
 SELECT EXISTS(SELECT 1 FROM builds b JOIN nodes n ON n.tenant_id=b.tenant_id
 WHERE b.id=build AND n.id=node AND project_node_allowed(b.project_id,n.id) AND
 CASE WHEN b.planning_context->'node_access'->>'mode'='nodes'
 THEN b.planning_context->'node_access'->'ids' ? n.id::text
 ELSE coalesce(b.planning_context->'node_access'->'ids',b.planning_context->'pool_ids','[]') ? n.pool_id::text END)
$$;
-- Keep the original build snapshot, including explicit empty grants. Legacy recipe
-- builds without a snapshot inherit the project's scope at the maintenance cutover.
UPDATE builds b SET planning_context=planning_context||jsonb_build_object('node_access',jsonb_build_object('mode','pools','ids',coalesce(b.planning_context->'pool_ids',to_jsonb(p.pool_ids)))) FROM projects p WHERE p.id=b.project_id;
CREATE FUNCTION freeze_build_access() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 NEW.planning_context=NEW.planning_context||jsonb_build_object('tenant_id',(SELECT tenant_id FROM projects WHERE id=NEW.project_id),'node_access',
 (SELECT jsonb_build_object('mode',node_access_mode,'ids',CASE WHEN node_access_mode='pools' THEN pool_ids ELSE node_ids END) FROM projects WHERE id=NEW.project_id));
 RETURN NEW;
END $$;
CREATE TRIGGER freeze_device_access BEFORE INSERT ON builds FOR EACH ROW EXECUTE FUNCTION freeze_build_access();
CREATE FUNCTION verify_tenant_owner() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE tenant uuid;
BEGIN
 tenant=coalesce((to_jsonb(NEW)->>CASE WHEN TG_TABLE_NAME='tenants' THEN 'id' ELSE 'tenant_id' END)::uuid,(to_jsonb(OLD)->>CASE WHEN TG_TABLE_NAME='tenants' THEN 'id' ELSE 'tenant_id' END)::uuid);
 IF EXISTS(SELECT 1 FROM tenants t WHERE t.id=tenant AND NOT EXISTS(SELECT 1 FROM tenant_members m WHERE m.tenant_id=t.id AND m.user_id=t.owner_id AND m.role='owner'))
 THEN RAISE EXCEPTION 'tenant must retain its owner'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER tenant_owner_check AFTER INSERT OR UPDATE OR DELETE ON tenant_members DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION verify_tenant_owner();
CREATE CONSTRAINT TRIGGER tenant_owner_check AFTER INSERT OR UPDATE ON tenants DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION verify_tenant_owner();
