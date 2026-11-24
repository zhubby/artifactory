# terminal/
> L2 | Parent: ../AGENTS.md

mod.rs: 定义受信 Shell、父注册确认、有界输入和真实进程事件。
runtime.rs: 保留注册前进程身份；排空或回收失败不发正常退出。
unix.rs: 校验未回收 PTY Child，按 SID 回收前台与后台进程。
windows.rs: 将 ConPTY 子进程纳入关闭即终止的 Job Object。

[PROTOCOL]: Update this header on change, then check AGENTS.md
