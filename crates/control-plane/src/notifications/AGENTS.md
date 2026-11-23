# notifications/
> L2 | Parent: ../AGENTS.md

mod.rs: 启动收件箱、三类渠道和异常维护的独立后台任务。
catalog.rs: 固定事件版本、双语文案与安全业务链接；按范围和角色白名单描述成员权限变化。
channels.rs: 导出邮件、Webhook 和投递 worker 边界。
channels/: 验证地址、签名出口、复核绑定并执行持久投递。
../store/notifications/: 保存事务投影、渠道状态、健康查询与异常清扫。

[PROTOCOL]: Update this header on change, then check AGENTS.md
