# crates/control-plane/migrations/migrations/
> L2 | Parent: ../AGENTS.md

0000_baseline.sql: 创建租户、源码任务、镜像候选及两类 CR UID 绑定；保存 Release 版本说明、构建独立来源与目标、交付冻结和发布租约；仅安装到新数据库。
0001_image_failure_kind.sql: 为镜像失败追加有界分类；历史未分类结果保持空值。
0002_notifications.sql: 创建事务事件、含首次接收版本的收件箱、账户水位和当前授权函数。

0003_notification_channels.sql: 创建邮件地址绑定、Webhook 密文与带租约的外部投递队列。
0004_notification_producers.sql: 同事务生成业务事件，扫描异常、关闭失效项目签名告警并清除删除快照。
0005_dashboard_indexes.sql: 追加构建项目创建和完成时间索引，支持概览窗口与近期活动。
0006_source_progress.sql: 兼容追加有界源码阶段记录，不伪造历史进度。

[PROTOCOL]: Update this header on change, then check AGENTS.md
