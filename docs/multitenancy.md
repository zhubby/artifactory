<!--
[INPUT]: 租户路由、成员权限与共享设备授权契约。
[OUTPUT]: 定义多租户页面、权限及接口边界。
[POS]: 连接控制台工作区与服务端租户隔离模型。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 多租户与共享构建设备

账号是全局登录身份。每次登录后必须选择租户；新账号没有租户时创建租户，也可以先通过邀请加入。业务页面使用 `/tenants/{tenant-slug}/...`，不同浏览器页签可以选择不同租户。

## 页面地址

| 页面 | 示例 |
| --- | --- |
| 租户大厅 | `/tenants` |
| 租户首页 | `/tenants/default` |
| 项目详情 | `/tenants/default/projects/desktop-app` |
| 构建详情 | `/tenants/default/projects/desktop-app/builds/128` |
| 租户设置 | `/tenants/default/settings` |
| 项目设置 | `/tenants/default/projects/desktop-app/settings` |

进入租户后先展示项目列表；构建、制品和发布统一属于项目路径。顶栏选择租户，项目侧栏选择项目。租户侧栏只显示项目、构建节点和租户设置；项目侧栏只显示项目业务及授权的项目设置。打开设置后，同一侧栏替换为设置分区，内容区不增加第二列纵向导航。切换器只选择工作范围；租户菜单提供全部租户入口。

普通租户成员的设置入口只提供访问令牌，`/settings` 默认显示访问令牌。租户管理员默认进入基本信息。租户旧设置地址 `security` 对应 `tokens`，`resources` 对应 `ai`；项目 `resources` 保持资源与凭据含义。项目设置只对有效项目管理员显示，资源、凭据与签名授权仍遵循下文权限。切换项目时只在构建、制品和发布列表保留栏目。

账号入口位于桌面工作台侧栏底部、手机顶栏右侧。个人菜单只提供个人设置和退出，个人设置默认显示资料，并提供安全和偏好分区。设置覆盖面板、移动抽屉与生命周期流程见[租户与项目工作台](workspaces.md)。

租户和项目的 `slug` 在创建时从名称生成，使用小写英文字母、数字和连字符，最长 63 字符。同名时依次增加 `-2`、`-3`；名称没有可用英文或数字时使用 `tenant`、`project`。租户标识全局唯一，项目标识在租户内唯一，修改显示名称不会改变地址。构建使用项目内从 1 开始的编号，数据库按项目串行分配，删除构建后不复用已分配编号。新实例直接生成这些身份，不从旧库补齐数据。

直接切换新地址，不保留旧 `/t/{uuid}`、UUID 构建页面、重定向或双路由。浏览器通过当前成员可访问的租户标识解析 UUID，再发送 `X-Artifactory-Tenant`；缓存只保存稳定标识映射，业务权限仍逐次校验。未知路径和非法构建编号返回 404。

## 权限

租户所有者和管理员可访问本租户全部项目，并管理设备、节点池、凭证、模型和 Worker。每个租户只有一个所有者，只有当前所有者可以原子转移所有权。管理员不能移除或降级所有者。

普通租户成员需要项目授权。项目管理员管理项目配置和成员；操作员构建、取消、重试和发布；只读成员查看。项目管理员只能选择已有租户成员，不能扩大项目设备、凭证或签名授权；带凭证项目的仓库地址由租户管理员变更。历史 `users.role` 不再提供业务权限。

Session 业务请求必须携带 `X-Artifactory-Tenant`，服务端每次读取实际成员关系。缺少上下文返回 `400` 和 `code: tenant_required`，不自动选择默认租户。API Token 固定绑定租户及项目范围，使用时继续校验持有者的当前项目角色，不能通过修改 header 切换租户。账号登录、注册、身份和租户选择使用账号 Session，不使用项目 Token。

共享设备只保存一份身份、连接和槽位。项目可选择 `pools` 或 `nodes` 模式；两种授权列表分别保存，只使用当前模式。指定设备授权不受租户内换池影响。空列表有效，构建会排队并说明没有合格授权设备。构建创建时冻结授权，普通调度、AI、重选、MCP 和命令投递使用冻结范围与当前项目授权的交集。撤销授权阻止新分配，已经运行的任务可继续完成。槽位通过 PostgreSQL 节点行锁跨项目、跨控制平面副本共享。

普通成员可读取获授权项目的可用设备及总槽位占用；设备详情隐藏运维诊断和 OTA 错误，其他项目的任务、日志和凭证通过项目权限隔离。源码准备进程、Telemetry 和规划协调器通过控制平面确认的任务、设备或租约访问，不接受客户端自报的租户作为授权依据。规划 Worker 使用绑定 Pod 的投射令牌领取当前运行，模型密钥从该任务的租户配置获取。

## 接口

路径均以 `/api/v1` 开头。修改 Session 请求继续要求 CSRF header。

| 接口 | 用途 |
| --- | --- |
| `GET /tenants`、`POST /tenants` | 当前账号租户、幂等创建租户和默认池 |
| `GET /tenants/{slug}` | 当前账号成员可访问的租户；未知或非成员均为 404 |
| `GET /projects/by-slug/{slug}` | 当前租户内项目，继续校验项目角色和 Token 范围 |
| `GET /projects/by-slug/{slug}/builds/{number}` | 当前租户及项目内编号解析构建，继续校验读取权限 |
| `GET /tenant`、`PATCH /tenant` | 当前租户及有效项目角色；PATCH `{name}` 改名，slug 不变 |
| `GET, POST /tenant/lifecycle`、`GET, POST /projects/{id}/lifecycle` | 影响数量；归档、恢复或删除 |
| `GET /operations`、`GET /operations/{id}` | 账号级持久操作记录，不需要租户上下文 |
| `POST /operations/{id}/cancel`、`POST /operations/{id}/retry` | 撤销等待阶段操作、重试失败步骤 |
| `GET /tenant/members`、`POST /tenant/members` | 成员列表、按准确用户名添加已有账号 |
| `PATCH /tenant/members/{user_id}`、`DELETE /tenant/members/{user_id}` | 更改角色、移除成员 |
| `POST /tenant/owner` | 当前所有者转移所有权，提交 `{user_id}` |
| `GET /tenant/invitations`、`POST /tenant/invitations` | 邀请状态、生成邀请，提交 `{role}` |
| `DELETE /tenant/invitations/{id}` | 撤销邀请 |
| `GET /invitations/{token}`、`POST /invitations/{token}/accept` | 公开预览、登录后确认加入 |
| `GET /projects/{id}/members`、`GET /projects/{id}/member-options` | 项目成员及可授权租户成员 |
| `PUT /projects/{id}/members/{user_id}`、`DELETE /projects/{id}/members/{user_id}` | 授权或移除项目成员 |
| `PUT /projects/{id}/node-access` | 租户管理员提交 `{mode: "pools" \| "nodes", ids: [...]}` |

创建租户提交 `{name, idempotency_key}`，失败重试复用同一 UUID。邀请有效期 72 小时，单次消费；数据库只保存令牌摘要。完整邀请链接只在创建时返回，之后仅能查看状态或撤销。`/users` 是租户成员接口的兼容入口，不枚举全站账号，不接受初始密码。

租户和项目响应包含稳定的 `slug`，创建响应返回 `{id, slug}`。邀请确认响应包含 `tenant_slug`。构建列表、详情和创建响应包含 `number`、`project_slug`，用于生成页面地址；内部操作、事件流及资源关联继续使用 UUID。

项目响应包含 `node_access`、`node_ids`、`pool_ids` 和 `effective_role`。`pool_ids` 保留为节点池模式兼容字段；与 `node_access` 同时提交时必须与其模式及 ID 完全一致。

## 初始化与验收

当前版本只支持新实例，业务与规划数据库分别采用 `2026112101`、`2026111901` 基线。程序拒绝旧库，不提供旧授权模型的数据迁移或自动重置。

新库不恢复旧业务列表或授权，设备向新实例重新注册。新旧实例使用独立数据库和资源名称，不混跑两套授权模型。旧实例的 S3/Registry 对象、PKI 和主密钥由管理员独立备份和退役；安装新版本不会删除这些资源。

恢复入口前验证租户选择、邀请确认、跨租户直接 ID 拒绝、项目设备授权交集、任务材料范围与分页对话权限。任何越权读取或分配都应阻断入口并保留日志。
