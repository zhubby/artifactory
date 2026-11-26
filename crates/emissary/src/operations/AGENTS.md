# crates/emissary/src/operations/
> L2 | Parent: ../AGENTS.md

mod.rs: 复核认证身份与截止，串行准入和取消，关闭等待执行任务及耐久终态。
journal.rs: 原子记录接受与终态，重启标记interrupted，摘要回执释放正文，销毁显式解锁。
executor.rs: 调用固定工具探测并通过耐久回调创建独立清单观察。
tests.rs: 验证落盘顺序、重启不重跑、取消清理与关闭存储失败保留恢复。

[PROTOCOL]: Update this header on change, then check AGENTS.md
