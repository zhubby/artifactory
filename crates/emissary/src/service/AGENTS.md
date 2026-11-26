# crates/emissary/src/service/
> L2 | Parent: ../AGENTS.md

windows.rs: 将节点生命周期接入 SCM，保留专用账号，原子记录最后一次具体失败。
maintenance.rs: 保留服务定义、ACL与节点身份，以可写句柄刷盘备份并替换固定启动器，提供中断恢复。
maintenance/: 按服务身份发布私有围栏，注入替换故障验证维护和恢复。

[PROTOCOL]: Update this header on change, then check AGENTS.md
