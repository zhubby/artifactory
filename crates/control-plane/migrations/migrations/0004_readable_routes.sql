-- Stable public page identifiers. UUIDs remain the internal resource identities.
-- Complete deferred owner checks from earlier migrations before ALTER locks.
SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE tenants ADD COLUMN slug text;
ALTER TABLE projects ADD COLUMN slug text;
ALTER TABLE projects ADD COLUMN next_build_number bigint NOT NULL DEFAULT 1;
ALTER TABLE builds ADD COLUMN number bigint;
CREATE UNIQUE INDEX tenants_slug_key ON tenants(slug);
CREATE UNIQUE INDEX projects_tenant_slug_key ON projects(tenant_id,slug);

CREATE FUNCTION assign_route_slug() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE base text; candidate text; suffix text; sequence bigint := 1; occupied boolean;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF OLD.slug IS NOT NULL AND NEW.slug IS DISTINCT FROM OLD.slug THEN
   RAISE EXCEPTION 'route slug is immutable' USING ERRCODE='23514';
  END IF;
  IF TG_TABLE_NAME='projects' THEN
   IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id THEN
    RAISE EXCEPTION 'project tenant is immutable' USING ERRCODE='23514';
   END IF;
  END IF;
 END IF;
 IF NEW.slug IS NOT NULL THEN RETURN NEW; END IF;
 base := trim(both '-' from left(regexp_replace(lower(NEW.name COLLATE "C"),'[^a-z0-9]+','-','g'),63));
 IF TG_TABLE_NAME='tenants' THEN
  base := coalesce(nullif(base,''),'tenant');
  PERFORM pg_advisory_xact_lock(hashtextextended('tenant-route-slugs',93));
 ELSE
  base := coalesce(nullif(base,''),'project');
  PERFORM pg_advisory_xact_lock(hashtextextended('project-route-slugs:'||NEW.tenant_id::text,93));
 END IF;
 LOOP
  suffix := CASE WHEN sequence=1 THEN '' ELSE '-'||sequence::text END;
  candidate := rtrim(left(base,63-length(suffix)),'-')||suffix;
  IF TG_TABLE_NAME='tenants' THEN
   SELECT EXISTS(SELECT 1 FROM tenants WHERE slug=candidate) INTO occupied;
  ELSE
   SELECT EXISTS(SELECT 1 FROM projects WHERE tenant_id=NEW.tenant_id AND slug=candidate) INTO occupied;
  END IF;
  EXIT WHEN NOT occupied;
  sequence := sequence+1;
 END LOOP;
 NEW.slug := candidate;
 RETURN NEW;
END $$;
CREATE TRIGGER route_slug BEFORE INSERT OR UPDATE OF slug ON tenants FOR EACH ROW EXECUTE FUNCTION assign_route_slug();
CREATE TRIGGER route_slug BEFORE INSERT OR UPDATE OF slug,tenant_id ON projects FOR EACH ROW EXECUTE FUNCTION assign_route_slug();

-- Populate existing development data in a stable order; preserve every UUID.
DO $$
DECLARE row_id uuid;
BEGIN
 FOR row_id IN SELECT id FROM tenants ORDER BY created_at,id LOOP
  UPDATE tenants SET slug=NULL WHERE id=row_id;
 END LOOP;
 FOR row_id IN SELECT id FROM projects ORDER BY created_at,id LOOP
  UPDATE projects SET slug=NULL WHERE id=row_id;
 END LOOP;
END $$;
ALTER TABLE tenants ALTER COLUMN slug SET NOT NULL;
ALTER TABLE projects ALTER COLUMN slug SET NOT NULL;
ALTER TABLE tenants ADD CONSTRAINT tenants_slug_format CHECK (length(slug)<=63 AND slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$');
ALTER TABLE projects ADD CONSTRAINT projects_slug_format CHECK (length(slug)<=63 AND slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$');

WITH numbered AS (
 SELECT id,row_number() OVER (PARTITION BY project_id ORDER BY created_at,id) AS number FROM builds
)
UPDATE builds b SET number=n.number FROM numbered n WHERE n.id=b.id;
UPDATE projects p SET next_build_number=coalesce((SELECT max(number) FROM builds WHERE project_id=p.id),0)+1;
ALTER TABLE builds ALTER COLUMN number SET NOT NULL;
ALTER TABLE builds ADD CONSTRAINT builds_number_range CHECK (number BETWEEN 1 AND 9007199254740991);
CREATE UNIQUE INDEX builds_project_number_key ON builds(project_id,number);

CREATE FUNCTION assign_build_number() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.number IS DISTINCT FROM OLD.number OR NEW.project_id IS DISTINCT FROM OLD.project_id THEN
   RAISE EXCEPTION 'build number and project are immutable' USING ERRCODE='23514';
  END IF;
 ELSE
  -- Updating the project row serializes concurrent allocation; deleted builds leave gaps.
  UPDATE projects SET next_build_number=next_build_number+1 WHERE id=NEW.project_id
   RETURNING next_build_number-1 INTO NEW.number;
  IF NOT FOUND THEN RAISE EXCEPTION 'build project not found' USING ERRCODE='23503'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER build_number BEFORE INSERT OR UPDATE OF number,project_id ON builds FOR EACH ROW EXECUTE FUNCTION assign_build_number();
