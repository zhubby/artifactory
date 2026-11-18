# image-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义构建池、平台集合与固定 SourceManifest 身份的不可变 ImageBuild CRD。
resources.rs: 创建源码 init、原生 BuildKit sidecar 与发布 Job。
controller.rs: 协调任务准入，先写终态再清理 Job 与 Pod。
leadership.rs: 使用独立 Lease 保护 Image 控制器写入。
worker.rs: 校验 init 准备的源码，配置仓库认证并提交脱敏日志。
transport.rs: 提供任务令牌认证的控制平面回执。
publisher.rs: 使用 /work 临时磁盘卷校验 Blob，汇总平台结果并发布不可变 Index。
lib.rs: 导出镜像执行模块。
main.rs: 启动独立 Image Operator。
bin/: 提供 CRD、Runner 和 Publisher 进程入口。

resource_tests.rs: 验证平台、sidecar 生命周期、发布临时卷、任务材料与 CRD 字段类型。

[PROTOCOL]: Update this header on change, then check AGENTS.md
