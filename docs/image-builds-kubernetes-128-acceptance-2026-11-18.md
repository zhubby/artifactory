<!--
[INPUT]: 1.28 API、正式节点 rootless 探针和本轮回归证据。
[OUTPUT]: 区分代码验证、隔离验收和正式部署状态。
[POS]: 普通容器镜像生命周期的实际验收记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

# Kubernetes 1.28 镜像构建验收 · 2026-11-18

本轮保留正式 Kubernetes 1.28.3。
使用普通源码 init、Runner 和 BuildKit 两个普通容器。
Publisher 使用独立单容器 Job。
原 1.37 验收不作为本轮 1.28 通过依据。

## 已取得的证据

| 范围 | 结果 |
| --- | --- |
| 隔离 API | kind 0.24.0、Kubernetes 1.28.13、containerd 1.7.18 |
| 原生 sidecar | kubelet 明确设置 `SidecarContainers: false` |
| CRD | 两个 Operator 的五个 CRD 通过 1.28 服务端校验 |
| Helm | 两个 Operator 在 1.28.3 版本下 lint、template 通过 |
| 控制器 | 34 项回归通过，包含删除 finalizer、UID 台账和残留 Pod 容量 |
| Linux 监督 | 17 项真实进程与协议测试通过，包含收尾、EOF、期限和有界诊断 |
| Control | 22 项真实数据库与 Kube HTTP 回归通过 |
| 1.28 源码主链 | 29 项通过，含 AI、原生构建、会话、租约、切主与材料清理 |
| 节点能力 | 五个 worker 的 `user.max_user_namespaces=28633`；master 为 0 |
| 正式节点 RUN | node234、内核 3.10.0-1127.el7、containerd 1.6.24 |
| rootless | 固定官方镜像在非特权、无宿主挂载条件下执行真实 amd64 RUN |
| snapshotter | 显式 native 与自动选择均成功；自动选择回退到 native |
| 最终 amd64 监督入口 | 正式 3.10 节点真实 RUN、导出、Stop ACK 与 EOF 对照通过 |
| 静态 ABI | amd64 静态 PIE、arm64 静态 ELF，均无 `PT_INTERP` |

官方 BuildKit 基线为 `moby/buildkit:v0.26.2-rootless`。
固定索引摘要为 `sha256:0ffa2fcf6b8757c47d569b3ef0f03f9d5eb3b9ff5ce68d858f994f89b749da0c`。
源码 init、任务凭据与 Runner 私有卷不进入 BuildKit。

## PID namespace 的实测收敛

正式 1.28 把 `procMount: Unmasked` 重置为 `Default`。
额外 rootlesskit PID namespace 无法挂载对应 procfs。
该路径已从实现中移除，不要求开启 Alpha 功能。

监督程序使用 BuildKit 容器自身的 PID namespace。
它作为 PID 1 和 subreaper 回收进程树。
Runner 使用另一容器的 PID namespace 和私有卷。
BuildKit 内的 RUN 能看到监督进程，仍只接受可信源码。

## 待完成的环境验收

arm64 的 fsGroup 修复产物已完成内外仓全链。
最终 cc 链接产物正在隔离 1.28 复验。
amd64 仿真、Index 和故障收尾尚未在本记录中标记通过。
正式业务部署与原型数据库重置尚未执行。
PKI、主密钥、S3 对象和 Registry 对象保持原有边界。

去敏机器证据保存在本轮私有验收目录。
本文只记录白名单事实，不包含 kubeconfig 或任务认证材料。

[PROTOCOL]: Update this header on change, then check AGENTS.md
