-- History is backfilled using the control-plane project map during maintenance.
CREATE TABLE runtime_project_tenants(project_id uuid PRIMARY KEY,tenant_id uuid NOT NULL,UNIQUE(project_id,tenant_id));
ALTER TABLE runtime_runs ADD COLUMN tenant_id uuid;
ALTER TABLE runtime_runs ADD CONSTRAINT runtime_run_tenant FOREIGN KEY(project_id,tenant_id) REFERENCES runtime_project_tenants(project_id,tenant_id);
CREATE INDEX runtime_runs_tenant ON runtime_runs(tenant_id,status);
