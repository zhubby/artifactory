# 全站 UX/UI 重构 Kubernetes 交付记录

后续更新：23:10 CST 已发布网页巡检修复至 revision 8，详情见[真实浏览器巡检与修复](../acceptance/ui/2026-11-15-browser-audit.md)。下文保留首次重构发布 revision 7 的历史记录。

日期：2026-11-15，Asia/Shanghai。新 Web 已于 20:51 CST 部署至现有 Kubernetes `1.28.3` 的 `artifactory` 命名空间，Helm release `artifactory` revision **7**、状态 **deployed**。访问入口为 `http://127.0.0.1:38100`，原有端口转发继续可用。

## 实现与验证

本地完整重构及 code review 的两个 P2 修复均已实现。个人设置、设备侧面板及其他模态窗口不会被 Cmd/Ctrl K 抢走焦点；从其他标签返回执行日志时，自动跟随追上最新输出，暂停阅读时保留位置。修复前已分别复现，永久行为回归覆盖三个视口。

TypeScript、生产构建通过；完整 UI 275 项通过、1 项按条件跳过；文档 12 项通过。实际部署的 amd64 standalone 镜像另通过 6 项修复行为回归及 12 项文档测试。Helm lint 和现有 Helm 初始化脚本 7 项测试通过。原先隔离真实后端 14 项浏览器检查及原生链路 35 项证据见[UI 验收记录](../acceptance/ui/2026-11-15.md)；它们不构成本次目标集群已完成的构建或 AI 供应商验收。

源码快照包含全部未提交、新增的运行代码与文档，共 189 个文件；不包括凭据、`.local`、Git 数据、node_modules 或生成的 Next/mdBook 产物。源码 manifest SHA256 为 `faa4dd73f2b9e3d2f5f9d800b49167f6891ae297fd25f69f3c6a02ed704a0187`。基准 Git HEAD 为 `34be99b58ee73b4f149a6cc9f823d2d94d8f2b9f`，镜像以 `-dirty` 明确区分，不声称该 HEAD 包含重构。

部署固定使用以下镜像：

```text
ghcr.nju.edu.cn/zhubby/artifactory-web:ux-20261115-faa4dd73f2b9@sha256:5c6896ac3b9c75bc6c29fff84d2e9ac57da6e080328b6e35920b1ee5ba5875ed
```

上游为 `ghcr.io/zhubby/artifactory-web`，amd64 config 摘要为 `sha256:e17af46aa0be15ff72fdcdaa2f92f2eac4d2f3babd2a6dccc743cb8db63e7b08`。先前 CCR / Quay 发布的 manifest 摘要为 `3137fe5b1987c292a4b2adec3fcf0fd802eda313afd539e87509e892138add0e`；GHCR 发布后 manifest 摘要不同，但 config、平台及 OCI 源码身份一致。目标集群使用上面的 `5c6896…` 摘要。

## 发布前现场检查

升级前 Helm revision 为 6、状态 deployed。三个 Control 副本、其余六个 Deployment 和 Gateway StatefulSet 均就绪；10 个业务 Pod 就绪且容器零重启。认证入口可用，原有一个租户、三台设备、一个 Worker，项目为零。保存了 45 个目标命名空间资源及 158 个选定非目标资源的 UID / 规格摘要；原有凭据内容仅保存私有摘要及受限备份。

此次只更新 Web，不迁移数据库、不更换后端镜像、不变更 PVC、凭据或配额。渲染阶段已验证其他 Helm 资源保持一致。原 Web 镜像摘要 `sha256:e0821d56879f03f8d2e4db90e6c47e5017f0e4f63c7aea56d0df615d5a808f9b` 及完整值已保存用于回滚。

现有 CPU quota 无法容纳两个 Web Pod。替换期间临时使用 `maxSurge=0 / maxUnavailable=1`，完成后已恢复原 `25% / 25%` 滚动策略；quota 保持不变。镜像拉取验证完成后才切换旧 Web。单副本替换期间可能短暂不可用，本次未宣称零停机发布。

## 镜像网络排障与部署结果

腾讯云 CCR 在本机认证及 manifest 读取成功，但节点连接 `ccr.ccs.tencentyun.com` 的 `106.55.124.140:443` 超时；节点直连 GHCR、Quay 与 Docker Hub 也超时。南京大学 GHCR 镜像代理可达。Quay 仓库为私有，代理匿名拉取没有权限。

用户完成 GitHub `write:packages` 授权后，将已验证镜像发布至既有公共 GHCR 仓库，核对上游与代理的 manifest、config、平台及源码标签。五个真实节点 `234 / 235 / 236 / 238 / 239` 全部完成预拉取，并在镜像中验证 amd64、内置字体与文档、部署 API 转发配置。全部诊断及预拉取 Pod 随后删除，没有增加节点或集群全局镜像配置。

使用发布前保存的真实 Helm values 渲染并比较 22 个资源，只有 Web 镜像发生变化。20:51:02 CST 开始，20:51:08 CST Helm upgrade 完成，revision 从 6 更新至 7。

Web Pod `artifactory-web-586c597fc4-4fqb2` 在 `boao-test-node235` 上 2/2 Ready，运行 image ID 与固定摘要一致。10 个业务 Pod 全部就绪，容器零重启，三个 Control 副本保持就绪。后端 Pod UID 与发布前一致；既有业务资源除 Web 镜像外 UID / 规格摘要一致，只有 Helm release 记录按升级预期更新；158 项选定非目标资源身份和规格未变。没有更换后端镜像、迁移数据库或变更 PVC、凭据、配额及 Gateway。

## 目标集群真实页面验收

通过原入口及真实 API 完成以下检查，没有模拟 API，没有提交业务构建：

- 在一个临时租户的两个空项目之间切换，验证栏目保持、深链接刷新与 Cmd/Ctrl K 项目导航。
- 验证个人设置及设备侧面板打开时快捷键保留当前焦点，快捷面板自身可开关。
- 三步构建引导可返回编辑，主题与语言切换保留输入；只到确认阶段。
- 原租户三台真实设备的列表与详情；手机和平板导航抽屉的快捷键、Esc 和焦点恢复。
- 桌面 / 平板 / 手机 × 中文 / 英文 × 浅色 / 深色，在项目、构建列表、设置页面生成 36 张矩阵截图，无整页横向溢出。
- 匿名 mdBook 阅读、Mermaid 和深色主题。

另有构建引导、设备详情、文档 3 张截图，合计 **39 张**。36 张矩阵截图另行等待 API 与字体加载完成后重拍，页面错误 **0**，API 错误 **0**。重拍发生在临时空间等待删除期间，截图包含该空间的只读排空提示；原有租户未进入排空。截图使用 Chromium，手机为设备模拟，不代表实体 iPhone、Safari、Firefox 或屏幕阅读器人工验收。

原有 Windows 设备 `codehub-windows` 在升级前已经离线，两台 macOS 设备和一个 Worker 在线。保留原有设备与 Worker 身份、吊销状态及证书有效期，在线状态按发布前基线比较，不将历史离线设备表述为本次升级故障，也不声称所有设备心跳均新鲜。

## 稳定性与验收清理

20:59:12–21:08:25 CST 完成 **10 次按分钟采样，持续约 9 分 13 秒**。每次均为 10 个 Pod 就绪、容器零重启，工作负载副本数正确，认证 API 和匿名文档可用。原有两台 macOS 设备及一个 Worker 保持在线，心跳在观察期间推进，最终检查时均在 30 秒内；原有 Windows 设备保持升级前的离线状态。设备与 Worker 的身份、吊销状态、证书有效期及原有项目保持一致。

临时验收租户及两个项目已于 21:08 CST 通过生命周期 API 删除，操作状态 `succeeded`，租户与项目列表恢复发布前基线。删除曾等待已取消的 `source_refs` 查询任务按既有 15 分钟有效期自然到期，没有强制修改数据库或中断原有后端。全部临时 Pod 和未使用的拉取 Secret 均已移除，未遗留额外命名空间业务资源。

21:08:39 CST 最终核验确认 Helm revision 7 为 deployed、滚动策略恢复、原入口和匿名文档可用、158 项选定非目标资源未变。本次专用验收 Session 已注销，原 Cookie 请求返回 401，本地会话文件已删除；原端口转发保留。

## 回滚与证据

发布前 revision 6 的完整 values 与 Web 镜像已保存于私有交付目录。原镜像为：

```text
ghcr.nju.edu.cn/zhubby/artifactory-web:sha-dc1594baeddf33c45d0233d83cb81b84a380a003@sha256:e0821d56879f03f8d2e4db90e6c47e5017f0e4f63c7aea56d0df615d5a808f9b
```

如需回滚，同样先临时允许单副本替换，再使用保存的 revision / values 恢复 Web，核对运行摘要并恢复原滚动策略。没有 schema 变更，无需数据回滚。本次没有执行回滚。

证据保存在忽略的 `.local/k8s-monochrome-20261115/`：

| 记录 | 文件 |
| --- | --- |
| 源码、镜像及 GHCR 校验 | `source-identity.json`、`source-manifest.json`、`published-ghcr-image.json`、`published-image.json` |
| 节点预拉取与部署 | `prepull-verification.json`、`render-verification.json`、`upgrade-start.json`、`upgrade-result.json` |
| 资源及稳定性核验 | `runtime-verification.json`、`stability-verification.json`、`final-verification.json` |
| 真实浏览器验收 | `browser-verification.json`、`browser-verification.log`、`browser-matrix-stable.json`、`screenshots.md`、`live-*.png` |
| 临时空间清理 | `browser-cleanup-operation.json`、`browser-cleanup-verification.json` |
| 实际镜像测试 | `ui-image-report/`、`docs-image-results/` |

真实 AI 供应商尚未配置，计划生成与补充的成功路径仍以模拟 API 验证；本次目标集群没有提交生产构建，完整执行、SSE、取消、下载及发布来自前述隔离真实后端验收。两类证据独立记录。

验收和交付记录在镜像构建完成后追加，最新记录没有重新固化进已部署的文档镜像。原有本地入口与独立生产预览 `http://127.0.0.1:32161` 保留。用户后续明确授权了本次 Web 部署；没有进行 Git 提交、推送或合并。
