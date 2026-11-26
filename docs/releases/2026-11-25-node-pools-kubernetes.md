<!--
[INPUT]: 独立节点池提交、不可变镜像和线上验收回执。
[OUTPUT]: 记录节点池菜单、池写入和设备筛选的部署证据。
[POS]: 本轮 Kubernetes 上线记录；操作契约见 workspaces.md。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 节点池 Kubernetes 上线记录 · 2026-11-25

## 源码与镜像

本轮部署租户侧栏的节点池入口和节点池页面。
租户 owner/admin 可以创建、重命名和删除空池。
默认池、设备、项目引用和升级历史受到删除保护。
设备计数按当前授权聚合，设备列表按池 ID 精确筛选。

部署源为独立分支 `codex/node-pools-deploy`。
基线为 `f0a1afce5b3eb79a848b3a67c2e181cb01cb3c24`。
功能提交为 `5770541cc8c54176561f59d78eec41648b3fda47`。
该提交保存在私有部署快照内，尚未合并至共享工作区。
设备统一版本和 Windows 清理的未提交改动未进入镜像。
源码快照摘要为 `f6be1be91cc58b51270b3e9891906fc86d667bfb3efc6592f2d35a9f03cef79e`。

两个镜像均为 Linux AMD64，使用不可变来源标签。
标签为 `git-5770541cc8c54176`。
镜像仓库为 `registry.kubeease.cn/pre-release`。

| 镜像 | SHA-256 |
| --- | --- |
| Control / Telemetry | `bd98b8e6386066f47c22f9f17f5ba60e5b5013c4c6cbd87fcb11525a0b8aae30` |
| Web | `b3f660edff8dfd8cff2bb58dce56c761a81d669eb519e80517bb619a10ba42ed` |

构建前后核对全部冻结文件摘要。
OCI 导出核对平台、manifest、config 和所有层的摘要。
发布保留 OCI 摘要，匿名按标签与摘要读回成功。
Web 生产构建和 TypeScript 检查通过。

## 集群与持久数据

入口为 <http://10.169.1.239:30820>。
集群为 Kubernetes 1.28.3，命名空间为 `artifactory`。
Helm 从 revision 37 升级至 revision 38。
revision 38 状态为 `deployed`。

升级前备份业务、遥测和规划三个数据库。
备份通过恢复目录和完整解码校验。
服务端预演确认仅三个 Deployment 的镜像字段改变。
Telemetry 与 Control 共用镜像，随同滚动更新。
镜像补丁使用资源版本和旧镜像作为前置条件。
补丁后等待就绪，再使 Helm 清单收敛。

三个 Control、一个 Telemetry 和一个 Web 副本就绪。
运行摘要逐一匹配发布摘要，重启次数均为零。
九个 Deployment 全部就绪。
既有 Helm 历史、Secret、服务和持久卷身份保留。
非目标运行 Pod 的身份和镜像保持一致。
原有业务行、设备 ID 和业务计数通过前后校验。
本轮没有数据库迁移，迁移末项仍为 `2026112502`。

## 真实接口与页面

使用 `ai-workspace` 租户的 owner 会话验收。
接口验收创建空池、重命名并删除该池。
默认池删除返回 HTTP 409。
删除同时撤销注册令牌，三项审计记录存在。
所有现有池的授权设备计数与精确筛选结果一致。

浏览器从租户侧栏打开节点池页面。
页面创建、重命名、删除和顶部通知通过。
空池的“查看设备”保留 URL 中的池 ID。
设备页的自定义筛选器选择该池，列表显示零项。
默认池显示三台设备、三台在线，删除按钮禁用。
320px 视口的文档宽度为 320px，没有横向溢出。
手机抽屉包含节点池入口。

两次验收创建的临时池均已删除。
浏览器与接口的验收会话均已退出。
完整备份、回执和截图保存在私有部署目录。
目录为 `.deploy/node-pools-20261125/`。
本轮没有升级 Emissary 或改变现有设备、项目授权。
