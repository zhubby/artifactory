<!--
[INPUT]: TaskNotes 固定提交与 Kubernetes 1.28 正式环境回执。
[OUTPUT]: 多平台构建、公开网关下载和真实运行的验收边界。
[POS]: 真实业务应用验收；不替代基础镜像生命周期验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# TaskNotes 真实应用验收 · 2026-11-18

本轮使用 [TaskNotes 仓库](https://github.com/zhubby/multi-platform-build-demo)。
固定提交为 `d56401a5c86547db0d27456b4737b478fde9376c`，应用版本为 `0.1.0`。
应用来自“设计 Artifactory 多平台应用”任务。
本轮没有修改应用源码、锁文件或参考任务。

平台运行在 Kubernetes `1.28.3`，正式服务源码基线为 `83ba0dcd946d27ae60e5d0336747a41346f9bbbb`。
每个平台使用独立项目，任务直接检出同一个固定提交。
原生任务限制在指定设备执行。
源码与任务缓存放在任务目录或 Pod 的 emptyDir。
平台保留构建计划、日志、源码清单和制品。

## 已完成的原生构建

| 目标 | 实际执行环境 | 构建 ID | 下载主制品 | 状态 |
| --- | --- | --- | --- | --- |
| macOS | Mac mini，原生 arm64 | `246c98ac-d9a8-4c12-b0da-0718a1d07bfb` | `tasknotes-macos-arm64.zip` | 检查、14 项 Swift 测试、构建、源码校验与下载校验通过 |
| iOS Simulator | Mac mini，原生 arm64 | `e10d5357-2a57-4f66-84d1-bdb98e5d3afe` | `tasknotes-ios-arm64-simulator.zip` | 检查、14 项 Swift 测试、Xcode 构建与下载校验通过 |
| Android | Cassiopeia，JDK 17 / SDK 36 | `5f80b429-511d-4d03-bde3-0a8d60ea808b` | `tasknotes-android-debug.apk` | 检查、单元测试、APK 构建与下载校验通过 |
| Linux GTK | 临时原生 linux/amd64 节点 | `dea5a42d-3bb7-4957-92e2-e35292e42607` | `tasknotes-linux-x64.tar.gz` | 检查、核心测试、GTK 构建与下载校验通过 |
| Go 服务端 | 临时原生 linux/amd64 节点 | `d056f0fc-fcee-43af-aad8-0a5c540a9756` | `tasknotes-server-linux-x64.tar.gz` | go vet、race 测试、构建与下载校验通过 |

每个成功目标生成主制品、manifest 和 SHA256SUMS。
三份文件均通过公开 Pingora 网关下载。
下载逐项核对平台记录的 SHA256 与大小。
应用 manifest 同时验证版本、完整提交、目标架构和 `source_dirty=false`。
SHA256SUMS 覆盖主制品及 manifest 本身。

| 主制品 | 字节数 | SHA256 |
| --- | ---: | --- |
| macOS ZIP | 258141 | `3be93d1d3017c62d38a1fd1156a5b7634f3097c114e84416c857e6ce70741a06` |
| iOS ZIP | 255635 | `a25fe67f633a5d1f2a5b6bef0afae998f1a19d0515f26c57926c24241afe4266` |
| Android APK | 9349985 | `e64551a19cb7e7bb58b97b0dee93b1e7507764f32833b3d9ee25609fe6a0c498` |
| Linux GTK TAR.GZ | 1570982 | `33266b1d32c7eb2ac750cbfbbdcff53934dd85ca787df7ce3d22320aba9ae618` |
| 服务端 TAR.GZ | 4790177 | `dc970b823d896ef099df0fb9a8914d11e5c2e353c38f81cd7272d78fed9da9a5` |

## OCI 镜像

镜像构建在 Kubernetes 1.28 的原生 amd64 节点完成。
构建 ID 为 `4a6a2524-8af1-4f3f-869e-d6d99fb00344`。
执行 ID 为 `86e99f36-349b-4b6e-a332-61e18fefeaa4`。
目标为 `linux/amd64`，`allow_emulation=false`。
基础镜像为 Go 1.27.3 Alpine，源码仍为同一固定提交。
镜像推入内建仓库，标签为 `tasknotes-d56401a`。

根 Index 摘要为：
`sha256:6b595d456a8daa0403f4f08492081a875fe58b7fc77222a5f90c24e3d8b1e4c2`。
仓库路径为：
`eabaa8f7-3736-4c00-9468-6122b633bee8/083daf23-d228-4340-b3f8-9bdfe59c40c4/images/tasknotes-server`。

通过公开 Pingora 网关读回完整 OCI 图。
逐项校验 Index、Manifest、Config、Layer 的摘要与大小。
根摘要和标签返回同一 Index。
GET、HEAD 与 Layer Range 均通过。
最终镜像使用 scratch，用户为 `65532:65532`，入口为 `/tasknotes`。
读取身份在校验后已撤销。

从网关下载的原始 Config 与 Layer 导入 Docker。
逐层校验解压内容与 rootfs diff_id。
Docker Image ID 等于 OCI Config 摘要。
在本机 arm64 Docker 引擎中仿真运行该 amd64 镜像。
真实容器通过版本、健康检查、401、中文 CRUD、409 冲突和删除检查。
根文件系统只读，SQLite 写入 UID 65532 的临时目录。
本轮未声明该镜像在原生 amd64 容器运行时完成运行验收。
验收容器和本轮导入的应用镜像标签均已删除。

## 真实运行

macOS 下载包使用 `ditto` 解压，保留可执行权限。
`codesign --verify --deep --strict` 通过。
实际启动下载的 TaskNotes.app，观察到任务、笔记和连接设置界面。
Bundle ID 为 `com.tasknotes.demo.macos`。
macOS 客户端尚未连接真实服务验证写入。

Android 下载的新 APK 已安装到 API 36 arm64 模拟器。
包名为 `com.tasknotes.app`，版本为 `0.1.0`。
本轮 APK 使用新的开发签名，验收先卸载旧测试包，再安装新包。
应用已实际启动，并连接本轮下载的服务端读取任务和笔记。
停止服务后，应用保留缓存并进入只读。
重新启动服务后，应用刷新并恢复连接。

Linux 下载包中的文件为原生 amd64 ELF。
该文件以 UID 10001 在 Xvfb 中启动。
X11 确认创建了标题为 TaskNotes 的 GTK 窗口。
本轮未验证 Linux 窗口的联机业务操作。

服务端下载包中的文件为原生 amd64 ELF。
`--version` 返回 `0.1.0` 和完整固定提交。
该文件在临时 Linux Pod 中以 UID 10001 运行。
真实 HTTP 验证健康检查、401 鉴权、中文任务与笔记 CRUD、409 冲突和删除。
实际停止并重启进程后，任务和笔记仍保存在同一个 SQLite 数据库。
服务端临时令牌经 stdin 注入。
验收回执不保存令牌。

## 尚未通过的环境边界

Windows 原生节点已执行应用入口。
核心测试通过 Unicode、公共样例、缓存隔离、原子刷新、离线写入限制和令牌排除检查。
WinUI 的 VS C++ 组件检查失败。
后续检查确认 VS 安装实例不完整，尚无完整 Windows ZIP 或窗口运行证据。
独立 .NET SDK 8.0.408 通过官方 SHA512 校验。
安装器出现下载失败及包摘要不匹配，验收没有绕过校验。
工具准备等待 3500 秒后超时。
安装器处于 Session 0，无可操作的取消窗口。
22:39 的最终核对中，三个安装器进程仍在运行。
Windows 环境准备未完成。
本轮没有强制终止第三方安装进程。

Mac mini 没有已安装的 iOS Simulator Runtime，剩余空间约 9 GB。
本轮没有下载 Runtime，也没有接受本机未接受的 Xcode 许可。
iOS 构建通过不能代替模拟器运行验收。

## 构建资源与网络调优

OCI 的 Debian 构建层使用原生快照后端。
构建在 Go 编译阶段超过 8 GiB emptyDir 限额，被 Kubernetes 驱逐。
重试保持源码及 Go 1.27.3 不变，使用 Alpine 构建基础镜像。
Alpine 首次执行在 500m CPU、768 MiB 内存下进入 SQLite 编译。
内存限额计数超过 980 万次，CPU 持续限流。
本轮主动取消该执行，临时改为 1 CPU、2 GiB 内存重试。
调整资源后，Go 编译和镜像导出完成。
首次推送被本轮临时 HTTPS 代理拒绝。
验收网络包装镜像补充内建仓库地址的 NO_PROXY。
补充 NO_PROXY 后，构建、推送、读回和容器 HTTP 验收通过。
通过条件为临时 1 CPU、2 GiB 内存与 8 GiB emptyDir。
默认构建池容量仍需按真实应用规划。
本轮镜像目标仅指定 linux/amd64，未验证该应用的 OCI arm64 构建。

## 本轮发现的问题

规划 Job 的源码 init 因网络失败退出后，AgentRun 仍显示 `Running`。
该任务占用唯一规划槽位，后续任务持续排队。
复现任务为 iOS 构建 `39968507-d0da-4206-9e32-dab8cf0bf06f`。
对应 AgentRun 为 `2bef8ff6-ee8f-4f7a-a037-fe0e40c14901`。
源码 init 在 `13:25:58Z` 以 exit 1 退出。
AgentRun 仍显示 `Running / PodInitializing`。
通过平台取消构建后，任务资源回收，队列继续执行。
验收时保留现场证据；该故障传播缺陷当时尚未修复。
修复必须把逐容器终态传回 Runtime，并保持 Pod UID 与 generation 的身份校验。
后续代码修复和回归范围见[平台问题与修复](tasknotes-platform-fixes-2026-11-18.md)。

iOS 最新成功构建的目标为 `ios`，产物为正确的 Simulator 包。
其 `config_snapshot.targets.ios` 和 job 配置均缺少 `application_platform`。
本轮没有把原生产物成功视为平台分类字段正确。

## 网络与工具准备

集群无法稳定直连 GitHub、ECR 和 crates.io。
本轮使用临时、明确白名单的 TLS 字节转发。
转发保持 canonical Git URL 和端到端 TLS，不缓存源码。
源码准备、BuildKit 和 Worker 的网络包装镜像只增加环境变量。
Linux 工具镜像不包含业务源码、凭据或宿主项目目录。
部分工具镜像通过内网导入指定节点，限定本轮使用。

Gradle 8.11.1 在执行前核对项目指定的 SHA256：
`f397b287023acdba1e9f6fc5ea72d22dd63669d59ed4a289a29b1a76eee151c6`。
每个任务从预装工具复制到独立的 GRADLE_USER_HOME。
没有共享可变项目缓存，也没有修改 Gradle wrapper 或依赖锁。
Go 依赖代理通过任务环境变量指定。

## 清理

本轮全部构建已进入终态，项目与制品保留供复查。
AgentPool 和 BuilderPool 的临时字段均已恢复。
BuilderPool 恢复为 500m CPU、768 MiB 内存与 3 GiB 临时存储。
本轮通过结果不能代表该默认容量足以重建 TaskNotes 镜像。
临时原生节点已撤销并设为 draining。
Linux Pod、网络 Pod、出口 Service 和专属 NetworkPolicy 均已删除。
本地出口、观察器和测试端口均已退出。
指定节点上本轮导入的五个工具镜像标签已删除。
Android 连接设置已清除，本轮启动的模拟器已关闭。
正式 Gateway 保持两个就绪副本。
Windows 安装器未被强制终止，环境准备仍未通过。

## 证据与复验

私有证据位于 `.deploy/k8s-image-verification/tasknotes-20261118/`。
`state.json` 关联各项目、构建与重试。
`*-detail.json` 与 `*-logs.json` 保存原生任务回执。
`image-platform-logs.json` 保存完整分页的镜像专用日志回执。
`image-verification.json` 保存 OCI 图校验与读取身份撤销。
`image-runtime-verification.json` 保存仿真容器、HTTP 与本地镜像清理回执。
`cleanup-verification.json` 保存构建终态、Pool 恢复和临时 Kubernetes 资源清理。
`runtime-verification.json` 只记录已通过的服务端与 Android 运行断言。
`linux-runtime-verification.json` 保存原生 GTK 的 X11 窗口证据。
`windows-environment-verification.json` 保存安装器仍运行及 VS 实例不完整的终态边界。
`planning-init-failure.json` 保存规划 init 失败的 Kubernetes 现场。
私有目录不进入 Git，公开文档不包含凭据或签名下载链接。
