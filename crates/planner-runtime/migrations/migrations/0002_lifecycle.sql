CREATE TABLE runtime_deleted_projects(project_id uuid PRIMARY KEY,deleted_at timestamptz NOT NULL DEFAULT now());
CREATE FUNCTION runtime_workspace_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('runtime-project:'||NEW.project_id::text,96));
 IF EXISTS(SELECT 1 FROM runtime_deleted_projects WHERE project_id=NEW.project_id) THEN RAISE EXCEPTION 'project retired' USING ERRCODE='55000'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER runtime_run_admission BEFORE INSERT ON runtime_runs FOR EACH ROW EXECUTE FUNCTION runtime_workspace_admission();
CREATE TRIGGER runtime_project_admission BEFORE INSERT ON runtime_project_tenants FOR EACH ROW EXECUTE FUNCTION runtime_workspace_admission();
