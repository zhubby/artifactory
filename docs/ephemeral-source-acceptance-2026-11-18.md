# 临时源码执行验收 · 2026-11-18

本次用 SourceManifest 固定源码身份。
平台保存清单、对话、日志、计划和制品。
任务各自从上游拉取，结束后清理临时目录。
最终镜像和对话引用的源码片段继续保存。

## 已完成的代码验证

| 范围 | 结果 |
| --- | --- |
| Rust 工作区 | `cargo test --workspace --all-targets` 通过；外部依赖测试另行执行 |
| 业务数据库 | 真实 PostgreSQL 与隔离 S3 的 95 项业务回归通过 |
| Runtime 数据库 | 原有 11 项及大清单、并发准备授权 2 项通过 |
| 原生源码 | 每次上游拉取、子模块、LFS、临时缓存及孤儿进程清理通过 |
| 并发授权 | 原生租约、撤销、取消；镜像截止时间、平台、取消和终态通过 |
| 大清单 | 超过 3 MiB、65,000 个 tag 的材料回执只保留一份清单 |
| Worker | 17 项测试通过，包含 UTF-8 大响应边界 |
| Web | 类型检查及中英文规划、会话分页和终态刷新通过 |
| 部署脚本 | 36 项测试通过；Kubernetes 验收夹具 11 项通过 |
| 文档 | mdBook 构建和全部本地链接检查通过 |

TokenReview 的网络和服务端故障返回 503。
错误身份返回 403。
Runtime 在源码材料返回前复核 execution。
取消和 generation 切换使旧 Pod 失效。
源码材料读取在数据库锁等待后复核授权。

## 实际环境状态

本机为 macOS arm64，原生进程测试已执行。
专属 kind 集群使用原生 arm64 节点。
Kubernetes 完整任务链正在准备镜像和网络。
既有 amd64 集群的业务部署尚未变更。
尚未执行本次原型数据库重置。
Windows、内外仓库推送和正式下载链仍待验收。

真实数据库性能采样因转发连接断开结束。
本次不提供有效 p95 结论。
原生 shell 只接受可信项目，不提供系统级沙箱。

## 部署观察

部署后观察 SourceTask、AgentRun 和 ImageBuild。
检查准备、规划、构建和清理的状态收敛。
检查 `CleanupPending`、材料授权拒绝和 503 日志。
空闲后运行 `scripts/check_ephemeral_source.py --idle`。
该检查要求任务 Pod、Job、材料 Secret 全部消失。
检查不再存在 Source 服务或源码 PVC。
复核 Registry 只保留最终制品和有效引用。

重置前停止写入口及后台任务。
等待最长 15 分钟的在途票据失效。
保留制品对象、Registry 对象、PKI 和主密钥。
新业务与 Runtime 库不恢复旧授权和列表。
设备重新注册后再验收原生执行。

[PROTOCOL]: Update this header on change, then check AGENTS.md
