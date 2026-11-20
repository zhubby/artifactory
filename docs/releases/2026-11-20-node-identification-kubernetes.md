<!--
[INPUT]: 固定源码、镜像、集群与双设备回执。
[OUTPUT]: 记录设备识别部署、页面与原生验收边界。
[POS]: 节点名称与识别信息的正式交付记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 设备识别 Kubernetes 上线记录

日期：2026-11-20，Asia/Shanghai。
平台、双设备与真实页面均已通过验收。

## 实现范围

注册默认使用系统主机名，已保存名称保持稳定。
owner/admin 通过独立接口修改显示名称。
名称修改只写 `nodes.name`，审计与更新同事务提交。
心跳、同启动重连和新启动不覆盖名称。
列表直接展示可见的主机名、硬件与短 ID。
详情按接口展示 IPv4、IPv6 与采样时间。
搜索支持名称、主机名、型号、CPU 和 ID。
[设计规范](../design-system.md)约束识别信息与交互。

## 固定源码与镜像

| 身份 | 固定值 |
| --- | --- |
| main 提交 | `e0d56c1d90aa55aab7e6dbd2b3fc01405f1e68e3`，已推送 |
| 版本与源码 | `0.2.0`；纯净固定归档 |
| 归档 SHA-256 | `cb1c0a4512255a0f6a7cd8f82d948d33740daa2ba22bd6dfae6ab3e9633c4449` |
| 镜像标签 | `git-e0d56c1d90aa55aa` |
| Control 运行摘要 | `sha256:be8f225ae40b22b58e8c60400b2851339ae8fbe1eac30dc5309c793d3a6ab521` |
| Web 运行摘要 | `sha256:5b24a586f080e1012041416546461c7e90f3d6d3bc76d7287e25425a83e221bd` |

镜像仓库前缀为 `registry.kubeease.cn/pre-release/`。
镜像名称分别为 `artifactory-control-plane`、`artifactory-web`。
两镜像与固定源码、版本及归档一致。
Web 来自 [GitHub Actions 44719920105](https://github.com/zhubby/artifactory/actions/runs/44719920105)。
镜像保摘要发布，Harbor 匿名读回通过。
Control 与 Telemetry 两程序均通过 x86_64 ELF 核验。
Control 镜像用户为 `10001:10001`。
其默认入口为 `artifactory-control-plane`。

## Kubernetes 结果

命名空间与 Helm Release 均为 `artifactory`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
公开[节点页面](http://10.169.1.239:30820/tenants/ai-workspace/nodes)使用既有入口。
Helm revision 从 9 升至 10，状态为 deployed。
核验时间为 `2026-11-20T10:37:22.311248+00:00`。

预览允许四个既有容器镜像路径变化。
当前安装没有 signing-keyring init。
实际只更新 Control、Telemetry、Web 主容器镜像。
Helm 仅更新两个镜像值，未使用 post-renderer。
Control 与 Telemetry 共用同一固定镜像。
最终清单与预览一致，九项 Deployment 全部就绪。
三个组件的运行摘要正确，五个 Pod 重启次数为零。
所有其他 spec、资源身份及现有 Secret/PVC UID 保留。
`/healthz`、`/login`、`/docs/` 均返回 HTTP 200。

## Mac 原生设备

租户为 `ai-workspace`，沿用既有节点身份。
先排空并确认空闲，再替换标准服务程序。
更新后恢复原有 draining 设置。

| 项目 | 验收结果 |
| --- | --- |
| 节点 UUID | `c30da6b0-52f4-403e-a03b-b7909b1635bc` |
| 名称与 hostname | `codehubdeMac-mini.local` |
| 系统与硬件 | Darwin arm64；`Mac16,10`；Apple M4；16 GiB |
| 程序 SHA-256 | `476771d37ec739cc241c1a85c7a904f17bffdcb310576ebf3f6a33e6eb3c110c` |
| 程序身份 | `0.2.0`、protocol 1、完整固定提交 |
| 保留边界 | UUID、配置、证书、节点池、服务注册、ACL 不变 |
| 停机数据清单 | 14 项及其内容、权限保持一致 |
| 网络对照 | 12 接口、19 地址与 OS 完全一致；未截断 |
| 平台观测 | online；遥测 fresh；dropped_batches 为 0 |
| 搜索 | hostname、短 ID、CPU 均命中原节点 |

Chrome 使用“使用当前主机名”并保存成功。
该操作沿用原节点，没有重新注册。

## Windows 原生设备

同一固定源码的 release 构建通过，worker 已退出。
程序为 `0.2.0 windows/amd64`，包含完整固定提交。
程序 SHA-256 为 `2066b8438218f6f9a484e5874b49dc46feb2af0d4684be028c31d1196e87812e`。
Windows 网络 5 项与真实 Collector 1 项通过。
两条测试过滤器均实际运行，exit 均为 0。
真实 API 的 5 接口、10 地址与 OS 完全一致，未截断。
现场没有已分配地址的真实无 MAC VPN 样本。
其 IPv4/IPv6 保留与地址移除由 Windows ABI 夹具覆盖。
节点 UUID 为 `05c69825-ffc7-46f4-88fe-2cdada414b38`。
名称与 hostname 均为 `LAPTOP-VFIEIL8E`。
硬件为 LENOVO `20U2A6XYCD`、Intel Core i5-10210U。
空闲后替换标准服务程序，已恢复原 draining 设置。
SCM 本机 `emissarybuild` 账户、Auto 与 ACL 保留。
新程序 PID 为 `10444`，安装摘要与构建回执一致。
UUID、配置、证书、节点池、服务注册与停机数据清单保留。
在线、遥测 fresh、0 drop，hostname、短 ID、CPU 搜索通过。
双平台网络在 IPv6 作用域归一后与 OS 完全一致。
私有更新器补充短命令载荷与 PowerShell 5 空值绑定兼容。
产品源码与镜像保持同一固定提交。

## 真实页面与回归

Chrome 390px 手机浅色、深色和 1440px 桌面深色通过。
页面与弹层无横向溢出。
手机下拉宽度为 324px，桌面为 618px。
弹层与输入框边缘误差为 0。
改名草稿经过轮询后保持，保存显示顶部 notify。
短 ID `c30da6b0` 在现场筛选中只命中原节点。
手机网络详情的 IPv4/IPv6 可读，宽度 389px 无溢出。
首次读取可见内容骨架与顶部 progressbar。
“正在读取”只用于 sr-only 可访问状态。
验收后已恢复“跟随系统”主题与原视口。
手机结果为浏览器视口验证。
现场使用 admin；只读权限由夹具验证。
最终 1728px 桌面与 390px 手机均显示两台新名称在线。
两种最终视口均无横向溢出，双设备网络详情可读。
浏览器 consoleErrors 为空，最终截图已保存。

| 必要验证 | 结果 |
| --- | --- |
| 节点数据库 | 15 项通过；最后新增范围 5 项通过 |
| 协议与 gRPC | 协议 8 项、gRPC 3 项通过 |
| Emissary | lib 42 项、CLI 3 项通过；2 项既有 Android 测试忽略 |
| Control lib | 41 项通过，21 项环境条件忽略 |
| Web | 专项与回归 33、45、22 项；最后 Unicode 12 项通过 |
| 构建与文档 | typecheck、生产构建、格式与文档检查通过 |

独立审查通过，最终两项 P2 均已修复并复审。
修复涵盖 Windows 无 MAC VPN 与完整 96 KiB 信封。
metadata 64 KiB、网络 8 KiB 和严格遥测结构保留。
Windows 真实 API/Collector 已通过上述原生验证。
环境扩展测试失败与条件忽略不计为通过。

## 实施裁定与收尾边界

- 互不重叠实现并行；文件归属与共享契约固定。
- 必要的隔离工作树属于实施授权的可逆操作。
- 共享 Control 镜像使用标准 Helm 值并正常滚动 Telemetry。

间隔约 101 秒的后续心跳确认两台名称稳定。
遥测接收时间均推进，丢失批次数均为 0。
双设备本轮构建目录、staged 与回滚程序已清理。
两份清理回执均为 cleaned，rollback_retained 为 false。
清理后配置、身份、服务注册与当前程序摘要保持不变。
另删除 Mac 三枚、Windows 两枚旧验收程序。
固定运维目录扫描中，两台均只剩一枚当前程序。
临时宿主凭据已删除，清理后平台复核通过。
清理保留 SDK、缓存、验收数据和签名材料。
必要审查与脱敏回执保存在 `.deploy/node-identification-20261120/`。
骨架、改名、手机深色与桌面深色截图已保存。
本记录晚于镜像生成，不包含在上述固定镜像内。

[PROTOCOL]: Update this header on change, then check AGENTS.md
