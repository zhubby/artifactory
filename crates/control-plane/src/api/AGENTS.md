# api/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合 HTTP 路由、会话入口与请求约束。
ai_management.rs: 提供模型配置与 AI Worker 接口。
builds.rs: 校验构建权限，提供规划、发布、下载与事件接口。
emissary_updates.rs: 提供 Emissary 升级包与升级请求接口。
lifecycle.rs: 提供工作区生命周期任务接口。
management.rs: 提供凭据、设备与设备池接口。
projects.rs: 提供项目配置与源码引用查询接口。
source_http.rs: 向规划任务和执行节点提供只读源码内容。
telemetry.rs: 提供设备与构建时序查询接口。
tenants.rs: 提供租户、成员与项目授权接口。

[PROTOCOL]: Update this header on change, then check AGENTS.md
