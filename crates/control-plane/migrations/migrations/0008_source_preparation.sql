-- [INPUT]: 依赖构建、规划运行与现有清单身份。
-- [OUTPUT]: 保存准备协议与不含秘密的执行阶段。
-- [POS]: 追加迁移；历史构建保持协议 1，不补造记录。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
ALTER TABLE builds ADD COLUMN source_preparation_version smallint NOT NULL DEFAULT 1 CHECK(source_preparation_version IN (1,2));
ALTER TABLE planning_runs ADD COLUMN source_preparation jsonb NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE planning_runs ADD COLUMN source_preparation_events jsonb NOT NULL DEFAULT '[]'::jsonb CHECK(jsonb_typeof(source_preparation_events)='array' AND jsonb_array_length(source_preparation_events)<=256);
