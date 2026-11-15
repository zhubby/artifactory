CREATE TABLE telemetry_deleted_identities(kind text NOT NULL CHECK(kind IN ('node','attempt')),id uuid NOT NULL,PRIMARY KEY(kind,id));
