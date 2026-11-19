# api/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合 HTTP 路由、会话入口与请求约束。
ai_management.rs: 提供模型配置、澄清、计划和分页对话接口。
builds.rs: 校验构建权限，提供规划、发布与下载接口。
build_logs.rs: 提供日志静态分页与终态排空后结束的 SSE。
emissary_updates.rs: 提供 Emissary 升级包与升级请求接口。
lifecycle.rs: 提供工作区生命周期任务接口。
management.rs: 提供凭据、设备与设备池接口。
projects.rs: 提供项目配置与源码引用查询接口。
signing.rs: 提供租户凭据、签名方案和项目默认设置。
telemetry.rs: 提供设备与构建时序查询接口。
tenants.rs: 提供租户、成员与项目授权接口。

images.rs: 鉴权后确认候选，按令牌角色隔离构建与发布日志，提供固定目标发布。

artifact_gateway.rs: 将加密票据转换为网关内部对象请求。

[PROTOCOL]: Update this header on change, then check AGENTS.md
