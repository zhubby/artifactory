# image-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义不可变执行身份、池级具名代理、分角色资源与 Pod UID 台账。
resources.rs: 按角色预算创建源码 init、Runner/BuildKit 和发布 Pod，校验内部直连并注入池代理；信任卷只投影公开 CA。
controller.rs: 先预留创建意图，再保存 UID 与停机证据；收尾后释放 CR Finalizer。
diagnostics.rs: 脱敏任务秘密与编码后的代理凭据，清除诊断 URL 并限制日志与摘要长度。
leadership.rs: 使用独立 Lease 保护 Image 控制器写入。
buildkit.rs: 在容器 PID namespace 监督 rootless 进程树；只经有界协议交付失败诊断。
lifecycle.rs: 定义有界 Unix 帧，区分 readiness 前后 EOF，Runner 独占连接并确认收尾。
worker.rs: 校验 init 清单与源码，传递标准代理构建参数并脱敏日志；确认 BuildKit 收尾后回报候选结果。
transport.rs: 提供任务令牌认证回执，先写脱敏错误日志，再写失败摘要。
publisher.rs: 使用 /work 临时卷校验 Blob，发布不可变 Index 并报告脱敏失败。
lib.rs: 隔离默认业务模块与最小本地协议构建。
main.rs: 启动独立 Image Operator。
bin/: 提供 CRD、Runner、Publisher 和 BuildKit 监督入口。

resource_tests.rs: 验证 1.28 资源、公开 CA 投影、凭据隔离与架构放置。

[PROTOCOL]: Update this header on change, then check AGENTS.md
