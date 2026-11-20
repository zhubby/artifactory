<!--
[INPUT]: 当前 HTTP 路由、会话和租户项目授权。
[OUTPUT]: 定义公开接口、输入、返回值与权限边界。
[POS]: 控制台与外部集成共用的 HTTP 契约入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# HTTP API v1

统一前缀 `/api/v1`。生产网关将 `/api/*` 直接转给控制平面；开发控制台可使用同源代理。JSON 错误形态为 `{"error":"..."}`；401 未登录、403 禁止、404 不存在、409 冲突、400 参数错误；无效 JSON / schema 由 Axum 返回 4xx。

## 身份与权限

- `POST /auth/login`：`{username,password}`，设置 HttpOnly / SameSite=Strict cookie，生产添加 Secure。
- `GET /auth/config`：无需登录，返回 `{registration_enabled}`；响应禁止缓存。
- `POST /auth/register`：无需登录，仅接受 `{username,password}`，用户名去除首尾空白后为 1–100 UTF-8 字节，区分大小写且不能包含 NUL；密码为 12–256 UTF-8 字节。未知字段（包括 `role`、确认密码）被拒绝。新账号是独立全局身份，没有租户或项目权限，需要创建租户或接受邀请。
- 注册成功返回 `201 {id,username,display_name}`，创建与登录相同的 24 小时会话。用户、会话与 `user_registered` 审计原子提交；密码仅以 Argon2 哈希保存。无效输入返回 `400`，注册关闭或 CSRF 不符合要求返回 `403`，用户名冲突返回 `409`。
- 注册在参数校验后、密码哈希前消耗限流额度：每实例滚动 5 分钟最多受理 10 次有效格式的尝试，重复用户名和后续失败也消耗额度；额度持久化在 `registration_attempt` 审计中，通过数据库事务锁防止并发越限。超限返回 `429` 和 `Retry-After`（秒）；重启不会重置额度，其他接口和登录限流不受影响。
- `GET /auth/me` 返回 `{id,username,display_name}`；`PATCH /auth/me` 接收 `{display_name}`，去除首尾空白后为 1–100 个字符，用户名不可修改。仅 Session，账号接口不依赖租户 header。
- `POST /auth/password` 接收 `{current_password,new_password}`，新密码为 12–256 UTF-8 字节。当前密码错误返回 `400`，不会注销当前会话；成功返回 `{ok:true}` 并注销账号全部会话。密码校验尝试按账号持久化限流，5 分钟最多 10 次，超限 `429`。登录和改密原子复核已验证的密码哈希，阻止旧凭据会话竞态。
- `POST /auth/logout` 注销当前 Session。
- Cookie 写请求需要 `x-artifactory-csrf: 1`，有 Origin 时必须精确匹配 `ARTIFACTORY_PUBLIC_URL`。Bearer Token 不使用 cookie 身份。
- 业务请求必须携带 `X-Artifactory-Tenant`，租户所有者和管理员管理共享配置；普通成员按项目 admin/operator/viewer 授权。租户生命周期仅所有者可操作，项目生命周期允许项目管理员。账号历史角色不授予跨租户权限。
- API Token 明确绑定项目 ID 列表与 `read/build/release` scopes，同时不能超过所属账户角色；不能调用管理 API 或再创建 Token。
- session 有效期 24 小时；Token 有效期 1 到 365 天。登录失败按账号持久化限流，反向代理仍应部署连接 / IP 级保护。

## 资源

| 方法 / 路径 | 输入 / 说明 |
|---|---|
| `GET /overview` | 概览统计，仅会话 |
| `GET, POST /tenant/members` | 当前租户成员；添加已有账号 `{username,role}`，不创建全局账号 |
| `GET, PATCH /tenant` | 读取租户上下文；PATCH `{name}` 改名并保持 slug |
| `GET /tenant/lifecycle`、`GET /projects/{id}/lifecycle` | 返回受影响项目、构建、制品和设备数量 |
| `POST /tenant/lifecycle`、`POST /projects/{id}/lifecycle` | `{action,idempotency_key,slug?,current_password?}`；action 为 archive/restore/delete，删除必填完整 slug 和当前密码，返回 `202` 操作记录 |
| `GET /operations`、`GET /operations/{id}` | 仅账号 Session，查询本人持久操作记录，无需租户 header |
| `POST /operations/{id}/cancel`、`POST /operations/{id}/retry` | 空对象；撤销排空等待，或重试失败步骤，返回操作记录 |
| `GET, POST /tokens` | 创建 `{name,project_ids,scopes,days}`，仅本次返回明文 |
| `DELETE /tokens/{id}` | 吊销当前用户的 Token |
| `GET, POST /pools` | 创建 `{name}` |
| `POST /enrollment-tokens` | `{pool_id}`，返回一次性令牌 |
| `GET /nodes` | 实际能力、诊断、identity 摘要、slots、online、active_jobs；不携带全量 metadata |
| `GET /nodes/{id}/metadata` | viewer 会话；完整启动 metadata、startup_id、内容摘要与服务端接收时间 |
| `GET /nodes/{id}/telemetry` | viewer 会话；最新主机采样、fresh 与版本化清单 |
| `GET /nodes/{id}/inventory` | viewer 会话；最近 100 次清单观察，附 truncated |
| `GET /nodes/{id}/telemetry/history` | viewer 会话；主机时序，显式 from / to / resolution |
| `GET /attempts/{id}/telemetry` | 项目 read；构建资源时序，显式 from / to / resolution |
| `GET /attempts/{id}/processes` | 项目 read；托管进程明细，显式 from / to / resolution=raw |
| `PATCH /nodes/{id}` | `{draining,revoked,slots,labels,signing_profiles,pool_id}`；吊销不可逆 |
| `PATCH /nodes/{id}/name` | 仅租户 owner/admin 会话；`{name}`，成功返回 `{ok:true}`，只更新名称 |
| `GET, POST /credentials` | 创建 `{name,kind,username,secret,known_hosts}`；kind 为 https / ssh |
| `GET /audit` | 最近 200 条审计事件，仅管理员 |
| `GET, POST /projects` | 创建 `{name,repository_url,credential_id,pool_ids,signing_profiles,model_profile_id?,build_context?,source_credentials?}` |
| `GET, PATCH /projects/{id}` | PATCH 使用完整项目设置，不是任意 JSON Patch |
| `POST /projects/{id}/releases` | 项目 release；`{version,ref,goal,target_requirements,targets}`，创建严格 SemVer 草稿 |
| `GET, POST /releases/{id}/builds` | 读取版本构建；创建需要 build 权限，只接受 `{parameters?,goal?}` |
| `PATCH /releases/{id}` | 项目 release；`{spec_revision,goal?,target_requirements?,targets?}`，仅草稿，递增目标修订 |
| `GET /builds` | 可选 `?project_id=`，最多 100 条 |
| `GET /builds/{id}` | 完整源码清单、当前计划 ID、修订次数、澄清、目标、设备 attempt、步骤、制品、镜像执行和发布记录 |
| `GET /builds/{id}/plans` | 不可变计划历史、摘要、文件依据、规划运行和调用统计；不返回模型密钥或 checkpoint |
| `GET /builds/{id}/planning/conversations` | 项目 read；按运行与 generation 返回会话摘要 |
| `GET /builds/{id}/planning/conversations/{run}/{generation}/messages` | 项目 read；`after` 游标与 `limit`（最多 100），返回消息及 `next_cursor` |
| `POST /builds/{id}/clarifications` | `{answer}`；只在 needs_input 继续规划，仍固定原源码与目标 |
| `GET, POST /model-profiles` | 管理员；接口类型/地址/模型/密钥/parameters/enabled/is_default/max_calls/timeout_seconds |
| `PATCH, DELETE /model-profiles/{id}` | 更新增加配置版本；密钥空值保留原值，查询仅返回掩码 |
| `GET /model-options` | 操作者会话可读模型 ID、名称和 enabled，用于项目选择 |
| `POST /builds/{id}/cancel` | 空对象，停止队列和在途任务 |
| `POST /jobs/{id}/retry` | 空对象，仅失败 / lost / canceled 且尚未发布的 job |
| `POST /releases/{id}/publish` | 项目 release；`{build_id}`，冻结当前修订的一次完整成功构建，失败只重试相同交付 |
| `GET /builds/{id}/logs` | 静态历史日志；`attempt_id?`、`step?` 筛选，`before?` 向前翻页；返回 `{logs, next_before}`，每页最多 100 块，页内按 ID 升序；无更早日志时游标为 `null` |
| `GET /builds/{id}/events` | SSE `snapshot` / `log` / `complete`；`Last-Event-ID` 或 `?after=` 恢复日志；终态排空日志后发送包含最终详情的 `complete`，客户端停止重连 |
| `GET /artifacts` | 可按项目过滤，不返回对象 key |
| `GET /artifacts/{id}/download` | 项目 read 权限后重定向到有期限的网关票据地址；不返回 S3 地址 |
| `GET /images` | 当前权限范围内的已验证镜像和 digest |
| `GET /image-executions/{id}/logs?after=0` | 项目 read；镜像日志，按返回 ID 继续读取 |
| `POST /image-executions/{id}/publish` | 项目 release；`{credential_id,repository}`；仅分发已发布版本选中的镜像，标签自动等于 Release 版本，拒绝 tags 字段 |
| `GET /projects/{id}/image-credentials` | 项目 read；只返回名称、类别和启用状态 |
| `POST /projects/{id}/image-credentials` | 项目 admin；`{kind:"registry",name,login:{endpoint,username,password}}` 或 `{kind:"secret",name,value}` |
| `DELETE /projects/{id}/image-credentials/{credential}` | 项目 admin；吊销凭据，不返回加密材料 |
| `GET /releases` | 可按项目过滤，按 SemVer 降序 |
| `GET /releases/{id}` | 草稿目标、固定源码、状态、修订、构建数量与正式交付 |
| `GET /releases/{id}/manifest`、`GET /releases/{id}/checksums` | published 版本的命名交付清单与 SHA256SUMS |

创建租户和项目返回 `{id,slug}`，构建返回 `{id,number,project_slug}`；其他资源创建通常返回 `{id}`；Token 类创建还返回明文值；修改操作通常返回 `{ok:true}`。所有查询使用真实数据库，不返回演示对象。

遥测历史使用 ISO 8601 `from/to` 左闭右开区间，`resolution=raw|minute|hour`，默认 minute；单次范围分别限 6 小时、2 天、31 天，并受数据保留期约束。历史及清单查询返回 `{"samples":[],"truncated":false}`，结果超过行数或字节预算时标记截断。采集范围、未知值、权限和保留策略见 [时序观测](telemetry.md)。

## 触发示例

```sh
curl --fail-with-body -X POST 'https://build.example.com/api/v1/projects/PROJECT_UUID/releases' \
  -H "Authorization: Bearer $ARTIFACTORY_API_TOKEN" \
  -H 'X-Artifactory-Tenant: TENANT_UUID' \
  -H 'Content-Type: application/json' \
  --data '{"version":"1.2.3-rc.1","ref":"main","goal":"Build and verify the CLI","targets":["cli"],"target_requirements":[{"kind":"native","key":"cli","os":"linux","arch":"amd64","artifact_key":"cli","format_suffix":".tar.gz"}]}'
```

随后向 `/releases/RELEASE_UUID/builds` 提交 `{}` 发起构建，向 `/releases/RELEASE_UUID/publish` 提交 `{"build_id":"BUILD_UUID"}` 发布。旧项目级创建构建和构建级补版本发布入口已移除。

版本严格遵循稳定或预发布 SemVer，最多 100 ASCII 字节，不接受 `v` 或 `+metadata`。版本和源码 ref 创建后不可改。目标合同固定原生 key/os/arch/application_platform 或镜像 key/platforms。Build、Artifact 和 Image 响应带 Release 身份，下载名由服务器生成。完整约束见[项目版本交付](release-delivery.md)。

创建 Release 与发布需要 release 权限，执行与重试需要 build 权限。项目模型为空时继承默认模型，创建构建固定配置版本。源码在首次构建解析后供同 Release 的全部构建复用。人工 job 重试使用原提交与计划，只允许 draft Release。构建状态仍为 resolving/planning/needs_input/replanning/queued/running/succeeded/failed/canceled/lost。

生命周期状态、清理顺序和管理页面见[工作台](workspaces.md)。操作记录与目标业务行独立保存，状态为 waiting/running/succeeded/failed/canceled；等待阶段可取消，实际清理开始后只能查询或重试。归档保持查询下载，禁止新增业务写入。

当前列表为内部小团队规模的有界查询，并未提供全量导出、复杂分页或外部商店发布 API；Webhook 和 OIDC 尚未实现。

OCI 客户端通过公开网关的 `/v2/` 访问内建仓库。Registry Token realm 为 `/api/v1/registry/token`，使用 HTTP Basic 携带平台 API Token 或执行专属短期身份，不使用浏览器 Cookie。返回的仓库 JWT 有效期为 60 秒，scope 严格约束租户、项目和仓库。普通 API Token 对正式仓只读；平台发布 JWT 额外绑定 Release 与发布租约代次。构建回执与 publication 接口只接受执行专属 Bearer Token。部署与取消边界见 [镜像构建与网关](image-builds.md)。

## Emissary 版本与升级

`GET /nodes` 的 `identity` 只投影正确类型的 `hostname`、`device_model`、`device_vendor`、`cpu_model` 和 `memory_total_bytes`。缺失字段省略，可见范围与 `diagnostics` 相同；受限项目视角返回空对象。`q` 支持名称、系统、架构、节点 UUID，以及当前会话可见的主机名、型号和 CPU。列表不读取全量启动快照。

节点名称去除首尾空白后必须为1–100个 UTF-8 字节，且不能包含控制字符。注册和改名使用相同校验。改名需要 CSRF 和可写租户，普通成员、项目管理员和 API Token 均无此权限。成功记录 `node_renamed` 审计及 `old_name`、`new_name`，不更换节点 UUID、证书或节点设置。心跳与重连不覆盖已保存名称。

版本目录、节点 / 节点池目标、执行记录、重试及取消见 [Emissary OTA 管理接口](emissary-ota.md#管理接口)。这些管理写操作只允许管理员会话，项目 API Token 不获得升级权限。
