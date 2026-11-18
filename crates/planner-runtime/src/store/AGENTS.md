# crates/planner-runtime/src/store/
> L2 | Parent: ../AGENTS.md

model_requests.rs: 原子预留模型预算，拒绝重放调用。
rows.rs: 统一运行行投影与事务锁。
models.rs: 声明 Runtime 私有数据模型。
conversations.rs: 按 generation 保存会话，分页读取持久消息。

[PROTOCOL]: Update this header on change, then check AGENTS.md
