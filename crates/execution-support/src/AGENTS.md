# crates/execution-support/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出执行原语，不依赖节点或镜像领域。
events.rs: 提供有界事件管道，保留 Worker 事件编码。
fs.rs: 写入私有文件，保留独占创建和持久化语义。
process.rs: 固定未回收leader回收后代，空组二次确认不重发信号；公开失败读取任务的截止。
redact.rs: 在事件写入前替换跨分块秘密。
terminal/: 提供可选 PTY/ConPTY、窗口调整和终端进程树回收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
