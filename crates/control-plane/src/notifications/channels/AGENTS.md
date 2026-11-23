# channels/
> L2 | Parent: ../AGENTS.md

email.rs: 规范邮箱，按绑定与任务派生验证令牌，以总超时发送双格式 TLS 邮件。
webhook.rs: 复核出口地址，固定 DNS 解析并签名 CloudEvents；配置事务委托给 store。
runtime.rs: 独立执行三类协议预算，在事务外发送，委托 store 复验与阻断晚结果。
[PROTOCOL]: Update this header on change, then check AGENTS.md
