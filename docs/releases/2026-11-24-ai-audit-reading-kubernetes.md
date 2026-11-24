<!--
[INPUT]: 固定提交、镜像摘要、Helm 回执与线上审计页面。
[OUTPUT]: 记录 AI 审计阅读改版的部署与验收边界。
[POS]: 仅 Web 镜像更新的 Kubernetes 交付记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# AI 审计阅读 Kubernetes 上线记录

日期：2026-11-24，UTC。
验证入口：[历史构建 #9](http://10.169.1.239:30820/tenants/ai-workspace/projects/workflow-acceptance-20261122/releases/2708dcdf-6767-4666-824d-f147223315ea/builds/9)。

## 固定交付

实现提交：`2c5f67c5d6f669f72625562fe39e1c99351c1f57`，已提交本地 main。
从 Git 归档导出提交，不读取未提交的终端规划。
源码快照 SHA-256：`be92f87453dedf8daebc84647dffae69ba47b6923cbdf415cda3ba3aee251792`。
版本保持 `0.2.0`。

Web 镜像：

```text
registry.kubeease.cn/pre-release/artifactory-web:git-2c5f67c5d6f669f7@sha256:fb259be750c702021af2c74dd8940185dbf9ff0d6347d9ffa6b6901c98a11d37
```

构建使用锁定依赖、固定基础镜像和 webpack。
OrbStack 构建平台为 `linux/amd64`。
Docker 产物转换 OCI 后逐层校验摘要。
发布保留摘要，标签与纯摘要匿名读回一致。
镜像标签记录提交和源码快照。
本记录在镜像生成后编写，不包含在该镜像内。
本轮没有推送远端。

## Kubernetes 验证

命名空间为 `artifactory`，UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
主 Helm revision 从 27 升至 28，最终为 `deployed`。
预演与实际清单只有 Web Deployment 镜像变化。
九项 Deployment 均就绪。
新 Web Pod 运行摘要与发布摘要一致，重启次数为零。
Control、Telemetry、Operator 和规划池保留既有镜像。
Secret、PVC、服务、权限和配置身份均通过前后比对。
只新增 Helm 第 28 版 Secret。
业务库、原有业务计数和迁移记录均一致。
没有创建构建或调用模型，也没有配置代理。

部署前数据库备份为 `1011124` 字节。
备份 SHA-256：`9525d796589631418208d3aae8cb2bda4a7e05fd75c0d8b2b6980a900596a6c4`。
备份通过目录读取与完整解析。

## 分层验收

| 验证层 | 结果 |
| --- | --- |
| 实现回归 | 五个 spec、108 项通过，覆盖三个视口 |
| 真实记录本地回放 | 桌面与手机、明暗主题共 4 项通过 |
| 构建 | 本地与容器生产构建、TypeScript 检查通过 |
| 线上轮次 | 首次规划与两次失败修订分别可读 |
| 线上文件 | README 正文解码，目录显示 10 项文件 |
| 线上失败日志 | 按步骤呈现真实 `QA_GATE_CLOSED` |
| 线上提案 | 每轮只有一个提交动作，关联平台接纳回执 |
| 线上提示 | Worker 预算归为平台提示，不显示为用户消息 |
| 线上阅读状态 | 切换轮次和折叠重开保留原始记录标签、DOM 与 90px 滚动位置 |
| 线上手机 | 375px 页面无横向溢出，详情位于单列布局 |
| 原始失败构建 | 构建 #1 显示 0 次会话和“尚无规划对话” |
| 匿名边界 | 会话接口返回 HTTP 401 |

线上检查使用真实 API，没有替换响应。
线上截图为中文浅色；中英文明暗主题与无障碍覆盖来自生产模式回归。
手机为 Chromium 窄屏检查，不代表实体 Safari 验收。
本轮只验证已保存的历史记录，不代表新增构建成功验收。
原项目的 GitHub 出口限制仍保留既有结论。
独立验收会话已注销，浏览器已关闭。

私有回执、备份和截图位于 `.deploy/audit-reading-20261124/`。
