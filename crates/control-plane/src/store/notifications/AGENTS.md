# store/notifications/
> L2 | Parent: ../AGENTS.md

mod.rs: 定义账户收件箱 DTO、过滤输入与外部渠道持久模块。
projection.rs: 按事件版本聚合个人通知；静默更新已有事项，锁定账户水位。
reads.rs: 重验当前权限，保留首次接收后的时间线，生成范围并维护回执和项目订阅。

channel_email.rs: 原子更新邮件绑定代数、验证摘要与个人订阅；新订阅复核 SMTP，取消旧地址任务。
channel_webhook.rs: 保存管理员端点配置、密文与诊断，隔离绑定变更和旧代重试。
channel_delivery.rs: 路由事件并声明租约，复验发送权限，重试保留验证令牌和原始期限。

health.rs: 按租户汇总通知积压、重试、失败与暂停端点，不返回正文或身份标签。
maintenance.rs: 串行扫描异常周期，按统一锁序清理过期通知与删除资源快照。

channel_delivery_tests.rs: 验证 SMTP 总超时、验证链接重试和绑定退役后的晚结果围栏。
maintenance_tests.rs: 验证生产清理函数的并发锁序、待处理保留与事件引用安全。

[PROTOCOL]: Update this header on change, then check AGENTS.md
