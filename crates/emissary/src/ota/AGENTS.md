# crates/emissary/src/ota/
> L2 | Parent: ../AGENTS.md

download.rs: 在单次授权期限内校验空间、严格续传并原子提交签名资产。
activity.rs: 保存真实排空快照，仅确认运行时已关闭准入的确切维护标识。
progress.rs: 保存绑定执行身份的有界真实进度与当前启动器能力握手。
report.rs: 对齐持久序号，限频构造观察并认证转交原资产URL续签。
storage.rs: 校验受管路径，提供存储/请求交接锁、空间预算和受保护缓存清扫。
url_refresh.rs: 通过绑定更新身份的私有邮箱领取同一冻结资产URL。

[PROTOCOL]: Update this header on change, then check AGENTS.md
