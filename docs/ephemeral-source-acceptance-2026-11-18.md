<!--
[INPUT]: 临时源码执行、独立集群与不可变镜像验收证据。
[OUTPUT]: 记录已通过的检查与正式部署边界。
[POS]: 临时源码方案的实际环境验收记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

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
| SSH | 隔离 Linux sshd 的固定提交检出、主机信任和客户端密钥拒绝通过 |
| 源码校验 | 私有临时索引拒绝隐藏改动；只读工作树、LFS、子模块通过 |
| 并发授权 | 原生租约、撤销、取消；镜像截止时间、平台、取消和终态通过 |
| 大清单 | 超过 3 MiB、65,000 个 tag 的材料回执只保留一份清单 |
| Worker | 17 项测试通过，包含 UTF-8 大响应边界 |
| Web | 类型检查及中英文规划、会话分页和终态刷新通过 |
| 镜像发布 | Publisher 改用 `/work` 临时磁盘卷；Operator 10 项、Registry 4 项通过 |
| 镜像材料 | 3,552,288 字节合法清单只生成 1,345 字节信封；控制平面 9 项、镜像协议 2 项通过 |
| CRD 安装 | 删除无类型的 `snapshotId`；两份固定清单 CRD 通过 Kubernetes 1.37 服务端校验 |
| 部署脚本 | 36 项测试通过；Kubernetes Python 夹具 23 项及 TLS LFS 回归通过 |
| 文档 | mdBook 构建和全部本地链接检查通过 |

TokenReview 的网络和服务端故障返回 503。
错误身份返回 403。
Runtime 在源码材料返回前复核 execution。
取消和 generation 切换使旧 Pod 失效。
源码材料读取在数据库锁等待后复核授权。
镜像 Secret 的源码部分只保存清单 ID 和摘要。
执行配置与凭据照常下发。
init 直接领取完整清单并写入任务临时卷。
Runner 核对清单身份和内容后校验只读工作树。
旧 `source` 字段被拒绝，原 900 KiB 材料上限保留。

## 实际环境状态

本机为 macOS arm64，原生进程测试已执行。
专属 kind 集群使用原生 arm64 节点。
Kubernetes 1.37 完整任务链通过 29 项独立检查。
SourceTask、AI init、原生制品与会话持久化通过。
网关 GET、HEAD、Range 和 LFS 实字节校验通过。
Operator 主副本接管、Pod 丢失和新 generation 通过。
累计预算、原截止时间、撤权和 Runtime 重启通过。
终态 Pod、Job、任务 Secret 与源码 PVC 清零。
本机 macOS arm64 外部原生构建通过。
宿主直接拉取 Git 与 LFS，并完成制品下载。
宿主遥测新鲜，托管进程采样共 9 条。
临时设备、公告地址、排空标志和网络规则已恢复。
三轮冷 Pi Pod 验证 Runtime 与 Git 可达。
控制平面、Gateway、数据库与 S3 均被策略阻断。
arm64 原生和 amd64 仿真镜像真实 `RUN` 通过。
两架构从同一固定清单检出并生成 OCI Index。
内建 S3 仓库和外仓的 Index 摘要一致。
平台配置、ELF 架构和实际执行标记均匹配。
网关 SHA256、HEAD、Range 与任务资源回收通过。
`d08e782` 四个构建 Pod 的十二条源码、认证及 BuildKit 路径已消失。
实际任务材料约 1.9 KiB，未包含完整清单。
真实 OOM 恢复通过，状态为 `OOMKilled`、退出码 137。
只调整已核对 Pi 容器的内存限制。
只读容器及同 Pod 父 cgroup 的计数增长。
自然租约到期后 generation 从 1 切到 2。
累计调用从 1 增到 2，上限、原截止时间和固定清单保留。
旧会话为 interrupted，新 Pod 创建新会话。
旧租约心跳拒绝，新租约心跳成功。
当前旧租约心跳返回通用 500，运行状态未改变。
取消后的任务、夹具和临时网络策略均已清理。

独立 Cilium 集群启用 `policyCIDRMatchMode=nodes`。
原生验收只临时放行该节点 IP 和 Git 端口。
宿主及受限 Pod 读取 refs 成功后才提交任务。
LFS 下载地址使用本次 HTTPS 请求的源地址。
既有 amd64 集群使用 Kubernetes 1.28.3。
本段历史验收曾采用要求 1.33+ 的原生 sidecar。
当前实现已改为普通容器，保留现有 1.28 集群。
本轮结果见 [1.28 验收](image-builds-kubernetes-128-acceptance-2026-11-18.md)。
业务部署和原型数据库重置均尚未执行。
现有业务集群的部署下载链仍待验收。
提交 `d08e782` 的六平台原生生命周期全部通过。
覆盖 Linux、macOS、Windows 的 amd64 与 arm64。
提交 `d08e782` 的完整平台 CI 十项全部通过。
包含失联取消、准入负载、冷 Pod 隔离和外部 Linux Git/LFS。
大清单修复 `d08e782` 的三种镜像模式复验全部通过。
提交 `d08e782` 的十个 amd64 镜像发布成功。
四个 Chart 和十六个镜像配置槽已校验。
项目镜像固定完整源码 SHA 和公布的摘要。
PostgreSQL、S3、Registry 与 BuildKit 镜像保持部署前引用。
部署候选已渲染，尚未应用到业务集群。
镜像站十个组件的标签和固定摘要共二十个 manifest 校验通过。
本次只读取 manifest，正式集群拉取镜像层仍待部署验收。

[平台 CI](https://github.com/zhubby/artifactory/actions/runs/44244785790)
与[不可变镜像发布](https://github.com/zhubby/artifactory/actions/runs/44244880860)
保留实际执行结果。

业务构建完成与 Runtime 接受回执跨库收敛。
验收等待同一会话终态，保留原有摘要与消息断言。
等待期间仅保存会话元数据、消息数量和类型。

后续遥测验收发现构建池被前一项测试暂停。
暂停测试在退出时恢复原准入配置并等待就绪。
后续测试先检查准入，并保存白名单诊断。
该超时由验收状态泄漏引起。
独立集群已实际验证暂停后恢复原容量并等待 Ready。
遥测服务离线期间，真实规划、构建和制品交付通过。
恢复后持久 spool 补传并清空。
过程样本 6 条，主机样本 20 条。
PID、步骤身份与隐私字段隔离检查通过。
最终空闲检查通过，Linux 设备源码和认证目录零残留。
本次专属 kind 集群已删除，全部临时进程已停止。
其他 kind 集群、默认 kubeconfig 和 PKI 保持不变。

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
