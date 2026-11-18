<!--
[INPUT]: 独立 Operator、Runtime 和一次性源码镜像。
[OUTPUT]: 定义源码、规划和镜像 Job 的安装及收尾约束。
[POS]: Kubernetes 1.28 工作负载运维入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Kubernetes Agent Operator 运维

Agent Operator 是独立 Rust 服务。它协调 `AgentPool`、`AgentRun` 和 `SourceTask`。源码任务与规划任务各自按 AgentPool 预算准入，均使用临时 Job，不创建常驻源码服务、源码 PVC 或源码 S3 对象。

Kubernetes 最低版本为 1.28。使用同一命名空间部署一套 Runtime 与 Agent Operator。Operator 的两个副本共享 `artifactory-agent-operator` Lease，只有持有 Lease 的副本修改资源。

## 安装

先准备业务数据库、Runtime 数据库和服务身份。Runtime Secret 包含 `DATABASE_URL`、`ARTIFACTORY_SERVICE_KEY`、`ARTIFACTORY_SERVICE_TOKEN`。Control 使用相同服务令牌。TLS Secret 包含 `tls.crt`、`tls.key`、`ca.crt`，证书覆盖 Runtime DNS。源码任务仅使用投影的任务身份，不取得业务数据库或 S3 凭据。

```sh
kubectl --context YOUR_CONTEXT apply -f operators/agent-operator/config/crd/resources.yaml
helm upgrade --install native charts/artifactory-agent-operator \
  --kube-context YOUR_CONTEXT -n artifactory -f agent-values.yaml
```

```yaml
runtime:
  secretName: artifactory-runtime
  url: https://artifactory-runtime:8091
  grpcUrl: https://artifactory-runtime:8092
controlUrl: https://artifactory-gateway-internal:8445
tls:
  secretName: artifactory-services-tls
agentPool:
  name: default
  maxActive: 8
  maxPerProject: 4
  image: ghcr.io/zhubby/artifactory-agent-worker:0.2.0
  sourcePreparerImage: ghcr.io/zhubby/artifactory-source-preparer:0.2.0
  ephemeralStorage: 10Gi
networkPolicy:
  gitPorts: [443, 80, 22]
```

私有 Git 使用其他端口时，将端口加入 `networkPolicy.gitPorts`。Control 的 `planning.namespace` 与 `planning.agentPool` 指向此命名空间和池。跨命名空间部署时，Runtime 和 Control 地址须包含命名空间 DNS。Gateway 内部策略只放行主部署及配置的规划、镜像命名空间。`planning.runtimeUrl` 指向 Runtime；`planning.serviceSecretName` 提供服务令牌；`planning.serviceCaSecretName` 提供 `ca.crt`。

## 源码任务

Control 创建 `SourceTask`，固定 `taskId`、`tenantId`、`projectId`、`purpose`、`pool` 与 `timeoutSeconds`。`purpose=resolve` 解析固定 commit、子模块、tag 和 LFS 元数据，默认总期限 900 秒。`purpose=refs` 查询分支与 tag，由 Control 设置 30 秒总期限。任务取消只能从 false 改为 true。

Operator 创建 `source-<taskId>` Job。Job 使用 `artifactory-source-preparer` ServiceAccount，投影 audience 为 `artifactory-source-prepare` 的短期 token。源码准备进程向 `/internal/source/prepare` 领取材料。Control 使用 TokenReview 验证 token，再核对 Pod→Job→SourceTask 的 owner UID 和业务任务状态。仓库凭据只进入任务进程与内存卷。

解析首次得到的 ref OID 和每个仓库的 tag 集合通过 `/internal/source/prepare/pin` 固定。重试读取首次固定值。准备进程通过 `/internal/source/prepare/commit` 提交元数据。数据库保存固定清单与摘要，不保存 Git 对象、LFS 对象或工作树。

## 规划任务

`AgentRun` 固定 `runId`、`generation`、`pool`、`sourceManifestId` 和 `sourceDigest`。选机任务没有源码清单，因此不创建源码 init container。

有源码的规划 Pod 先执行 `artifactory-source-preparer checkout`。init container 通过 Runtime `/v1/prepare` 领取固定源码与凭据，每次从原 Git/LFS 上游重新拉取。`/workspace` 是有大小限额的 emptyDir。init 专属 token、SSH key 和 askpass 材料不挂载给主容器。

主容器只读挂载 `/workspace/source` 和 `/workspace/source-manifest.json`，工具在本地读取文件。模型调用、预算、租约和结果仍由 Runtime 控制。AI 对话、日志、计划及明确产出的制品允许持久保存。

## 清理与故障

镜像平台 Job 使用普通源码 init、Runner 和 BuildKit 两个普通容器。它不需要 `SidecarContainers` 功能开关。BuildKit 镜像必须包含 `artifactory-image-buildkit` 监督入口，不能直接替换成上游镜像。Runner 通过本地 Unix socket 维持租约，构建校验完成后等待 BuildKit 进程树停止，再提交平台结果。

任一应用容器非零退出或 OOM，Image Operator 立即进入失败收尾。Published 回报只保存候选摘要。Control 核对原 ImageBuild UID、完整 Job 成功和任务期限后，才提交业务成功与制品记录。

ImageBuild 保存所属 Job 的 UID 台账。创建 Job 前，Operator 安装清理 finalizer。直接删除 ImageBuild 也须等待实际任务 Pod 收尾，才能移除该 finalizer。删除 Job 后，Operator 仍按 Pod owner UID 识别残留执行。活动、状态未知或尚未确认停止的 Pod 占用容量。所有容器已终止的 Pod 保留诊断证据，但释放执行槽，允许 Publisher 准入。节点失联时保留 `CleanupPending`，不能以 API 逻辑删除代替实际停止。

Job 不共享仓库或 BuildKit 缓存。源码任务结束、失败或取消后，Operator 按 owner UID 删除 Job，并等待 Pod 消失。删除失败保留 `CleanupPending`，后续继续重试。源码 Job 的 300 秒 TTL 仅在任务终态清理时设置，避免丢失终态观察后重新执行任务。规划 Job 有 300 秒 TTL 兜底，其执行状态以 Runtime 为准。

源码任务 `backoffLimit=2`，重试共享总截止期限。规划 Job 不在 Kubernetes 内重试，恢复由 Runtime generation 控制。上游固定 commit 被删除或无法拉取时，任务报源码不可用，不切换当前分支。

Operator 重启后按 CRD、Job owner UID 和 Runtime 状态继续清理。节点失联时，逻辑删除不能证明节点本地磁盘已经回收；待节点恢复后检查 kubelet 回收，退役节点按设备磁盘策略处置。

```sh
python3 scripts/check_ephemeral_source.py --context YOUR_CONTEXT --namespace artifactory --idle
```

环境验收需覆盖私有 SSH/HTTPS、显式 HTTP、子模块、tag、LFS、任务取消、OOM、节点恢复与 Operator 切主。验证任务结束后没有源码 Pod/PVC，S3 没有新增源码对象，下次任务仍访问原 Git。
