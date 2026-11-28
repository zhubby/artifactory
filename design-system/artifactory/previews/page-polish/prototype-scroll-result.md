# 原型手机正文滚动 P2 修复

完成。实现已冻结。未提交、未部署。

## 修复

只修改 design-system/artifactory/prototype/flow-audit.js:110：给 .df-audit-detail-body 增加 data-flow-scroll="audit-body-${round.id}"。复用现有按构建状态捕获与按轮次稳定键恢复机制，不增加状态容器或其他功能。

L3 已描述证据阅读职责，L2 flow-audit.js 条目已描述按轮次保留证据，L1 模块边界不变；已检查，无需文档修改。

## 验证

1. 修复前，375×812 手机正文在 scrollTop=80 后分页重绘，实际变为0。
2. 新增直接浏览器验证脚本 /tmp/artifactory-final-polish/prototype-scroll-check.cjs。375×812 × light/dark 两组中，正文scrollTop=80后分页、切换轮次再返回、切换构建计划标签再返回，均保持80。结果：prototype-evidence/mobile-scroll.json。
3. 重跑原型审计四组合：1440/375 × light/dark，4/4通过，28个异常场景通过，搜索/筛选保持证据、分页、复制成功/失败、手机焦点和Axe检查全部通过。结果：prototype-evidence/results.json。
4. node --check flow-audit.js 与 git diff --check 通过。

验证范围仅独立演示原型。未修改生产Web、包或其他文档。
