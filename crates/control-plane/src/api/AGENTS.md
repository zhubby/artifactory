# api/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合统一版本、手动升级与业务路由，保持各自请求约束。
ai_management.rs: 提供模型兼容校验、澄清与受权规划运行摘要查询。
builds.rs: 校验构建权限，提供过程、构建查询与制品下载。
dashboard.rs: 校验概览范围与会话权限，返回完整窗口的租户和项目统计。
releases.rs: 提供版本元数据、说明编辑、独立构建创建和交付清单。
build_logs.rs: 提供日志静态分页与终态排空后结束的 SSE。
emissary_updates.rs: 提供产品统一版本、手动升级、签名目录与升级策略接口。
lifecycle.rs: 提供工作区生命周期任务接口。
management.rs: 提供设备改名、识别摘要、凭据与设备池接口。
node_installation.rs: 配置系统设备地址，固定同版本可信发行，生成有界安装材料与脚本。
projects.rs: 提供项目配置、授权原生平台与源码引用查询接口。
signing.rs: 提供租户凭据、签名方案和项目默认设置。
telemetry.rs: 提供设备与构建时序查询接口。
tenants.rs: 提供租户、成员与项目授权接口。
terminals.rs: 校验设备终端票据和同源 WebSocket，以关闭码区分撤权、终态和内部短断。

images.rs: 隔离构建与发布日志，仅分发已发布版本的固定标签。

artifact_gateway.rs: 将加密票据转换为网关内部对象 URL 与固定交付文件名。
registry_guard.rs: 复核网关身份、仓库 JWT 与冻结租约，授权单次正式 Manifest。

notification_channels.rs: 提供个人邮件验证、退订与管理员 Webhook 诊断接口。
notifications.rs: 提供账户收件箱、完整筛选范围、版本安全已读和 SSE 失效信号。
notification_health.rs: 提供租户管理员健康摘要和无身份标签的 Prometheus 指标。

[PROTOCOL]: Update this header on change, then check AGENTS.md
