# HTTP API v1

统一前缀 `/api/v1`。生产网关将 `/api/*` 直接转给控制平面，源码流不经过 Next 请求体缓冲；开发控制台可使用同源代理。JSON 错误形态为 `{"error":"..."}`；401 未登录、403 禁止、404 不存在、409 冲突、400 参数错误；无效 JSON / schema 由 Axum 返回 4xx。

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
| `GET /nodes` | 实际能力、诊断、slots、online、active_jobs；不携带全量 metadata |
| `GET /nodes/{id}/metadata` | viewer 会话；完整启动 metadata、startup_id、内容摘要与服务端接收时间 |
| `GET /nodes/{id}/telemetry` | viewer 会话；最新主机采样、fresh 与版本化清单 |
| `GET /nodes/{id}/inventory` | viewer 会话；最近 100 次清单观察，附 truncated |
| `GET /nodes/{id}/telemetry/history` | viewer 会话；主机时序，显式 from / to / resolution |
| `GET /attempts/{id}/telemetry` | 项目 read；构建资源时序，显式 from / to / resolution |
| `GET /attempts/{id}/processes` | 项目 read；托管进程明细，显式 from / to / resolution=raw |
| `PATCH /nodes/{id}` | `{draining,revoked,slots,labels,signing_profiles,pool_id}`；吊销不可逆 |
| `GET, POST /credentials` | 创建 `{name,kind,username,secret,known_hosts}`；kind 为 https / ssh |
| `GET /audit` | 最近 200 条审计事件，仅管理员 |
| `GET, POST /projects` | 创建 `{name,repository_url,credential_id,pool_ids,signing_profiles,model_profile_id?,build_context?,source_credentials?}` |
| `GET, PATCH /projects/{id}` | PATCH 使用完整项目设置，不是任意 JSON Patch |
| `POST /projects/{id}/builds` | `{ref,goal,target_requirements?,parameters?,targets?,release_version?}`；所有新构建走 AI |
| `GET /builds` | 可选 `?project_id=`，最多 100 条 |
| `GET /builds/{id}` | 源码快照、当前计划 ID、修订次数、澄清、目标、设备 attempt、步骤、制品、镜像执行和发布记录 |
| `GET /builds/{id}/plans` | 不可变计划历史、摘要、文件依据、规划运行和调用统计；不返回模型密钥或 checkpoint |
| `POST /builds/{id}/clarifications` | `{answer}`；只在 needs_input 继续规划，仍固定原源码与目标 |
| `GET, POST /model-profiles` | 管理员；接口类型/地址/模型/密钥/parameters/enabled/is_default/max_calls/timeout_seconds |
| `PATCH, DELETE /model-profiles/{id}` | 更新增加配置版本；密钥空值保留原值，查询仅返回掩码 |
| `GET /model-options` | 操作者会话可读模型 ID、名称和 enabled，用于项目选择 |
| `GET /ai-workers` | 管理员；在线、排空、吊销、证书期限和活动规划任务 |
| `POST /ai-enrollment-tokens` | 管理员；15 分钟一次性 worker 注册令牌 |
| `PATCH /ai-workers/{id}` | `{draining,revoked}`；吊销不可逆 |
| `GET /source-metrics` | 管理员；平台/Emissary 缓存命中、磁盘占用及下载字节，fetched_bytes 是磁盘增长 |
| `POST /builds/{id}/cancel` | 空对象，停止队列和在途任务 |
| `POST /jobs/{id}/retry` | 空对象，仅失败 / lost / canceled 且尚未发布的 job |
| `POST /builds/{id}/publish` | `{version?}`；既有版本不可更改；重试复用制品 |
| `GET /builds/{id}/logs` | 静态历史日志；`attempt_id?`、`step?` 筛选，`before?` 向前翻页；返回 `{logs, next_before}`，每页最多 100 块，页内按 ID 升序；无更早日志时游标为 `null` |
| `GET /builds/{id}/events` | SSE `snapshot` / `log` / `complete`；`Last-Event-ID` 或 `?after=` 恢复日志；终态排空日志后发送包含最终详情的 `complete`，客户端停止重连 |
| `GET /artifacts` | 可按项目过滤，不返回对象 key |
| `GET /artifacts/{id}/download` | 项目 read 权限后重定向到有期限的网关票据地址；不返回 S3 地址 |
| `GET /images` | 当前权限范围内的已验证镜像和 digest |
| `GET /image-executions/{id}/logs?after=0` | 项目 read；镜像日志，按返回 ID 继续读取 |
| `POST /image-executions/{id}/publish` | 项目 release；`{credential_id?,repository?,tags:[]}`；省略外部凭据时重试完整平台结果的内建发布，否则按原 digest 复制到同项目凭据指定仓库 |
| `GET /projects/{id}/image-credentials` | 项目 read；只返回名称、类别和启用状态 |
| `POST /projects/{id}/image-credentials` | 项目 admin；`{kind:"registry",name,login:{endpoint,username,password}}` 或 `{kind:"secret",name,value}` |
| `DELETE /projects/{id}/image-credentials/{credential}` | 项目 admin；吊销凭据，不返回加密材料 |
| `GET /releases` | 可按项目过滤 |
| `GET /releases/{id}` | 不可变版本与制品目录 |

创建租户和项目返回 `{id,slug}`，构建返回 `{id,number,project_slug}`；其他资源创建通常返回 `{id}`；Token 类创建还返回明文值；修改操作通常返回 `{ok:true}`。所有查询使用真实数据库，不返回演示对象。

遥测历史使用 ISO 8601 `from/to` 左闭右开区间，`resolution=raw|minute|hour`，默认 minute；单次范围分别限 6 小时、2 天、31 天，并受数据保留期约束。历史及清单查询返回 `{"samples":[],"truncated":false}`，结果超过行数或字节预算时标记截断。采集范围、未知值、权限和保留策略见 [时序观测](telemetry.md)。

## 触发示例

```sh
curl --fail-with-body -X POST 'https://build.example.com/api/v1/projects/PROJECT_UUID/builds' \
  -H "Authorization: Bearer $ARTIFACTORY_API_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"ref":"main","goal":"Build the CLI, test it and collect the executable","target_requirements":[{"os":"linux","arch":"amd64"}],"parameters":{"CHANNEL":"preview"},"release_version":"v1.0.0-preview.1"}'
```

发布版本的请求与会重新启用自动发布的 retry 均需要 release scope。更改项目仓库不会改变已经创建的构建源地址。凭据轮换应创建新的引用并修改项目；历史构建不静默切换凭据。

未指定 goal 时使用项目平台侧 build_context；两者均为空时拒绝。项目模型为空时继承平台默认值，创建构建固定该配置版本。`target_requirements` 是 `{os,arch}` 数组；`targets` 若提供则固定目标名集合。人工 job 重试使用原提交与计划，规划中不可重试。状态为 resolving / planning / needs_input / replanning / queued / running / succeeded / failed / canceled / lost。执行失败最多两次自动修订，权限、认证和源码凭据错误不自动修订，详见 [AI 构建](ai-builds.md)。

生命周期状态、清理顺序和管理页面见[工作台](workspaces.md)。操作记录与目标业务行独立保存，状态为 waiting/running/succeeded/failed/canceled；等待阶段可取消，实际清理开始后只能查询或重试。归档保持查询下载，禁止新增业务写入。

当前列表为内部小团队规模的有界查询，并未提供全量导出、复杂分页或外部商店发布 API；Webhook 和 OIDC 尚未实现。

OCI 客户端通过公开网关的 `/v2/` 访问内建仓库。Registry Token realm 为 `/api/v1/registry/token`，使用 HTTP Basic 携带平台 API Token 或执行专属短期身份，不使用浏览器 Cookie。返回的仓库 JWT 有效期为 60 秒，scope 严格约束租户、项目和仓库。构建回执与 publication 接口只接受执行专属 Bearer Token。部署与取消边界见 [镜像构建与网关](image-builds.md)。

## Emissary 版本与升级

版本目录、节点 / 节点池目标、执行记录、重试及取消见 [Emissary OTA 管理接口](emissary-ota.md#管理接口)。这些管理写操作只允许管理员会话，项目 API Token 不获得升级权限。
