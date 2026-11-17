<!--
[INPUT]: 依赖目标集群、Helm release、隔离协议回归与私有证据。
[OUTPUT]: 提供镜像交付快照、Operator 验收、取消锁回归与未验证边界。
[POS]: docs/ 的真实环境记录；补充 image-builds.md 和 operator-operations.md。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

# 集群镜像构建与独立 Operator 验收

日期：2026-11-17，Asia/Shanghai。
本记录保存首次完整镜像验收的集群快照。
表内 revision 和镜像摘要均对应该快照。
四个 Helm release 已部署，服务副本均就绪。
独立 Agent Operator 的 TLS、认证、存储与限额恢复通过。
Image Operator 的空闲 leader 删除与副本接管通过。
普通 sidecar 模式的运行中取消与任务资源清理通过。
原生制品经 Pingora 下载的摘要、HEAD 和 Range 通过。
正式平台 token 访问 Registry 通过，独立 S3 身份的私有 ACL 检查通过。
第八次 amd64 镜像构建成功，OCI 摘要读回和显式 Release 发布通过。
arm64、外部仓库和超时清理仍未验收。

## 集群与部署边界

目标集群的 master 为 `10.169.1.237`。
Kubernetes 版本为 `v1.28.3`。
六台节点均为 `amd64`，内核均为 `3.10.0-1127.el7.x86_64`。
本轮没有 arm64 节点，也没有启用 QEMU 或 binfmt。

命名空间为 `artifactory`，产品与 Chart 版本为 `0.2.0`。
部署前已保存数据库 dump、工作负载、Secret 和 Helm 参数。
原 PostgreSQL、S3、PVC、主密钥和设备 CA 保留。
本轮没有执行数据库恢复演练。

namespace quota `artifactory-trial` 在部署前备份。
新增网关、仓库及独立服务后，限额调整如下。
调整扩大 namespace 预算，不改变节点容量或各 Pod 限额。

| quota 参数 | 部署前 | 调整后 |
| --- | ---: | ---: |
| `limits.cpu` | `12` | `24` |
| `limits.memory` | `12Gi` | `24Gi` |
| `requests.cpu` | `4` | `8` |
| `requests.memory` | `6Gi` | `12Gi` |
| `pods` | `20` | `40` |
| `persistentvolumeclaims` | `6` | `8` |
| `requests.storage` | `40Gi` | `80Gi` |

部署前限额 CPU 已使用 `11900m`，PVC 请求合计 `31Gi`。
15:11 的读回为 CPU limits `21900m`、PVC 请求 `36Gi`。
该次读回有 23 个 Pod 和 5 个 PVC，均未超出新限额。
原始 quota 保存在私有 `quota-backup.json`。
调整前后投影保存在 `namespace-quota-verification.json`。

| Helm release | Revision | 状态 | 部署职责 |
| --- | ---: | --- | --- |
| `artifactory` | 15 | deployed | Control、控制台、原生构建服务及三类 Pingora 网关 |
| `registry` | 2 | deployed | 内建 Distribution OCI 仓库 |
| `image-operator` | 2 | deployed | 独立镜像构建 Operator |
| `agent-operator` | 3 | deployed | 独立 Agent Operator、Runtime 与 Source |

Control 为 `3/3` Ready。
edge、device、internal 三类网关各为 `2/2` Ready。
Registry 和 Image Operator 各为 `2/2` Ready。
Agent Operator、独立 Runtime、Source 各为 `1/1` Ready。
原 Web、Agent Worker、local-source、telemetry、PostgreSQL、S3 均就绪。
既有 `artifactory-emissary-gateway` StatefulSet 保留且就绪。

本轮开发入口为 `http://10.169.1.239:30810`。
控制台地址、镜像控制地址、仓库地址和 token realm 使用该入口。
它同时供集群 Pod 与局域网客户端访问。
这是显式 HTTP 开发验收，不代表公网 TLS 已验收。
S3 继续通过私有 ClusterIP 提供服务。
Pingora 反向代理本平台仓库和制品，不缓存公共上游仓库。

## 固定运行镜像

项目镜像采用固定源码 SHA tag 与 digest。
本轮从 `ghcr.nju.edu.cn/zhubby/` 拉取项目镜像。
该快照的源码为 `fef4b9aac54511b09369dc1055cb9aade1e4bf54`。
项目镜像摘要固定到 `linux/amd64` manifest。
Distribution 保持原固定引用。
下表摘要均为 `sha256`。

| 组件 | 源码提交 | 运行镜像摘要 |
| --- | --- | --- |
| Control、local-source、telemetry | `fef4b9a` | `0b9a94a3dc7d856bebea3b4106de563a4d7da66c2edf0d475bdabbd6c8c95aa6` |
| Pingora 网关 | `fef4b9a` | `e45e45f86ab293e4d7f8bf0f93c3c17bb6bd2b182135ddd4fa644c97f372b955` |
| Web | `fef4b9a` | `a2dd15d85b3c975cf6e8adaab24e5efbb73395cd0c34ad93bb5f435c86cdd6a0` |
| 原 Agent Worker | `fef4b9a` | `bd82dd65a2405d4d9baa83f63e0b0da46a7abd4cc796511a2a08a3360a7403fe` |
| Image Operator | `fef4b9a` | `a9dea4a1f0cbdec44026dc87826e1cb166a64a3cd179101343bb46ff62a607b3` |
| Image Worker | `fef4b9a` | `d313e601f4cc4ae34e95e8c6607a00c8d470092a08dfc4cc98efe704aed5f182` |
| Agent Operator | `fef4b9a` | `b157ec4e1259280b9f0d05dbf1e6f33580574f7b8093c849dd0286304627244d` |
| Planner Runtime | `fef4b9a` | `6a15e66292187cf5dcec7f4235d63d6b2dc01d22356a6fb75b2a344f3637561e` |
| Source Service | `fef4b9a` | `ef2c952b23bc836a5ef180157b47a2a32434085e447d2cd18b363fe759c14715` |
| Distribution 3.0.0 | 第三方 | `4ba3adf47f5c866e9a29288c758c5328ef03396cb8f5f6454463655fa8bc83e2` |

PostgreSQL 和 S3 保持部署前的镜像摘要。
源码提交 `a177429` 提供镜像构建与独立 Operator 主实现。
后续提交分别修复旧 Kubernetes sidecar、CRD 集合校验和 Source 限额。

## Kubernetes 1.28 兼容与构建池

该集群 API 会丢弃 initContainer 的 `restartPolicy: Always`。
因此，本轮不能使用 Kubernetes native sidecar。
提交 `3db5373` 提供显式普通容器模式。
Runner 等待 BuildKit 就绪，完成后写共享退出标记。
BuildKit 读取标记并退出，Job 保留构建结果。
本轮没有修改集群 feature gate 或主机 sysctl。

`BuilderPool/default` 使用以下约束：

| 参数 | 本轮值 |
| --- | --- |
| `nativeSidecar` | `false` |
| `maxActive` / `maxPerProject` | `1` / `1` |
| CPU / 内存 | `500m` / `768Mi` |
| 临时存储 | `3Gi` |
| 节点 | `boao-test-node239` |
| `emulationPlatforms` | 空集合 |
| BuildKit 配置 | `artifactory-buildkit-config` |

BuildKit 为 rootless `v0.26.2`。
固定镜像摘要为：
`sha256:0ffa2fcf6b8757c47d569b3ef0f03f9d5eb3b9ff5ce68d858f994f89b749da0c`。
实际构建仅在指定节点验证。
本轮没有证明其他节点满足 rootless 构建条件。

Image CRD 的平台字段改为 Kubernetes set。
提交 `fce8f1d` 修复 CEL 对重复平台的校验契约。
服务器校验通过后，集群成功创建 BuilderPool 与镜像执行资源。

Agent Chart 最低 Kubernetes 版本改为 `>=1.28.0-0`。
提交 `c50e1b6` 记录该兼容范围。
官方 Chart 在 `1.28.3` 参数下 strict lint 通过。
CRD CEL、投射 ServiceAccount Token、Lease 和 StatefulSet 通过服务器校验。
API 保留 `whenDeleted: Retain`、`whenScaled: Retain` 的 PVC 策略。
该策略依赖 `StatefulSetAutoDeletePVC`。

## Image Operator 切主验证

15:10，两个 Image Operator 副本均就绪。
ImageBuild、镜像 Job 和镜像 Worker Pod 均无活动任务。
Lease `artifactory-image-operator` 的 holder 为原 leader Pod UID。
本轮仅删除该 leader Pod，未修改其他工作负载。

| 阶段 | Pod | UID |
| --- | --- | --- |
| 原 leader，删除对象 | `image-operator-image-operator-699c7d676c-hr5bd` | `895e2197-8d8b-4a57-a89d-7a4ab034e775` |
| 原 follower，接管者 | `image-operator-image-operator-699c7d676c-4rfqt` | `64e4d30f-d70a-44bf-a404-68515b58c42f` |
| Deployment 补充副本 | `image-operator-image-operator-699c7d676c-tjm7j` | `87ac7079-ee25-43f3-9530-720dc581cab0` |

15:11:13，观察到原 follower 已接管 Lease。
从删除请求到观察到接管，经过 `47.9` 秒。
同次检查中，原 leader 已消失，新副本 Ready。
Deployment 的 ready、available 和 updated 副本均为 2。
Lease 有效期为 15 秒；47.9 秒包含 Pod 退出和观测时间。
本轮证明空闲主副本退出后的接管与副本恢复。
运行中镜像任务的故障恢复仍需单独验收。

## 独立 Agent Operator 验证

独立 Runtime 和 Source 使用新数据库及角色：
`artifactory_runtime_k8s`、`artifactory_source_k8s`。
两者具有独立加密 key，并共享本次新建的服务 token。
服务证书由原 CA 签发，SAN 覆盖服务和 Source shard。
现有项目的 `planner_backend` 保持原值。
本轮没有把既有构建切换到新 Runtime。

15:22:56，独立 release 完成最终镜像升级，revision 为 3。
升级参数仅改变 Operator、Runtime、Source 三项镜像 tag。
三项服务均使用上述固定源码和摘要，并各为 `1/1` Ready。
新 Operator Pod 持有 Lease，租约持续续期。
原服务 Secret 的 UID 和内容指纹逐项保持一致。
数据库配置、SourcePool 的非镜像字段和 Pod 资源限额保持一致。
Source PVC UID 与容量也保持一致。

并发滚动曾触发 namespace CPU quota 拒绝。
旧副本退出与 quota 回算后，Runtime 自动重试并就绪。
独立 release 保持默认 RollingUpdate。
本次升级没有再次扩大 quota，也没有改变资源限额。

`SourcePool/sources` 就绪，Source StatefulSet 为 `1/1` Ready。
`cache-sources-0` PVC 为 Bound，容量 `5Gi`，StorageClass 为 `local-path`。
PVC UID 为 `d6351bc9-3f2a-43b4-ab7b-e857bc12b3e8`。

Source 独立使用 Bucket `artifactory-source-k8s`。
Bucket 创建、对象写入和读回摘要通过，测试对象已删除。
本轮复用既有私有 S3 身份，没有修改原凭据。
独立 Bucket 不代表已完成独立 S3 身份隔离。

首次部署执行以下 TLS 与服务认证检查：

| 服务接口 | 无认证 | 正确服务 token | TLS 校验 |
| --- | ---: | ---: | --- |
| Runtime `/v1/queue` | 401 | 200 | CA 与主机名均通过 |
| Source shard `/metrics` | 401 | 200 | CA 与主机名均通过 |

Lease `artifactory-agent-operator` 正常选主。
本轮没有验证 Pi 任务执行、Operator 故障切主或新设备构建。

旧 Operator 仅给 Source 设置 requests，namespace quota 拒绝其 Pod。
提交 `4cec405` 让 limits 使用 SourcePool 的 CPU 和内存值。
有 TLS、无 TLS 两类构造回归均覆盖资源限额。
Agent Operator 的 11 项测试与全目标 Clippy 均通过。

升级后移除现场补充的 limits，Operator 在 `9.44` 秒内自动恢复。
恢复值为 `cpu=500m`、`memory=768Mi`，Source 再次就绪。
limits 的字段管理者仅为 `artifactory-operator`。
最终结果不依赖临时限额补丁。

## 原生制品与网关

通过新 edge 网关下载一项既有原生制品。
本轮没有创建新的原生构建。

| 检查 | 结果 |
| --- | --- |
| 制品 ID | `d4062e22-dc0c-4408-844d-449c4548916d` |
| 大小 | `188` 字节 |
| SHA-256 | `5648292064be5eb451ee1178b1dd2a1ba56febd9fd93191796a0644d793a10f5` |
| GET 读回摘要 | 通过 |
| HEAD | 通过 |
| Range | 通过 |
| 公开入口访问 internal API | 拒绝 |
| 匿名访问 Registry | 返回认证 challenge |

15:43:23，在最终镜像部署后重复上述网关检查，全部通过。
同时确认一个 AI Worker 在线，未排空、未撤销。
旧同名 Worker 身份已撤销并排空，不计入在线能力。

device 网关已部署，但新设备 mTLS 全链路未在本轮验收。

## 仓库认证与独立 S3 身份

早期构建上传候选镜像时，Registry blob HEAD 返回 401。
当时 token 端点返回 200，Registry 未接受该 token。
独立 RFC 7638 RSA JWK 指纹探针得到 200。
提交 `0b64a7f` 将 Distribution 3 的 token key ID 改为该指纹。
4 项回归、全目标 Clippy 与格式检查通过。
[修复镜像流水线](https://github.com/zhubby/artifactory/actions/runs/44036299596)
已成功，修复镜像已随上述最终 release 部署。

15:20:26，平台正式签发 token 后，请求 Registry `/v2/` 返回 200。
token 端点也返回 200，匿名请求仍返回 401 challenge。
本次临时仓库读取 token 已撤销，原始 token 未保存。
旧 401 记录另存为 `registry-auth-before-fix.json`。

Registry 使用独立身份与 Bucket `artifactory-registry`。
Distribution 上传对象时设置 private ACL。
该操作还要求 `s3:PutObjectAcl`，对应存储权限 `WriteAcp`。
本轮仅为该身份增加 `WriteAcp:artifactory-registry`。
Registry 凭据保持原值，原生制品存储身份也保持原值。
变更前的凭据配置与 S3 Deployment 已保存私有备份。
S3 保持原固定镜像，修改权限后恢复为 `1/1` Ready。

15:40:42，使用 Registry 身份执行独立存储探针：

| 操作 | 结果 |
| --- | --- |
| 带 private ACL 的 PUT | 200 |
| 签名 HEAD / GET | 200，内容匹配 |
| 匿名 GET | 403 |
| 列举原生制品 Bucket | 403 |
| DELETE | 204 |
| 删除后 HEAD | 404 |

原生存储初始化的 Admin 权限已覆盖 ACL，无需修改初始化脚本。
提交 `408ea37` 将独立 Registry 身份的 ACL 要求写入文档和 Chart 注释。
本节证明正式 token 认证与独立存储权限。
下节记录完整镜像推送、最终摘要与 Release 验证。

## 镜像构建结果

验证使用独立项目和固定源码，目标为 `linux/amd64`。
不允许模拟架构，Dockerfile 为 scratch 示例。

| 标识 | 固定值 |
| --- | --- |
| 项目 | `92e028c4-a5a2-4f55-8c36-23c4b7d7ca2c` |
| Build | `3163793c-348e-41ee-9a79-07a06f93582e` |
| 源码 SHA | `4cec405e0d274fb43f721626f2de14e872f91a8b` |
| 源码快照 | `9627f961-2c04-47aa-91a1-54de7f831fb3` |
| Context | `examples/image` |
| Dockerfile | `examples/image/Dockerfile.scratch` |
| 成功执行 ID | `c2844b62-cf16-4f6d-8313-6763154a0e04` |
| 当前构建状态 | `succeeded` |
| Build 的 `publish_status` | `succeeded` |
| 显式 Release | `a6569901-a1f2-423f-84e7-3b1c985c332d` |
| Release 版本 | `image-verify-69087bee9824` |

平台登录、Source HTTPS 仓库 refs、固定源码和 AI 计划均通过。
15:45:44，第八次执行完成 BuildKit 构建和仓库推送。
平台记录 `linux/amd64` manifest，并生成最终 OCI Index。
目标仓库为：
`00000000-0000-0000-0000-000000000001/92e028c4-a5a2-4f55-8c36-23c4b7d7ca2c/images/verification`。
本次 tag 为 `verify-64da44f4a29b`。

| 镜像对象 | SHA-256 | 字节数 |
| --- | --- | ---: |
| 最终 OCI Index | `b7bafae0422465d4a712fd9bce0ea002e67035a1461fde693ddf62c61293aa59` | — |
| amd64 manifest | `179c06ab710ebdc4c34ca3453a13c5dd15d5f5260518416e8916415e3957c3f4` | 476 |
| layer | `dc0dbf982d27be99d8c342ac06a1a3d2c3a5f564c5c1669603d9b92ecb6b0c03` | 148 |

15:48:01，正式平台只读 API token 完成镜像独立读回。
Index、manifest、config 和 layer 均逐项校验 SHA-256。
layer 的 HEAD 和 Range 检查通过。
执行结果、Registry 内容和平台 root digest 相互一致。
验证完成后，临时读取 token 已撤销。

本轮通过显式接口创建并发布项目 Release。
Release 包含一项镜像，其 `images.digest` 与上述最终 Index 一致。
发布后刷新 API，Build 与发布状态均为 `succeeded`。
本轮没有执行外部仓库复制或发布重试。

第七次执行 `9ba3c6ba-3ba1-495d-8e4d-a09553b61b01` 曾失败。
该次日志只有一条 Git 初始化记录，没有 BuildKit 推送日志。
第八次重跑未复现该失败。
现有日志不足以认定原因，本记录不推断其具体故障。
第八次原始日志仅保存在私有目录，文件权限为 `0600`。

## 运行中取消与资源清理

15:12:25，重试同一固定源码构建，执行 ID 为：
`b2f19b2d-45ca-4ccc-a081-0fcd2446e7d3`。
取消前，平台 execution 状态为 `running`。
实际 worker Pod 已观察到 `Running`。

调用平台构建取消接口后，build 和 execution 均为 `canceled`。
`4.38` 秒内，该 execution 的 worker Pod、Job 均归零。
以该 ImageBuild 为 owner 的任务 Secret 也归零。
这证明 Kubernetes 1.28 普通 sidecar 模式的运行中取消与清理通过。
本次取消完成时，build 状态为 `canceled`，发布为 `not_requested`。
超时触发与超时资源清理没有在本轮单独验收。

## 清理与最终状态

15:53:25，最终检查中的 16 个工作负载全部就绪。
namespace 使用 23 个 Pod、5 个 PVC，存储请求为 `36Gi`。
CPU limits 占用为 `21900m`，低于 `24` 的 quota。
15:54:54，本任务临时设施清理完成。

local-source 的 `HTTPS_PROXY`、`NO_PROXY` 已删除。
删除环境变量后，服务重建并恢复 Ready。
临时 GitHub 出口 NetworkPolicy 与网络探针已删除。
保留诊断用的失败 Job 和成功 Job 均已删除。
最终 `remaining_image_jobs` 为空。

master 的 `38445` relay 已停止。
本任务的 SSH reverse forward 与 ControlMaster 已退出。
本机 CONNECT proxy 和任务端口转发均已停止。
用户在本任务前已有的连接保留。
API、开发 NodePort 和 edge 的 LAN/PodCIDR 规则继续保留。
保留规则快照写入 `final-network-policies.json`。

## 网络检查与验收限制

验证为 namespace 补充 API Server 和开发入口的指定出口。
edge 网关仅补充局域网及 PodCIDR 的 8080 入口。
修正策略后，Source Git 访问完成重复验证。
本轮 GitHub 访问使用了仅允许 `github.com:443` 的临时 TLS CONNECT 代理。
该代理及其专用出口规则现已撤销。
后续 GitHub 访问需要集群自身提供可用出口。
本轮没有完成集群永久 GitHub 出网改造。

既有通用 allow policy 与 Pi policy 按相加语义生效。
本轮不声明生产级 Pi 网络隔离已经通过。
HTTP 开发入口和单节点构建也不证明生产可用性。

以下内容仍需单独验收：

- arm64 原生构建与 amd64/arm64 双架构 Index。
- 外部镜像仓库推送、重试和凭据轮换。
- 镜像构建超时、运行中任务故障恢复和跨租户拒绝。
- 生产域名、TLS、网关负载和设备连接恢复。
- 独立 Runtime 的 Pi 任务、设备构建和存储身份隔离。

## 控制平面取消锁回归

后续 Linux 协议验收发现节点锁等待问题。
事件和心跳先锁定 attempt，再等待节点授权。
旧 `FOR UPDATE` 阻断取消命令的外键检查。
因此，等待节点授权的事件会阻塞取消提交。

`renew_attempt` 和 `submit_execution_event` 改用 `FOR NO KEY UPDATE`。
执行身份保持不变，取消命令可引用该身份。
事件写入仍互斥，节点授权仍持有共享锁。
事务在锁等待后重新检查权限、租约与取消状态。

本机回归执行全部九个真实业务迁移。
真实 Store 验证 accept、finished 和 heartbeat。
三类操作等待节点时，取消均能提交。
撤权、证书过期和 session 替换的九组请求均被拒绝。
另有三组请求在 attempt 锁等待后拒绝过期租约。
预期状态在释放夹具锁前读取，拒绝请求不改变状态。

本机完整 `live_protocol` 通过，耗时 `209.28` 秒。
独立 PostgreSQL、S3 兼容 rclone 与 Pingora 实际运行。
Control API、gRPC 和调度复用应用实现。
数据库先执行全部迁移，再创建 Store。
该协议启动器与 Timescale 验收分开执行。
取消、授权、制品、并发请求、存储暂停与完成重放均通过。

私有锁链证据位于 `.deploy/ci-live-cancel/`。
该回归不改变上表的首次镜像验收快照。

## 取消锁修复的集群升级

18:50，主平台升级为 Helm revision `16`。
源码提交为 `db86735cf7b837a75c449e9e73a1c046f3f8ff7b`。
[镜像发布流水线](https://github.com/zhubby/artifactory/actions/runs/44067324865)
的十个服务镜像均发布成功。
仅更新本次发生业务代码变化的 Control 镜像。
该镜像也承载 local-source 与 telemetry。
其他组件保留首次验收时的固定摘要。

新镜像摘要为 `sha256:4a352d0cd741cedb3dc71743dbf3ff3b74317eecdd13f5c7b92b9857441c3c18`。
GHCR 与镜像代理均通过 manifest 和 config 摘要验证。
平台为 `linux/amd64`，revision 标签与源码提交一致。
Control、local-source、telemetry 分别为 `3/3`、`1/1`、`1/1` Ready。
五个旧 Pod UID 全部消失。

升级后完整制品复验通过。
OCI Index、manifest、config、layer 的 SHA-256 和 HEAD 均通过。
config、layer 和原生制品的 Range 均返回正确字节。
原生制品的 SHA-256 和 HEAD 也通过。
既有 Build、Release 和镜像摘要保持不变。
AI Worker 在线，公开 internal API 继续返回 `403`。
匿名 Registry 返回 `401`，健康入口和首页返回 `200`。
十三个业务 Secret 的 UID 和数据指纹保持不变。
五个 PVC 的 UID 和 PV 保持不变，状态均为 Bound。
临时项目只读 token 已撤销，验收会话已退出。
本轮没有新建构建或 Release。

首次历史数据读取出现一次未定位的 `ApiFailure`。
该次记录未包含具体接口和 HTTP 状态。
后续逐接口读取和完整制品复验均通过。
该现象不归因为锁修复或网关连接。

## 证据位置

私有证据保存在 `.deploy/k8s-image-verification/`。
该目录不纳入 Git，不公开 Secret、kubeconfig 或私钥。

| 记录 | 证据职责 |
| --- | --- |
| `image-end-to-end-verification.json` | 成功构建、OCI 摘要读回、显式 Release 与临时 token 撤销 |
| `image-attempt-8-summary.json` | 第八次成功执行、manifest 和最终 Index 摘要 |
| `image-attempt-4-logs.json` | BuildKit 推送与 Registry 401 |
| `image-attempt-7-logs.json` | 第七次失败执行仅有 Git 初始化日志 |
| `registry-auth-before-fix.json` | 修复前的 Registry 401 与 token 端点 200 |
| `registry-auth-smoke.json` | 正式平台 token、Registry 200 与临时 token 撤销 |
| `registry-rfc7638-probe.json` | RSA JWK 指纹独立探针 |
| `registry-s3-permission-fix.json` | Registry Bucket 限定的 WriteAcp 授权与备份 |
| `registry-s3-private-acl-probe.json` | 独立身份的 private ACL、签名读写、匿名与原生 Bucket 拒绝 |
| `native-artifact-verification.json` | 原生制品下载、摘要和公开接口拒绝 |
| `post-upgrade-service-verification.json` | 最终镜像部署后的网关复验与在线 AI Worker |
| `final-cluster-verification.json` | 16 个工作负载就绪、资源预算和临时设施清理 |
| `final-network-policies.json` | 最终保留的 API、NodePort 与 LAN/PodCIDR 规则 |
| `agent-operator/deployment-checks.json` | 独立工作负载、数据库和 PVC |
| `final-deployment-images.json` | 最终源码、项目镜像平台摘要和仓库引用 |
| `agent-operator/final-deployment-checks.json` | 最终 revision、镜像就绪、原身份与资源及 PVC 复核 |
| `agent-operator/service-checks.json` | Runtime 与 Source 的 TLS 和服务认证 |
| `agent-operator/storage-checks.json` | Source Bucket 的对象写入、读回和删除 |
| `agent-operator/quota-reconcile-check.json` | Source 资源限额的自动恢复 |
| `image-operator-failover.json` | 空闲 Image Operator leader 删除、Lease 接管和副本恢复 |
| `image-cancellation-verification.json` | Running worker 取消、状态回写和任务资源清理 |
| `quota-backup.json` | namespace quota 调整前的原始备份 |
| `namespace-quota-verification.json` | namespace quota 调整前后与使用量读回 |
| `verification-network-policy.json` | 现场验证的限定网络规则 |
| `source-git-after-network-policy.json` | 网络修正后的 Source Git 检查 |
| `lock-fix-upgrade-20261117T104303Z/` | revision 16 升级前备份、旧 Pod UID 和滚动升级读回 |
| `lock-fix-pre-rollout-baseline.json` | 十三个 Secret 与五个 PVC 的升级前身份指纹 |
| `post-lock-fix-service-verification-recheck.json` | 锁修复升级后的 OCI、原生制品、持久身份与 token 清理 |

[PROTOCOL]: Update this header on change, then check AGENTS.md
