CREATE TABLE source_deleted_projects(project_id uuid PRIMARY KEY,inventory jsonb NOT NULL DEFAULT '{}',completed boolean NOT NULL DEFAULT false);
CREATE TABLE source_project_credentials(project_id uuid NOT NULL,credential_id uuid NOT NULL REFERENCES credentials(id),PRIMARY KEY(project_id,credential_id));
INSERT INTO source_project_credentials SELECT DISTINCT project_id,credential_id FROM source_repositories WHERE credential_id IS NOT NULL;
ALTER TABLE source_operations ADD COLUMN project_id uuid;
UPDATE source_operations o SET project_id=s.project_id FROM source_snapshots s WHERE s.id=o.snapshot_id;
CREATE FUNCTION source_workspace_admission() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_advisory_xact_lock_shared(hashtextextended(NEW.project_id::text,1));
 IF EXISTS(SELECT 1 FROM source_deleted_projects WHERE project_id=NEW.project_id) THEN RAISE EXCEPTION 'project retired' USING ERRCODE='55000'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER source_repository_admission BEFORE INSERT ON source_repositories FOR EACH ROW EXECUTE FUNCTION source_workspace_admission();
CREATE TRIGGER source_snapshot_admission BEFORE INSERT ON source_snapshots FOR EACH ROW EXECUTE FUNCTION source_workspace_admission();
CREATE TRIGGER source_operation_admission BEFORE INSERT ON source_operations FOR EACH ROW EXECUTE FUNCTION source_workspace_admission();
CREATE TRIGGER source_credential_admission BEFORE INSERT ON source_project_credentials FOR EACH ROW EXECUTE FUNCTION source_workspace_admission();
