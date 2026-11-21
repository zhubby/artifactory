<!--
[INPUT]: 固定 main、CI 镜像、数据库摘要和真实页面回执。
[OUTPUT]: 记录 Release 解耦上线、配置保留与验证边界。
[POS]: 版本元数据与独立构建输入的正式部署记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Release 与构建解耦 Kubernetes 上线记录

日期：2026-11-21，Asia/Shanghai。
实现已合并 main、推送并部署。

## 实现范围

Release 的业务输入只有 SemVer 与版本说明。
版本创建后不可修改。
版本说明在各发布状态下均可修改。
说明默认空字符串，限制为 16000 个 UTF-8 字节。

每次构建独立提交源码引用、构建要求和目标。
分支与 tag 在发起构建时选择。
构建固定项目仓库与凭据引用的快照。
每次新构建创建独立源码任务。
已有构建的重试保留原源码清单。

正式发布选择一个完整成功的构建。
交付清单使用 schema 2，不包含可变版本说明。
数据库约束拒绝混入同 Release 的其他构建制品。
业务合同见 [Release 与版本交付](../release-delivery.md)。

## 固定源码与镜像

| 身份 | 固定值 |
| --- | --- |
| 实现提交 | `6dea90726b61260f9829b9beafee5111b4eff8e1` |
| 版本 | `0.2.0` |
| 源码归档 SHA-256 | `d5739d0934ea2db551f9564f66790bc38ecf8135d89cc14ed7c50cdd7f4be39b` |
| 镜像标签 | `git-6dea90726b61260f` |
| Control 与 Telemetry 摘要 | `sha256:c4b242f624e1b9b6201e30561acdaf3f3a791d8fcdd60492e6421caf6c3450ff` |
| Web 摘要 | `sha256:1c15a8ea0b73543fb52ee5db43f9d0ef4c5705b554c68682ddb7df8db26e81c9` |

镜像来自 [GitHub Actions 44858247262](https://github.com/zhubby/artifactory/actions/runs/44858247262)。
Control 和 Web 的镜像任务均成功。
纯净提交的 linux/amd64 镜像保摘要复制至 Harbor。
源仓库为 `ghcr.io/zhubby/artifactory-<component>`。
部署仓库为 `registry.kubeease.cn/pre-release/artifactory-<component>`。
源码、版本与平台标签均已核对。
匿名读回摘要与源镜像一致。
Telemetry 使用同一个 Control 镜像。

同次 CI 的 Planning Worker 在既有镜像探测阶段失败。
失败任务重试后仍在该阶段退出。
本轮保留该组件和其余执行组件的原固定镜像。
误触发的旧提交镜像任务已取消，未用于集群。

## 数据库与身份保留

应用的业务基线从 `2026112001` 更新为 `2026112101`。
新程序拒绝旧基线，不提供旧 Release 自动迁移。
现场没有 Release、Build、源码清单或交付记录。
现场存在一个项目、三个节点和两条已完成的引用查询缓存。
缓存没有关联 Release 或 Build。

本轮创建独立业务数据库 `artifactory_release_20261121`。
固定 Control 镜像通过 `migrate` 命令初始化新基线。
初始化 Job 不创建管理员，不启动调度服务。
旧业务数据库 `artifactory` 完整保留。
规划数据库、遥测数据库和对象存储保持原值。

切换前停止 Web、Control 和 Telemetry。
确认所有服务 Pod 停止后保存一致备份。
复制既有账户、租户、项目、凭据和节点数据。
引用查询缓存按新列投影复制，仅移除空的 `release_id`。
58 张业务表的数据摘要全部一致。
全部序列一致，114 项外键检查通过。
迁移身份表保留新基线。

业务 Secret 只修改两个业务连接 URL 的数据库名。
Secret 的 UID 与其他键值均保留。
新库沿用原遥测只读角色和最小表授权。
旧库备份与私有凭据不进入 Git。
本轮复制只适用于上述无 Release 历史的现场。

## Kubernetes 结果

命名空间与 Helm Release 均为 `artifactory`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
入口为 [Artifactory 控制台](http://10.169.1.239:30820)。
Helm revision 从 11 升至 12，状态为 deployed。

模板与服务端预演只改变三个 Deployment 的镜像字段。
更新范围是 Control、Telemetry 和 Web。
最终 Helm values 与 manifest 均符合预览。
九项 Deployment 全部就绪。
三个 Control、一个 Telemetry 和一个 Web Pod 的摘要符合回执。
新 Pod 重启次数均为零。
其他六项 Deployment 的 spec 与 UID 保持一致。
应用 Secret、PVC、CA 和设备身份均保留。
临时初始化 Job 与 Secret 已删除。

## 验证结果与边界

| 验证 | 结果 |
| --- | --- |
| 合并后 Rust 工作区默认测试 | 通过；需环境的 ignored 测试另行验证 |
| Control 隔离数据库单元测试 | 67 项通过 |
| 业务数据库集成测试 | 115 项通过；设备负载项未运行 |
| 基线拒绝与新身份测试 | 2 项通过 |
| Control Clippy | 通过，禁止 warnings |
| Web 类型检查与生产构建 | 通过 |
| 三视口构建与源码边界回归 | 81 项通过 |
| 真实健康、登录和静态文档 | HTTP 200，通过 |
| 真实 Release 创建与说明修改 | 通过 |
| 旧字段、修改版本与缺失构建引用 | 接口拒绝，通过 |
| 真实页面 1440、820、390px | 通过，无横向溢出或浏览器错误 |
| 配置摘要与节点 ID 保留 | 通过 |

真实页面验证使用当前生产镜像。
Release 创建表单只含版本和说明。
构建弹窗独立展示源码引用、构建要求和目标。
源码选项使用固定夹具，不请求外部仓库。
验收没有提交构建，也没有执行正式交付。
本轮临时项目通过正式生命周期接口精确删除。
原项目列表恢复，验收会话均已退出。
最终保留一个用户、两个租户、一个项目和三个节点。

私有回执、备份和截图位于 `.deploy/release-decoupling-20261121/`。
本记录在镜像生成后编写，不包含在上述镜像内。

[PROTOCOL]: Update this header on change, then check AGENTS.md
