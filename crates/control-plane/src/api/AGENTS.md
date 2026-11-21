# api/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合业务路由与规划运行时前缀，保持各自的请求约束。
ai_management.rs: 提供模型配置、澄清、计划和分页对话接口。
builds.rs: 校验构建权限并提供查询与制品下载。
releases.rs: 提供版本元数据、说明编辑、独立构建创建和交付清单。
build_logs.rs: 提供日志静态分页与终态排空后结束的 SSE。
emissary_updates.rs: 提供 Emissary 升级包与升级请求接口。
lifecycle.rs: 提供工作区生命周期任务接口。
management.rs: 提供设备改名、识别摘要、凭据与设备池接口。
projects.rs: 提供项目配置与源码引用查询接口。
signing.rs: 提供租户凭据、签名方案和项目默认设置。
telemetry.rs: 提供设备与构建时序查询接口。
tenants.rs: 提供租户、成员与项目授权接口。

images.rs: 隔离构建与发布日志，仅分发已发布版本的固定标签。

artifact_gateway.rs: 将加密票据转换为网关内部对象 URL 与固定交付文件名。
registry_guard.rs: 复核网关身份、仓库 JWT 与冻结租约，授权单次正式 Manifest。

[PROTOCOL]: Update this header on change, then check AGENTS.md
