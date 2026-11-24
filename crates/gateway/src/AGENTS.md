# gateway/src/
> L2 | Parent: ../AGENTS.md

main.rs: 注册全部必要监听与共享就绪状态，固定日志上限为 info。
config.rs: 校验固定后端、监听端口、内部证书与独立连接预算。
http.rs: 隔离终端与普通请求配额，固定下载文件名并拦截正式 Manifest。
registry.rs: 最多缓冲四 MiB 正式 Manifest，校验正文摘要与发布授权后只写一次。
listener.rs: 用接受回调确认监听已启动，监督任务失败并撤销关闭时的就绪状态。
tcp.rs: 设备和遥测监听独立选后端，透传完整 TLS 字节流。

[PROTOCOL]: Update this header on change, then check AGENTS.md
