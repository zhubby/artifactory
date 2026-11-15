SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE users ADD COLUMN display_name text;
UPDATE users SET display_name=username;
ALTER TABLE users ALTER COLUMN display_name SET NOT NULL;
ALTER TABLE users ALTER COLUMN display_name SET DEFAULT '';
ALTER TABLE users ADD CONSTRAINT display_name_length CHECK (char_length(display_name) BETWEEN 1 AND 100);
CREATE FUNCTION initialize_display_name() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.display_name='' THEN NEW.display_name:=NEW.username; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER account_display_name BEFORE INSERT ON users FOR EACH ROW EXECUTE FUNCTION initialize_display_name();

CREATE TABLE account_security_attempts (
 user_id uuid NOT NULL REFERENCES users(id),
 attempted_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX account_security_attempts_recent ON account_security_attempts(user_id,attempted_at);
