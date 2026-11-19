---
title: Release 中心的版本交付
status: complete
---

# Release 中心的版本交付

项目包含 Release。Release 包含多次构建。正式交付选择一次完整成功构建。

## 已确认边界

- 版本采用严格 SemVer：稳定版与预发布版，最多 100 ASCII 字节。拒绝 `v` 前缀、构建元数据、前导零和隐式转换。
- Release 版本创建后不变，同项目唯一。列表按 SemVer 排序。
- 首次构建固定源码。并发构建共用解析身份。失败后保留已 pin 的 OID 与 frozen tags。
- Release 草稿持有目标合同。修改目标递增 `spec_revision`。构建快照目标修订。
- 发布选择当前修订的一次完整成功构建。所有源码、规划、原生、镜像和执行 Pod 已停止后才冻结交付。
- 生命周期为 `draft → publishing → published`；失败进入 `publish_failed`，只重试相同交付。
- 交付记录单独关联 selected Build，保存不可变 source、revision、manifest、文件 sha256 和完整 OCI 图。
- 原生环境与 BuildKit 参数注入 `ARTIFACTORY_RELEASE_ID` 和 `ARTIFACTORY_RELEASE_VERSION`。AI 不得改变版本、源码或缩减目标。
- 镜像构建仅生成 execution 隔离候选。正式仓路径保持 tenant/project/images/component；正式标签只等于版本。
- 正式仓普通 Token 只读。Publisher 写入须经过 Gateway，按冻结仓库、版本、完整 manifest digest 白名单和发布租约准入。
- Gateway 有界缓存 manifest，先校验后转发。Registry 网络入口仅允许 Gateway。
- 原生下载名为 `<project>-<artifact_key>-<version>-<platform>-<arch><suffix>`，安全、唯一、最多 255 字节。保留签名字节。
- 下载票据绑定名称。GET、HEAD、Range 返回相同 Content-Disposition。
- Release 提供版本 manifest 与 SHA256SUMS。外仓仅分发 published Release，版本标签只读。
- 第一版保证版本来源、交付名、镜像标签和 OCI 元数据。包内版本不作全平台一致性承诺。

## 共享接口合同

- `POST /api/v1/projects/{project}/releases`：`{version,ref,goal,target_requirements,targets}`。
- `target_requirements` 是目标合同数组。原生项：`{kind:"native",key,os,arch,application_platform?,artifact_key?,format_suffix?}`。镜像项：`{kind:"image",key,platforms:["linux/amd64",...]}`。key 为小写安全逻辑键。原生后缀支持 `.tar.gz`。
- `PATCH /api/v1/releases/{id}`：`{spec_revision,goal?,target_requirements?,targets?}`。仅 draft，版本与源码 ref 不可改。
- `GET /api/v1/releases/{id}`：`id,project_id,project_slug,project_name,version,status,requested_ref,goal,target_requirements,targets,spec_revision,source_manifest_id,commit_sha,build_count,selected_build_id,artifacts,images,delivery_manifest,publish_error`。
- `GET/POST /api/v1/releases/{id}/builds`：创建只接受 `{parameters?,goal?}`；版本、源码、目标从 Release 复制。Build 响应包含 `release_id,release_version,spec_revision`。
- `POST /api/v1/releases/{id}/publish`：`{build_id}`，不接受 version。返回 Release 身份。
- 移除项目直接创建构建与 build 后补版本接口。保留项目跨版本构建与产物查询。
- `release_deliveries` 唯一关联 release_id/build_id，冻结 `manifest` JSON。`release_artifacts` 增加 `delivery_filename`。`release_images` 保存正式仓库、版本、root_digest 和 manifest 白名单。
- 公共制品与镜像投影包含 `release_id,release_version`；正式制品带 `delivery_filename`。

## 实施与验证清单

- [x] 领域模型、空库基线、SemVer 与租户约束。
- [x] Release API、构建归属、目标修订、固定源码复用。
- [x] 发布冻结、并发围栏、文件命名与幂等失败重试。
- [x] 候选镜像、正式推广、版本参数与 OCI 元数据。
- [x] Gateway manifest 准入、下载名与 Registry 网络隔离。
- [x] 版本工作台、构建入口、导航、权限与测试 fixture。
- [x] 单元、隔离数据库、接口、浏览器与部署检查。
- [x] 独立代码审查、文档合同同步。
- [x] 新源码快照与镜像 digest 读回。
- [x] 仅重置并部署 `artifactory-20261119`，保留旧实例和基础身份。
- [x] K8s fixture 构建、发布、下载/pull、防覆盖与旧资源指纹核验。

## 部署限制

仅使用 `.deploy/k8s-image-verification/kubeconfig`。保留旧 namespace `artifactory`。
停止新实例写入后清空三个逻辑库和两个 Bucket。保留 namespace、PVC、Secret、CA 与基础设施密钥。
初始化账号 `admin / 123456`。公网 GitHub 出口曾超时，验收必须注明实际源码来源。

## Gateway 与 Registry 本机验证

Manifest 正文上限为四 MiB，每监听最多缓冲八个正文。Control 授权前不向 Distribution 写正文。Basic 首次握手和过期 JWT 保留 Distribution 认证挑战。租约代次失效后拒绝新授权；已经通过授权的正文仍受永久摘要绑定约束。

Gateway 五项单测、Storage 三项单测、冻结摘要围栏单测、十九项 Helm 初始化测试与 Registry Helm lint 通过。`tests/images/gateway.py` 使用真实 TCP/TLS 验证固定文件名、GET/HEAD/Range、摘要篡改拒绝、版本标签拒绝、普通身份拒绝、正文上限、认证挑战与写请求不重放。已有宽 NetworkPolicy 已排除 Registry。集群实测 Web Pod 直连 Registry Service 与两 Pod 的 TCP 5000 连接均超时；Gateway 返回正常 Distribution 认证挑战。

最终部署使用 `workspace-170929781a08c04b`，九项产品 Deployment 全部就绪。镜像与原生 Release 均完成两次构建，并发布第二次结果。源码快照复用、版本交付、防覆盖、下载校验和六项真实浏览器测试通过。临时夹具已清理，旧实例与基础身份最终比对通过。源码与模型使用集群内夹具，原生验收限 Linux/amd64 Pod。完整记录见[本轮 Kubernetes 交付](../releases/2026-11-20-release-delivery-kubernetes.md)。

[PROTOCOL]: Update this header on change, then check AGENTS.md
