# terminals/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合设备与浏览器流，固定 owner 并按会话创建有界转发状态机。
access.rs: 从授权查询开始计五秒截止，取消流和阻塞转发，丢弃撤权积压。
broker.rs: 确认附着消费偏移并排空输出，停止转发后重试终态持久化。
relay.rs: 生成进程 TLS 和签名身份，登记实例租约，签发四秒委托并预留一秒时钟差。
rpc.rs: 强制设备 mTLS 与内部签名首帧，拒绝身份失效和跨 owner 循环。

[PROTOCOL]: Update this header on change, then check AGENTS.md
