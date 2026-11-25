# terminal/
> L2 | Parent: ../AGENTS.md

mod.rs: 定义受信 Shell、父注册确认、有界输入和真实进程事件。
runtime.rs: 保留注册身份；Unix读写可取消，ConPTY关闭与排空并行，失败拒绝正常退出。
unix.rs: 创建非阻塞PTY读写，校验未回收Child并按SID回收进程。
windows.rs: 将 ConPTY 子进程纳入关闭即终止的 Job Object。

[PROTOCOL]: Update this header on change, then check AGENTS.md
