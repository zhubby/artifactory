<!--
[INPUT]: 固定提交、镜像回执、数据库备份和线上终端证据。
[OUTPUT]: 记录设备终端部署、审查修复、清理与验收边界。
[POS]: 设备终端 Kubernetes 交付记录；承接本地验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 设备终端 Kubernetes 上线记录

日期：2026-11-25，UTC。统一版本保留 `0.2.0`。
入口：`http://10.169.1.239:30820`。
范围仅为从控制台打开被控设备终端。
Control 不执行本地、容器或宿主 Shell。

## 审查与修复

本地阶段和完整审查范围见[本地验收](../acceptance/2026-11-25-device-terminals.md)。
本轮合并前复审修复以下问题，并运行对应回归：

- 授权五秒期限从查询开始计时，防止旧 MVCC 快照晚返回延长授权。
- 原渲染器重连偏移推进消费水位，恢复满 1 MiB 窗口的最后 ACK 丢失。
- 浏览器入口到 owner 的临时连接、握手和中断使用恢复关闭码。
- 转发停止后的关闭记录写入失败按有界退避重试，期间不转发或续租。
- 原宽泛入站策略排除 Control，防止允许规则并集暴露私有 8446。

线上验收又发现 xterm 清屏遗漏无障碍播报缓存。
补充断言先观察到失败，再通过公开选项重建无障碍视图。
屏幕、回滚缓冲和播报区在同一清屏路径重置。
正常 Shell 退出仍保留输出与真实退出码。

## 固定交付

实现提交：`4d86b7d03b8dbf3fdda93ce0726414493f8eb2c9`。
Web 清屏修复：`b468f13d9911ebdee2df14891e4a239e92265fed`。
两项提交均已快进合并本地 main，未推送远端。
本地基线 `6ca171b` 的六项未发布提交保留。
原 main 工作区两份未提交规划文件逐字节保留。

| 组件 | 提交 | 镜像摘要 |
| --- | --- | --- |
| Control / Telemetry | `4d86b7d` | `sha256:e74b80781b61f80b13a0c6e1c37fc898834dde5fa5e71fbeddea69de3bc329dd` |
| Gateway | `4d86b7d` | `sha256:28d7800b1f0d340ebfff4d5796aa53e5ab7a167b47b6bdf4bdfaf0067ea82074` |
| 最终 Web | `b468f13` | `sha256:3eb01739a38a807fc892660dbd3ed337518c2afff58c00da928a6d82105ffa9a` |

仓库前缀为 `registry.kubeease.cn/pre-release/artifactory-`。
标签为 `git-<提交前 16 位>`，部署同时固定摘要。
初轮 Web 摘要为 `sha256:822666891ed8377b640608098f1efee76094130a6a4e4499808005fe9b3b5ade`。

实现源码快照 SHA-256：
`b1a5c0b8fdf6f6b97ea3ed1c3745f37926b81bfbee56399839b1834183561971`。
Web 补丁源码快照 SHA-256：
`01a52315876ae93836296297bea68ba941a4496297058cb8bccfa626509bae76`。

镜像使用本机 OrbStack 的 `linux/amd64` 仿真构建。
依赖沿用冻结锁文件和固定基础镜像摘要。
Docker 归档转换为 OCI 后，逐项核对配置、层和来源标签。
发布保留摘要，标签与纯摘要均匿名读回一致。
本记录在镜像生成后编写，不包含在上述镜像内。

## Kubernetes 与数据

Kubernetes：`v1.28.3`，六个 AMD64 节点。
命名空间：`artifactory`。
命名空间 UID：`5397584f-9b1b-4320-8d0a-7904f9687547`。
数据库保留 `artifactory_release_20261121`。
兼容追加迁移 `2026112403`，此前七项迁移保留。

先备份并预演，再滚动更新三个 Control 副本。
确认迁移与运行摘要后更新 Telemetry、Gateway 和 Web。
最后一次升级仅改变 Web 镜像。
九项 Deployment 最终均就绪。
七个更新 Pod 的摘要与回执一致，重启均为零。

| 一致数据库备份 | 字节 | SHA-256 |
| --- | --- | --- |
| 终端迁移前 | 1,011,187 | `edfec19728da41f59253984c1286503242488a1356d25ec200d074638b1c2adb` |
| Web 补丁前 | 1,022,122 | `e8831ff06c8bee35ff032704f19fd4677d406b7ed5b2eabaf5e254f545003304` |

两份备份均通过目录读取和完整解析。
未跨迁移边界回滚旧程序。
恢复须评估部署后的新增数据，并匹配迁移身份。

### Helm 异常与恢复

revision 29 因单项 SSA 所有权冲突失败。
冲突字段是 `ARTIFACTORY_TERMINAL_RELAY_HOST.valueFrom.fieldRef`。
分阶段 JSON Patch 持有默认 `apiVersion: v1` 字段。
Helm 同源清单省略该默认值，造成所有权冲突。
核对运行摘要、数据及同源服务端预演后，以 `--force-conflicts` 重试。
revision 30 为 `deployed`，数据库未回滚。
Web 补丁升级后的最终 revision 31 为 `deployed`。

初轮 Helm 默认历史上限清理了 revisions 2–20 的发布元数据 Secret。
这些历史元数据未恢复。
业务凭据 Secret 的 UID 与内容摘要均未改变。
后续升级显式使用 `--history-max 0`。
最终保留 revisions 21–31，Web 补丁未再清理历史。

原服务、PVC、权限、配置和无关 Deployment 的身份及内容均已核对。
Control Service 的 8446 仅为 ClusterIP，公网 Gateway 无此端口。
从真实 Control Pod 可连接三个 Control 的 8446。
从真实 Web Pod 连接三个 8446 均被阻断。
数据库中三个终端实例租约均有效。

## 验证

| 层次 | 结果与边界 |
| --- | --- |
| Control 终端数据库回归 | 21 项通过，真实 PostgreSQL 与 TLS；部分字节端为模拟设备 |
| 静态检查 | Control 全目标 Clippy、终端 API 单元、格式、diff、模块边界与版本检查通过 |
| 部署回归 | Helm 28 项、部署脚本 7 项通过 |
| 真实 Emissary 回归 | 4 项通过，真进程、mTLS 和 PTY |
| Web 补丁回归 | 清屏断言先失败，修复后三视口 39 项通过，类型与生产构建通过 |
| 线上真实终端 | Chromium → Gateway → Control → 前台 Emissary → macOS PTY 通过 |
| 多副本实际连接 | 在真实 Pod 的 TCP 连接表观察到跨 Pod TLS 内部流 |
| 网络隔离 | 三个 Control 的 8446 入站允许与拒绝探测通过 |
| 数据与资源 | 临时租户删除后，原业务哈希、计数、节点 ID 和资源身份一致 |

线上测试只注册临时租户中的两个一次性节点。
Emissary 使用本机 macOS arm64 开发账户前台运行。
没有安装系统服务或升级既有设备。

真实终端执行 `test -t 0 && test -t 1` 得到 `K8S_PTY_OK`。
`stty size` 确认窗口由 18×125 调整为 18×109。
断开浏览器 WebSocket 后，原页面重新附着。
重连前后 Shell PID 均为 29293。
Ctrl+C 中断 `sleep 120`，随后命令继续执行。
中文 composition 输入正确到达 PTY 并显示。
Vim 全屏打开，Escape 与 `:q!` 返回 Shell。
`exit 7` 正确展示，持久会话和审计也保存退出码 7。
桌面和 320px 页面完成交互与截图检查，无页面横向溢出。

初轮撤销临时节点权限后，浏览器在 1.194 秒内关闭。
Shell 31303 与子进程 31373 均消失，终端注册文件为零。
该轮发现播报区残留，不能视作完整清屏通过。
Web 补丁后再次撤权，屏幕及播报区文本均为空。
Shell 40404 与子进程 40975 均消失，注册文件为零。
三次终端会话的审计仅保存状态、账户与字节计数，不保存终端内容。

实际连接包含 `10.233.66.255 → 10.233.124.193:8446`，
以及后续会话的 `10.233.77.170 → 10.233.124.193:8446`。
这些证据证明跨 Pod 转发，不代表单会话三种入口均强制分布。
T3 preview 两次快照均超时后，切换 agent-browser 完成操作。
独立浏览器未发现页面异常，最终已注销并关闭。

## 清理与验收边界

临时租户通过正式生命周期接口删除。
保留既有 50 秒安全窗口，操作最终为 `succeeded`。
临时租户、两个节点、会话、票据和注册令牌均已清除。
删除墓碑与空间删除审计按业务设计保留。
两个前台 Emissary、PTY 与子进程均已退出。
独立 API 会话已注销，临时注册身份文件已删除。

| 既有业务 | 验收前与清理后数量 |
| --- | --- |
| 节点 | 3 |
| Release | 8 |
| Build | 25 |
| Job | 14 |
| Attempt | 44 |
| 交付记录 | 2 |

公开页面使用 HTTP NodePort，浏览器传输为 WS。
设备连接和私有内部流使用 TLS。
本轮未验证公开 HTTPS / WSS 入口。
设备终端仍默认关闭，只允许租户 owner/admin 的原登录 Session。

Windows x64 原生 ConPTY、Session 0、嵌套 Job、专用服务账户和原生 IME 仍待验收。
完整 Windows Emissary 交叉构建仍受本机 Windows SDK 缺少 `assert.h` 阻塞。
Linux 容器 PTY 与 macOS 开发账户结果不能替代 systemd / launchd 专用账户验收。
未在既有物理远程设备安装或启用终端。
其他 Kubernetes 版本、终端大输出与真实构建并行、生产容量仍待环境验收。

私有回执、备份和截图位于 `.deploy/device-terminals-20261125/`。
Web 补丁回执位于 `.deploy/device-terminals-a11y-20261125/`。
