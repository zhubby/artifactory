# gateway/src/
> L2 | Parent: ../AGENTS.md

main.rs: 统一启动四监听，固定日志上限为 info，保留单入口运行方式。
config.rs: 校验固定后端、监听端口、内部证书与独立连接预算。
http.rs: 流式代理 HTTP，隔离内部 TLS；回池连接空闲三十秒过期，仅记录错误白名单字段。
tcp.rs: 设备和遥测监听独立选后端，透传完整 TLS 字节流。

[PROTOCOL]: Update this header on change, then check AGENTS.md
