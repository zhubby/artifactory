# 多租户与共享构建设备

账号是全局登录身份。每次登录后必须选择租户；新账号没有租户时创建租户，也可以先通过邀请加入。业务页面使用 `/tenants/{tenant-slug}/...`，不同浏览器页签可以选择不同租户。

## 页面地址

| 页面 | 示例 |
| --- | --- |
| 租户首页 | `/tenants/default` |
| 项目详情 | `/tenants/default/projects/desktop-app` |
| 构建详情 | `/tenants/default/projects/desktop-app/builds/128` |
| 租户设置 | `/tenants/default/settings` |

租户和项目的 `slug` 在创建时从名称生成，使用小写英文字母、数字和连字符，最长 63 字符。同名时依次增加 `-2`、`-3`；名称没有可用英文或数字时使用 `tenant`、`project`。租户标识全局唯一，项目标识在租户内唯一，修改显示名称不会改变地址。构建使用项目内从 1 开始的编号，数据库按项目串行分配，删除构建后不复用已分配编号。已有开发数据按创建时间和 UUID 顺序补齐标识及编号，保留全部内部身份。

直接切换新地址，不保留旧 `/t/{uuid}`、UUID 构建页面、重定向或双路由。浏览器通过当前成员可访问的租户标识解析 UUID，再发送 `X-Artifactory-Tenant`；缓存只保存稳定标识映射，业务权限仍逐次校验。未知路径和非法构建编号返回 404。

## 权限

租户所有者和管理员可访问本租户全部项目，并管理设备、节点池、凭证、模型和 Worker。每个租户只有一个所有者，只有当前所有者可以原子转移所有权。管理员不能移除或降级所有者。

普通租户成员需要项目授权。项目管理员管理项目配置和成员；操作员构建、取消、重试和发布；只读成员查看。项目管理员只能选择已有租户成员，不能扩大项目设备、凭证或签名授权；带凭证项目的仓库地址由租户管理员变更。历史 `users.role` 不再提供业务权限。

Session 业务请求必须携带 `X-Artifactory-Tenant`，服务端每次读取实际成员关系。缺少上下文返回 `400` 和 `code: tenant_required`，不自动选择默认租户。API Token 固定绑定租户及项目范围，使用时继续校验持有者的当前项目角色，不能通过修改 header 切换租户。账号登录、注册、身份和租户选择使用账号 Session，不使用项目 Token。

共享设备只保存一份身份、连接和槽位。项目可选择 `pools` 或 `nodes` 模式；两种授权列表分别保存，只使用当前模式。指定设备授权不受租户内换池影响。空列表有效，构建会排队并说明没有合格授权设备。构建创建时冻结授权，普通调度、AI、重选、MCP 和命令投递使用冻结范围与当前项目授权的交集。撤销授权阻止新分配，已经运行的任务可继续完成。槽位通过 PostgreSQL 节点行锁跨项目、跨控制平面副本共享。

普通成员可读取获授权项目的可用设备及总槽位占用；设备详情隐藏运维诊断和 OTA 错误，其他项目的任务、日志和凭证通过项目权限隔离。Source、Telemetry 和 Runtime 通过控制平面确认的任务、设备或租约访问，不接受客户端自报的租户作为授权依据。Worker 只领取注册令牌所属租户的任务，模型密钥从该任务的租户配置获取。

## 接口

路径均以 `/api/v1` 开头。修改 Session 请求继续要求 CSRF header。

| 接口 | 用途 |
| --- | --- |
| `GET /tenants`、`POST /tenants` | 当前账号租户、幂等创建租户和默认池 |
| `GET /tenants/{slug}` | 当前账号成员可访问的租户；未知或非成员均为 404 |
| `GET /projects/by-slug/{slug}` | 当前租户内项目，继续校验项目角色和 Token 范围 |
| `GET /projects/by-slug/{slug}/builds/{number}` | 当前租户及项目内编号解析构建，继续校验读取权限 |
| `GET /tenant` | 当前租户及有效项目角色 |
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

## 协调升级与恢复

本版本改变授权模型，必须使用协调维护窗口。禁止旧版全局授权实例与新版同时接收业务请求。升级按以下顺序执行，保留已有设备身份和证书。

1. 保存当前镜像摘要、配置和密钥；安排维护窗口。暂停入口、构建提交和 OTA 发起，等待在途构建、发布、AI、Runtime 和 OTA 完成，或明确取消后确认终态。
2. 停止全部旧控制平面、Runtime、Worker 业务实例以及调度入口；维护期间不允许其他客户端写业务库。保留数据库和对象存储。先验证备份空间和 PostgreSQL 客户端版本。
3. 通过私有环境提供 `DATABASE_URL`，使用 Runtime 的安装同时提供 `ARTIFACTORY_RUNTIME_DATABASE_URL`。不要在命令参数、工单或日志中粘贴连接 URL、密码和主密钥。

   ```sh
   python3 scripts/tenant_migrate.py
   python3 scripts/tenant_migrate.py --apply --maintenance-confirmed \
     --backup-dir /secure/backups/artifactory-tenant-upgrade
   ```

   默认只检查在途任务，不应用迁移。应用模式要求新的备份目录，使用 `pg_dump` 自定义格式备份两库，检查可读取性并保存 SHA256 清单，再执行追加的 Toasty 迁移。PostgreSQL URL 的 TLS 和 options 参数映射到私有 `PG*` 环境，不进入子进程参数。工具不会替你停止服务；维护确认参数必须对应已经完成的停写操作。

4. 旧资源整体归入默认租户。最早管理员成为所有者，其他原管理员成为租户管理员；普通账号保留登录能力，但不继承旧项目权限。旧 Session 清空，普通用户 Token 撤销，管理员 Token 绑定默认租户且保留原项目范围。设备和 Worker 身份、证书以及历史项目、构建、制品关系保留；项目迁为节点池模式。旧构建已有的节点池授权快照保持原值，包括空列表；没有快照的旧构建使用维护窗口时的项目范围。
5. Runtime 按控制平面可信项目映射回填历史租户。缺失或冲突的项目映射会回滚该次回填；不要人工猜测归属。控制库迁移和 Runtime 回填不是跨库原子事务，任何步骤失败都保持维护状态，修正后幂等重试，或恢复两库同一备份组。Source、Telemetry 历史数据沿用原关联。
6. 在隔离库实际演练恢复，核对 `manifest.json` 的 SHA256；使用私有 libpq 环境指向新的恢复库：

   ```sh
   pg_restore --single-transaction --no-owner --no-acl \
     --dbname=artifactory_control_restore /secure/backups/artifactory-tenant-upgrade/control.dump
   pg_restore --single-transaction --no-owner --no-acl \
     --dbname=artifactory_runtime_restore /secure/backups/artifactory-tenant-upgrade/runtime.dump
   ```

   核对项目、设备、构建、制品数量与关联，检查迁移历史、成员和 Token。需要回退旧版本时，统一恢复升级前两库备份及旧镜像、配置，不能将旧实例指向迁移后的业务库。
7. 统一更新控制平面、Telemetry、Source、Runtime、Worker 与 Web，保持入口关闭完成健康检查。验证默认租户管理员登录和选择、普通账号无继承权限、历史制品及设备认证、Worker 归属，再恢复入口。新注册令牌必须从对应租户创建。

## 升级后验证与监控

由执行升级的管理员在恢复入口前以及恢复后至少 30 分钟检查：租户选择和邀请流程、两个租户直接 ID 访问拒绝、项目设备授权交集、Worker 任务归属、历史制品下载。检查 `tenant_required`、`cross-tenant reference`、`cross-tenant link`、`planning operation failed`、授权错误率、排队原因、过期租约和 Runtime 回填错误。健康信号是合法成员请求正常、越权请求被拒绝、已有任务正常完成、队列能消退且槽位不超分。

出现任何跨租户读取或分配、成员撤权后仍能发起新请求、历史关联丢失、持续迁移或租约错误时，立即关闭业务入口并保留日志；按上一节协调恢复，禁止通过放宽权限或混跑旧实例缓解。
