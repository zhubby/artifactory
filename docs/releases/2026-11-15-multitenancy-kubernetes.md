# 多租户 Kubernetes 升级记录

日期：2026-11-15，Asia/Shanghai。本文记录现有内网试部署的协调升级；目标为 Kubernetes `1.28.3`、`artifactory` 命名空间和核心 Helm Chart。

## 修复与源码

- `bdda235`：交付多租户实现，并修复编码 UUID 绕过租户检查、项目管理员保留凭证重定向仓库、事务内再次借用连接造成连接池耗尽三个审查问题。
- `fa5c3db0302c912e172315a3696538a037f8f99b`：补充只发布 `sha-<commit>` 标签的部署镜像工作流，也是本次运行镜像的源码提交。
- `d435822`：修正 CI 性能夹具，创建默认租户并显式授权设备池。修改只在 `#[cfg(test)]` 模块中，不改变已部署的生产二进制。先在真实隔离 PostgreSQL 复现失败，修复后控制平面库测试含性能用例共 14 项通过，fmt 和 Clippy 通过。
- `de0240d`：为 CI 数据库回归步骤生成独立随机 Runtime 服务令牌；两个原先缺少令牌的 Runtime 用例按同样方式重跑通过，actionlint 通过。令牌仅存在于该测试步骤的进程环境中。

三项审查回归先失败后通过，完整源码检查结果见[实施与审查修复](../plans/2026-11-15-multitenancy.md)。本次没有移动 `v0.2.0` 标签、覆盖版本镜像或重写既有发布资产。

## 镜像身份

[deployment-images 运行 43658613503](https://github.com/zhubby/artifactory/actions/runs/43658613503) 全部成功。三个镜像均核对 GHCR 与拉取端点的索引、amd64 manifest、config 摘要及 OCI 源码提交；集群按以下不可变索引摘要部署。

| 组件 | 索引摘要 |
| --- | --- |
| Control Plane / Source / Telemetry | `sha256:638202392fd6f8209a2b70a70564224b8ce0c4ad7023bb6ffe95f302fae70b45` |
| Agent Worker | `sha256:d3eb256758e37ba454753a6a6ee1dc760e1d4e092646f67ba6ff295c0eb65895` |
| Web | `sha256:44af1ad51c6a2d653f54fbeb442d379ca4ba534e8306a582c842c38992a601ba` |

在维护前通过命名空间内的九个临时 Pod，完成三个镜像在三个工作节点的实际拉取并比对 image ID；验收后删除临时 Pod。运行中的 Pod 另行核对镜像身份。

## 协调迁移

升级前确认只有一个既有管理员、三台设备和一个 Worker，项目、构建、制品及发布均为空，也没有构建、发布、规划、有效执行租约或 OTA 在途任务。该安装未配置 Runtime，不存在生产 Runtime 回填步骤。

1. 08:44 CST 开始维护，将 Web、Worker、Control Plane、Telemetry、Source 缩容至零并等待旧业务 Pod 退出；PostgreSQL 和对象存储保持运行。
2. 在停写状态重新检查在途任务，执行 `tenant_migrate.py --apply --maintenance-confirmed`。生成真实 `pg_dump`、可读性检查和分块 SHA256 manifest，然后应用追加迁移。
3. 在独立临时数据库执行 TimescaleDB 恢复前处理、`pg_restore --single-transaction --no-owner --no-acl` 和恢复后处理。核对恢复的是旧 schema、业务表数量与迁移前一致、备份摘要一致，随后删除临时恢复库。
4. 启动新镜像的三个 Control Plane 副本及 Source、Telemetry、Worker，保持 Web 关闭，完成内部及权限隔离验收。
5. Helm 升级至 revision `3`、状态 `deployed`，恢复 Web。08:50 CST 确认控制台和匿名文档返回 200，开始恢复入口后的连续观察。

本次升级没有使用 Helm `--atomic`：已迁移数据库不能接回旧二进制。需要恢复时，应按[多租户迁移与恢复步骤](../multitenancy.md)同时恢复匹配的数据库和应用版本。

备份、恢复日志、校验文件和私有配置均保存在本机受限目录；没有将数据库连接串、登录凭据、会话、Token 或 PKI 写入 Git。

## 现场验收

| 范围 | 结果 |
| --- | --- |
| 控制平面 | 3/3 Ready，分布在三个不同工作节点；逐副本 readiness、同一登录 Session、带租户上下文的设备查询均返回 200 |
| 业务身份 | 原管理员成为 Default 租户所有者；三台设备和一个 Worker 的 ID 与证书摘要保留，全部在线 |
| 升级会话 | 迁移前 Session 返回 401；重新登录后的用户身份不再暴露全局角色 |
| 租户上下文 | 缺少上下文返回 `400 tenant_required`；新注册账号不继承既有租户或项目权限 |
| 跨租户资源 | 普通与百分号编码的设备／模型 ID 都受租户隔离；同租户合法编码 ID 可正常使用 |
| 成员与邀请 | 租户创建幂等、默认设备池、单次邀请、显式项目授权通过；移除成员立即撤销现有 Session 和 Token 的访问 |
| 凭证保护 | 项目管理员可以改项目名称；保留已有凭证重定向仓库返回 403，租户所有者操作成功 |
| 临时验收数据 | 临时用户、租户、项目、凭证、模型、邀请和 Token 已清理，保留原有业务身份 |
| 内部 TLS | 使用私有 CA 校验 Source `/readyz` 和带令牌的 Telemetry `/metrics`，均返回 200 |
| 设备遥测 | 三台既有设备的最新主机采样均为 `fresh: true`，迁移后的真实采样持续入库，清单查询正常 |
| 真实浏览器 | 五组检查通过：匿名文档及认证／CSRF边界、登录后租户选择、Default 中原有设备显示、手机英文深色无横向溢出、退出后 Session 失效；没有拦截或模拟 API |
| 资源 | 7 个 Deployment 加既有的 1 个 Gateway StatefulSet，10 个 Pod Ready；4 个 PVC 全部 Bound，UID 不变；凭据及 PKI Secret 内容不变 |
| 非目标审计 | 139 项既有非目标资源的 UID 与选定规格摘要均未变化；范围不包含集群所有对象或运行状态 |
| 连续观察 | 08:50:52—09:20:52 CST，完整 30 分钟、31 次采样全部通过；控制平面持续 3/3、10 个 Pod Ready、3 台设备和 1 个 Worker 在线，容器重启为零 |

连续观察每分钟读取工作负载、容器重启次数、认证 API、设备和 Worker 在线状态及文档路由；仅执行读取，不向现有设备发送构建任务。详细带时间样本、截图、资源与镜像核对保存在本机忽略目录 `.local/k8s-multitenancy-20261115/`。

## 边界与 CI

三个副本是应用控制平面；PostgreSQL、S3、Source、Telemetry 和 Worker 仍为单副本。未做万台连接压测、节点故障注入、真实模型调用或现有设备构建。本次生产库没有历史构建或制品，因此历史制品下载检查不适用；隔离环境中的迁移与恢复关系验证另有记录。

原镜像源码的 [platform-contracts 运行 43658597728](https://github.com/zhubby/artifactory/actions/runs/43658597728) 中，版本契约、Compose TLS 和六个平台的 native-emissary job 成功；Linux 暴露上述性能夹具遗漏，已在 `d435822` 修正。

[修复后的运行 43660554652](https://github.com/zhubby/artifactory/actions/runs/43660554652) 中，性能用例及控制平面库测试通过，六个平台的 native-emissary job 成功。Linux 数据库回归为 75 项通过、3 项失败：既有 OpenSSH fixture 被 `/tmp` ownership / modes 检查拒绝，以及两个 Runtime 用例缺少服务令牌。后两项已在 `de0240d` 补齐 CI 环境并通过针对性重跑。Kubernetes job 仍在 kind 初始化时因旧版工具生成的 kubeadm `v1beta3` 配置不兼容而失败，没有修改目标集群配置绕过此问题。完整 CI 不能表述为全部通过。

[`de0240d` 的运行 43661749621](https://github.com/zhubby/artifactory/actions/runs/43661749621) 已在远端确认两个 Runtime 用例均通过，性能用例和控制平面库测试 14 项通过；控制库回归为 77 项通过，仅上述既有 SSH fixture 失败。本次三项审查修复、性能夹具及 Runtime 测试环境问题均有通过证据。30 分钟观察结束后再次核对运行镜像、业务数量、PVC、Secret 和 139 项非目标资源，结果保持一致。
