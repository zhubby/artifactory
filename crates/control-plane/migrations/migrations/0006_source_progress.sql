-- [INPUT]: 依赖已有源码任务与执行围栏。
-- [OUTPUT]: 追加有界源码进度；保留历史任务和业务身份。
-- [POS]: 兼容迁移，不伪造历史进度。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
ALTER TABLE source_tasks ADD COLUMN progress jsonb NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE source_tasks ADD CONSTRAINT source_tasks_progress_bound
    CHECK (jsonb_typeof(progress)='array' AND jsonb_array_length(progress)<=256);
