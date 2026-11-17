# gateway/src/
> L2 | Parent: ../AGENTS.md

main.rs: 统一启动公开、内部、设备与遥测监听，保留单入口运行方式。
config.rs: 校验固定后端、监听端口、内部证书与独立连接预算。
http.rs: 流式代理 API、内建仓库和私有对象，隔离内部 TLS 入口。
tcp.rs: 设备和遥测监听独立选后端，透传完整 TLS 字节流。

[PROTOCOL]: Update this header on change, then check AGENTS.md
