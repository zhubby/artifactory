<!--
[INPUT]: 固定 main、CI 镜像、数据库备份与真实页面回执。
[OUTPUT]: 记录通知中心上线、迁移、持久身份与验收边界。
[POS]: 账户通知基础设施的正式 Kubernetes 部署记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 通知中心 Kubernetes 上线记录

日期：2026-11-23，UTC。
通知中心已合并 main、推送并部署。
入口为 [通知中心](http://10.169.1.239:30820/notifications)。

## 实现范围

账户收件箱跨工作区聚合通知。
工作区顶部提供铃铛和通知抽屉。
完整页面提供状态、类别、工作区和项目筛选。
读取、投影和外发均复核当前权限。

业务变更与通知事件在同一事务提交。
构建阶段按事项聚合，并保留用户已读版本。
权限、设备、凭据和发布使用统一事件目录。
Control 内执行投影、异常扫描、重试和清理。
邮件与 Webhook 使用独立持久投递队列。
渠道合同见[通知中心](../notifications.md)。

## 固定源码与镜像

| 身份 | 固定值 |
| --- | --- |
| 实现提交 | `70358cdade52502cfe6e28b0f4af0bb52e486c5e` |
| 版本 | `0.2.0` |
| 源码归档 SHA-256 | `b027295f9d0efcff69583e215a16c95a744f15a38a1c53749b0c29b36c8c8bb1` |
| 镜像标签 | `git-70358cdade52502c` |
| Control 与 Telemetry 摘要 | `sha256:aaeaf44a72d0bcddebf3e2e43f40261831620b049e5a1d4c551a1e3b77879828` |
| Web 摘要 | `sha256:da69fc167408ebce6933aa361d44549e3ff87c51ef2e9ab6aa104bedb378a443` |

镜像来自 [GitHub Actions 45121622105](https://github.com/zhubby/artifactory/actions/runs/45121622105)。
Control 与 Web 任务均成功，平台为 linux/amd64。
源码、版本与仓库 OCI 标签均已核对。
Telemetry 复用同一个 Control 镜像。

同次工作流的 Planning Worker 在既有镜像探测步骤失败。
其余九项镜像任务成功。
本次只采用 Control 与 Web 的镜像产物。
其他执行组件沿用现场固定镜像。

源仓库为 `ghcr.io/zhubby/artifactory-<component>`。
部署仓库为 `registry.kubeease.cn/pre-release/artifactory-<component>`。
公网复制出现连接超时，镜像代理的冷内容读取也超时。
最终使用临时 Pod 串行上传原始镜像内容。
每个配置和镜像层均验证长度与 SHA-256。
上传保留原 manifest，标签与纯摘要均匿名读回一致。
仓库连接保留 TLS 证书与主机名校验。
临时 Pod 未挂载凭据，传输凭据仅驻留进程内存。
临时传输 Pod 与拉取探针均已删除。

## 数据库与身份保留

业务库沿用 `artifactory_release_20261121`。
迁移前保存一致 `pg_dump` 备份，并完整解析验证。
原迁移 `2026112101`、`2026112201` 保留。
本次追加 `2026112301`、`2026112302`、`2026112303`。

追加迁移创建十张通知表、九个函数和十八个触发器。
已有业务表的行数和持久配置摘要一致。
现有一账户、两租户、三项目和三节点均保留。
六个 Release、22 个 Build、13 个 Job、43 个 Attempt 和两条交付记录均保留。
节点 ID、凭据、密钥、CA 和 PVC 身份均保留。
遥测仍使用已有最小只读授权。

先单独滚动更新 Control，等待三个副本就绪。
确认完整迁移身份和运行摘要后，再执行 Helm 升级。
Telemetry 与 Web 使用该次 Helm 升级更新。
升级未启用旧镜像自动回滚。
旧程序启动时会拒绝新增迁移身份。
失败恢复优先修复前进。
回退旧程序须匹配迁移前备份，并评估新增业务数据。
备份不含 ACL，恢复后须重新应用遥测最小授权。

## Kubernetes 结果

命名空间与 Helm Release 均为 `artifactory`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
Helm revision 从 18 升至 19，状态为 deployed。

模板预览与服务端预演均通过。
清单只改变 Control、Telemetry 和 Web 的镜像引用，
并向 Control 加入空的通知私网出口白名单。
SMTP Secret 和私网白名单均为空，外发默认关闭。
最终 values 与 manifest 均与预览一致。

九项 Deployment 全部就绪。
三个 Control、一个 Telemetry 和一个 Web Pod 的运行摘要正确。
新 Pod 重启次数均为零。
其余六项 Deployment 的 spec 与 UID 保持一致。
Service、配置、网络策略、权限、池和 PVC 均通过身份比对。
全部应用 Secret 的 UID 和内容摘要一致。
Helm 历史仅新增 revision 19，原历史完整保留。

## 验证结果与边界

| 现场验证 | 结果 |
| --- | --- |
| 健康、登录、通知页面与静态文档 | HTTP 200；文档经过正常重定向 |
| 数据库迁移与通知对象 | 五项迁移、十表、九函数、十八触发器，符合预期 |
| 业务数据与持久身份 | 计数、配置摘要、节点 ID、Secret 和 PVC 均保留 |
| 通知投影 | 两个当前设备告警已入箱，待投影与重试均为零 |
| 通知完整性 | 孤立事件引用、无效版本均为零 |
| 邮箱、启用 Webhook 和外部投递 | 均为零 |
| 桌面 1440、平板 820、手机 390px | 中文亮色与英文暗色，共六组通过 |
| 铃铛、抽屉、完整页面与关闭焦点 | 通过，保存十八张截图 |
| 状态、类别与两个工作区筛选 | 真实 API 响应通过，通知请求使用账户范围 |
| 通知设置与已读版本 | 默认外发关闭；读取前后偏好和已读版本一致 |
| 页面横向溢出与浏览器错误 | 均为零 |
| 验收会话 | 六个会话均已精确注销 |
| 新服务日志 | 检查窗口内无错误、警告或通知任务失败 |

Web 启动期出现一次 readiness 连接拒绝。
随后就绪，未发生 liveness 故障或容器重启。

现场通知数据为两个工作区的设备告警。
项目筛选覆盖空选项状态，未现场生成项目通知。
构建阶段、权限事件与渠道重试由合并前的隔离回归覆盖。
本轮真实页面验收只读业务数据，未提交构建或修改权限。
真实邮件和 Webhook 发送尚未现场验证。

私有备份、回执和截图位于 `.deploy/notification-center-20261123/`。
本记录在镜像生成后编写，不包含在上述镜像内。

[PROTOCOL]: Update this header on change, then check AGENTS.md
