-- [INPUT]: 依赖现有运行、执行围栏和跨库 outbox。
-- [OUTPUT]: 追加清单绑定与本代源码就绪证据。
-- [POS]: 兼容迁移；历史意图在读取时保留固定清单模式。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
ALTER TABLE planning_runs ADD COLUMN source_manifest jsonb;
ALTER TABLE planning_runs ADD COLUMN source_frozen_tags jsonb NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE planning_runs ADD COLUMN source_commit_event uuid;
ALTER TABLE planning_runs ADD COLUMN source_bound boolean NOT NULL DEFAULT false;
ALTER TABLE planning_runs ADD COLUMN source_ready_execution jsonb;
ALTER TABLE planning_runs ADD COLUMN source_preparation jsonb NOT NULL DEFAULT '{}'::jsonb;
