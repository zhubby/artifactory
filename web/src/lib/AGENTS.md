# web/src/lib/
> L2 | Parent: ../AGENTS.md

api.ts: 发送带租户和 CSRF 边界的请求并维护可停止轮询的查询。
build-state.ts: 定义日志事件类型与构建终态判断。
errors.ts: 转换失败为本地化错误。
events.ts: 订阅构建事件，维护游标并在 complete 后停止重连。
preferences.ts: 定义语言和视觉偏好的存储键。
repository-refs.ts: 查询项目引用，取消过期请求并隔离迟到响应。
routes.ts: 校验工作区页面和签名设置地址。
tenant.tsx: 提供租户作用域、导航和请求头。
types.ts: 定义公开领域数据和签名元数据。
unsaved.tsx: 保护未保存编辑。
utils.ts: 格式化日期、摘要和制品大小。

[PROTOCOL]: Update this header on change, then check AGENTS.md
