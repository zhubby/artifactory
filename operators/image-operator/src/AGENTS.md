# image-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义 BuilderPool 和 ImageBuild 的 Kubernetes 契约。
resources.rs: 用非阻塞归属创建独立 BuildKit 和发布 Job。
controller.rs: 协调任务准入、子资源归属、终态与取消。
leadership.rs: 使用独立 Lease 保护 Image 控制器写入。
worker.rs: 检出源码，配置认证 CA 并提交限长脱敏日志。
transport.rs: 提供任务令牌认证的控制平面回执。
publisher.rs: 汇总平台结果并发布不可变 Index。
lib.rs: 导出镜像执行模块。
main.rs: 启动独立 Image Operator。
bin/: 提供 CRD、Runner 和 Publisher 进程入口。

resource_tests.rs: 验证平台选择、任务材料与 CRD 不变量。

[PROTOCOL]: Update this header on change, then check AGENTS.md
