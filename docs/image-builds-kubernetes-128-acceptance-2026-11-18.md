<!--
[INPUT]: 1.28 API、正式部署、原生设备和本轮回归证据。
[OUTPUT]: 区分隔离故障验收与正式集群运行结果。
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
| 控制器 | 35 项回归通过，包含删除 finalizer、UID 台账、残留 Pod 容量和公开 CA 投影 |
| Linux 监督 | 17 项真实进程与协议测试通过，包含收尾、EOF、期限和有界诊断 |
| Control | 22 项真实数据库与 Kube HTTP 回归通过 |
| 1.28 源码主链 | 29 项通过，含 AI、原生构建、会话、租约、切主与材料清理 |
| 节点能力 | 五个 worker 的 `user.max_user_namespaces=28633`；master 为 0 |
| 正式节点 RUN | node234、内核 3.10.0-1127.el7、containerd 1.6.24 |
| rootless | 固定官方镜像在非特权、无宿主挂载条件下执行真实 amd64 RUN |
| snapshotter | 显式 native 与自动选择均成功；自动选择回退到 native |
| 最终 amd64 监督入口 | 正式 3.10 节点真实 RUN、导出、Stop ACK 与 EOF 对照通过 |
| 静态 ABI | amd64 静态 PIE、arm64 静态 ELF，均无 `PT_INTERP` |
| 最终 arm64 镜像链 | cc 静态产物在原生 arm64 节点通过，75.883 秒 |
| amd64 显式仿真 | Docker 与节点 chroot 先证实执行能力，完整镜像链通过，54.122 秒 |
| 双架构 Index | 两个平台真实 RUN、Index、全部 Blob、HEAD、Range 及外仓通过，66.239 秒 |
| 进程故障 | Runner、BuildKit daemon、监督程序分别被杀后失败收尾通过 |
| OOM | Runner 与监督程序实际退出 137，内核同组 OOM 记录、业务失败与清理通过；监督程序连续复验两次 |
| 任务收尾 | 取消、源码 init 失败、Job DeadlineExceeded 与 Operator 重启通过 |
| 节点断联 | 8 次采样保留物理孤儿与容量，39.081 秒；恢复 kubelet 后清理通过 |
| 正式发布镜像 | 同一提交的 11 个 amd64 镜像完成发布、源仓与镜像站摘要、架构和源码标签读回 |
| 正式 node239 | 固定发布摘要完成 amd64 原生与 arm64 仿真真实 RUN、Stop ACK，所有容器退出 0 |
| 远端完整 CI | `438478c` 与 CA 修复 `f1fe2db` 均完成 11 项作业，含 1.28 主链、10 项故障与六平台原生生命周期 |

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

## 验收工具的实测修复

完整镜像链覆盖固定源码 init、内外仓推送和材料清理。
首次 OOM 注入在 cgroup 身份校验阶段停止，未写入内存限制。
校验已覆盖精确 systemd 与 cgroupfs Pod/容器路径。
CRI 有时先返回 `Error/137`，随后容器记录即被清理。
OOM 验收要求实际退出 137，且内核记录证明同一 cgroup 因内存限制整组终止。
仅有退出码、其他容器 OOM 或普通强杀均不能通过。
节点恢复后，转发在 kubelet 缓存重建前退出；验收工具现已限时重建。

首次远端 1.28 CI 在导入 Distribution 归档时失败。
kind 的全平台与 digest 别名会解包无标签证明清单。
导入现只选择目标平台和已有标签，不改变运行 manifest。
相同非运行清单夹具已证明旧入口失败、新入口成功。
远端复验 `44280867250` 全部 11 项作业成功。
1.28 完成固定源码、AI、原生交付、内外仓和全部故障验收。
最终 amd64/arm64 多架构验收通过，无任务材料残留。

CA 修复的 [完整 CI 44292315783](https://github.com/zhubby/artifactory/actions/runs/44292315783) 同样通过十一项作业。
Windows ARM 首次因 Chocolatey HTTP 504 未安装 protoc 而失败。
只重跑该作业一次，真实 Rust 契约与专用账号服务生命周期均通过。
其余十项成功作业未重跑。
[十一镜像发布 44292333345](https://github.com/zhubby/artifactory/actions/runs/44292333345) 全部成功。
正式环境只采用其中新的 Image Operator，其他组件保持原发布基线。

## 正式集群切换

正式集群保留 Kubernetes 1.28.3 与 Calico 3.26.1。
四个 Helm release 已完成切换。
主服务、Registry、Agent Operator 与 Image Operator 的 revision 分别为 20、3、1、4。
Gateway 只有一个 Deployment，运行两个副本。
普通构建容器不依赖原生 sidecar 或 Alpha 功能。

十个组件保留发布源码 `ecc980dba99e71678b495f4d6f8149df8345eb1c`。
Image Operator 使用公开 CA 投影修复 `f1fe2dbc6bdc2823f3b062d671195009b4f8c0d4`。
其固定摘要为 `sha256:c0f02962ab4b56d61cee65f898d3c673c36ab045a58cccb9bb7093aacc9a3dde`。
两批镜像分别保存发布回执与逐组件源码身份。
不能把此次混合发布记为所有组件来自同一提交。

维护期阻断公开入口，停止旧服务和设备。
从最后一个旧 Control Pod 退出起完整等待 900 秒。
两个业务库的 dump 通过目录与完整归档读取校验。
随后重建 Control 与 Runtime 两库，不导入旧业务行。
保留数据库角色、遥测库、PKI、主密钥和制品存储。
十一个保留 Secret 的 UID 与数据指纹未变。
三个保留 PVC/PV 的 UID 与绑定关系未变。

旧 Agent release 的历史 manifest 引用已删除的 SourcePool。
备份后只删除三条旧 Helm 历史 Secret。
新 release 采用原 ownership 的现有对象，保留对象 UID。
不恢复旧 CRD，不加入兼容或迁移分支。

两个专属源码 PVC/PV 已回收。
逐节点探针确认两个 local-path 宿主目录均不存在。
三个旧源码对象前缀为空。
Registry 在停止写入后完成快照、精确缓存引用删除和 GC。
GC 未启用 `--delete-untagged`。
旧最终 Index、manifest 和 blob 在新 Gateway 后仍可读取。

## 正式任务链

通过公开 Gateway 完成 amd64 原生、arm64 显式仿真和双架构构建。
三种模式均使用固定提交与真实 Git/LFS 上游。
每个平台执行真实 `RUN`，核对 ELF 与执行标记。
Index、全部 blob、HEAD、Range 和任务材料清理通过。
正式集群均为 amd64；arm64 在 node239 显式仿真。
上表的 arm64 原生结果来自独立环境。

amd64 Index 为 `sha256:25037a6a42cd1654cdcbbe6c799c49b1c25520212b0677a8164197e0633a3664`。
双架构 Index 为 `sha256:cf99a6d6e91e5c988573bc085171efafef5bbb765bf82efe64e3fcf4876cd84c`。
源码 init、Runner 与 BuildKit 均实际退出 0。
构建与 Publisher 的 CA 卷只投影 `ca.crt`。
正式容器已核对该目录，不包含服务私钥。

本机 Mac、Mac mini 与 Windows 以新身份完成原生构建。
制品 GET、HEAD、Range、哈希与临时目录清理通过。
临时 Git CA 配置已恢复原值。
重新启动后核对实际服务账号、launcher 与 OTA 程序摘要。
服务定义和 OTA 公钥保持原值。
Windows 使用唯一 `app/native/windows/amd64` 目标。
真实 PowerShell 构建退出 0。
Windows 配置按原字节 SHA256 与原 ACL 精确恢复。
三台设备在线、版本一致、心跳新鲜且无活动任务。

独立 Publisher 把双架构 Index 推送到认证外仓。
内外仓根摘要一致，全部图对象与实际 RUN 标记核对通过。
运行中取消使用固定 90 秒延迟，确认两个普通容器运行后才取消。
受控失败只修改构建参数，日志明确记录缺失 COPY 输入。
两条路径均核对业务终态与 Job、Pod、任务 Secret 归零。
临时仓库凭据和只读令牌均已撤销。

第一次外仓上传成功，最终回读被本轮临时策略阻断。
同一 Control Pod 的无认证请求在修复前超时。
补充限定出口后，同一路径返回预期 401。
新 publication 完整成功，生产代码未变。
临时策略只允许 Control 到本轮外仓 TCP 5000。
验收后两条临时策略与夹具 namespace 均按 UID 删除。
临时 Git 叶私钥、外仓密码和 Registry GC 恢复点已回收。
数据库备份与去敏证据保留。

独立预检 namespace 与本轮专属 kind 已删除。
清理前扫描全部可删除的命名空间资源，未知归属为零。
其他本机容器和默认 kubeconfig 未改变。
最终 `check_ephemeral_source.py --idle` 通过。
公开入口后十八项只读检查全部通过。
包含固定镜像 imageID、双副本网关、独立 Service 边界及身份保留。
正式工作负载的 Pod UID、镜像、重启次数与开放前快照一致。

## 验证边界

真实故障矩阵在独立 1.28 环境执行。
正式共享集群完成公开业务链与取消、构建失败验收。
正式节点未注入节点级磁盘压力。
不把空间限制配置或一般失败测试记为磁盘压力实测。

去敏机器证据保存在本轮私有验收目录。
本文只记录白名单事实，不包含 kubeconfig 或任务认证材料。

[PROTOCOL]: Update this header on change, then check AGENTS.md
