# web/src/lib/
> L2 | Parent: ../AGENTS.md

api.ts: 发送租户与 CSRF 请求，下载附件，去重轮询错误并允许显式重试。
build-state.ts: 定义日志事件类型与构建终态判断。
errors.ts: 转换本地化错误并判定轮询错误等价。
events.ts: 订阅构建事件，维护游标并在 complete 后停止重连。
notify.ts: 隔离提示来源，统一四类操作通知，单次恢复跨整页跳转的成功文案。
preferences.ts: 定义语言、主题类型与存储键，动画没有持久化开关。
repository-refs.ts: 查询项目引用，取消过期请求并隔离迟到响应。
releases.ts: 校验严格 SemVer 和目标合同，按 SemVer 比较版本。
routes.ts: 校验工作区、版本内构建和签名设置地址，生成 Release 嵌套路由。
tenant.tsx: 提供租户作用域、导航和请求头。
types.ts: 定义 Release 目标合同、构建归属、冻结交付、应用平台与会话审计。
unsaved.tsx: 保护未保存编辑。
utils.ts: 格式化日期、摘要和制品大小。
workspace-navigation.ts: 项目业务导航以概览和 Release 为中心；统一设置分组、权限可见性和规范分区。

[PROTOCOL]: Update this header on change, then check AGENTS.md
