# crates/execution-support/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出执行原语，不依赖节点或镜像领域。
events.rs: 提供有界事件管道，保留 Worker 事件编码。
fs.rs: 写入私有文件，保留独占创建和持久化语义。
process.rs: 启动进程树，按取消和超时回收并读取有界输出。
redact.rs: 在事件写入前替换跨分块秘密。

[PROTOCOL]: Update this header on change, then check AGENTS.md
