# crates/execution-support/tests/
> L2 | Parent: ../AGENTS.md

process_tree.rs: 验证输入 EOF、取消与超时回收真实后代进程。
private_files.rs: 验证独占写入、覆盖截断与 Unix 文件权限。
terminal_process.rs: 验证真实 PTY、ConPTY 游标握手、输入突发、退出码、注册与回收。
terminal_process/: 隔离 Windows ConPTY 输出停顿后的取消回归。

[PROTOCOL]: Update this header on change, then check AGENTS.md
