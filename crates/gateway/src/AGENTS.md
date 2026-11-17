# gateway/src/
> L2 | Parent: ../AGENTS.md

main.rs: 按 control、data、device、internal 或 all 启动入口。
config.rs: 校验管理员固定后端与独立连接预算。
http.rs: 流式代理 API、内建仓库和私有对象，隔离内部 TLS 入口。
tcp.rs: 在连接建立时选后端，透传完整 TLS 字节流。

[PROTOCOL]: Update this header on change, then check AGENTS.md
