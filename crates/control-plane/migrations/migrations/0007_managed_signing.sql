-- [INPUT]: 依赖租户、项目、节点、构建与制品表。
-- [OUTPUT]: 提供托管签名库、不可变版本、默认方案快照及制品验签约束。
-- [POS]: 签名授权和历史证据的持久化边界。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE signing_credentials (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id), name text NOT NULL,
 platform text NOT NULL CHECK(platform IN ('macos','android')), enabled boolean NOT NULL DEFAULT true,
 created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(tenant_id,id), UNIQUE(tenant_id,name)
);
CREATE TABLE signing_versions (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL, credential_id uuid NOT NULL,
 metadata jsonb NOT NULL, envelope jsonb NOT NULL, enabled boolean NOT NULL DEFAULT true,
 created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(tenant_id,id),
 FOREIGN KEY(tenant_id,credential_id) REFERENCES signing_credentials(tenant_id,id) ON DELETE CASCADE
);
CREATE INDEX signing_versions_latest ON signing_versions(credential_id,created_at DESC,id DESC);
CREATE TABLE signing_profiles (
 tenant_id uuid NOT NULL REFERENCES tenants(id), name text NOT NULL,
 credential_id uuid NOT NULL, notarize boolean NOT NULL DEFAULT false,
 enabled boolean NOT NULL DEFAULT true, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,name),
 FOREIGN KEY(tenant_id,credential_id) REFERENCES signing_credentials(tenant_id,id) ON DELETE CASCADE
);
ALTER TABLE projects ADD COLUMN signing_defaults jsonb NOT NULL DEFAULT '{}';
ALTER TABLE artifacts ADD COLUMN signing jsonb;
ALTER TABLE attempts ADD CONSTRAINT attempts_tenant_identity UNIQUE(tenant_id,id);
CREATE TABLE signing_evidence (
 tenant_id uuid NOT NULL, attempt_id uuid NOT NULL, path text NOT NULL, sha256 text NOT NULL,
 evidence jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(attempt_id,path),
 FOREIGN KEY(tenant_id,attempt_id) REFERENCES attempts(tenant_id,id) ON DELETE CASCADE
);
CREATE FUNCTION immutable_signing_version() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF (to_jsonb(NEW)-'enabled') IS DISTINCT FROM (to_jsonb(OLD)-'enabled')
 THEN RAISE EXCEPTION 'signing versions are immutable' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER signing_version_immutable BEFORE UPDATE ON signing_versions FOR EACH ROW EXECUTE FUNCTION immutable_signing_version();

CREATE FUNCTION freeze_signing_defaults() RETURNS trigger LANGUAGE plpgsql AS $$
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
CREATE TRIGGER freeze_signing_defaults BEFORE INSERT ON builds FOR EACH ROW EXECUTE FUNCTION freeze_signing_defaults();

CREATE FUNCTION verify_artifact_signing() RETURNS trigger LANGUAGE plpgsql AS $$
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
CREATE TRIGGER verify_artifact_signing BEFORE INSERT ON artifacts FOR EACH ROW EXECUTE FUNCTION verify_artifact_signing();

CREATE TRIGGER workspace_signing_credentials BEFORE INSERT OR UPDATE ON signing_credentials FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_signing_profiles BEFORE INSERT OR UPDATE ON signing_profiles FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_signing_versions BEFORE INSERT OR UPDATE ON signing_versions FOR EACH ROW EXECUTE FUNCTION workspace_admission();
CREATE TRIGGER workspace_signing_defaults BEFORE UPDATE OF signing_defaults ON projects FOR EACH ROW EXECUTE FUNCTION project_configuration_admission();
