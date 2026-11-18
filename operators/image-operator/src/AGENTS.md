# image-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义不可变执行身份、构建池和有界 Job UID 归属台账。
resources.rs: 创建源码 init、Runner/BuildKit 和发布 Job，信任卷只投影公开 CA。
controller.rs: 先装 Finalizer，再建 Job；CR 删除前保留 UID 台账和残留容量。
leadership.rs: 使用独立 Lease 保护 Image 控制器写入。
buildkit.rs: 在容器 PID namespace 监督 rootless 进程树；只经有界协议交付失败诊断。
lifecycle.rs: 定义限长 Unix 控制帧与诊断帧，Runner 独占连接并确认进程收尾。
worker.rs: 以任务清单 ID 与摘要校验 init 清单和源码，配置仓库认证并脱敏构建日志及 daemon 诊断；确认 BuildKit 收尾后回报候选结果。
transport.rs: 提供任务令牌认证的控制平面回执。
publisher.rs: 使用 /work 临时磁盘卷校验 Blob，汇总平台结果并发布不可变 Index。
lib.rs: 隔离默认业务模块与最小本地协议构建。
main.rs: 启动独立 Image Operator。
bin/: 提供 CRD、Runner、Publisher 和 BuildKit 监督入口。

resource_tests.rs: 验证 1.28 资源、公开 CA 投影、凭据隔离与架构放置。

[PROTOCOL]: Update this header on change, then check AGENTS.md
