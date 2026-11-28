# AI 审计文档复核

已按当前 planning-conversations.tsx 与 planning-audit-content.tsx 同步以下文档：

- docs/frontend-experience.md：补充已加载记录、工具与提案、待关注动作计数；搜索范围；四类筛选；复制原始记录的统一反馈；手机查看详情与返回列表的滚动和焦点行为。
- docs/design-system.md：移除“审计默认折叠”的过期说明，定义独立标签、Standard 阅读表面、轮次导航、已加载范围、证据保留与复制合同。
- docs/AGENTS.md：同步两份文档的职责。两份正文的 L3 合同已更新。

核验：

- 对照真实组件逐条核对计数来源、搜索字段、异常判定、分页保留、原始复制、notify 成败反馈、手机 scroll/focus。
- git diff --check 通过。
- L3/L2 与 800 行约束通过。
- 保留带日期的历史验收记录，不把本次文档核对表述为浏览器运行验收。

范围：仅修改以上三份文档；未提交、未部署、未修改业务实现。
