/**
 * [INPUT]: 依赖 ui.js 图标与状态组件、展厅材质变量和生命周期信号。
 * [OUTPUT]: 导出 feedbackGroups 组件目录与 mountFeedback 本地交互入口。
 * [POS]: 组件展厅的反馈与业务分区；正文用标准材质，临时任务用玻璃。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, esc, badge, progress } from './ui.js';

const primaryActions = new Set(['notify-success','simulate-load','retry','reconnect','apply-popover','sheet-apply','coach-next','stage-next','approval-allow','digest-copy','credential-check']);
const button = (label, action = '', glyph = '', extra = '') => `<button type="button" class="cf-button${primaryActions.has(action) ? ' cf-primary' : ''}${['archive','approval-reject'].includes(action) ? ' cf-danger-action' : ''}" ${action ? `data-cf-action="${action}"` : ''} ${extra}>${glyph ? icon(glyph, 16) : ''}${label}</button>`;
const output = (text = '') => `<p class="cf-output" role="status">${text}</p>`;
const wrap = (id, html) => `<div class="cf-demo" data-cf-demo="${id}">${html}</div>`;
const item = (group, id, name, en, description, render, tags = []) => ({ id: `${group}-${id}`, name, en, description, tags: [group, ...tags], render: () => wrap(`${group}-${id}`, render()) });
const row = html => `<div class="cf-row">${html}</div>`;
const meta = (label, value) => `<div class="cf-meta"><span>${label}</span><strong>${value}</strong></div>`;
const note = (text, kind = '') => `<div class="cf-note ${kind}">${icon(kind === 'cf-warning' ? 'warn' : 'check', 18)}<span>${text}</span></div>`;
const menu = (label, entries, action = 'menu-choice') => `<details class="cf-menu"><summary>${label}${icon('down', 16)}</summary><div class="cf-menu-panel">${entries.map(([glyph, text]) => button(text, action, glyph, `data-cf-value="${esc(text)}"`)).join('')}</div></details>`;
const dialog = (id, title, body, kind = '', footer = '') => `<dialog class="cf-dialog ${kind}" id="cf-${id}" aria-labelledby="cf-${id}-title"><header><div><span class="cf-eyebrow">PRISM / FOCUSED TASK</span><h2 id="cf-${id}-title">${title}</h2></div>${button('关闭', 'close-dialog', 'close')}</header><div class="cf-dialog-body">${body}</div><footer>${footer || button('知道了', 'close-dialog', 'check')}</footer></dialog>`;
const open = (label, id, glyph = 'external') => button(label, 'open-dialog', glyph, `data-cf-target="cf-${id}" data-cl-glass="regular"`);
const art = () => `<div class="cf-art" role="img" aria-label="Prism 玻璃制品概念图"><span class="cf-art-orbit"></span><div class="cf-art-lens">${icon('box', 52)}<span>PRISM / 2.4</span></div></div>`;

const feedback = [
    item('feedback', 'alert', '提示条', 'Alert', '通过图标、标题和正文共同表达状态。', () => `<div data-cf-alert>${note('<strong>签名身份即将到期</strong><br>请在 7 天内更新项目授权。', 'cf-warning')}</div>${button('收起提示', 'toggle-alert', 'eye')}${output('可关闭的局部说明，不代替操作结果通知。')}`, ['警告', '消息']),
    item('feedback', 'notification', '顶部通知', 'Notification', '操作结果固定在顶部，不挤压页面。', () => `${row(button('成功通知', 'notify-success', 'check') + button('失败通知', 'notify-error', 'warn'))}${output('点击查看可关闭的顶部居中通知。')}`, ['通知', 'Toast']),
    item('feedback', 'message-banner', '消息横幅', 'Message Banner', '将范围变化和下一步放在同一区域。', () => `<div class="cf-banner"><div>${badge('新版本', 'neutral')}<strong>构建规划已更新</strong><span class="cf-banner-revision">REVISION 02 / 06 TARGETS</span><p>修订 2 已补全双架构镜像目标。</p></div>${button('查看变化', 'banner-details', 'arrow')}</div><p class="cf-disclosure" data-cf-detail hidden>新增 linux/arm64；签名身份与源码清单保持固定。</p>`, ['横幅']),
    item('feedback', 'spinner', '加载旋转器', 'Spinner', '短任务使用局部加载状态并防止重复触发。', () => `${button('模拟校验', 'simulate-load', 'shield')}<div class="cf-load-line"><span class="cf-spinner" aria-hidden="true" hidden></span><span data-cf-load-label>等待校验</span></div>${output()}`, ['加载']),
    item('feedback', 'skeleton', '内容骨架', 'Skeleton', '按最终信息结构预留空间。', () => `${button('切换加载状态', 'skeleton', 'refresh')}<div class="cf-skeleton-zone" aria-busy="true"><div class="cf-skeleton-row"><span class="cf-skeleton cf-skeleton-avatar"></span><span class="cf-skeleton cf-skeleton-title"></span></div><span class="cf-skeleton"></span><span class="cf-skeleton cf-skeleton-short"></span><p data-cf-loaded hidden><strong>Orbital Desktop</strong><br>构建内容已经就绪。</p></div>${output('正在显示静态骨架。')}`, ['占位', '加载']),
    item('feedback', 'loading-overlay', '局部加载层', 'Loading Overlay', '仅锁定当前任务区域，保留取消入口。', () => `${button('刷新资源', 'loading-overlay', 'refresh')}<div class="cf-overlay-demo"><span class="cf-kicker">PROJECT RESOURCES</span><strong>项目资源</strong><p>4 个可信构建节点</p><div class="cf-local-overlay" hidden><span class="cf-spinner" aria-hidden="true"></span><span>刷新演示资源</span>${button('取消', 'cancel-loading', 'close')}</div></div>${output('内容保持原位。')}`, ['刷新', '加载']),
    item('feedback', 'result', '结果页', 'Result', '用结果、原因和下一步形成完整闭环。', () => `<div class="cf-result"><span class="cf-kicker">DELIVERY VALIDATED</span><span class="cf-result-icon">${icon('check', 28)}</span><h4 data-cf-result-title>校验已通过</h4><p data-cf-result-desc>全部 6 个目标具有完整交付证据。</p></div>${button('切换结果状态', 'result', 'refresh')}`, ['成功', '失败']),
    item('feedback', 'error-state', '可恢复失败', 'Error State', '保留任务上下文，提供局部重试。', () => `<div class="cf-error-card"><span class="cf-kicker">CONNECTION INTERRUPTED</span>${icon('warn', 24)}<strong data-cf-error-title>无法读取构建记录</strong><p data-cf-error-desc>连接暂时中断。当前筛选已保留。</p>${button('重试演示', 'retry', 'refresh')}</div>${output()}`, ['重试', '错误']),
    item('feedback', 'offline-banner', '离线提示', 'Offline Banner', '标出快照时间和恢复连接动作。', () => `<div class="cf-note cf-warning" data-cf-offline>${icon('warn', 18)}<span><strong>当前离线</strong><br>正在查看 14:32 的演示快照。</span></div>${button('重新连接', 'reconnect', 'refresh')}${output('离线不会自动丢弃草稿。')}`, ['离线']),
    item('feedback', 'undo', '可撤销操作', 'Undo', '轻量移除提供明确恢复入口。', () => `<div class="cf-undo-row"><span class="cf-kicker">PROJECT LABELS</span><span data-cf-undo-item>${icon('tag', 16)} release-candidate</span>${button('移除标签', 'remove-tag', 'close')}</div><div class="cf-undo-action" hidden><span>标签已移除</span>${button('撤销', 'undo', 'arrowLeft')}</div>${output()}`, ['撤销'])
];
const overlays = [
    item('overlays', 'tooltip', '工具提示', 'Tooltip', '指针和键盘均能读取辅助说明。', () => `<div class="cf-tooltip-wrap"><button type="button" class="cf-button" aria-describedby="cf-tooltip-copy">${icon('help', 17)}签名摘要</button><span class="cf-tooltip" role="tooltip" id="cf-tooltip-copy">摘要固定制品内容；不会暴露签名私钥。</span></div><p class="cf-help">悬停或使用 Tab 聚焦按钮。</p>`, ['提示', '帮助']),
    item('overlays', 'popover', '信息浮层', 'Popover', '将轻量设置放在触发器附近。', () => `<details class="cf-menu"><summary>${icon('filter', 16)}显示偏好${icon('down', 16)}</summary><div class="cf-menu-panel cf-popover"><strong>构建列表</strong><label><input type="checkbox" checked> 显示源码提交</label><label><input type="checkbox"> 显示排队耗时</label>${button('应用偏好', 'apply-popover', 'check')}</div></details>${output('源码提交可见。')}`, ['浮层', '筛选']),
    item('overlays', 'dropdown', '操作菜单', 'Dropdown', '集中低频操作，危险动作单独分隔。', () => `${menu('项目操作', [['star', '关注项目'], ['copy', '复制项目链接'], ['folder', '归档演示项目']])}${output('展开菜单选择一项。')}`, ['菜单']),
    item('overlays', 'context-menu', '上下文菜单', 'Context Menu', '右键与可见按钮提供同一组操作。', () => `<div class="cf-context-area" tabindex="0" data-cf-context><span class="cf-file-stamp">DMG</span>${icon('file', 22)}Orbital-2.4.dmg<span>右键或 Shift + F10</span></div>${menu('打开文件菜单', [['eye', '预览制品'], ['copy', '复制摘要'], ['download', '准备演示下载']])}${output('支持可见按钮与键盘入口。')}`, ['右键', '菜单']),
    item('overlays', 'dialog', '模态对话框', 'Dialog', '单一任务阻断背景并保存焦点来源。', () => `${open('打开玻璃对话框', 'information')}${dialog('information', '交付证据已齐备', `${note('6 个目标通过校验。')}${meta('源码提交', 'e7a2c91')}${meta('构建身份', '#1047')}<p>本对话框仅展示组件交互，不触发发布。</p>`)}${output('Escape 关闭并返回触发器。')}`, ['弹窗']),
    item('overlays', 'drawer', '侧边抽屉', 'Drawer', '保留页面上下文，展示对象详情。', () => `${open('查看设备侧栏', 'device', 'monitor')}${dialog('device', 'Mac Studio', `${note('设备在线，签名身份有效。')}${meta('系统', 'macOS 15.2')}${meta('架构', 'arm64')}${meta('项目授权', 'Orbital Desktop')}<p>侧栏滚动独立于背景内容。</p>`, 'cf-drawer')}`, ['侧栏']),
    item('overlays', 'confirm-dialog', '危险操作确认', 'Confirm Dialog', '明确对象与后果，再解锁最终动作。', () => `${open('归档演示项目', 'archive', 'folder')}${dialog('archive', '归档 Orbital Desktop？', `<p>新构建入口将关闭。历史制品保持可读。</p><label class="cf-field">输入 ARCHIVE 确认<input class="input" data-cf-archive-input autocomplete="off" placeholder="ARCHIVE"></label>${output('仅改变当前组件演示。')}`, '', button('返回', 'close-dialog') + button('确认归档', 'archive', 'folder', 'disabled data-cf-confirm'))}`, ['确认', '危险']),
    item('overlays', 'sheet', '底部面板', 'Bottom Sheet', '手机上的短任务从底部进入。', () => `${open('选择交付目标', 'targets', 'layers')}${dialog('targets', '选择交付目标', `<div class="cf-choice-list">${['macOS arm64', 'Windows amd64', 'Linux amd64'].map((x, i) => `<label><input type="checkbox" ${i < 2 ? 'checked' : ''}>${x}</label>`).join('')}</div>`, 'cf-sheet', button('使用所选目标', 'sheet-apply', 'check'))}${output('默认选择 2 个目标。')}`, ['手机', '面板']),
    item('overlays', 'coachmark', '引导气泡', 'Coachmark', '分步解释当前界面，用户随时跳过。', () => `<details class="cf-menu"><summary>${icon('spark', 16)}查看三步引导</summary><div class="cf-menu-panel cf-popover"><span class="cf-eyebrow" data-cf-coach-step>01 / 03</span><strong data-cf-coach-title>连接源码</strong><p data-cf-coach-text>通过凭据引用读取仓库。</p>${row(button('跳过', 'coach-skip') + button('下一步', 'coach-next', 'arrow'))}</div></details>${output('引导不会替用户提交表单。')}`, ['引导']),
    item('overlays', 'lightbox', '聚焦预览', 'Lightbox', '放大媒体内容并保留关闭路径。', () => `${button('打开制品封面', 'open-dialog', 'eye', 'data-cf-target="cf-lightbox" data-cl-glass="regular"')}${dialog('lightbox', '制品封面预览', art() + '<p class="cf-help">概念封面 · 本地 SVG 图标与 CSS 材质。</p>', 'cf-lightbox')}`, ['预览', '图片'])
];
const delivery = [
    item('delivery', 'build-stage', '构建阶段', 'Build Stage', '阶段状态、顺序和当前动作同时可见。', () => `<span class="cf-kicker">BUILD #1048 / PIPELINE</span><ol class="cf-stages">${['源码', '规划', '设备', '构建', '校验'].map((x, i) => `<li class="${i === 0 ? 'cf-current' : ''}" ${i === 0 ? 'aria-current="step"' : ''}><span>${i + 1}</span>${x}</li>`).join('')}</ol>${button('推进演示阶段', 'stage-next', 'arrow')}${output('当前阶段：源码。')}`, ['流程', '阶段']),
    item('delivery', 'target-matrix', '目标矩阵', 'Target Matrix', '按平台展示独立目标及授权状态。', () => `<div class="cf-table-scroll"><table class="cf-table"><thead><tr><th scope="col">目标</th><th scope="col">资源</th><th scope="col">选择</th></tr></thead><tbody>${[['macOS arm64', '在线'], ['Windows amd64', '在线'], ['Linux amd64', '排队']].map(([x, y], i) => `<tr><th scope="row">${x}</th><td>${y}</td><td><input type="checkbox" data-cf-target-check aria-label="选择 ${x}" ${i < 2 ? 'checked' : ''}></td></tr>`).join('')}</tbody></table></div>${output('已选择 2 个交付目标。')}`, ['平台', '矩阵']),
    item('delivery', 'log-viewer', '日志查看器', 'Log Viewer', '等宽日志支持筛选，保留时间与级别。', () => `<div class="cf-console-caption"><span>BUILD #1047</span><span>4 EVENTS</span></div><label class="cf-field">过滤演示日志<input class="input" data-cf-log-filter placeholder="如 sign"></label><div class="cf-console" tabindex="0" aria-label="演示构建日志"><p data-cf-log-line>14:32:01 INFO source resolved</p><p data-cf-log-line>14:32:08 INFO build target ready</p><p data-cf-log-line>14:33:21 INFO sign identity verified</p><p data-cf-log-line>14:33:25 INFO sign completed</p></div>${output('4 行日志。')}`, ['日志']),
    item('delivery', 'terminal', '终端会话', 'Terminal', '明确授权和会话范围，只运行本地模拟。', () => `<div class="cf-terminal-caption">${icon('terminal', 15)}<strong>本地模拟终端</strong><span>NO CONNECTION</span></div><div class="cf-console" aria-label="本地终端演示"><p data-cf-terminal-output>PRISM DEMO / 未连接设备<br>输入 help 查看模拟命令。</p><label class="cf-terminal-line"><span aria-hidden="true">$</span><input data-cf-terminal aria-label="演示终端命令" placeholder="help" autocomplete="off" spellcheck="false"></label></div><p class="cf-help">Enter 执行；仅支持 help、pwd、uname、clear。</p>`, ['终端', '命令']),
    item('delivery', 'artifact-card', '制品卡', 'Artifact Card', '同时呈现平台、体积和校验来源。', () => `<div class="cf-object cf-artifact-object"><span class="cf-object-icon">${icon('box', 24)}</span><div><strong>Orbital-2.4.0.dmg</strong><p>macOS arm64 · 148.2 MB</p></div>${badge('已签名', 'success')}</div>${button('展开制品证据', 'artifact-evidence', 'shield')}<div class="cf-disclosure" hidden data-cf-detail>${meta('来源', '#1047 / e7a2c91')}${meta('校验', 'sha256:84fb…e9d4')}</div>`, ['制品', '证据']),
    item('delivery', 'release-gate', '发布门禁', 'Release Gate', '前置证据与不可逆动作明确分开。', () => `<span class="cf-kicker">RELEASE READINESS / 03 CHECKS</span><div class="cf-choice-list"><label><input type="checkbox" data-cf-gate> 全部目标成功</label><label><input type="checkbox" data-cf-gate> 签名与摘要完整</label><label><input type="checkbox" data-cf-gate> 已确认版本不可覆盖</label></div>${button('冻结演示交付', 'release', 'lock', 'disabled data-cf-release')}${output('完成 3 项确认后解锁。')}`, ['发布', '审批']),
    item('delivery', 'queue-card', '排队任务', 'Queue Card', '解释等待原因，保留排序与取消入口。', () => `<div class="cf-queue"><span class="cf-queue-index">03</span><div><strong>macOS amd64</strong><p>等待项目授权的 Mac mini 上线。</p></div>${badge('排队', 'warning')}</div>${button('调整为优先演示', 'queue-priority', 'arrow')}${output('原生目标保留排队，不自动换平台。')}`, ['队列']),
    item('delivery', 'dependency-graph', '依赖图', 'Dependency Graph', '图中节点支持选择和文字详情。', () => `<div class="cf-graph" aria-label="构建依赖图">${[['source', '源码', 'git'], ['build', '构建', 'cpu'], ['sign', '签名', 'shield']].map(([x, y, z], i) => `${i ? '<span class="cf-graph-edge" aria-hidden="true">→</span>' : ''}<button type="button" class="cf-graph-node ${i === 0 ? 'cf-selected' : ''}" data-cf-action="graph-select" data-cf-value="${x}" aria-pressed="${i === 0}">${icon(z, 22)}${y}</button>`).join('')}</div>${output('源码：固定提交 e7a2c91，供构建节点读取。')}`, ['依赖', '图']),
    item('delivery', 'approval-card', '审批卡', 'Approval Card', '把申请范围与决策理由放在一起。', () => `<span class="cf-kicker">PROJECT-SCOPED REQUEST</span><div class="cf-approval"><span class="cf-avatar">陈</span><div><strong>陈雨申请签名授权</strong><p>Orbital Desktop · 仅本次构建 #1048</p></div></div>${row(button('拒绝演示', 'approval-reject', 'close') + button('批准演示', 'approval-allow', 'check'))}${output('待管理员复核。')}`, ['授权', '审批']),
    item('delivery', 'digest-copy', '摘要复制', 'Digest Copy', '完整摘要可读取，复制有明确反馈。', () => `<span class="cf-kicker">CONTENT IDENTITY / SHA-256</span><label class="cf-field">SHA-256 摘要<input class="input cf-mono" readonly value="84fb26c7d18a49c0e13aa102e9d4" data-cf-digest></label>${button('复制演示摘要', 'digest-copy', 'copy')}${output('摘要用等宽字体，避免内容混淆。')}`, ['复制', '摘要'])
];
const organization = [
    item('organization', 'member-card', '成员卡', 'Member Card', '身份、角色和项目范围同时呈现。', () => `<div class="cf-object"><span class="cf-avatar">林</span><div><strong>林知远</strong><p>lin@example.test</p></div>${badge('所有者', 'neutral')}</div>${menu('查看项目范围', [['folder', 'Orbital Desktop'], ['folder', 'Atlas API']])}${output('账户成员身份与项目范围独立。')}`, ['成员']),
    item('organization', 'permission-matrix', '权限矩阵', 'Permission Matrix', '用可操作表格表达细粒度授权。', () => `<div class="cf-table-scroll"><table class="cf-table"><thead><tr><th scope="col">角色</th><th scope="col">读取</th><th scope="col">构建</th><th scope="col">发布</th></tr></thead><tbody>${[['开发者', [true, true, false]], ['观察者', [true, false, false]]].map(([role, vals]) => `<tr><th scope="row">${role}</th>${vals.map((v, i) => `<td><input type="checkbox" data-cf-permission aria-label="${role}${['读取', '构建', '发布'][i]}权限" ${v ? 'checked' : ''}></td>`).join('')}</tr>`).join('')}</tbody></table></div>${output('当前启用 3 项权限。')}`, ['权限']),
    item('organization', 'credential-reference', '凭据引用', 'Credential Reference', '只展示资源引用和授权，不展示密钥。', () => `<div class="cf-object"><span class="cf-object-icon">${icon('key', 24)}</span><div><strong>signing/orbital-macos</strong><p>Apple Developer ID · 项目授权</p></div></div>${row(button('检查演示授权', 'credential-check', 'shield') + button('展开引用详情', 'credential-detail', 'eye'))}<div data-cf-detail hidden class="cf-disclosure">引用有效期：2027-03-30。此处不存在真实证书或密钥。</div>${output('通过引用选择凭据。')}`, ['凭据', '安全']),
    item('organization', 'device-card', '设备资源卡', 'Device Card', '资源状态与授权范围共同决定可用性。', () => `<div class="cf-object"><span class="cf-object-icon">${icon('apple', 24)}</span><div><strong>Mac Studio</strong><p>macOS arm64 · 1 / 4 槽位</p></div><span data-cf-device-badge>${badge('在线', 'success')}</span></div>${progress(42, '设备内存占用 42%')}${button('切换在线状态', 'device-state', 'refresh')}${output('已授权 Orbital Desktop。')}`, ['设备']),
    item('organization', 'resource-meter', '资源仪表', 'Resource Meter', '精确数值优先于装饰，单位保持可见。', () => `<span class="cf-kicker">MAC STUDIO / LIVE DEMO</span><div class="cf-meter-head"><strong>CPU 使用率</strong><span data-cf-meter-value>68%</span></div><meter min="0" max="100" value="68" aria-label="CPU 使用率" data-cf-meter>68%</meter>${row(button('轻负载', 'meter-low', 'cpu') + button('高负载', 'meter-high', 'activity'))}${output('资源数据为固定演示值。')}`, ['资源']),
    item('organization', 'audit-event', '审计事件', 'Audit Event', '记录谁、何时、对哪个对象执行动作。', () => `<div class="cf-audit"><span class="cf-object-icon">${icon('shield', 22)}</span><div><strong>林知远更新项目授权</strong><p><time datetime="2026-11-27T14:32:00Z">14:32 UTC</time> · Orbital Desktop</p></div></div>${button('查看变更前后', 'audit-detail', 'eye')}<div data-cf-detail hidden class="cf-disclosure">签名凭据：未授权 → 已授权。<br>请求标识：req-demo-2048。</div>`, ['审计']),
    item('organization', 'activity-feed', '活动流', 'Activity Feed', '按时间串联对象变化并按类型筛选。', () => `${row(button('全部', 'activity-all', '', 'aria-pressed="true"') + button('仅构建', 'activity-build', '', 'aria-pressed="false"'))}<ol class="cf-feed"><li data-cf-feed="build"><strong>构建 #1047 成功</strong><span>2 分钟前 · 林知远</span></li><li data-cf-feed="member"><strong>陈雨加入项目</strong><span>18 分钟前 · 所有者邀请</span></li><li data-cf-feed="build"><strong>构建 #1046 开始</strong><span>35 分钟前 · 陈雨</span></li></ol>${output('显示全部活动。')}`, ['动态', '活动']),
    item('organization', 'quota-card', '配额卡', 'Quota Card', '剩余量、限制和超额后果清晰可见。', () => `<span class="cf-kicker">STORAGE / ORGANIZATION</span><div class="cf-meter-head"><strong>制品存储</strong><span data-cf-quota-value>72 / 100 GB</span></div>${progress(72, '存储配额使用率 72%')}<p data-cf-quota-desc class="cf-help">剩余 28 GB；达到限制后保留下载能力。</p>${button('预览接近上限', 'quota', 'activity')}${output()}`, ['配额']),
    item('organization', 'notification-item', '通知条目', 'Notification Item', '阅读状态和业务对象同时保留。', () => `<div class="cf-inbox-item"><span class="cf-inbox-dot" aria-hidden="true"></span><span class="cf-object-icon">${icon('bell', 22)}</span><div><strong>版本 2.4.0 等待复核</strong><p>Orbital / Orbital Desktop</p><span class="cf-help" data-cf-read-state>未读 · 2 分钟前</span></div></div>${button('标为已读', 'read-notification', 'check')}${output('账户收件箱按权限聚合。')}`, ['收件箱', '通知']),
    item('organization', 'workspace-switcher', '工作空间切换', 'Workspace Switcher', '切换工作空间时保留角色与范围提示。', () => `${menu('Orbital · 所有者', [['layers', 'Orbital · 所有者'], ['layers', 'Northstar · 开发者']], 'workspace')}${output('当前演示空间：Orbital。')}`, ['工作空间'])
];

export const feedbackGroups = [
    { id: 'feedback', label: '反馈与状态', description: '让等待、结果、异常与恢复路径清晰可见。', items: feedback },
    { id: 'overlays', label: '浮层与引导', description: '把临时任务放在正确层级，并保留焦点与返回。', items: overlays },
    { id: 'delivery', label: '构建与交付', description: '用真实对象模型组织跨平台交付证据。', items: delivery },
    { id: 'organization', label: '组织与资源', description: '呈现权限、身份、配额、设备和账户通知。', items: organization }
];

export function mountFeedback(root, signal) {
    const timers = new Map();
    const triggers = new WeakMap();
    let toast;
    const say = (demo, text) => { const el = demo.querySelector('.cf-output'); if (el) el.textContent = text; };
    const later = (key, fn, ms = 1000) => { clearTimeout(timers.get(key)); timers.set(key, setTimeout(() => { timers.delete(key); if (!signal.aborted && root.isConnected) fn(); }, ms)); };
    const notify = (text, error = false) => {
        toast?.remove();
        toast = document.createElement('div');
        toast.className = `cf-floating-notify glass ${error ? 'cf-notify-error' : ''}`;
        toast.setAttribute('role', 'status');
        toast.dataset.clGlass = 'regular';
        toast.innerHTML = `${icon(error ? 'warn' : 'check', 19)}<span>${esc(text)}</span>${button('关闭通知', 'dismiss-notify', 'close')}`;
        root.append(toast);
        if (!error) later('toast', () => toast?.remove(), 4000);
    };
    const close = d => { d.close(); triggers.get(d)?.focus({ preventScroll: true }); };
    const toggleDetail = demo => { const detail = demo.querySelector('[data-cf-detail]'); if (detail) detail.hidden = !detail.hidden; };
    const finishLoad = demo => {
        const spinner = demo.querySelector('.cf-spinner');
        if (spinner) spinner.hidden = true;
        const b = demo.querySelector('[data-cf-action="simulate-load"]');
        if (b) b.disabled = false;
        demo.querySelector('[data-cf-load-label]').textContent = '校验已完成';
        say(demo, '演示校验通过。');
    };
    root.addEventListener('click', async e => {
        const b = e.target.closest('[data-cf-action]');
        if (!b || !root.contains(b) || b.disabled) return;
        const demo = b.closest('[data-cf-demo]');
        const action = b.dataset.cfAction;
        if (action === 'dismiss-notify') { toast?.remove(); return; }
        if (!demo) return;
        const menuEl = b.closest('details');
        switch (action) {
            case 'notify-success': notify('构建校验成功，交付证据已齐备。'); break;
            case 'notify-error': notify('连接暂时中断。请检查网络后重试。', true); break;
            case 'toggle-alert': {
                const el = demo.querySelector('[data-cf-alert]'); el.hidden = !el.hidden;
                b.innerHTML = icon('eye', 16) + (el.hidden ? '恢复提示' : '收起提示');
                break;
            }
            case 'banner-details': case 'artifact-evidence': case 'credential-detail': case 'audit-detail': toggleDetail(demo); b.setAttribute('aria-expanded', String(!demo.querySelector('[data-cf-detail]').hidden)); break;
            case 'simulate-load':
                b.disabled = true; demo.querySelector('.cf-spinner').hidden = false; demo.querySelector('[data-cf-load-label]').textContent = '正在校验'; say(demo, '校验已开始。'); later(demo, () => finishLoad(demo), 900); break;
            case 'skeleton': {
                const zone = demo.querySelector('.cf-skeleton-zone'); const loading = zone.getAttribute('aria-busy') !== 'true';
                zone.setAttribute('aria-busy', String(loading)); zone.querySelectorAll('.cf-skeleton-row,.cf-skeleton').forEach(el => el.hidden = !loading); zone.querySelector('[data-cf-loaded]').hidden = loading; say(demo, loading ? '正在显示静态骨架。' : '内容已就绪。'); break;
            }
            case 'loading-overlay': {
                const zone = demo.querySelector('.cf-local-overlay'); zone.hidden = false; demo.querySelector('.cf-overlay-demo').setAttribute('aria-busy', 'true');
                later(demo, () => { zone.hidden = true; demo.querySelector('.cf-overlay-demo').setAttribute('aria-busy', 'false'); say(demo, '资源刷新完成。'); }, 1800); break;
            }
            case 'cancel-loading': clearTimeout(timers.get(demo)); demo.querySelector('.cf-local-overlay').hidden = true; demo.querySelector('.cf-overlay-demo').setAttribute('aria-busy', 'false'); demo.querySelector('[data-cf-action="loading-overlay"]').focus(); say(demo, '刷新已取消，现有内容保留。'); break;
            case 'result': {
                const failed = demo.querySelector('.cf-result').classList.toggle('cf-failed'); demo.querySelector('.cf-result .cf-kicker').textContent = failed ? 'REVIEW REQUIRED' : 'DELIVERY VALIDATED'; demo.querySelector('.cf-result-icon').innerHTML = icon(failed ? 'warn' : 'check', 28); demo.querySelector('[data-cf-result-title]').textContent = failed ? '仍有目标需要复核' : '校验已通过'; demo.querySelector('[data-cf-result-desc]').textContent = failed ? 'Windows 签名身份已过期，请更新授权后重试。' : '全部 6 个目标具有完整交付证据。'; break;
            }
            case 'retry': demo.querySelector('.cf-error-card').classList.add('cf-recovered'); demo.querySelector('.cf-error-card>.icon').outerHTML = icon('check',24); demo.querySelector('.cf-error-card .cf-kicker').textContent='CONNECTION RESTORED'; demo.querySelector('[data-cf-error-title]').textContent = '构建记录已恢复'; demo.querySelector('[data-cf-error-desc]').textContent = '已重新读取当前筛选范围。'; say(demo, '局部重试成功。'); break;
            case 'reconnect': demo.querySelector('[data-cf-offline]').classList.remove('cf-warning'); demo.querySelector('[data-cf-offline]').innerHTML = icon('check', 18) + '<span><strong>连接已恢复</strong><br>当前内容为最新演示快照。</span>'; say(demo, '演示连接已恢复。'); break;
            case 'remove-tag': demo.querySelector('[data-cf-undo-item]').hidden = true; demo.querySelector('.cf-undo-action').hidden = false; b.disabled = true; demo.querySelector('[data-cf-action="undo"]').focus(); say(demo, '标签已移除，可撤销。'); break;
            case 'undo': demo.querySelector('[data-cf-undo-item]').hidden = false; demo.querySelector('.cf-undo-action').hidden = true; demo.querySelector('[data-cf-action="remove-tag"]').disabled = false; demo.querySelector('[data-cf-action="remove-tag"]').focus(); say(demo, '标签已恢复。'); break;
            case 'menu-choice': say(demo, `已选择：${b.dataset.cfValue}。`); menuEl.open = false; menuEl.querySelector('summary').focus(); break;
            case 'workspace': menuEl.querySelector('summary').innerHTML = esc(b.dataset.cfValue) + icon('down', 16); menuEl.open = false; menuEl.querySelector('summary').focus(); say(demo, `当前演示空间：${b.dataset.cfValue.split(' · ')[0]}。`); break;
            case 'apply-popover': say(demo, `显示偏好已应用：${[...menuEl.querySelectorAll('input:checked')].map(el => el.parentElement.textContent.trim()).join('、') || '仅基础字段'}。`); menuEl.open = false; menuEl.querySelector('summary').focus(); break;
            case 'open-dialog': {
                const d = demo.querySelector(`#${b.dataset.cfTarget}`); triggers.set(d, b); d.showModal(); break;
            }
            case 'close-dialog': close(b.closest('dialog')); break;
            case 'archive': close(b.closest('dialog')); say(demo, '演示项目已归档。历史制品仍可读取。'); notify('归档演示已完成。'); break;
            case 'sheet-apply': { const d = b.closest('dialog'); say(demo, `已选择 ${d.querySelectorAll('input:checked').length} 个交付目标。`); close(d); break; }
            case 'coach-skip': menuEl.open = false; menuEl.querySelector('summary').focus(); say(demo, '引导已跳过。'); break;
            case 'coach-next': {
                const step = (Number(menuEl.dataset.step || 0) + 1) % 3; menuEl.dataset.step = step;
                const titles = ['连接源码', '授权资源', '开始交付'], texts = ['通过凭据引用读取仓库。', '选择项目授权的原生设备与镜像构建池。', '复核源码、平台和证据后创建构建。'];
                menuEl.querySelector('[data-cf-coach-step]').textContent = `0${step + 1} / 03`; menuEl.querySelector('[data-cf-coach-title]').textContent = titles[step]; menuEl.querySelector('[data-cf-coach-text]').textContent = texts[step]; break;
            }
            case 'stage-next': {
                const steps = [...demo.querySelectorAll('.cf-stages li')]; const n = (Number(demo.dataset.stage || 0) + 1) % steps.length; demo.dataset.stage = n;
                steps.forEach((el, i) => { el.classList.toggle('cf-current', i === n); el.classList.toggle('cf-complete', i < n); if (i === n) el.setAttribute('aria-current', 'step'); else el.removeAttribute('aria-current'); }); say(demo, `当前阶段：${['源码', '规划', '设备', '构建', '校验'][n]}。`); break;
            }
            case 'release': b.disabled = true; demo.querySelectorAll('[data-cf-gate]').forEach(el => el.disabled = true); say(demo, '演示交付已冻结，版本内容不可覆盖。'); notify('演示版本已冻结。'); break;
            case 'queue-priority': demo.querySelector('.cf-queue-index').textContent = demo.querySelector('.cf-queue-index').textContent === '01' ? '03' : '01'; say(demo, '仅调整当前演示队列顺序，设备条件仍需满足。'); break;
            case 'graph-select': demo.querySelectorAll('.cf-graph-node').forEach(el => { el.classList.toggle('cf-selected', el === b); el.setAttribute('aria-pressed', String(el === b)); }); say(demo, ({ source: '源码：固定提交 e7a2c91，供构建节点读取。', build: '构建：读取固定源码，输出隔离候选制品。', sign: '签名：读取已校验制品，输出签名证据。' })[b.dataset.cfValue]); break;
            case 'approval-allow': case 'approval-reject': say(demo, action === 'approval-allow' ? '已批准本次演示申请，不扩大项目范围。' : '已拒绝本次演示申请。'); break;
            case 'digest-copy': {
                const input = demo.querySelector('[data-cf-digest]');
                try { await navigator.clipboard.writeText(input.value); if (!signal.aborted) say(demo, '演示摘要已复制。'); }
                catch { input.focus(); input.select(); say(demo, '摘要已选中，请使用系统复制快捷键。'); }
                break;
            }
            case 'credential-check': say(demo, '演示授权有效：仅供 Orbital Desktop 使用。'); break;
            case 'device-state': {
                const off = demo.dataset.offline !== 'true'; demo.dataset.offline = String(off); demo.querySelector('[data-cf-device-badge]').innerHTML = badge(off ? '离线' : '在线', off ? 'neutral' : 'success'); say(demo, off ? '设备离线。已有目标保留排队。' : '设备在线。可接受已授权任务。'); break;
            }
            case 'meter-low': case 'meter-high': {
                const value = action === 'meter-low' ? 24 : 91; demo.querySelector('[data-cf-meter]').value = value; demo.querySelector('[data-cf-meter]').textContent = `${value}%`; demo.querySelector('[data-cf-meter-value]').textContent = `${value}%`; say(demo, `CPU 演示使用率：${value}%。`); break;
            }
            case 'activity-all': case 'activity-build': demo.querySelectorAll('[data-cf-action^=activity-]').forEach(el=>el.setAttribute('aria-pressed', String(el===b))); demo.querySelectorAll('[data-cf-feed]').forEach(el => el.hidden = action === 'activity-build' && el.dataset.cfFeed !== 'build'); say(demo, action === 'activity-all' ? '显示全部活动。' : '仅显示构建活动。'); break;
            case 'quota': {
                const high = demo.dataset.high !== 'true', value = high ? 96 : 72; demo.dataset.high = String(high); demo.querySelector('[data-cf-quota-value]').textContent = `${value} / 100 GB`; demo.querySelector('.progress-track').setAttribute('aria-valuenow', value); demo.querySelector('.progress-track').setAttribute('aria-label', `存储配额使用率 ${value}%`); demo.querySelector('.progress-track > span').style.width = `${value}%`; demo.querySelector('[data-cf-quota-desc]').textContent = `剩余 ${100 - value} GB；达到限制后保留下载能力。`; say(demo, high ? '接近配额上限，请规划容量。' : '存储配额正常。'); break;
            }
            case 'read-notification': {
                const read = demo.dataset.read !== 'true'; demo.dataset.read = String(read); demo.querySelector('.cf-inbox-dot').hidden = read; demo.querySelector('[data-cf-read-state]').textContent = `${read ? '已读' : '未读'} · 2 分钟前`; b.innerHTML = icon(read ? 'bell' : 'check', 16) + (read ? '标为未读' : '标为已读'); say(demo, read ? '通知已标为已读。' : '通知已标为未读。'); break;
            }
        }
    }, { signal });
    root.addEventListener('input', e => {
        const demo = e.target.closest('[data-cf-demo]'); if (!demo) return;
        if (e.target.matches('[data-cf-archive-input]')) demo.querySelector('[data-cf-confirm]').disabled = e.target.value !== 'ARCHIVE';
        if (e.target.matches('[data-cf-log-filter]')) {
            const q = e.target.value.toLowerCase(); const lines = [...demo.querySelectorAll('[data-cf-log-line]')]; lines.forEach(el => el.hidden = !el.textContent.toLowerCase().includes(q)); say(demo, `${lines.filter(el => !el.hidden).length} 行日志。`);
        }
    }, { signal });
    root.addEventListener('change', e => {
        const demo = e.target.closest('[data-cf-demo]'); if (!demo) return;
        if (e.target.matches('[data-cf-gate]')) demo.querySelector('[data-cf-release]').disabled = demo.querySelectorAll('[data-cf-gate]:checked').length !== 3;
        if (e.target.matches('[data-cf-target-check]')) say(demo, `已选择 ${demo.querySelectorAll('[data-cf-target-check]:checked').length} 个交付目标。`);
        if (e.target.matches('[data-cf-permission]')) say(demo, `当前启用 ${demo.querySelectorAll('[data-cf-permission]:checked').length} 项权限。`);
    }, { signal });
    const contextMenu = target => { const demo = target.closest('[data-cf-demo]'), menuEl = demo.querySelector('.cf-menu'); menuEl.open = true; menuEl.querySelector('button').focus(); };
    root.addEventListener('contextmenu', e => { const el = e.target.closest('[data-cf-context]'); if (el) { e.preventDefault(); contextMenu(el); } }, { signal });
    root.addEventListener('keydown', e => {
        const demo = e.target.closest('[data-cf-demo]'); if (!demo) return;
        if ((e.shiftKey && e.key === 'F10') || e.key === 'ContextMenu') { const el = e.target.closest('[data-cf-context]'); if (el) { e.preventDefault(); contextMenu(el); } }
        if (e.key === 'Escape') { const menuEl = e.target.closest('.cf-menu[open]'); if (menuEl) { menuEl.open = false; menuEl.querySelector('summary').focus(); } }
        if (e.target.matches('[data-cf-terminal]') && e.key === 'Enter') {
            e.preventDefault(); const value = e.target.value.trim(); const out = demo.querySelector('[data-cf-terminal-output]');
            const commands = { help: 'help · pwd · uname · clear。所有命令均为本地模拟。', pwd: '/demo/orbital-desktop', uname: 'Darwin demo-mac arm64 (simulated)' };
            if (value === 'clear') out.textContent = ''; else { const line = document.createElement('span'); line.textContent = `$ ${value}\n${commands[value] || '未识别的模拟命令。输入 help 查看帮助。'}\n`; out.append(line); }
            e.target.value = '';
        }
        const menuEl = e.target.closest('.cf-menu[open]');
        if (menuEl && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) {
            const buttons = [...menuEl.querySelectorAll('button:not(:disabled)')]; if (!buttons.length) return; e.preventDefault(); const n = buttons.indexOf(document.activeElement); const next = e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (n + (e.key === 'ArrowUp' ? -1 : 1) + buttons.length) % buttons.length; buttons[next].focus();
        }
    }, { signal });
    root.querySelectorAll('.cf-dialog').forEach(d => {
        d.addEventListener('close', () => triggers.get(d)?.focus({ preventScroll: true }), { signal });
        d.addEventListener('click', e => { if (e.target !== d) return; const r = d.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) close(d); }, { signal });
    });
    signal.addEventListener('abort', () => { timers.forEach(clearTimeout); timers.clear(); toast?.remove(); root.querySelectorAll('.cf-dialog[open]').forEach(d => d.close()); }, { once: true });
}
