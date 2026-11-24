-- [INPUT]: 依赖现有构建项目、创建时间与最终完成时间。
-- [OUTPUT]: 提供完整窗口统计和近期活动的项目时间索引。
-- [POS]: 兼容追加迁移；保留已有构建与迁移身份。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE INDEX builds_project_created_at ON builds(project_id, created_at DESC);
CREATE INDEX builds_project_finished_at ON builds(project_id, finished_at DESC)
    WHERE finished_at IS NOT NULL;
