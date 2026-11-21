-- [INPUT]: 依赖已安装的镜像执行表。
-- [OUTPUT]: 追加可空失败类型；保留历史未分类结果。
-- [POS]: 镜像执行与自动修订之间的持久化契约。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
ALTER TABLE image_executions ADD COLUMN failure_kind text
    CHECK (failure_kind IN ('execution','source','authorization','artifact_transport','infrastructure'));
