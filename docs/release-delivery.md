<!--
[INPUT]: 项目 Release、目标修订、固定源码与冻结交付。
[OUTPUT]: 定义用户版本、构建归属、命名和正式交付边界。
[POS]: 项目版本工作台的产品契约；平台自身版本见 versioning.md。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 项目版本交付

项目包含多个 Release。每次构建必须属于一个 Release。正式发布选择当前目标修订的一次完整成功构建。

## 创建版本

版本严格采用 SemVer，例如 `1.2.3` 或 `1.2.3-rc.1`。版本最多 100 ASCII 字节，不接受 `v` 前缀、`+metadata`、空格或数字前导零。大小写保留，不自动改名。列表按 SemVer 排序。

同项目版本唯一，创建后不可修改。`1.2.3-rc.1` 和 `1.2.3` 是独立版本。发布 RC 后创建稳定版并重新构建，不通过改名升级。

创建时填写源码 ref、构建目标说明和完整目标合同。原生目标固定逻辑 key、主机系统、架构与应用平台。镜像目标固定组件 key 和全部 OCI 平台。

草稿可以修改目标合同，每次修改增加 `spec_revision`。历史构建保留原修订。旧修订构建不能发布。

## 构建与源码

首次构建冻结仓库地址、凭据引用和绑定。项目之后更换仓库不改变该 Release 的来源。

首次构建解析 ref 并固定完整 SourceManifest，包含根提交、子模块、LFS 对象和标签。并发构建等待同一个解析任务。解析失败后保留已 pin 的 OID 和标签，重试不追随移动分支。成功后同 Release 的所有构建复用该清单。

每次构建执行该修订的全部目标。AI 选择构建方法，服务端检查目标 key、原生平台、镜像组件与全部架构。AI 不能减少交付目标或指定正式标签。

原生环境与 BuildKit 参数均包含 `ARTIFACTORY_RELEASE_ID`、`ARTIFACTORY_RELEASE_VERSION`。平台强制写入并读回 OCI `org.opencontainers.image.version` 和 `org.opencontainers.image.revision`。

项目仍需在自己的构建脚本或 Dockerfile 中消费版本参数。本期不验证所有包格式的内部版本，不改写签名后的文件内容。

## 正式发布

Release 生命周期为 `draft → publishing → published`。发布失败进入 `publish_failed`。

发布要求一次构建完整成功且使用当前修订。源码任务、规划、原生 attempt、镜像执行与执行 Pod 均已停止。发布与构建创建、重试共享 Release 锁。

首次发布冻结 selected Build、源码清单、目标修订、文件 sha256、完整 OCI 图摘要和 delivery manifest。冻结后不能修改目标、换构建或启动新的构建。失败仅重试相同内容，不能退回草稿。

构建阶段只生成 execution 隔离的候选镜像。Release 发布才推广正式仓：

```text
{tenantUUID}/{projectUUID}/images/{component_key}:{release.version}
```

每个组件的唯一版本标签指向完整多架构 OCI Index。正式仓不提供 `latest` 或用户别名。普通 API Token 只能 pull。平台发布身份必须经过 Gateway 的租约与完整 Manifest 白名单校验。版本与根摘要永久绑定，失败或租约过期不释放绑定。

Registry 网络入口仅允许 Gateway。Manifest 在转发前完成有界读取与校验；Blob 保持流式传输。

## 下载与分发

原生交付名采用：

```text
<project_slug>-<artifact_key>-<version>-<application_platform_or_host_os>-<arch><format_suffix>
myapp-desktop-1.2.3-windows-amd64.zip
```

逻辑 key 使用小写安全字符。后缀支持 `.tar.gz`。完整名称必须安全、在版本内唯一且不超过 255 字节；冲突和超长直接拒绝，不截断。原始路径、对象 key、文件字节、sha256 和签名证据保持原身份。

下载票据绑定正式名称。GET、HEAD 与 Range 返回相同 `Content-Disposition`。版本同时提供 `<project>-<version>-release.json` 和 `<project>-<version>-SHA256SUMS`。

外仓只分发已发布版本中选定的镜像。标签自动等于 Release 版本，冲突拒绝并读回校验。分发状态独立，不改变 Release 身份。外仓离开平台后的防覆盖由其不可变策略保证。
