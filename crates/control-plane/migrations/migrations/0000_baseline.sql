-- [INPUT]: 依赖全新的 PostgreSQL 数据库。
-- [OUTPUT]: 创建业务、租户、固定源码及源码和镜像 CR UID 约束。
-- [POS]: 原型重置后的唯一初始化基线。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE FUNCTION assign_build_number() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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

CREATE FUNCTION assign_route_slug() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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


CREATE FUNCTION check_link_tenants() RETURNS trigger
    LANGUAGE plpgsql
    AS $_$
DECLARE left_tenant uuid; right_tenant uuid;
BEGIN
 EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[0]) INTO left_tenant USING (to_jsonb(NEW)->>TG_ARGV[1])::uuid;
 EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[2]) INTO right_tenant USING (to_jsonb(NEW)->>TG_ARGV[3])::uuid;
 IF left_tenant IS NULL OR right_tenant IS NULL OR left_tenant<>right_tenant THEN
  RAISE EXCEPTION 'cross-tenant link' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $_$;

CREATE FUNCTION check_tenant_reference() RETURNS trigger
    LANGUAGE plpgsql
    AS $_$
DECLARE target uuid; ref uuid;
BEGIN
 ref=(to_jsonb(NEW)->>TG_ARGV[1])::uuid;
 IF ref IS NOT NULL THEN
  EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[0]) INTO target USING ref;
  IF target IS NULL OR NEW.tenant_id<>target THEN RAISE EXCEPTION 'cross-tenant reference' USING ERRCODE='23514'; END IF;
 END IF;
 RETURN NEW;
END $_$;

CREATE FUNCTION derive_tenant() RETURNS trigger
    LANGUAGE plpgsql
    AS $_$
DECLARE parent uuid;
BEGIN
 EXECUTE format('SELECT tenant_id FROM %I WHERE id=$1',TG_ARGV[0]) INTO parent USING (to_jsonb(NEW)->>TG_ARGV[1])::uuid;
 IF parent IS NULL OR (NEW.tenant_id IS NOT NULL AND NEW.tenant_id<>parent) THEN
  RAISE EXCEPTION 'cross-tenant reference' USING ERRCODE='23514';
 END IF;
 IF TG_OP='UPDATE' AND OLD.tenant_id<>parent THEN RAISE EXCEPTION 'tenant ownership is immutable'; END IF;
 NEW.tenant_id=parent; RETURN NEW;
END $_$;

CREATE FUNCTION freeze_build_access() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 NEW.planning_context=NEW.planning_context||jsonb_build_object('tenant_id',(SELECT tenant_id FROM projects WHERE id=NEW.project_id),'node_access',
 (SELECT jsonb_build_object('mode',node_access_mode,'ids',CASE WHEN node_access_mode='pools' THEN pool_ids ELSE node_ids END) FROM projects WHERE id=NEW.project_id));
 RETURN NEW;
END $$;

CREATE FUNCTION freeze_signing_defaults() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE item record; binding jsonb; bindings jsonb := '[]';
BEGIN
 FOR item IN SELECT d.key AS platform,d.value AS profile FROM projects p,LATERAL jsonb_each_text(p.signing_defaults) d WHERE p.id=NEW.project_id LOOP
  SELECT jsonb_build_object('profile',s.name,'platform',c.platform,'credential_id',c.id,'version_id',v.id,'metadata',v.metadata,'notarize',s.notarize)
   INTO binding FROM signing_profiles s JOIN signing_credentials c ON c.id=s.credential_id AND c.tenant_id=s.tenant_id
   JOIN signing_versions v ON v.id=(SELECT x.id FROM signing_versions x WHERE x.credential_id=c.id ORDER BY created_at DESC,id DESC LIMIT 1)
   JOIN projects p ON p.id=NEW.project_id AND p.tenant_id=s.tenant_id
   WHERE s.name=item.profile AND c.platform=item.platform AND s.enabled AND c.enabled AND v.enabled
   AND s.name=ANY(p.signing_profiles) AND (v.metadata->>'not_before')::bigint<=extract(epoch FROM clock_timestamp())
   AND (v.metadata->>'not_after')::bigint>extract(epoch FROM clock_timestamp())
   AND (NOT s.notarize OR (v.metadata->>'notarization_available')::boolean) FOR SHARE OF s,c,v;
  IF binding IS NULL THEN RAISE EXCEPTION 'default signing credential is unavailable' USING ERRCODE='55000'; END IF;
  bindings=bindings||jsonb_build_array(binding);
 END LOOP;
 NEW.planning_context=NEW.planning_context||jsonb_build_object('signing_defaults',bindings);
 RETURN NEW;
END $$;

CREATE FUNCTION image_tenant_guard() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE p uuid; t uuid;
BEGIN
 SELECT tenant_id INTO t FROM projects WHERE id=NEW.project_id;
 IF t IS DISTINCT FROM NEW.tenant_id THEN RAISE EXCEPTION 'image tenant mismatch'; END IF;
 IF TG_TABLE_NAME='image_executions' THEN
  SELECT b.project_id INTO p FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=NEW.job_id;
  IF p IS DISTINCT FROM NEW.project_id OR NOT EXISTS(SELECT 1 FROM source_manifests s WHERE s.id=NEW.source_manifest_id AND s.project_id=p AND s.tenant_id=t) THEN RAISE EXCEPTION 'image source mismatch'; END IF;
  IF NEW.destination_credential IS NOT NULL AND NOT EXISTS(SELECT 1 FROM image_credentials c WHERE c.id=NEW.destination_credential AND c.project_id=p AND c.tenant_id=t AND c.kind='registry') THEN RAISE EXCEPTION 'image credential mismatch'; END IF;
  IF NEW.parent_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM image_executions e WHERE e.id=NEW.parent_id AND e.project_id=p AND e.tenant_id=t AND e.job_id=NEW.job_id AND e.operation='build') THEN RAISE EXCEPTION 'image parent mismatch'; END IF;
 END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION immutable_signing_version() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 IF (to_jsonb(NEW)-'enabled') IS DISTINCT FROM (to_jsonb(OLD)-'enabled')
 THEN RAISE EXCEPTION 'signing versions are immutable' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION immutable_tenant() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id THEN RAISE EXCEPTION 'tenant ownership is immutable'; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION initialize_display_name() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 IF NEW.display_name='' THEN NEW.display_name:=NEW.username; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION notify_control_work() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    PERFORM signal_control_work();
    RETURN NULL;
END;
$$;

CREATE FUNCTION ota_release_admission() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM workspace_tasks WHERE tenant_id=NEW.tenant_id AND kind='release_import' AND object_hash=NEW.manifest_hash AND expires_at>clock_timestamp()) THEN PERFORM require_active_tenant(NEW.tenant_id); END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION persist_device_cancellation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.cancel_requested AND NOT OLD.cancel_requested THEN
        INSERT INTO device_commands(id,node_id,attempt_id,kind)
        SELECT gen_random_uuid(),a.node_id,a.id,'cancel' FROM attempts a JOIN jobs j ON j.id=a.job_id
        WHERE j.build_id=NEW.id AND a.status IN ('leased','running')
        ON CONFLICT(attempt_id,kind) DO NOTHING;
        PERFORM signal_control_work();
    END IF;
    RETURN NULL;
END;
$$;

CREATE FUNCTION persist_device_command() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP='INSERT' AND NEW.status='leased' THEN
        INSERT INTO device_commands(id,node_id,attempt_id,kind)
        VALUES(NEW.command_id,NEW.node_id,NEW.id,'assign');
    END IF;
    IF TG_OP='UPDATE' THEN
        UPDATE device_commands SET acknowledged_at=clock_timestamp()
        WHERE attempt_id=NEW.id AND acknowledged_at IS NULL
          AND (NEW.status NOT IN ('leased','running') OR (kind='assign' AND NEW.status='running'));
    END IF;
    PERFORM signal_control_work();
    RETURN NULL;
END;
$$;

CREATE FUNCTION project_configuration_admission() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 PERFORM require_active_project(NEW.id);
 RETURN NEW;
END $$;

CREATE FUNCTION project_membership_admission() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 PERFORM require_active_project(NEW.project_id);
 RETURN NEW;
END $$;


CREATE FUNCTION publish_runtime_intent() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 IF TG_OP='INSERT' THEN INSERT INTO runtime_intents(run_id,version,kind) VALUES(NEW.id,1,'create');
 ELSIF NEW.state='canceled' AND OLD.state<>'canceled' THEN INSERT INTO runtime_intents(run_id,version,kind) VALUES(NEW.id,2,'cancel') ON CONFLICT DO NOTHING; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION require_active_project(p uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE t uuid; s text;
BEGIN
 SELECT tenant_id INTO t FROM projects WHERE id=p;
 PERFORM require_active_tenant(t);
 SELECT status INTO s FROM projects WHERE id=p FOR SHARE;
 IF s IS DISTINCT FROM 'active' THEN RAISE EXCEPTION 'workspace is read only' USING ERRCODE='55000'; END IF;
END $$;

CREATE FUNCTION require_active_tenant(t uuid) RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE current_status text;
BEGIN
 SELECT status INTO current_status FROM tenants WHERE id=t FOR SHARE;
 IF current_status IS DISTINCT FROM 'active' THEN RAISE EXCEPTION 'workspace is read only' USING ERRCODE='55000'; END IF;
END $$;

CREATE FUNCTION reschedule_project_work() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    UPDATE jobs j SET dispatch_after=clock_timestamp() FROM builds b
    WHERE j.build_id=b.id AND b.project_id=NEW.id AND j.status='queued';
    PERFORM signal_control_work();
    RETURN NULL;
END;
$$;

CREATE FUNCTION signal_control_work() RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- NOTIFY serializes commits on a database-wide lock. Only one overlapping
    -- transaction sends a wakeup; durable work and periodic reconciliation
    -- also cover transactions that become visible after that wakeup.
    IF pg_try_advisory_xact_lock(hashtextextended('artifactory:work-notify',12)) THEN
        PERFORM pg_notify('artifactory_work','');
    END IF;
END;
$$;

CREATE FUNCTION source_execution_scope() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE p uuid;
BEGIN
 IF TG_TABLE_NAME='attempts' THEN SELECT b.project_id INTO p FROM jobs j JOIN builds b ON b.id=j.build_id WHERE j.id=NEW.job_id;
 ELSE SELECT project_id INTO p FROM builds WHERE id=NEW.build_id; END IF;
 IF NEW.source_manifest_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM source_manifests s WHERE s.id=NEW.source_manifest_id AND s.project_id=p AND s.tenant_id=NEW.tenant_id) THEN RAISE EXCEPTION 'source manifest mismatch' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION source_manifest_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$ BEGIN RAISE EXCEPTION 'source manifest is immutable' USING ERRCODE='23514'; END $$;

CREATE FUNCTION source_task_scope() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 IF NEW.build_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM builds WHERE id=NEW.build_id AND project_id=NEW.project_id AND tenant_id=NEW.tenant_id) THEN RAISE EXCEPTION 'source build mismatch' USING ERRCODE='23514'; END IF;
 IF EXISTS(SELECT 1 FROM jsonb_array_elements(NEW.bindings) b LEFT JOIN credentials c ON c.id=(b->>'credential_id')::uuid WHERE c.tenant_id IS DISTINCT FROM NEW.tenant_id) THEN RAISE EXCEPTION 'source credential mismatch' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION sync_project_grants() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
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

CREATE FUNCTION verify_artifact_signing() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE required boolean;
BEGIN
 SELECT EXISTS(SELECT 1 FROM attempts a JOIN jobs j ON j.id=a.job_id,LATERAL jsonb_array_elements(coalesce(j.target_config->'signing_outputs','[]')) o
  WHERE a.id=NEW.attempt_id AND o->>'path'=NEW.path) INTO required;
 IF required THEN
  SELECT evidence INTO NEW.signing FROM signing_evidence WHERE attempt_id=NEW.attempt_id AND path=NEW.path AND sha256=NEW.sha256;
  IF NEW.signing IS NULL THEN RAISE EXCEPTION 'matching signing evidence required' USING ERRCODE='23514'; END IF;
 END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION verify_tenant_owner() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE tenant uuid;
BEGIN
 tenant=coalesce((to_jsonb(NEW)->>CASE WHEN TG_TABLE_NAME='tenants' THEN 'id' ELSE 'tenant_id' END)::uuid,(to_jsonb(OLD)->>CASE WHEN TG_TABLE_NAME='tenants' THEN 'id' ELSE 'tenant_id' END)::uuid);
 IF EXISTS(SELECT 1 FROM tenants t WHERE t.id=tenant AND NOT EXISTS(SELECT 1 FROM tenant_members m WHERE m.tenant_id=t.id AND m.user_id=t.owner_id AND m.role='owner'))
 THEN RAISE EXCEPTION 'tenant must retain its owner'; END IF;
 RETURN NULL;
END $$;

CREATE FUNCTION workspace_admission() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
 PERFORM require_active_tenant(NEW.tenant_id);
 RETURN NEW;
END $$;

CREATE TABLE account_security_attempts (
    user_id uuid NOT NULL,
    attempted_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL
);

CREATE TABLE api_tokens (
    id uuid NOT NULL,
    user_id uuid NOT NULL,
    name text NOT NULL,
    token_hash text NOT NULL,
    project_ids uuid[] NOT NULL,
    scopes text[] NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE TABLE artifacts (
    id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    path text NOT NULL,
    object_key text NOT NULL,
    size bigint NOT NULL,
    sha256 text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL,
    signing jsonb
);

CREATE TABLE attempt_events (
    attempt_id uuid NOT NULL,
    sequence bigint NOT NULL,
    kind text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE TABLE attempts (
    id uuid NOT NULL,
    job_id uuid NOT NULL,
    node_id uuid NOT NULL,
    lease_token text NOT NULL,
    lease_expires_at timestamp with time zone NOT NULL,
    status text NOT NULL,
    command_id uuid NOT NULL,
    last_sequence bigint DEFAULT 0 NOT NULL,
    error text,
    started_at timestamp with time zone,
    finished_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    plan_revision_id uuid,
    source_manifest_id uuid,
    failure_kind text DEFAULT 'execution'::text NOT NULL,
    tenant_id uuid NOT NULL,
    CONSTRAINT attempts_failure_kind_check CHECK ((failure_kind = ANY (ARRAY['execution'::text, 'source'::text, 'authorization'::text, 'artifact_transport'::text, 'emissary'::text]))),
    CONSTRAINT attempts_status_check CHECK ((status = ANY (ARRAY['leased'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'canceled'::text, 'lost'::text])))
);

CREATE TABLE audit_events (
    id bigint NOT NULL,
    actor text NOT NULL,
    action text NOT NULL,
    resource_id text NOT NULL,
    details jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid
);

CREATE SEQUENCE audit_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE audit_events_id_seq OWNED BY audit_events.id;

CREATE TABLE builds (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    requested_ref text NOT NULL,
    commit_sha text,
    config_yaml text,
    config_snapshot jsonb,
    parameters jsonb DEFAULT '{}'::jsonb NOT NULL,
    selected_targets text[] DEFAULT '{}'::text[] NOT NULL,
    status text DEFAULT 'resolving'::text NOT NULL,
    error text,
    release_version text,
    publish_status text DEFAULT 'not_requested'::text NOT NULL,
    publish_error text,
    cancel_requested boolean DEFAULT false NOT NULL,
    requested_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    source_repository_url text NOT NULL,
    source_credential_id uuid,
    goal text,
    target_requirements jsonb DEFAULT '[]'::jsonb NOT NULL,
    planning_context jsonb DEFAULT '{}'::jsonb NOT NULL,
    model_config_snapshot jsonb,
    source_manifest_id uuid,
    revision_count integer DEFAULT 0 NOT NULL,
    clarification jsonb,
    current_plan_id uuid,
    tenant_id uuid NOT NULL,
    number bigint NOT NULL,
    CONSTRAINT builds_number_range CHECK (((number >= 1) AND (number <= '9007199254740991'::bigint))),
    CONSTRAINT builds_publish_status_check CHECK ((publish_status = ANY (ARRAY['not_requested'::text, 'pending'::text, 'succeeded'::text, 'failed'::text]))),
    CONSTRAINT builds_revision_count_check CHECK (((revision_count >= 0) AND (revision_count <= 2))),
    CONSTRAINT builds_status_check CHECK ((status = ANY (ARRAY['resolving'::text, 'planning'::text, 'needs_input'::text, 'replanning'::text, 'queued'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'canceling'::text, 'canceled'::text, 'lost'::text])))
);

CREATE TABLE credentials (
    id uuid NOT NULL,
    name text NOT NULL,
    kind text NOT NULL,
    ciphertext text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL,
    CONSTRAINT credentials_kind_check CHECK ((kind = ANY (ARRAY['ssh'::text, 'https'::text])))
);

CREATE TABLE deleted_projects (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    slug text NOT NULL,
    deleted_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE deleted_tenants (
    id uuid NOT NULL,
    slug text NOT NULL,
    deleted_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE device_commands (
    id uuid NOT NULL,
    node_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    kind text NOT NULL,
    next_delivery_at timestamp with time zone DEFAULT now() NOT NULL,
    acknowledged_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT device_commands_kind_check CHECK ((kind = ANY (ARRAY['assign'::text, 'cancel'::text])))
);

CREATE TABLE emissary_releases (
    id uuid NOT NULL,
    version text NOT NULL,
    manifest text NOT NULL,
    signature text NOT NULL,
    manifest_hash text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE TABLE emissary_updates (
    id uuid NOT NULL,
    generation bigint NOT NULL,
    node_id uuid NOT NULL,
    pool_id uuid NOT NULL,
    release_id uuid NOT NULL,
    previous_version text,
    status text DEFAULT 'queued'::text NOT NULL,
    cancel_requested boolean DEFAULT false NOT NULL,
    error text,
    retry_of uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    last_command_at timestamp with time zone,
    tenant_id uuid NOT NULL,
    CONSTRAINT emissary_updates_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'offered'::text, 'downloading'::text, 'draining'::text, 'activating'::text, 'succeeded'::text, 'failed'::text, 'rolled_back'::text, 'canceled'::text])))
);

CREATE SEQUENCE emissary_updates_generation_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE emissary_updates_generation_seq OWNED BY emissary_updates.generation;

CREATE TABLE enrollment_tokens (
    id uuid NOT NULL,
    token_hash text NOT NULL,
    pool_id uuid NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    used_at timestamp with time zone,
    tenant_id uuid NOT NULL
);

CREATE TABLE image_artifacts (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    job_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    repository text NOT NULL,
    digest text NOT NULL,
    platforms jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT image_artifacts_digest_check CHECK ((digest ~ '^sha256:[0-9a-f]{64}$'::text))
);

CREATE TABLE image_credentials (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    name text NOT NULL,
    kind text NOT NULL,
    envelope text NOT NULL,
    registry_origin text,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT image_credentials_check CHECK (((kind = 'registry'::text) = (registry_origin IS NOT NULL))),
    CONSTRAINT image_credentials_kind_check CHECK ((kind = ANY (ARRAY['registry'::text, 'secret'::text])))
);

CREATE TABLE image_executions (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    job_id uuid NOT NULL,
    source_manifest_id uuid NOT NULL,
    plan_revision_id uuid,
    parent_id uuid,
    operation text DEFAULT 'build'::text NOT NULL,
    status text DEFAULT 'preparing'::text NOT NULL,
    build_token_hash text NOT NULL,
    publish_token_hash text NOT NULL,
    task_envelope text,
    spec jsonb NOT NULL,
    results jsonb DEFAULT '[]'::jsonb NOT NULL,
    candidate_repository text NOT NULL,
    destination_repository text NOT NULL,
    destination_credential uuid,
    destination_registry text,
    tags text[] NOT NULL,
    root_digest text,
    published_digest text,
    image_build_uid text,
    error text,
    deadline_at timestamp with time zone NOT NULL,
    cancel_requested boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    last_dispatched_at timestamp with time zone,
    CONSTRAINT image_executions_operation_check CHECK ((operation = ANY (ARRAY['build'::text, 'publish'::text]))),
    CONSTRAINT image_executions_status_check CHECK ((status = ANY (ARRAY['preparing'::text, 'running'::text, 'publish_failed'::text, 'succeeded'::text, 'failed'::text, 'canceled'::text])))
);

CREATE TABLE image_logs (
    id bigint NOT NULL,
    execution_id uuid NOT NULL,
    platform text NOT NULL,
    sequence bigint NOT NULL,
    object_key text NOT NULL,
    content_hash text NOT NULL,
    CONSTRAINT image_logs_sequence_check CHECK ((sequence > 0))
);

ALTER TABLE image_logs ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME image_logs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);

CREATE TABLE image_platforms (
    execution_id uuid NOT NULL,
    platform text NOT NULL,
    pod_uid uuid NOT NULL,
    heartbeat_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT image_platforms_platform_check CHECK ((platform = ANY (ARRAY['linux/amd64'::text, 'linux/arm64'::text, 'publish'::text])))
);

CREATE TABLE job_placements (
    job_id uuid NOT NULL,
    placement jsonb NOT NULL,
    version bigint DEFAULT 1 NOT NULL,
    waiting_since timestamp with time zone,
    reason text,
    selected_node uuid,
    selected_reason text
);

CREATE TABLE jobs (
    id uuid NOT NULL,
    build_id uuid NOT NULL,
    target text NOT NULL,
    target_config jsonb NOT NULL,
    status text DEFAULT 'queued'::text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    plan_revision_id uuid,
    dispatch_after timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL,
    blocked_reason text,
    CONSTRAINT jobs_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'leased'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'canceled'::text, 'lost'::text])))
);

CREATE TABLE lifecycle_operations (
    id uuid NOT NULL,
    actor_id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid,
    slug text NOT NULL,
    action text NOT NULL,
    state text NOT NULL,
    phase text DEFAULT 'draining'::text NOT NULL,
    previous_status text NOT NULL,
    idempotency_key uuid NOT NULL,
    blockers jsonb DEFAULT '[]'::jsonb NOT NULL,
    counts jsonb DEFAULT '{}'::jsonb NOT NULL,
    manifest jsonb DEFAULT '{}'::jsonb NOT NULL,
    object_cursor bigint DEFAULT 0 NOT NULL,
    error text,
    lease_owner uuid,
    lease_until timestamp with time zone,
    generation bigint DEFAULT 0 NOT NULL,
    next_run_at timestamp with time zone DEFAULT now() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT lifecycle_operations_action_check CHECK ((action = ANY (ARRAY['archive'::text, 'restore'::text, 'delete'::text]))),
    CONSTRAINT lifecycle_operations_state_check CHECK ((state = ANY (ARRAY['waiting'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'canceled'::text])))
);

CREATE TABLE log_chunks (
    id bigint NOT NULL,
    attempt_id uuid NOT NULL,
    sequence bigint NOT NULL,
    step text NOT NULL,
    object_key text NOT NULL,
    sha256 text NOT NULL,
    size bigint NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE SEQUENCE log_chunks_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE log_chunks_id_seq OWNED BY log_chunks.id;

CREATE TABLE model_profiles (
    id uuid NOT NULL,
    name text NOT NULL,
    api text NOT NULL,
    base_url text NOT NULL,
    model text NOT NULL,
    key_ciphertext text NOT NULL,
    parameters jsonb DEFAULT '{}'::jsonb NOT NULL,
    revision integer DEFAULT 1 NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    max_calls integer DEFAULT 20 NOT NULL,
    timeout_seconds integer DEFAULT 600 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    next_call_at timestamp with time zone,
    tenant_id uuid NOT NULL,
    CONSTRAINT model_profiles_api_check CHECK ((api = ANY (ARRAY['openai-completions'::text, 'openai-responses'::text, 'anthropic-messages'::text]))),
    CONSTRAINT model_profiles_max_calls_check CHECK (((max_calls >= 1) AND (max_calls <= 100))),
    CONSTRAINT model_profiles_timeout_seconds_check CHECK (((timeout_seconds >= 30) AND (timeout_seconds <= 3600)))
);

CREATE TABLE node_pools (
    id uuid NOT NULL,
    name text NOT NULL,
    emissary_target_release uuid,
    emissary_update_concurrency integer DEFAULT 1 NOT NULL,
    emissary_updates_paused boolean DEFAULT false NOT NULL,
    tenant_id uuid NOT NULL,
    is_default boolean DEFAULT false NOT NULL,
    CONSTRAINT node_pools_emissary_update_concurrency_check CHECK (((emissary_update_concurrency >= 1) AND (emissary_update_concurrency <= 32)))
);

CREATE TABLE node_startups (
    node_id uuid NOT NULL,
    startup_id uuid NOT NULL,
    digest text NOT NULL,
    accepted_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL
);

CREATE TABLE nodes (
    id uuid NOT NULL,
    name text NOT NULL,
    os text NOT NULL,
    arch text NOT NULL,
    pool_id uuid NOT NULL,
    labels jsonb DEFAULT '{}'::jsonb NOT NULL,
    capabilities jsonb DEFAULT '[]'::jsonb NOT NULL,
    diagnostics jsonb DEFAULT '{}'::jsonb NOT NULL,
    signing_profiles text[] DEFAULT '{}'::text[] NOT NULL,
    slots integer DEFAULT 1 NOT NULL,
    draining boolean DEFAULT false NOT NULL,
    revoked boolean DEFAULT false NOT NULL,
    certificate_hash text NOT NULL,
    certificate_expires_at timestamp with time zone NOT NULL,
    active_session uuid,
    last_seen timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    emissary_target_release uuid,
    emissary_current_version text,
    emissary_launcher_version text,
    emissary_ota_status jsonb DEFAULT '{}'::jsonb NOT NULL,
    ota_draining boolean DEFAULT false NOT NULL,
    connection_owner uuid,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    startup_id uuid,
    metadata_digest text,
    metadata_received_at timestamp with time zone,
    tenant_id uuid NOT NULL,
    CONSTRAINT nodes_slots_check CHECK (((slots >= 1) AND (slots <= 32)))
)
WITH (fillfactor='80');

CREATE TABLE placement_rounds (
    build_id uuid NOT NULL,
    round integer NOT NULL,
    run_id uuid NOT NULL,
    versions jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    CONSTRAINT placement_rounds_round_check CHECK (((round >= 1) AND (round <= 2)))
);

CREATE TABLE plan_revisions (
    id uuid NOT NULL,
    build_id uuid NOT NULL,
    run_id uuid NOT NULL,
    revision integer NOT NULL,
    source_manifest_id uuid NOT NULL,
    contents jsonb NOT NULL,
    sha256 text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE TABLE planning_events (
    id bigint NOT NULL,
    run_id uuid NOT NULL,
    kind text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE SEQUENCE planning_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE planning_events_id_seq OWNED BY planning_events.id;

CREATE TABLE planning_runs (
    id uuid NOT NULL,
    build_id uuid NOT NULL,
    purpose text NOT NULL,
    state text DEFAULT 'queued'::text NOT NULL,
    failed_targets text[] DEFAULT '{}'::text[] NOT NULL,
    model_config jsonb NOT NULL,
    lease_hash text,
    lease_expires_at timestamp with time zone,
    deadline_at timestamp with time zone,
    calls integer DEFAULT 0 NOT NULL,
    usage jsonb DEFAULT '{}'::jsonb NOT NULL,
    error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    tenant_id uuid NOT NULL,
    CONSTRAINT planning_runs_purpose_check CHECK ((purpose = ANY (ARRAY['initial'::text, 'revision'::text, 'clarification'::text, 'placement'::text]))),
    CONSTRAINT planning_runs_state_check CHECK ((state = ANY (ARRAY['queued'::text, 'leased'::text, 'succeeded'::text, 'needs_input'::text, 'failed'::text, 'canceled'::text, 'lost'::text])))
);

CREATE TABLE project_members (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT project_members_role_check CHECK ((role = ANY (ARRAY['admin'::text, 'operator'::text, 'viewer'::text])))
);

CREATE TABLE project_node_grants (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    node_id uuid NOT NULL
);

CREATE TABLE project_pool_grants (
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    pool_id uuid NOT NULL
);

CREATE TABLE projects (
    id uuid NOT NULL,
    name text NOT NULL,
    repository_url text NOT NULL,
    credential_id uuid,
    pool_ids uuid[] DEFAULT '{}'::uuid[] NOT NULL,
    signing_profiles text[] DEFAULT '{}'::text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    model_profile_id uuid,
    build_context text DEFAULT ''::text NOT NULL,
    source_credentials jsonb DEFAULT '[]'::jsonb NOT NULL,
    tenant_id uuid NOT NULL,
    node_access_mode text DEFAULT 'pools'::text NOT NULL,
    node_ids uuid[] DEFAULT '{}'::uuid[] NOT NULL,
    slug text NOT NULL,
    next_build_number bigint DEFAULT 1 NOT NULL,
    status text DEFAULT 'active'::text NOT NULL,
    archived_at timestamp with time zone,
    signing_defaults jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT projects_node_access_mode_check CHECK ((node_access_mode = ANY (ARRAY['pools'::text, 'nodes'::text]))),
    CONSTRAINT projects_slug_format CHECK (((length(slug) <= 63) AND (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'::text))),
    CONSTRAINT projects_status_check CHECK ((status = ANY (ARRAY['active'::text, 'draining'::text, 'archived'::text, 'deleting'::text])))
);

CREATE TABLE publish_attempts (
    id uuid NOT NULL,
    build_id uuid NOT NULL,
    status text NOT NULL,
    error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE TABLE release_artifacts (
    release_id uuid NOT NULL,
    artifact_id uuid NOT NULL
);

CREATE TABLE release_images (
    release_id uuid NOT NULL,
    image_id uuid NOT NULL
);

CREATE TABLE releases (
    id uuid NOT NULL,
    project_id uuid NOT NULL,
    build_id uuid NOT NULL,
    version text NOT NULL,
    idempotency_key text NOT NULL,
    commit_sha text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tenant_id uuid NOT NULL
);

CREATE TABLE runtime_intents (
    id bigint NOT NULL,
    run_id uuid NOT NULL,
    version bigint NOT NULL,
    kind text NOT NULL,
    payload_ciphertext text,
    delivered_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE runtime_intents_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE runtime_intents_id_seq OWNED BY runtime_intents.id;

CREATE TABLE runtime_receipts (
    event_id uuid NOT NULL,
    input_hash text NOT NULL,
    result jsonb NOT NULL,
    run_id uuid NOT NULL,
    generation bigint,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE sessions (
    token_hash text NOT NULL,
    user_id uuid NOT NULL,
    expires_at timestamp with time zone NOT NULL
);

CREATE TABLE shared_object_deletions (
    hash text NOT NULL,
    operation_id uuid NOT NULL
);

CREATE TABLE signing_credentials (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    name text NOT NULL,
    platform text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT signing_credentials_platform_check CHECK ((platform = ANY (ARRAY['macos'::text, 'android'::text])))
);

CREATE TABLE signing_evidence (
    tenant_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    path text NOT NULL,
    sha256 text NOT NULL,
    evidence jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE signing_profiles (
    tenant_id uuid NOT NULL,
    name text NOT NULL,
    credential_id uuid NOT NULL,
    notarize boolean DEFAULT false NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE signing_versions (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    credential_id uuid NOT NULL,
    metadata jsonb NOT NULL,
    envelope jsonb NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE source_manifests (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    commit_sha text NOT NULL,
    manifest jsonb NOT NULL,
    digest text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT source_manifests_commit_sha_check CHECK ((commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'::text)),
    CONSTRAINT source_manifests_digest_check CHECK ((digest ~ '^[0-9a-f]{64}$'::text))
);

CREATE TABLE source_tasks (
    id uuid NOT NULL,
    source_task_uid text,
    source_execution jsonb,
    tenant_id uuid NOT NULL,
    project_id uuid NOT NULL,
    build_id uuid,
    purpose text NOT NULL,
    repository_url text NOT NULL,
    reference text NOT NULL,
    credential_id uuid,
    bindings jsonb DEFAULT '[]'::jsonb NOT NULL,
    pinned_ref_oid text,
    frozen_tags jsonb DEFAULT '{}'::jsonb NOT NULL,
    status text DEFAULT 'queued'::text NOT NULL,
    result jsonb,
    error text,
    deadline_at timestamp with time zone NOT NULL,
    cleanup_requested boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT source_tasks_check CHECK (((purpose = 'resolve'::text) = (build_id IS NOT NULL))),
    CONSTRAINT source_tasks_pinned_ref_oid_check CHECK ((pinned_ref_oid ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'::text)),
    CONSTRAINT source_tasks_purpose_check CHECK ((purpose = ANY (ARRAY['resolve'::text, 'refs'::text]))),
    CONSTRAINT source_tasks_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'canceled'::text])))
);

CREATE TABLE step_runs (
    attempt_id uuid NOT NULL,
    step_index integer NOT NULL,
    name text NOT NULL,
    phase text NOT NULL,
    status text NOT NULL,
    exit_code integer,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    tenant_id uuid NOT NULL
);

CREATE TABLE tenant_invitations (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    token_hash text NOT NULL,
    role text NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone DEFAULT (now() + '72:00:00'::interval) NOT NULL,
    revoked_at timestamp with time zone,
    accepted_at timestamp with time zone,
    accepted_by uuid,
    CONSTRAINT tenant_invitations_role_check CHECK ((role = ANY (ARRAY['admin'::text, 'member'::text])))
);

CREATE TABLE tenant_members (
    tenant_id uuid NOT NULL,
    user_id uuid NOT NULL,
    role text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT tenant_members_role_check CHECK ((role = ANY (ARRAY['owner'::text, 'admin'::text, 'member'::text])))
);

CREATE TABLE tenants (
    id uuid NOT NULL,
    name text NOT NULL,
    owner_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid NOT NULL,
    creation_key uuid NOT NULL,
    slug text NOT NULL,
    status text DEFAULT 'active'::text NOT NULL,
    archived_at timestamp with time zone,
    CONSTRAINT tenants_name_check CHECK (((length(name) >= 1) AND (length(name) <= 100))),
    CONSTRAINT tenants_slug_format CHECK (((length(slug) <= 63) AND (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'::text))),
    CONSTRAINT tenants_status_check CHECK ((status = ANY (ARRAY['active'::text, 'draining'::text, 'archived'::text, 'deleting'::text])))
);

CREATE TABLE uploads (
    id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    path text NOT NULL,
    object_key text NOT NULL,
    expected_size bigint NOT NULL,
    expected_sha256 text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    tenant_id uuid NOT NULL,
    CONSTRAINT uploads_expected_size_check CHECK (((expected_size > 0) AND (expected_size <= '21474836480'::bigint)))
);

CREATE TABLE users (
    id uuid NOT NULL,
    username text NOT NULL,
    password_hash text NOT NULL,
    role text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    display_name text DEFAULT ''::text NOT NULL,
    CONSTRAINT display_name_length CHECK (((char_length(display_name) >= 1) AND (char_length(display_name) <= 100))),
    CONSTRAINT users_role_check CHECK ((role = ANY (ARRAY['admin'::text, 'operator'::text, 'viewer'::text])))
);

CREATE TABLE workspace_tasks (
    id uuid NOT NULL,
    tenant_id uuid NOT NULL,
    project_id uuid,
    kind text NOT NULL,
    object_hash text,
    expires_at timestamp with time zone NOT NULL
);

ALTER TABLE ONLY audit_events ALTER COLUMN id SET DEFAULT nextval('audit_events_id_seq'::regclass);

ALTER TABLE ONLY emissary_updates ALTER COLUMN generation SET DEFAULT nextval('emissary_updates_generation_seq'::regclass);

ALTER TABLE ONLY log_chunks ALTER COLUMN id SET DEFAULT nextval('log_chunks_id_seq'::regclass);

ALTER TABLE ONLY planning_events ALTER COLUMN id SET DEFAULT nextval('planning_events_id_seq'::regclass);

ALTER TABLE ONLY runtime_intents ALTER COLUMN id SET DEFAULT nextval('runtime_intents_id_seq'::regclass);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_pkey PRIMARY KEY (id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_token_hash_key UNIQUE (token_hash);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_attempt_id_path_key UNIQUE (attempt_id, path);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_object_key_key UNIQUE (object_key);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY attempt_events
    ADD CONSTRAINT attempt_events_pkey PRIMARY KEY (attempt_id, sequence);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_tenant_identity UNIQUE (tenant_id, id);

ALTER TABLE ONLY audit_events
    ADD CONSTRAINT audit_events_pkey PRIMARY KEY (id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_pkey PRIMARY KEY (id);

ALTER TABLE ONLY credentials
    ADD CONSTRAINT credentials_pkey PRIMARY KEY (id);

ALTER TABLE ONLY deleted_projects
    ADD CONSTRAINT deleted_projects_pkey PRIMARY KEY (id);

ALTER TABLE ONLY deleted_projects
    ADD CONSTRAINT deleted_projects_tenant_id_slug_key UNIQUE (tenant_id, slug);

ALTER TABLE ONLY deleted_tenants
    ADD CONSTRAINT deleted_tenants_pkey PRIMARY KEY (id);

ALTER TABLE ONLY deleted_tenants
    ADD CONSTRAINT deleted_tenants_slug_key UNIQUE (slug);

ALTER TABLE ONLY device_commands
    ADD CONSTRAINT device_commands_attempt_id_kind_key UNIQUE (attempt_id, kind);

ALTER TABLE ONLY device_commands
    ADD CONSTRAINT device_commands_pkey PRIMARY KEY (id);

ALTER TABLE ONLY emissary_releases
    ADD CONSTRAINT emissary_releases_pkey PRIMARY KEY (id);

ALTER TABLE ONLY emissary_releases
    ADD CONSTRAINT emissary_releases_tenant_id_manifest_hash_key UNIQUE (tenant_id, manifest_hash);

ALTER TABLE ONLY emissary_releases
    ADD CONSTRAINT emissary_releases_tenant_id_version_key UNIQUE (tenant_id, version);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_generation_key UNIQUE (generation);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_pkey PRIMARY KEY (id);

ALTER TABLE ONLY enrollment_tokens
    ADD CONSTRAINT enrollment_tokens_pkey PRIMARY KEY (id);

ALTER TABLE ONLY enrollment_tokens
    ADD CONSTRAINT enrollment_tokens_token_hash_key UNIQUE (token_hash);

ALTER TABLE ONLY image_artifacts
    ADD CONSTRAINT image_artifacts_execution_id_key UNIQUE (execution_id);

ALTER TABLE ONLY image_artifacts
    ADD CONSTRAINT image_artifacts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY image_credentials
    ADD CONSTRAINT image_credentials_pkey PRIMARY KEY (id);

ALTER TABLE ONLY image_credentials
    ADD CONSTRAINT image_credentials_project_id_name_key UNIQUE (project_id, name);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY image_logs
    ADD CONSTRAINT image_logs_execution_id_platform_sequence_key UNIQUE (execution_id, platform, sequence);

ALTER TABLE ONLY image_logs
    ADD CONSTRAINT image_logs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY image_platforms
    ADD CONSTRAINT image_platforms_pkey PRIMARY KEY (execution_id, platform);

ALTER TABLE ONLY job_placements
    ADD CONSTRAINT job_placements_pkey PRIMARY KEY (job_id);

ALTER TABLE ONLY jobs
    ADD CONSTRAINT jobs_build_id_target_key UNIQUE (build_id, target);

ALTER TABLE ONLY jobs
    ADD CONSTRAINT jobs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY lifecycle_operations
    ADD CONSTRAINT lifecycle_operations_actor_id_idempotency_key_key UNIQUE (actor_id, idempotency_key);

ALTER TABLE ONLY lifecycle_operations
    ADD CONSTRAINT lifecycle_operations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY log_chunks
    ADD CONSTRAINT log_chunks_attempt_id_sequence_key UNIQUE (attempt_id, sequence);

ALTER TABLE ONLY log_chunks
    ADD CONSTRAINT log_chunks_object_key_key UNIQUE (object_key);

ALTER TABLE ONLY log_chunks
    ADD CONSTRAINT log_chunks_pkey PRIMARY KEY (id);

ALTER TABLE ONLY model_profiles
    ADD CONSTRAINT model_profiles_pkey PRIMARY KEY (id);

ALTER TABLE ONLY model_profiles
    ADD CONSTRAINT model_profiles_tenant_id_name_key UNIQUE (tenant_id, name);

ALTER TABLE ONLY node_pools
    ADD CONSTRAINT node_pools_pkey PRIMARY KEY (id);

ALTER TABLE ONLY node_pools
    ADD CONSTRAINT node_pools_tenant_id_name_key UNIQUE (tenant_id, name);

ALTER TABLE ONLY node_startups
    ADD CONSTRAINT node_startups_pkey PRIMARY KEY (node_id, startup_id);

ALTER TABLE ONLY nodes
    ADD CONSTRAINT nodes_certificate_hash_key UNIQUE (certificate_hash);

ALTER TABLE ONLY nodes
    ADD CONSTRAINT nodes_pkey PRIMARY KEY (id);

ALTER TABLE ONLY placement_rounds
    ADD CONSTRAINT placement_rounds_pkey PRIMARY KEY (build_id, round);

ALTER TABLE ONLY placement_rounds
    ADD CONSTRAINT placement_rounds_run_id_key UNIQUE (run_id);

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_build_id_revision_key UNIQUE (build_id, revision);

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_run_id_key UNIQUE (run_id);

ALTER TABLE ONLY planning_events
    ADD CONSTRAINT planning_events_pkey PRIMARY KEY (id);

ALTER TABLE ONLY planning_runs
    ADD CONSTRAINT planning_runs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY project_members
    ADD CONSTRAINT project_members_pkey PRIMARY KEY (project_id, user_id);

ALTER TABLE ONLY project_node_grants
    ADD CONSTRAINT project_node_grants_pkey PRIMARY KEY (project_id, node_id);

ALTER TABLE ONLY project_pool_grants
    ADD CONSTRAINT project_pool_grants_pkey PRIMARY KEY (project_id, pool_id);

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_pkey PRIMARY KEY (id);

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_tenant_identity UNIQUE (tenant_id, id);

ALTER TABLE ONLY publish_attempts
    ADD CONSTRAINT publish_attempts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY release_artifacts
    ADD CONSTRAINT release_artifacts_pkey PRIMARY KEY (release_id, artifact_id);

ALTER TABLE ONLY release_images
    ADD CONSTRAINT release_images_pkey PRIMARY KEY (release_id, image_id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_pkey PRIMARY KEY (id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_project_id_idempotency_key_key UNIQUE (project_id, idempotency_key);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_project_id_version_key UNIQUE (project_id, version);

ALTER TABLE ONLY runtime_intents
    ADD CONSTRAINT runtime_intents_pkey PRIMARY KEY (id);

ALTER TABLE ONLY runtime_intents
    ADD CONSTRAINT runtime_intents_run_id_version_key UNIQUE (run_id, version);

ALTER TABLE ONLY runtime_receipts
    ADD CONSTRAINT runtime_receipts_pkey PRIMARY KEY (event_id);

ALTER TABLE ONLY sessions
    ADD CONSTRAINT sessions_pkey PRIMARY KEY (token_hash);

ALTER TABLE ONLY shared_object_deletions
    ADD CONSTRAINT shared_object_deletions_pkey PRIMARY KEY (hash);

ALTER TABLE ONLY signing_credentials
    ADD CONSTRAINT signing_credentials_pkey PRIMARY KEY (id);

ALTER TABLE ONLY signing_credentials
    ADD CONSTRAINT signing_credentials_tenant_id_id_key UNIQUE (tenant_id, id);

ALTER TABLE ONLY signing_credentials
    ADD CONSTRAINT signing_credentials_tenant_id_name_key UNIQUE (tenant_id, name);

ALTER TABLE ONLY signing_evidence
    ADD CONSTRAINT signing_evidence_pkey PRIMARY KEY (attempt_id, path);

ALTER TABLE ONLY signing_profiles
    ADD CONSTRAINT signing_profiles_pkey PRIMARY KEY (tenant_id, name);

ALTER TABLE ONLY signing_versions
    ADD CONSTRAINT signing_versions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY signing_versions
    ADD CONSTRAINT signing_versions_tenant_id_id_key UNIQUE (tenant_id, id);

ALTER TABLE ONLY source_manifests
    ADD CONSTRAINT source_manifests_pkey PRIMARY KEY (id);

ALTER TABLE ONLY source_manifests
    ADD CONSTRAINT source_manifests_tenant_id_project_id_id_key UNIQUE (tenant_id, project_id, id);

ALTER TABLE ONLY source_tasks
    ADD CONSTRAINT source_tasks_pkey PRIMARY KEY (id);

ALTER TABLE ONLY step_runs
    ADD CONSTRAINT step_runs_pkey PRIMARY KEY (attempt_id, step_index);

ALTER TABLE ONLY tenant_invitations
    ADD CONSTRAINT tenant_invitations_pkey PRIMARY KEY (id);

ALTER TABLE ONLY tenant_invitations
    ADD CONSTRAINT tenant_invitations_token_hash_key UNIQUE (token_hash);

ALTER TABLE ONLY tenant_members
    ADD CONSTRAINT tenant_members_pkey PRIMARY KEY (tenant_id, user_id);

ALTER TABLE ONLY tenants
    ADD CONSTRAINT tenants_created_by_creation_key_key UNIQUE (created_by, creation_key);

ALTER TABLE ONLY tenants
    ADD CONSTRAINT tenants_pkey PRIMARY KEY (id);

ALTER TABLE ONLY uploads
    ADD CONSTRAINT uploads_attempt_id_path_key UNIQUE (attempt_id, path);

ALTER TABLE ONLY uploads
    ADD CONSTRAINT uploads_object_key_key UNIQUE (object_key);

ALTER TABLE ONLY uploads
    ADD CONSTRAINT uploads_pkey PRIMARY KEY (id);

ALTER TABLE ONLY users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);

ALTER TABLE ONLY users
    ADD CONSTRAINT users_username_key UNIQUE (username);

ALTER TABLE ONLY workspace_tasks
    ADD CONSTRAINT workspace_tasks_pkey PRIMARY KEY (id);

CREATE INDEX account_security_attempts_recent ON account_security_attempts USING btree (user_id, attempted_at);

CREATE INDEX acknowledged_device_commands ON device_commands USING btree (acknowledged_at) WHERE (acknowledged_at IS NOT NULL);

CREATE INDEX active_builds ON builds USING btree (id) WHERE (status = ANY (ARRAY['queued'::text, 'running'::text, 'canceling'::text]));

CREATE INDEX active_node_attempts ON attempts USING btree (node_id, lease_expires_at) WHERE (status = ANY (ARRAY['leased'::text, 'running'::text]));

CREATE INDEX api_tokens_tenant ON api_tokens USING btree (tenant_id);

CREATE INDEX artifacts_tenant ON artifacts USING btree (tenant_id);

CREATE INDEX attempt_events_tenant ON attempt_events USING btree (tenant_id);

CREATE INDEX attempts_tenant ON attempts USING btree (tenant_id);

CREATE INDEX audit_tenant ON audit_events USING btree (tenant_id, id DESC);

CREATE INDEX available_nodes ON nodes USING btree (pool_id, os, arch) WHERE ((NOT revoked) AND (NOT draining) AND (NOT ota_draining));

CREATE UNIQUE INDEX builds_project_number_key ON builds USING btree (project_id, number);

CREATE INDEX builds_tenant ON builds USING btree (tenant_id);

CREATE INDEX credentials_tenant ON credentials USING btree (tenant_id);

CREATE INDEX dispatch_queue ON jobs USING btree (dispatch_after, created_at, id) WHERE (status = 'queued'::text);

CREATE INDEX emissary_releases_tenant ON emissary_releases USING btree (tenant_id);

CREATE UNIQUE INDEX emissary_updates_one_active_node ON emissary_updates USING btree (node_id) WHERE (finished_at IS NULL);

CREATE INDEX emissary_updates_pool_active ON emissary_updates USING btree (pool_id) WHERE (finished_at IS NULL);

CREATE INDEX emissary_updates_tenant ON emissary_updates USING btree (tenant_id);

CREATE INDEX enrollment_tokens_tenant ON enrollment_tokens USING btree (tenant_id);

CREATE INDEX image_dispatch ON image_executions USING btree (created_at) WHERE (status = ANY (ARRAY['preparing'::text, 'running'::text]));

CREATE UNIQUE INDEX image_one_active_build ON image_executions USING btree (job_id) WHERE ((operation = 'build'::text) AND (status = ANY (ARRAY['preparing'::text, 'running'::text])));

CREATE UNIQUE INDEX image_one_active_publication ON image_executions USING btree (parent_id) WHERE ((operation = 'publish'::text) AND (destination_credential IS NULL) AND (status = ANY (ARRAY['preparing'::text, 'running'::text])));

CREATE INDEX invitations_tenant ON tenant_invitations USING btree (tenant_id, created_at);

CREATE INDEX job_placements_waiting ON job_placements USING btree (waiting_since) WHERE (waiting_since IS NOT NULL);

CREATE INDEX jobs_tenant ON jobs USING btree (tenant_id);

CREATE UNIQUE INDEX lifecycle_one_active_resource ON lifecycle_operations USING btree (tenant_id, COALESCE(project_id, '00000000-0000-0000-0000-000000000000'::uuid)) WHERE (state = ANY (ARRAY['waiting'::text, 'running'::text, 'failed'::text]));

CREATE INDEX lifecycle_pending ON lifecycle_operations USING btree (next_run_at, created_at) WHERE (state = ANY (ARRAY['waiting'::text, 'running'::text]));

CREATE INDEX log_chunks_tenant ON log_chunks USING btree (tenant_id);

CREATE INDEX model_profiles_tenant ON model_profiles USING btree (tenant_id);

CREATE INDEX node_pools_tenant ON node_pools USING btree (tenant_id);

CREATE INDEX nodes_tenant ON nodes USING btree (tenant_id);

CREATE UNIQUE INDEX one_active_attempt ON attempts USING btree (job_id) WHERE (status = ANY (ARRAY['leased'::text, 'running'::text]));

CREATE UNIQUE INDEX one_default_model ON model_profiles USING btree (tenant_id) WHERE is_default;

CREATE UNIQUE INDEX one_pending_planning_run ON planning_runs USING btree (build_id) WHERE (state = ANY (ARRAY['queued'::text, 'leased'::text]));

CREATE INDEX owned_connections ON nodes USING btree (connection_owner, id) WHERE (connection_owner IS NOT NULL);

CREATE INDEX pending_device_commands ON device_commands USING btree (node_id, next_delivery_at) WHERE (acknowledged_at IS NULL);

CREATE INDEX plan_revisions_tenant ON plan_revisions USING btree (tenant_id);

CREATE INDEX planning_events_tenant ON planning_events USING btree (tenant_id);

CREATE INDEX planning_runs_tenant ON planning_runs USING btree (tenant_id);

CREATE INDEX project_members_user ON project_members USING btree (tenant_id, user_id);

CREATE INDEX projects_tenant ON projects USING btree (tenant_id);

CREATE UNIQUE INDEX projects_tenant_slug_key ON projects USING btree (tenant_id, slug);

CREATE INDEX publish_attempts_tenant ON publish_attempts USING btree (tenant_id);

CREATE INDEX registration_attempts ON audit_events USING btree (created_at) WHERE (action = 'registration_attempt'::text);

CREATE INDEX releases_tenant ON releases USING btree (tenant_id);

CREATE INDEX scheduling_jobs ON jobs USING btree (created_at) WHERE (status = 'queued'::text);

CREATE INDEX signing_versions_latest ON signing_versions USING btree (credential_id, created_at DESC, id DESC);

CREATE INDEX source_tasks_pending ON source_tasks USING btree (created_at) WHERE ((status = ANY (ARRAY['queued'::text, 'running'::text])) OR (NOT cleanup_requested));

CREATE INDEX step_runs_tenant ON step_runs USING btree (tenant_id);

CREATE UNIQUE INDEX tenant_default_pool ON node_pools USING btree (tenant_id) WHERE is_default;

CREATE UNIQUE INDEX tenant_one_owner ON tenant_members USING btree (tenant_id) WHERE (role = 'owner'::text);

CREATE UNIQUE INDEX tenants_slug_key ON tenants USING btree (slug);

CREATE INDEX uploads_tenant ON uploads USING btree (tenant_id);

CREATE TRIGGER account_display_name BEFORE INSERT ON users FOR EACH ROW EXECUTE FUNCTION initialize_display_name();

CREATE TRIGGER attempt_command AFTER INSERT OR UPDATE OF status ON attempts FOR EACH ROW EXECUTE FUNCTION persist_device_command();

CREATE TRIGGER build_cancellation AFTER UPDATE OF cancel_requested ON builds FOR EACH ROW EXECUTE FUNCTION persist_device_cancellation();

CREATE TRIGGER build_number BEFORE INSERT OR UPDATE OF number, project_id ON builds FOR EACH ROW EXECUTE FUNCTION assign_build_number();

CREATE TRIGGER freeze_device_access BEFORE INSERT ON builds FOR EACH ROW EXECUTE FUNCTION freeze_build_access();

CREATE TRIGGER freeze_signing_defaults BEFORE INSERT ON builds FOR EACH ROW EXECUTE FUNCTION freeze_signing_defaults();

CREATE TRIGGER image_tenant_guard BEFORE INSERT OR UPDATE ON image_credentials FOR EACH ROW EXECUTE FUNCTION image_tenant_guard();

CREATE TRIGGER image_tenant_guard BEFORE INSERT OR UPDATE ON image_executions FOR EACH ROW EXECUTE FUNCTION image_tenant_guard();

CREATE TRIGGER node_connection_work AFTER UPDATE OF draining, revoked, slots, pool_id ON nodes FOR EACH STATEMENT EXECUTE FUNCTION notify_control_work();

CREATE TRIGGER project_grants AFTER INSERT OR UPDATE OF pool_ids, node_ids, node_access_mode, source_credentials ON projects FOR EACH ROW EXECUTE FUNCTION sync_project_grants();

CREATE TRIGGER project_scheduling_changed AFTER UPDATE OF signing_profiles, pool_ids ON projects FOR EACH ROW EXECUTE FUNCTION reschedule_project_work();

CREATE TRIGGER queued_job_work AFTER INSERT OR UPDATE OF status ON jobs FOR EACH STATEMENT EXECUTE FUNCTION notify_control_work();

CREATE TRIGGER route_slug BEFORE INSERT OR UPDATE OF slug, tenant_id ON projects FOR EACH ROW EXECUTE FUNCTION assign_route_slug();

CREATE TRIGGER route_slug BEFORE INSERT OR UPDATE OF slug ON tenants FOR EACH ROW EXECUTE FUNCTION assign_route_slug();

CREATE TRIGGER runtime_outbox AFTER INSERT OR UPDATE OF state ON planning_runs FOR EACH ROW EXECUTE FUNCTION publish_runtime_intent();

CREATE TRIGGER signing_version_immutable BEFORE UPDATE ON signing_versions FOR EACH ROW EXECUTE FUNCTION immutable_signing_version();

CREATE TRIGGER z_source_execution_scope BEFORE INSERT OR UPDATE OF tenant_id, job_id, source_manifest_id ON attempts FOR EACH ROW EXECUTE FUNCTION source_execution_scope();

CREATE TRIGGER z_source_execution_scope BEFORE INSERT OR UPDATE OF tenant_id, build_id, source_manifest_id ON plan_revisions FOR EACH ROW EXECUTE FUNCTION source_execution_scope();

CREATE TRIGGER source_manifest_immutable BEFORE UPDATE ON source_manifests FOR EACH ROW EXECUTE FUNCTION source_manifest_immutable();

CREATE TRIGGER z_source_task_scope BEFORE INSERT OR UPDATE OF tenant_id, project_id, build_id, bindings ON source_tasks FOR EACH ROW EXECUTE FUNCTION source_task_scope();

CREATE TRIGGER source_tasks_admission BEFORE INSERT ON source_tasks FOR EACH ROW EXECUTE FUNCTION project_membership_admission();

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, attempt_id ON artifacts FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts', 'attempt_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, attempt_id ON attempt_events FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts', 'attempt_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, job_id ON attempts FOR EACH ROW EXECUTE FUNCTION derive_tenant('jobs', 'job_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, project_id ON builds FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects', 'project_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, node_id ON emissary_updates FOR EACH ROW EXECUTE FUNCTION derive_tenant('nodes', 'node_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, pool_id ON enrollment_tokens FOR EACH ROW EXECUTE FUNCTION derive_tenant('node_pools', 'pool_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, build_id ON jobs FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds', 'build_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, attempt_id ON log_chunks FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts', 'attempt_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, pool_id ON nodes FOR EACH ROW EXECUTE FUNCTION derive_tenant('node_pools', 'pool_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, build_id ON plan_revisions FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds', 'build_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, run_id ON planning_events FOR EACH ROW EXECUTE FUNCTION derive_tenant('planning_runs', 'run_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, build_id ON planning_runs FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds', 'build_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, build_id ON publish_attempts FOR EACH ROW EXECUTE FUNCTION derive_tenant('builds', 'build_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, project_id ON releases FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects', 'project_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, project_id ON source_manifests FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects', 'project_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, project_id ON source_tasks FOR EACH ROW EXECUTE FUNCTION derive_tenant('projects', 'project_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, attempt_id ON step_runs FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts', 'attempt_id');

CREATE TRIGGER tenant_derive BEFORE INSERT OR UPDATE OF tenant_id, attempt_id ON uploads FOR EACH ROW EXECUTE FUNCTION derive_tenant('attempts', 'attempt_id');

CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON api_tokens FOR EACH ROW EXECUTE FUNCTION immutable_tenant();

CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON credentials FOR EACH ROW EXECUTE FUNCTION immutable_tenant();

CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON emissary_releases FOR EACH ROW EXECUTE FUNCTION immutable_tenant();

CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON model_profiles FOR EACH ROW EXECUTE FUNCTION immutable_tenant();

CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON node_pools FOR EACH ROW EXECUTE FUNCTION immutable_tenant();

CREATE TRIGGER tenant_immutable BEFORE UPDATE OF tenant_id ON projects FOR EACH ROW EXECUTE FUNCTION immutable_tenant();

CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON device_commands FOR EACH ROW EXECUTE FUNCTION check_link_tenants('attempts', 'attempt_id', 'nodes', 'node_id');

CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON placement_rounds FOR EACH ROW EXECUTE FUNCTION check_link_tenants('builds', 'build_id', 'planning_runs', 'run_id');

CREATE TRIGGER tenant_link BEFORE INSERT OR UPDATE ON release_artifacts FOR EACH ROW EXECUTE FUNCTION check_link_tenants('releases', 'release_id', 'artifacts', 'artifact_id');

CREATE CONSTRAINT TRIGGER tenant_owner_check AFTER INSERT OR DELETE OR UPDATE ON tenant_members DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION verify_tenant_owner();

CREATE CONSTRAINT TRIGGER tenant_owner_check AFTER INSERT OR UPDATE ON tenants DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION verify_tenant_owner();

CREATE TRIGGER tenant_ref_build_id BEFORE INSERT OR UPDATE OF tenant_id, build_id ON releases FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('builds', 'build_id');

CREATE TRIGGER tenant_ref_credential_id BEFORE INSERT OR UPDATE OF tenant_id, credential_id ON projects FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('credentials', 'credential_id');

CREATE TRIGGER tenant_ref_credential_id BEFORE INSERT OR UPDATE OF tenant_id, credential_id ON source_tasks FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('credentials', 'credential_id');

CREATE TRIGGER tenant_ref_current_plan_id BEFORE INSERT OR UPDATE OF tenant_id, current_plan_id ON builds FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('plan_revisions', 'current_plan_id');

CREATE TRIGGER tenant_ref_emissary_target_release BEFORE INSERT OR UPDATE OF tenant_id, emissary_target_release ON node_pools FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_releases', 'emissary_target_release');

CREATE TRIGGER tenant_ref_emissary_target_release BEFORE INSERT OR UPDATE OF tenant_id, emissary_target_release ON nodes FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_releases', 'emissary_target_release');

CREATE TRIGGER tenant_ref_model_profile_id BEFORE INSERT OR UPDATE OF tenant_id, model_profile_id ON projects FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('model_profiles', 'model_profile_id');

CREATE TRIGGER tenant_ref_node_id BEFORE INSERT OR UPDATE OF tenant_id, node_id ON attempts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('nodes', 'node_id');

CREATE TRIGGER tenant_ref_node_id BEFORE INSERT OR UPDATE OF tenant_id, node_id ON project_node_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('nodes', 'node_id');

CREATE TRIGGER tenant_ref_plan_revision_id BEFORE INSERT OR UPDATE OF tenant_id, plan_revision_id ON attempts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('plan_revisions', 'plan_revision_id');

CREATE TRIGGER tenant_ref_plan_revision_id BEFORE INSERT OR UPDATE OF tenant_id, plan_revision_id ON jobs FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('plan_revisions', 'plan_revision_id');

CREATE TRIGGER tenant_ref_pool_id BEFORE INSERT OR UPDATE OF tenant_id, pool_id ON emissary_updates FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('node_pools', 'pool_id');

CREATE TRIGGER tenant_ref_pool_id BEFORE INSERT OR UPDATE OF tenant_id, pool_id ON project_pool_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('node_pools', 'pool_id');

CREATE TRIGGER tenant_ref_project_id BEFORE INSERT OR UPDATE OF tenant_id, project_id ON project_members FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('projects', 'project_id');

CREATE TRIGGER tenant_ref_project_id BEFORE INSERT OR UPDATE OF tenant_id, project_id ON project_node_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('projects', 'project_id');

CREATE TRIGGER tenant_ref_project_id BEFORE INSERT OR UPDATE OF tenant_id, project_id ON project_pool_grants FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('projects', 'project_id');

CREATE TRIGGER tenant_ref_release_id BEFORE INSERT OR UPDATE OF tenant_id, release_id ON emissary_updates FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_releases', 'release_id');

CREATE TRIGGER tenant_ref_retry_of BEFORE INSERT OR UPDATE OF tenant_id, retry_of ON emissary_updates FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('emissary_updates', 'retry_of');

CREATE TRIGGER tenant_ref_run_id BEFORE INSERT OR UPDATE OF tenant_id, run_id ON plan_revisions FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('planning_runs', 'run_id');

CREATE TRIGGER tenant_ref_source_credential_id BEFORE INSERT OR UPDATE OF tenant_id, source_credential_id ON builds FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('credentials', 'source_credential_id');




CREATE TRIGGER tenant_ref_upload BEFORE INSERT OR UPDATE OF tenant_id, id ON artifacts FOR EACH ROW EXECUTE FUNCTION check_tenant_reference('uploads', 'id');

CREATE TRIGGER verify_artifact_signing BEFORE INSERT ON artifacts FOR EACH ROW EXECUTE FUNCTION verify_artifact_signing();

CREATE TRIGGER workspace_credentials_admission BEFORE INSERT OR UPDATE ON credentials FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_enrollment_admission BEFORE INSERT ON enrollment_tokens FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_invitations_admission BEFORE INSERT ON tenant_invitations FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_members_admission BEFORE INSERT OR UPDATE ON tenant_members FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_node_admission BEFORE INSERT ON nodes FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_node_configuration BEFORE UPDATE OF name, pool_id, slots, draining, emissary_target_release ON nodes FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_ota_admission BEFORE INSERT ON emissary_updates FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_ota_release_admission BEFORE INSERT ON emissary_releases FOR EACH ROW EXECUTE FUNCTION ota_release_admission();

CREATE TRIGGER workspace_pool_admission BEFORE INSERT OR UPDATE OF name, emissary_target_release, emissary_update_concurrency, emissary_updates_paused ON node_pools FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_profiles_admission BEFORE INSERT OR UPDATE OF name, api, base_url, model, parameters, enabled, key_ciphertext, is_default, max_calls, timeout_seconds ON model_profiles FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_project_admission BEFORE INSERT ON projects FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_project_configuration BEFORE UPDATE OF name, repository_url, credential_id, pool_ids, node_ids, node_access_mode, signing_profiles, model_profile_id, build_context, source_credentials ON projects FOR EACH ROW EXECUTE FUNCTION project_configuration_admission();

CREATE TRIGGER workspace_project_members BEFORE INSERT OR UPDATE ON project_members FOR EACH ROW EXECUTE FUNCTION project_membership_admission();

CREATE TRIGGER workspace_project_nodes BEFORE INSERT OR UPDATE ON project_node_grants FOR EACH ROW EXECUTE FUNCTION project_membership_admission();

CREATE TRIGGER workspace_project_pools BEFORE INSERT OR UPDATE ON project_pool_grants FOR EACH ROW EXECUTE FUNCTION project_membership_admission();

CREATE TRIGGER workspace_signing_credentials BEFORE INSERT OR UPDATE ON signing_credentials FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_signing_defaults BEFORE UPDATE OF signing_defaults ON projects FOR EACH ROW EXECUTE FUNCTION project_configuration_admission();

CREATE TRIGGER workspace_signing_profiles BEFORE INSERT OR UPDATE ON signing_profiles FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_signing_versions BEFORE INSERT OR UPDATE ON signing_versions FOR EACH ROW EXECUTE FUNCTION workspace_admission();

CREATE TRIGGER workspace_tokens_admission BEFORE INSERT ON api_tokens FOR EACH ROW EXECUTE FUNCTION workspace_admission();

ALTER TABLE ONLY account_security_attempts
    ADD CONSTRAINT account_security_attempts_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY api_tokens
    ADD CONSTRAINT api_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_attempt_id_fkey FOREIGN KEY (attempt_id) REFERENCES attempts(id);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_id_fkey FOREIGN KEY (id) REFERENCES uploads(id);

ALTER TABLE ONLY artifacts
    ADD CONSTRAINT artifacts_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY attempt_events
    ADD CONSTRAINT attempt_events_attempt_id_fkey FOREIGN KEY (attempt_id) REFERENCES attempts(id);

ALTER TABLE ONLY attempt_events
    ADD CONSTRAINT attempt_events_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_job_id_fkey FOREIGN KEY (job_id) REFERENCES jobs(id);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_node_id_fkey FOREIGN KEY (node_id) REFERENCES nodes(id);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_plan_revision_id_fkey FOREIGN KEY (plan_revision_id) REFERENCES plan_revisions(id);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_source_manifest_fkey FOREIGN KEY (source_manifest_id) REFERENCES source_manifests(id);

ALTER TABLE ONLY attempts
    ADD CONSTRAINT attempts_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY audit_events
    ADD CONSTRAINT audit_events_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_current_plan_id_fkey FOREIGN KEY (current_plan_id) REFERENCES plan_revisions(id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_requested_by_fkey FOREIGN KEY (requested_by) REFERENCES users(id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_source_credential_id_fkey FOREIGN KEY (source_credential_id) REFERENCES credentials(id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_source_manifest_fkey FOREIGN KEY (tenant_id, project_id, source_manifest_id) REFERENCES source_manifests(tenant_id, project_id, id);

ALTER TABLE ONLY builds
    ADD CONSTRAINT builds_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY credentials
    ADD CONSTRAINT credentials_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY device_commands
    ADD CONSTRAINT device_commands_attempt_id_fkey FOREIGN KEY (attempt_id) REFERENCES attempts(id);

ALTER TABLE ONLY device_commands
    ADD CONSTRAINT device_commands_node_id_fkey FOREIGN KEY (node_id) REFERENCES nodes(id);

ALTER TABLE ONLY emissary_releases
    ADD CONSTRAINT emissary_releases_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_node_id_fkey FOREIGN KEY (node_id) REFERENCES nodes(id);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_pool_id_fkey FOREIGN KEY (pool_id) REFERENCES node_pools(id);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_release_id_fkey FOREIGN KEY (release_id) REFERENCES emissary_releases(id);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_retry_of_fkey FOREIGN KEY (retry_of) REFERENCES emissary_updates(id);

ALTER TABLE ONLY emissary_updates
    ADD CONSTRAINT emissary_updates_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY enrollment_tokens
    ADD CONSTRAINT enrollment_tokens_pool_id_fkey FOREIGN KEY (pool_id) REFERENCES node_pools(id);

ALTER TABLE ONLY enrollment_tokens
    ADD CONSTRAINT enrollment_tokens_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY image_artifacts
    ADD CONSTRAINT image_artifacts_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES image_executions(id);

ALTER TABLE ONLY image_artifacts
    ADD CONSTRAINT image_artifacts_job_id_fkey FOREIGN KEY (job_id) REFERENCES jobs(id);

ALTER TABLE ONLY image_artifacts
    ADD CONSTRAINT image_artifacts_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY image_artifacts
    ADD CONSTRAINT image_artifacts_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY image_credentials
    ADD CONSTRAINT image_credentials_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY image_credentials
    ADD CONSTRAINT image_credentials_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_destination_credential_fkey FOREIGN KEY (destination_credential) REFERENCES image_credentials(id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_job_id_fkey FOREIGN KEY (job_id) REFERENCES jobs(id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES image_executions(id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_plan_revision_id_fkey FOREIGN KEY (plan_revision_id) REFERENCES plan_revisions(id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_source_manifest_fkey FOREIGN KEY (tenant_id, project_id, source_manifest_id) REFERENCES source_manifests(tenant_id, project_id, id);

ALTER TABLE ONLY image_executions
    ADD CONSTRAINT image_executions_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY image_logs
    ADD CONSTRAINT image_logs_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES image_executions(id) ON DELETE CASCADE;

ALTER TABLE ONLY image_platforms
    ADD CONSTRAINT image_platforms_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES image_executions(id) ON DELETE CASCADE;

ALTER TABLE ONLY job_placements
    ADD CONSTRAINT job_placements_job_id_fkey FOREIGN KEY (job_id) REFERENCES jobs(id) ON DELETE CASCADE;

ALTER TABLE ONLY jobs
    ADD CONSTRAINT jobs_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id);

ALTER TABLE ONLY jobs
    ADD CONSTRAINT jobs_plan_revision_id_fkey FOREIGN KEY (plan_revision_id) REFERENCES plan_revisions(id);

ALTER TABLE ONLY jobs
    ADD CONSTRAINT jobs_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY log_chunks
    ADD CONSTRAINT log_chunks_attempt_id_fkey FOREIGN KEY (attempt_id) REFERENCES attempts(id);

ALTER TABLE ONLY log_chunks
    ADD CONSTRAINT log_chunks_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY model_profiles
    ADD CONSTRAINT model_profiles_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY node_pools
    ADD CONSTRAINT node_pools_emissary_target_release_fkey FOREIGN KEY (emissary_target_release) REFERENCES emissary_releases(id);

ALTER TABLE ONLY node_pools
    ADD CONSTRAINT node_pools_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY node_startups
    ADD CONSTRAINT node_startups_node_id_fkey FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE;

ALTER TABLE ONLY nodes
    ADD CONSTRAINT nodes_emissary_target_release_fkey FOREIGN KEY (emissary_target_release) REFERENCES emissary_releases(id);

ALTER TABLE ONLY nodes
    ADD CONSTRAINT nodes_pool_id_fkey FOREIGN KEY (pool_id) REFERENCES node_pools(id);

ALTER TABLE ONLY nodes
    ADD CONSTRAINT nodes_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY placement_rounds
    ADD CONSTRAINT placement_rounds_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id) ON DELETE CASCADE;

ALTER TABLE ONLY placement_rounds
    ADD CONSTRAINT placement_rounds_run_id_fkey FOREIGN KEY (run_id) REFERENCES planning_runs(id) ON DELETE CASCADE;

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id);

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_run_id_fkey FOREIGN KEY (run_id) REFERENCES planning_runs(id);

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_source_manifest_fkey FOREIGN KEY (source_manifest_id) REFERENCES source_manifests(id);

ALTER TABLE ONLY plan_revisions
    ADD CONSTRAINT plan_revisions_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY planning_events
    ADD CONSTRAINT planning_events_run_id_fkey FOREIGN KEY (run_id) REFERENCES planning_runs(id);

ALTER TABLE ONLY planning_events
    ADD CONSTRAINT planning_events_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY planning_runs
    ADD CONSTRAINT planning_runs_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id);

ALTER TABLE ONLY planning_runs
    ADD CONSTRAINT planning_runs_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY project_members
    ADD CONSTRAINT project_members_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY project_members
    ADD CONSTRAINT project_members_tenant_id_user_id_fkey FOREIGN KEY (tenant_id, user_id) REFERENCES tenant_members(tenant_id, user_id) ON DELETE CASCADE;

ALTER TABLE ONLY project_node_grants
    ADD CONSTRAINT project_node_grants_node_id_fkey FOREIGN KEY (node_id) REFERENCES nodes(id);

ALTER TABLE ONLY project_node_grants
    ADD CONSTRAINT project_node_grants_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY project_pool_grants
    ADD CONSTRAINT project_pool_grants_pool_id_fkey FOREIGN KEY (pool_id) REFERENCES node_pools(id);

ALTER TABLE ONLY project_pool_grants
    ADD CONSTRAINT project_pool_grants_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_credential_id_fkey FOREIGN KEY (credential_id) REFERENCES credentials(id);

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_model_profile_id_fkey FOREIGN KEY (model_profile_id) REFERENCES model_profiles(id) ON DELETE SET NULL;

ALTER TABLE ONLY projects
    ADD CONSTRAINT projects_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY publish_attempts
    ADD CONSTRAINT publish_attempts_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id);

ALTER TABLE ONLY publish_attempts
    ADD CONSTRAINT publish_attempts_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY release_artifacts
    ADD CONSTRAINT release_artifacts_artifact_id_fkey FOREIGN KEY (artifact_id) REFERENCES artifacts(id);

ALTER TABLE ONLY release_artifacts
    ADD CONSTRAINT release_artifacts_release_id_fkey FOREIGN KEY (release_id) REFERENCES releases(id);

ALTER TABLE ONLY release_images
    ADD CONSTRAINT release_images_image_id_fkey FOREIGN KEY (image_id) REFERENCES image_artifacts(id);

ALTER TABLE ONLY release_images
    ADD CONSTRAINT release_images_release_id_fkey FOREIGN KEY (release_id) REFERENCES releases(id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_project_id_fkey FOREIGN KEY (project_id) REFERENCES projects(id);

ALTER TABLE ONLY releases
    ADD CONSTRAINT releases_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY runtime_intents
    ADD CONSTRAINT runtime_intents_run_id_fkey FOREIGN KEY (run_id) REFERENCES planning_runs(id);

ALTER TABLE ONLY runtime_receipts
    ADD CONSTRAINT runtime_receipts_run_id_fkey FOREIGN KEY (run_id) REFERENCES planning_runs(id);

ALTER TABLE ONLY sessions
    ADD CONSTRAINT sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY signing_credentials
    ADD CONSTRAINT signing_credentials_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY signing_evidence
    ADD CONSTRAINT signing_evidence_tenant_id_attempt_id_fkey FOREIGN KEY (tenant_id, attempt_id) REFERENCES attempts(tenant_id, id) ON DELETE CASCADE;

ALTER TABLE ONLY signing_profiles
    ADD CONSTRAINT signing_profiles_tenant_id_credential_id_fkey FOREIGN KEY (tenant_id, credential_id) REFERENCES signing_credentials(tenant_id, id) ON DELETE CASCADE;

ALTER TABLE ONLY signing_profiles
    ADD CONSTRAINT signing_profiles_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY signing_versions
    ADD CONSTRAINT signing_versions_tenant_id_credential_id_fkey FOREIGN KEY (tenant_id, credential_id) REFERENCES signing_credentials(tenant_id, id) ON DELETE CASCADE;

ALTER TABLE ONLY source_manifests
    ADD CONSTRAINT source_manifests_tenant_id_project_id_fkey FOREIGN KEY (tenant_id, project_id) REFERENCES projects(tenant_id, id);

ALTER TABLE ONLY source_tasks
    ADD CONSTRAINT source_tasks_build_id_fkey FOREIGN KEY (build_id) REFERENCES builds(id);

ALTER TABLE ONLY source_tasks
    ADD CONSTRAINT source_tasks_credential_id_fkey FOREIGN KEY (credential_id) REFERENCES credentials(id);

ALTER TABLE ONLY source_tasks
    ADD CONSTRAINT source_tasks_tenant_id_project_id_fkey FOREIGN KEY (tenant_id, project_id) REFERENCES projects(tenant_id, id);

ALTER TABLE ONLY step_runs
    ADD CONSTRAINT step_runs_attempt_id_fkey FOREIGN KEY (attempt_id) REFERENCES attempts(id);

ALTER TABLE ONLY step_runs
    ADD CONSTRAINT step_runs_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY tenant_invitations
    ADD CONSTRAINT tenant_invitations_accepted_by_fkey FOREIGN KEY (accepted_by) REFERENCES users(id);

ALTER TABLE ONLY tenant_invitations
    ADD CONSTRAINT tenant_invitations_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id);

ALTER TABLE ONLY tenant_invitations
    ADD CONSTRAINT tenant_invitations_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY tenant_members
    ADD CONSTRAINT tenant_members_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

ALTER TABLE ONLY tenant_members
    ADD CONSTRAINT tenant_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);

ALTER TABLE ONLY tenants
    ADD CONSTRAINT tenants_created_by_fkey FOREIGN KEY (created_by) REFERENCES users(id);

ALTER TABLE ONLY tenants
    ADD CONSTRAINT tenants_owner_id_fkey FOREIGN KEY (owner_id) REFERENCES users(id);

ALTER TABLE ONLY uploads
    ADD CONSTRAINT uploads_attempt_id_fkey FOREIGN KEY (attempt_id) REFERENCES attempts(id);

ALTER TABLE ONLY uploads
    ADD CONSTRAINT uploads_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES tenants(id);

CREATE FUNCTION project_node_allowed(project uuid, node uuid) RETURNS boolean
    LANGUAGE sql STABLE
    AS $$
 SELECT EXISTS(SELECT 1 FROM projects p JOIN nodes n ON n.tenant_id=p.tenant_id
 WHERE p.id=project AND n.id=node AND
 ((p.node_access_mode='pools' AND n.pool_id=ANY(p.pool_ids)) OR (p.node_access_mode='nodes' AND n.id=ANY(p.node_ids))))
$$;

CREATE FUNCTION build_node_allowed(build uuid, node uuid) RETURNS boolean
    LANGUAGE sql STABLE
    AS $$
 SELECT EXISTS(SELECT 1 FROM builds b JOIN nodes n ON n.tenant_id=b.tenant_id
 WHERE b.id=build AND n.id=node AND project_node_allowed(b.project_id,n.id) AND
 CASE WHEN b.planning_context->'node_access'->>'mode'='nodes'
 THEN b.planning_context->'node_access'->'ids' ? n.id::text
 ELSE coalesce(b.planning_context->'node_access'->'ids',b.planning_context->'pool_ids','[]') ? n.pool_id::text END)
$$;
