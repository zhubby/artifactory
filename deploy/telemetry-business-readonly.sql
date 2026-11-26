-- [INPUT]: 业务库、schema与专用遥测认证角色。
-- [OUTPUT]: 仅授予节点、attempt、startup归属与迁移版本读取。
-- [POS]: Telemetry认证连接的最小权限配置。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
-- Run with psql as the business database owner after Control initializes its baseline.
-- Supply business_database, business_schema and telemetry_business_role via psql -v.
GRANT CONNECT ON DATABASE :"business_database" TO :"telemetry_business_role";
GRANT USAGE ON SCHEMA :"business_schema" TO :"telemetry_business_role";
GRANT SELECT ON TABLE :"business_schema".nodes, :"business_schema".attempts, :"business_schema".node_startups,
    :"business_schema".__toasty_migrations
    TO :"telemetry_business_role";
