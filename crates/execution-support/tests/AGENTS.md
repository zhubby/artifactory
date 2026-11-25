# crates/execution-support/tests/
> L2 | Parent: ../AGENTS.md

process_tree.rs: 验证输入EOF、fork中取消/超时回收与失败日志读取截止。
private_files.rs: 验证独占写入、覆盖截断与 Unix 文件权限。
terminal_process.rs: 验证满队列取消、真实SIGINT、PTY输入、ConPTY握手、退出码与回收。
terminal_process/: 隔离 Windows ConPTY 输出停顿后的取消回归。

[PROTOCOL]: Update this header on change, then check AGENTS.md
