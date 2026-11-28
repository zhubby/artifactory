# 最终只读代码审查

已完成。未修改代码，未提交、未部署。

范围：当前 diff 的 flow-audit.js、flow-interactions.js、audit.css、image-credentials.tsx、emissary-ota.tsx、auth-shell.tsx，并核对调用方、样式和测试。

## 需修复

### [P2] 手机原始证据阅读位置在分页后丢失

位置：design-system/artifactory/prototype/flow-audit.js:110；design-system/artifactory/prototype/audit.css:111。

手机断点把 .df-audit-detail-body 设为 max-height:min(580px,70dvh);overflow:auto，但该节点没有 data-flow-scroll。flow-interactions.js:19-32 只捕获和恢复带 data-flow-scroll 的容器；外层 .df-inspector 的滚动值不能代表手机内层正文。点击“加载下一片记录”经过 flow-interactions.js:130 和 :165 重绘后，证据正文从原阅读位置跳回顶部。切换轮次或构建标签也走相同重绘路径。

实际复现：Chromium，375×812，打开 #build/1048 → AI 审计 → 读取构建配置 → 原始记录。正文 clientHeight=568、scrollHeight=670。设 scrollTop=80 后点击“加载下一片记录”，scrollTop 变为0。

建议复用现有捕获机制，为手机正文增加稳定 data-flow-scroll 键，或取消内层独立滚动，仅使用已有 .df-inspector 滚动。回归应直接断言手机 .df-audit-detail-body 的 scrollTop。

## 其余审查结论

- 未发现指定生产 Web 改动的新增正确性或权限阻断。
- ImageCredentials 保留 editable 权限门、原 mutation 路径、SubmitForm 秘密清空、轮询草稿和首读错误重试。
- AuthShell 使用既有 AccountSlot，在 AccountProvider 下移动单实例账户宿主；未新增认证或授权路径。
- EmissarySettings 的空态判断区分未读取和空列表；具名 tabIndex=0 区域保留历史操作的 admin 条件。
- 原型搜索和筛选直接更新记录 hidden，不重建证据；复制异常进入错误 notify；缺失/分片证据不生成缺失内容。
- 没有生产部署阻断；上述原型 P2 应在本轮原型交付前修复。

## 验证边界

本次审查只读生产代码与现有测试，未重复运行生产 Web 全套测试。补充运行了上述手机滚动复现，结果为80→0。此前四组审计、Axe、辅助偏好和桌面阅读位置结果存放 prototype-evidence/，其中桌面 .df-inspector 的阅读验证未覆盖手机内层正文；不能用该结果声称手机正文滚动已验证。
