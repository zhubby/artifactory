# image-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 复用共享出口代理，定义不可变执行身份、角色资源与 Pod UID 台账。
resources.rs: 按角色预算创建源码 init、Runner/BuildKit 和发布 Pod，校验内部直连并注入池代理；信任卷只投影公开 CA。
controller.rs: 仅将缺池 404 写为失败；预留创建意图，保存 UID 和停机证据，收尾后释放 Finalizer。
diagnostics.rs: 复用共享脱敏器清除任务秘密、代理凭据与诊断 URL，限制日志和摘要长度。
build_failure.rs: 转发脱敏 buildctl 标准错误，只从有界末尾求解诊断识别执行失败与外部故障。
leadership.rs: 使用独立 Lease 保护 Image 控制器写入。
buildkit.rs: 在容器 PID namespace 监督 rootless 进程树；只经有界协议交付失败诊断。
lifecycle.rs: 定义有界 Unix 帧，区分 readiness 前后 EOF，Runner 独占连接并确认收尾。
worker.rs: 校验 init 源码，分类 buildctl 失败并读回 OCI 版本标签；确认 BuildKit 收尾后回报结果。
transport.rs: 提供任务令牌认证回执，先写脱敏错误日志，再写摘要与正式失败类型。
publisher.rs: 使用 OCI 客户端与 /work 临时卷复制镜像图，以共享脱敏器保护失败报告。
lib.rs: 隔离默认业务模块与最小本地协议构建。
main.rs: 启动独立 Image Operator。
bin/: 提供 CRD、Runner、Publisher 和 BuildKit 监督入口。

resource_tests.rs: 验证 1.28 资源、公开 CA 投影、凭据隔离与架构放置。

[PROTOCOL]: Update this header on change, then check AGENTS.md
