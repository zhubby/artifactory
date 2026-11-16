# 托管应用签名 Kubernetes 交付记录

日期：2026-11-16，Asia/Shanghai。范围为 Kubernetes `1.28.3` 的 `artifactory` 命名空间。

## 实现和审查修复

功能提交为 `14db06cbe5f47f6a100f15d8fe6d32dbda75003a`。服务端最终运行源码为 `6ce851d2c94451286cd3be4b1a99b5df75c1e7bf`。节点文件锁修复为 `488647b205edc7652d3a985a4990475e0df65b93`。上述提交均已进入 main，并核对本地、跟踪引用和实时远端引用。

平台加密保存 macOS PKCS12 和 Android JKS、PKCS12。租户设置凭据及方案，项目设置默认方案。构建创建时固定版本；节点在签名阶段凭 mTLS、当前会话、租约及项目和节点授权临时领取。原生验签、摘要和上传使用同一制品副本。

审查和真实验证修复了以下问题：

| 问题 | 修复与验证 |
| --- | --- |
| 默认方案被异步列表覆盖 | 方案就绪后初始化表单；保留项目原选择 |
| 默认方案缺少不可用状态 | 统一最新版本可用条件，不回退历史版本 |
| DMG 镜像自身缺少公证检查 | 同时检查镜像和包内应用票据 |
| DMG 部分挂载失败遗留资源 | 挂载前建立清理守卫，覆盖失败和取消 |
| Android CI 缺少原生工具 | 显式安装 JDK、Build Tools 36.0.0 和 Android 36 |
| Helm 把模块地图解析为模板 | 排除 `templates/AGENTS.md`，strict lint 通过 |
| 跨平台签名锁误判和延迟释放 | 使用 fs2 平台错误契约；清理及 Drop 显式解锁 |
| 密钥环 init 容器缺少资源声明 | 沿用 Control 预算，真实配额准入通过 |
| Windows Worker 异步帧过大 | 摘要缓冲区改为堆分配，增加栈预算回归 |

文件锁继承回归和异步帧预算回归均先失败、修复后通过。节点锁修复之后，`git diff 6ce851d..488647b` 仅包含 Emissary 签名模块、模块地图和实施计划。集群镜像中的 Control、Worker、Web 代码与 main 的对应代码一致。外部主机的 Emissary 未在本次部署中升级。

## 验证

- 审查首轮 Rust 常规验证 75 项通过、103 项按声明忽略；全仓 Clippy 和格式检查通过。
- 隔离 PostgreSQL 签名专项 4 项通过；真实 JKS、AAB、APK 原生专项 3 项通过。
- 最新节点锁修复后，Emissary 全目标测试 44 项通过、4 项按声明忽略；Emissary Clippy、格式和差异检查通过。
- 规划 Worker 16 项通过。Web 类型检查、生产构建和 mdBook 构建通过。
- 签名与构建向导桌面、移动端回归 38 项通过。
- 启用签名 Secret 的 Helm strict lint 通过。目标集群完整清单的 server dry-run 通过。

隔离测试的忽略项不计入已执行验收。现场浏览器使用真实 API，先前带模拟响应的 UI 回归单独计数。

## 不可变镜像

[deployment-images 43888584267](https://github.com/zhubby/artifactory/actions/runs/43888584267) 的三项镜像构建均成功。核对 GHCR、集群镜像端点、amd64 manifest、config 和 OCI 源码 SHA。在三个节点完成九个临时 Pod 的实际预拉取。

| 组件 | 索引摘要 |
| --- | --- |
| Control / Source / Telemetry | `sha256:d89df4b4e99e903e6e295224d7a8faec28d4e9db1f33ee95f1242ee5a5c98103` |
| Agent Worker | `sha256:55773626227960e7ba59b5d303cbce024847a370c86a399fee6fe43128690a36` |
| Web | `sha256:b9c96bc40b82eb8c7013dd3617e31bd447a139bb7b8324e4a4e80c8834c7ecc1` |

镜像引用同时固定完整源码 SHA 标签和摘要。现场核对七个第一方运行容器的实际 image ID 和 config 摘要。部署清单只改变五个第一方 Deployment 的镜像及 Control 的密钥环挂载。

## 维护、备份和迁移

22:55:14 CST 进入维护。停止 Web、Worker、Control、Source 和 Telemetry，保留 PostgreSQL、S3 和独立 Gateway。停写前后均确认没有活动构建、规划、执行租约或节点升级。

停写后生成真实 Control `pg_dump` 和 SHA-256 manifest。备份位于本机受限目录 `.local/signing-delivery-20261116/backup-20261116T145547Z/`。22:56:00 CST 完成独立恢复库的 TimescaleDB 恢复前处理、`pg_restore`、恢复后处理，以及业务计数和设备身份比对。临时恢复库随后删除，备份保留。

22:56:01 CST 应用追加迁移 `2026111601 / 0007_managed_signing.sql`。新增签名凭据、不可变版本、方案、证据及构建冻结约束。当前服务使用显式 SQL 访问新增字段，保留既有 Toasty CRUD 投影。

22:56:21 CST Helm revision `11` 达到 `deployed`。保持三个 Control 副本，恢复原有其余副本数。迁移后未自动回退旧二进制。

部署写入使用命名空间内受限 deployer。账号没有 `/scale` 子资源权限，首次停服务操作被拒，业务未停写；随后使用该账号已有的 Deployment 更新权限完成维护。未扩大 RBAC 或修改集群全局配置。

## 独立密钥环

生成独立的 32 字节签名主密钥，与原源码凭据主密钥比对为不同值。Secret 为 `artifactory-signing`。仅三个 Control Pod 挂载密钥环；init 容器复制到内存卷，运行文件的所有者为 UID `10001`，权限为 `0600`。

密钥环备份单独保存在 `.local/signing-keyring-20261116/keys.json`。目录权限为 `0700`，文件权限为 `0600`。密钥、密码、会话和证书私钥未进入 Git。

## 现场验收

| 范围 | 结果 |
| --- | --- |
| 导入 | 临时 JKS 的真实私钥验证通过；错误私钥密码返回 400；公开响应仅含元数据 |
| 默认方案 | 项目默认方案和构建预览可用；轮换生成新版本，原构建保留旧版本 |
| 存储 | 数据库保存独立数据密钥的加密信封；未保存材料明文和密码 |
| 权限 | 匿名读取返回 401；只读 Token 不能导入或读取凭据；跨租户访问被拒 |
| 停用 | 最新版本停用后预览不可用；新默认配置返回 400，新构建返回 409 且未插入记录；允许显式移除默认方案 |
| 页面 | 真实桌面和手机视口验证租户签名页、项目默认页及已选方案；截图已检查，无页面脚本错误 |
| 逐副本 | 三个 Control 分布于三个节点；readiness、同一 Session、设备查询和签名配置均通过 |
| 内部服务 | Source readiness 和 Telemetry metrics 均通过私有 CA 校验和必要鉴权 |
| 持久资源 | 四个 PVC 保持原 UID 且 Bound；原凭据及 PKI Secret 的 UID 和内容不变；PostgreSQL、S3 规格及独立 Gateway 规格不变 |
| 既有业务 | 保留 2 个租户、1 个用户、1 个项目、5 个构建、10 个制品和 1 个发布；3 台节点及 2 个 Worker 的身份、证书摘要和撤销状态不变 |
| 在线状态 | 3 台原节点在线，1 个 Worker 在线；历史离线 Worker 身份保留 |
| 非目标资源 | 其他命名空间的 203 项选定资源 UID 与规格摘要全部不变 |
| 清理 | 临时租户、项目、构建、签名凭据、版本和 Token 通过生命周期 API 删除；保留审计、操作和墓碑 |

验收脚本先后修正隐藏选项选择器、停用冲突状态预期和容器运行时的 config 摘要投影。失败轮次的临时租户和会话均清除，原会话保留。最终完整 API 与浏览器轮次通过。

私有证据、截图和备份保存在 `.local/signing-delivery-20261116/`。九个镜像预拉取 Pod 已删除。原入口 `http://127.0.0.1:38100` 保留。

23:05:18 至 23:07:19 CST 完成四次稳定性采样，持续约 122 秒。三个 Control 副本持续就绪，10 个 Pod 全部 Ready，所有容器零重启。三台原节点和一个 Worker 持续在线。签名配置、认证 API 和匿名签名文档均可用。

观察完成后注销验收 Session，原 Cookie 返回 401。以迁移前备份比对会话表，没有新增验收会话，原会话集合保持一致。本次 PostgreSQL 转发已关闭，受限备份和密钥环备份保留。

## CI 状态

[节点锁修复的 platform-contracts 43890743494](https://github.com/zhubby/artifactory/actions/runs/43890743494) 已通过版本契约、Compose TLS，以及 Linux amd64、Linux ARM、Windows amd64、Windows ARM 和 macOS ARM 原生 job。记录时 macOS Intel job 仍在运行；[镜像源码的 platform-contracts 43888548125](https://github.com/zhubby/artifactory/actions/runs/43888548125) 已通过 macOS Intel 原生 job。

最新 Linux 数据库轮次为 96 项通过、1 项失败、1 项过滤。失败仍为既有 OpenSSH fixture 对 `/tmp` ownership / modes 的拒绝。Kubernetes job 在 kind 初始化阶段因 kubeadm `v1beta3` 配置不兼容失败，尚未进入应用部署。两项失败均从本轮日志核实，完整源码 CI 不为全绿。本次镜像构建、目标集群迁移和现场验收分别通过。

## 验收边界

正式签名仍需导入用户自己的证书，将执行节点升级到支持 `managed-signing-v1` 的 Emissary，并显式授权方案。本次没有在既有主机安装新 Emissary，没有运行正式证书构建或变更设备身份。

正式 Developer ID 公证、Gatekeeper 在线判定和 Android 真机覆盖升级未验收。手机页面使用 Chromium 视口模拟。三副本仅指应用 Control；PostgreSQL、S3、Source、Telemetry 和 Worker 仍为单副本。

本记录和最终验收状态在部署后追加到 main。运行镜像保持上述 `6ce851d` 源码；后续节点清理和文档提交不改变这些服务端组件。

## 后续真实证书验收

2026-11-17 完成 [AI-Workspace macOS 托管证书签名验收](../ai-workspace-signing-acceptance-2026-11-17.md)。
指定 Mac 节点已升级到包含 `cf14d16` 原生验签修复的运行时。
第 8 次构建的正式签名、两项 S3 下载验签和材料清理通过。
原服务端镜像保持不变，三个 Control 及命名空间内全部 10 个 Pod 均 Ready。
该后续结果不包含 Apple 公证或人工安装验收。
