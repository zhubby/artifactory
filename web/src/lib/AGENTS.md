# web/src/lib/
> L2 | Parent: ../AGENTS.md

api.ts: 发送独立账户、租户与 CSRF 请求，前台计数覆盖授权解析和响应，后台轮询静默，显式重试清除错误。
build-state.ts: 定义日志事件类型与构建终态判断。
build-activity.ts: 定义过程投影及基于源码、计划和执行证据的阶段状态。
planning-audit.ts: 合并完整分片，按调用与提案身份关联结果，隔离平台提示并保留原文。
dashboard-types.ts: 定义完整窗口、成功率、周转中位数、自然日序列和近期构建活动契约。
errors.ts: 转换本地化错误并判定轮询错误等价。
events.ts: 订阅构建事件，维护游标并在 complete 后停止重连。
idempotency.ts: 使用 HTTP 可用的随机源生成 UUID v4，请求重试复用表单键。
notify.ts: 隔离提示来源，统一四类操作通知，单次恢复跨整页跳转的成功文案。
node-identity.ts: 兼容新身份与旧诊断主机名，生成去重硬件摘要；不发送额外请求。
preferences.ts: 定义语言、主题类型与存储键，动画没有持久化开关。
progress.ts: 保存前台并发计数，向请求和导航提供幂等结束函数及订阅入口。
repository-refs.ts: 查询项目引用，取消过期请求并隔离迟到响应。
build-platforms.ts: 查询项目授权原生平台，优先在线默认并隔离项目迟到响应。
releases.ts: 校验严格 SemVer，按 SemVer 比较版本。
build-targets.ts: 校验构建目标合同，组合独立源码与目标的交付候选摘要。
routes.ts: 校验独立项目目录、工作区与设置地址，生成 Release 路由；邮箱登录返回只接受确认页面和单个令牌。
tenant.tsx: 提供租户作用域和请求头，以 Next 导航状态连接顶部进度并保留未保存保护。
types.ts: 定义 Release、原生与镜像计划目标、执行修订身份、规划实时摘要、节点身份与会话审计。
unsaved.tsx: 保护未保存编辑。
utils.ts: 格式化日期、摘要和制品大小。
workspace-navigation.ts: 租户概览和项目目录使用独立地址；统一项目业务导航、设置分组和权限可见性。

notification-types.ts: 声明账户收件箱、完整授权范围、修订、偏好与外部投递合同。
notification-copy.ts: 共享通知中心、关注星标与失败重试的中英文文案。

[PROTOCOL]: Update this header on change, then check AGENTS.md
