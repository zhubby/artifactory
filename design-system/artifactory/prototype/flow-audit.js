/**
 * [INPUT]: 依赖共享内容组件与按构建、轮次隔离的本地样例。
 * [OUTPUT]: 导出已加载记录筛选、证据阅读、原文复制和异常场景。
 * [POS]: 构建详情只读审计层；不执行工具或读取真实模型会话。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';

export const auditRounds = [
  { id: 'initial/1', title: '首次规划', purpose: 'initial', model: '项目配置模型', status: '已完成', time: '14:23:48', ended: '14:24:02', run: 'run_demo_initial', generation: 1 },
  { id: 'clarification/1', title: '澄清后规划', purpose: 'clarification', model: '项目配置模型', status: '已完成', time: '14:24:08', ended: '14:24:32', run: 'run_demo_clarification', generation: 1 },
  { id: 'revision/2', title: '失败修订 1 · 第 2 代', purpose: 'revision', model: '项目配置模型', status: '已完成', time: '14:25:10', ended: '14:25:41', run: 'run_demo_revision', generation: 2 },
];
export const auditRecords = [
  { id: 'read-tree', label: '读取源码目录', kind: '工具调用', subject: 'repository.list_tree', call: 'call_demo_tree', outcome: '已完成', parameters: { path: '.', depth: 2 }, result: { entries: ['Cargo.toml', 'src-tauri/', 'package.json', 'Dockerfile'], truncated: false } },
  { id: 'read-file', label: '读取构建配置', kind: '工具调用', subject: 'repository.read_file', call: 'call_demo_config', outcome: '已完成', parameters: { path: 'src-tauri/tauri.conf.json', start_line: 1, end_line: 48 }, result: { version: '2.4.0', bundle: { active: true, targets: ['dmg', 'nsis', 'appimage'] } } },
  { id: 'proposal', label: '提交构建计划', kind: '结构化方案', subject: 'plan.propose · revision 2', call: 'call_demo_plan', outcome: '已接受', parameters: { source_manifest: 'src_demo_e7a2c91', targets: 6 }, result: { native_targets: 4, image_platforms: ['linux/amd64', 'linux/arm64'], validation: 'passed' } },
  { id: 'check', label: '验证签名引用', kind: '工具调用', subject: 'credentials.inspect_reference', call: 'call_demo_credential', outcome: '已完成', parameters: { reference: 'signing/orbital-macos' }, result: { available: true, secret_material: 'not returned', scope: 'project' } },
  { id: 'summary', label: '模型消息', kind: '模型正文', subject: '固定源码与全部选定目标', outcome: '已完成', parameters: {}, result: { summary: '使用锁定依赖执行原生构建。签名凭据仅通过引用传入。镜像只写入候选，正式标签由 Release 发布。' } },
];
const filters = [['all', '全部动作'], ['tools', '工具与提案'], ['messages', '消息正文'], ['attention', '需要留意']];
const attention = record => ['已失败', '已拒绝', '记录缺失', '消息分片', '需要回答'].includes(record.outcome);
const isTool = record => ['工具调用', '结构化方案', '缺失记录'].includes(record.kind);
const button = (label, action, value = '', selected = false, extra = '') => `<button type="button" class="df-choice ${selected ? 'selected' : ''}" data-flow-action="${action}" data-flow-value="${U.esc(value)}" aria-pressed="${selected}" ${extra}>${label}</button>`;
const action = (label, name, glyph = '') => U.btn(label, '', 'secondary', glyph, `data-flow-action="${name}"`);
function recordsForRound(round) {
  if (round.purpose === 'revision') return auditRecords;
  if (round.purpose === 'clarification') return [
    { id: 'answer', label: '读取澄清回答', kind: '用户回答', subject: 'planning.clarification', outcome: '已保存', parameters: { question: 'macOS 产物使用哪个项目签名凭据引用？' }, result: { answer: '使用 signing/orbital-macos。Windows 使用项目已有代码签名引用。' } },
    auditRecords[3],
    { ...auditRecords[4], result: { summary: '签名引用已确认。重新规划将在第 2 代源码工作目录生成修订 2。' } },
  ];
  return [
    ...auditRecords.slice(0, 2),
    { id: 'question', label: '请求签名澄清', kind: '模型正文', subject: 'planning.needs_input', outcome: '需要回答', parameters: {}, result: { question: 'macOS 产物使用哪个项目签名凭据引用？', plan_revision: 1 } },
    auditRecords[4],
  ];
}
function exceptionFor(scenario) {
  return {
    failed: { id: 'failed-tool', label: '读取构建配置失败', kind: '工具调用', subject: 'repository.read_file', call: 'call_demo_failed', outcome: '已失败', parameters: { path: 'missing-build.toml' }, result: { error: '文件不存在', retryable: true } },
    rejected: { id: 'rejected-tool', label: '工具请求被拒绝', kind: '工具调用', subject: 'credentials.read_secret', call: 'call_demo_rejected', outcome: '已拒绝', parameters: { reference: 'signing/orbital-macos' }, result: { reason: '规划仅授权凭据引用，不返回密钥' } },
    missing: { id: 'missing-result', label: '工具结果未保存', kind: '缺失记录', subject: 'repository.read_file', call: 'call_demo_missing', outcome: '记录缺失', parameters: { path: 'Cargo.toml' }, result: null },
    partial: { id: 'partial-message', label: '消息尚未完整', kind: '消息分片', subject: 'message_demo_partial', outcome: '消息分片', parameters: { message_id: 'message_demo_partial', saved_parts: 1, total_parts: 2 }, result: null },
  }[scenario];
}
export function auditReading(view) {
  const round = auditRounds.find(item => item.id === view.auditRound) || auditRounds[0];
  const scenario = view.auditScenario || 'normal';
  view.auditReadings ||= {};
  const exception = exceptionFor(scenario);
  const reading = view.auditReadings[`${round.id}/${exception ? scenario : 'saved'}`] ||= { page: 1, entry: '', tab: 'result', query: '', filter: 'all' };
  const all = exception ? [exception, ...recordsForRound(round)] : recordsForRound(round);
  const records = all.slice(0, Math.max(1, reading.page) * 2);
  const selected = records.find(item => item.id === reading.entry) || records[0];
  return { round, scenario, reading, all, records, selected };
}
function matches(record, reading) {
  const query = reading.query.trim().toLocaleLowerCase();
  const search = [record.label, record.subject, record.call, record.parameters.path, record.parameters.reference].filter(Boolean).join(' ').toLocaleLowerCase();
  return (!query || search.includes(query)) && (reading.filter === 'all' || (reading.filter === 'tools' && isTool(record)) || (reading.filter === 'messages' && !isTool(record)) || (reading.filter === 'attention' && attention(record)));
}
export function auditRawText(view) {
  const { round, all, selected } = auditReading(view);
  return JSON.stringify({ demo: true, run_id: round.run, generation: round.generation, sequence: all.indexOf(selected) + 1, ...selected }, null, 2);
}
export function applyAuditFilter(root, view) {
  const { reading, records } = auditReading(view);
  let count = 0;
  root.querySelectorAll('[data-audit-record]').forEach(node => {
    const record = records.find(item => item.id === node.dataset.auditRecord);
    node.hidden = !record || !matches(record, reading);
    if (!node.hidden) count++;
  });
  const label = root.querySelector('[data-audit-visible-count]');
  if (label) label.textContent = `${count} 项`;
  root.querySelector('[data-audit-filter-empty]')?.toggleAttribute('hidden', count > 0);
}
function readable(record) {
  if (record.outcome === '记录缺失') return '<p class="df-audit-boundary">只保存了工具请求，结果未提供。缺失状态不能解释为成功或仍在运行。</p>';
  if (record.outcome === '消息分片') return '<p class="df-audit-boundary">已保存 1 / 2 个分片。正文尚不完整，因此不拼接或补写缺失内容。</p>';
  if (record.result.summary || record.result.answer || record.result.question) return `<div class="df-audit-document"><h4>${record.result.question ? '需要澄清' : record.result.answer ? '用户回答' : '规划说明'}</h4><p>${U.esc(record.result.summary || record.result.answer || record.result.question)}</p></div>`;
  if (record.result.entries) return `<div class="df-audit-document"><h4>源码目录</h4><ul class="df-audit-files">${record.result.entries.map(path => `<li>${U.icon('file', 15)}<code>${U.esc(path)}</code></li>`).join('')}</ul><p class="df-caption">${record.result.truncated ? '目录结果已截断。' : '此工具返回的目录结果未截断。'}</p></div>`;
  if (record.id === 'read-file') return `<div class="df-audit-document"><div class="df-audit-filebar">${U.icon('file', 16)}<code>${U.esc(record.parameters.path)}</code></div><h4>构建配置</h4><p>配置声明版本 ${U.esc(record.result.version)}，启用 ${record.result.bundle.targets.map(U.esc).join('、')} 目标。</p><details class="df-platform-hints"><summary>查看文件内容</summary><pre class="code-block"><code>${U.esc(JSON.stringify(record.result, null, 2))}</code></pre></details></div>`;
  const labels = { native_targets: '原生目标', image_platforms: '镜像平台', validation: '校验结果', available: '引用可用', secret_material: '密钥内容', scope: '授权范围', error: '失败原因', retryable: '可重试', reason: '拒绝原因' };
  const values = { true: '是', false: '否', passed: '通过', 'not returned': '未返回', project: '当前项目' };
  return `<dl class="df-audit-fields">${Object.entries(record.result).map(([key, value]) => `<div><dt>${U.esc(labels[key] || key)}</dt><dd>${Array.isArray(value) ? value.map(U.esc).join(' · ') : U.esc(values[String(value)] || String(value))}</dd></div>`).join('')}</dl>`;
}
export function renderBuildAudit(view) {
  const { round, scenario, reading, all, records, selected } = auditReading(view);
  const visible = records.filter(record => matches(record, reading));
  const filterLabel = filters.find(([id]) => id === reading.filter)?.[1] || '全部动作';
  const tone = ['已失败', '已拒绝'].includes(selected.outcome) ? 'danger' : attention(selected) ? 'warning' : 'success';
  const body = reading.tab === 'result' ? readable(selected) : `<pre class="code-block"><code>${U.esc(reading.tab === 'raw' ? auditRawText(view) : JSON.stringify(selected.parameters, null, 2))}</code></pre>`;
  return U.panel(`<header class="df-audit-intro"><span class="df-audit-mark">${U.icon('spark', 23)}</span><div><h2>每一步，都有据可循</h2><p>按规划轮次查看模型说明、工具动作与平台结果。</p></div>${U.badge(`${auditRounds.length} 次会话`)}</header>
    <p class="df-audit-demo">本地演示样例 · 不连接模型、不执行工具。搜索仅作用于已加载记录。</p>
    <nav class="df-rounds" aria-label="规划审计轮次">${auditRounds.map((item, index) => button(`<span class="df-round-index">${String(index + 1).padStart(2, '0')}</span><span class="df-round-copy"><strong>${item.title}</strong><time>${item.time}</time></span>${U.badge(item.id === round.id && scenario === 'interrupted' ? '已中断' : item.status, item.id === round.id && scenario === 'interrupted' ? 'neutral' : 'success')}`, 'audit-round', item.id, round.id === item.id)).join('')}</nav>
    ${scenario === 'interrupted' ? '<p class="df-audit-boundary" role="status">会话已中断。已保存记录继续可读，缺失尾部不会补写。</p>' : ''}${scenario === 'error' ? '<div class="df-inline-error" role="alert">下一片审计记录加载失败。当前证据与阅读位置已保留。</div>' : ''}${scenario === 'budget' ? '<p class="soft-note" role="status">平台提示样例：剩余调用预算不足。此处不推算未提供的调用次数或 Token 用量。</p>' : ''}
    ${['error', 'failed', 'interrupted'].includes(scenario) ? action('重试本地读取', 'audit-retry', 'refresh') : ''}
    <div class="df-audit-session"><div><span class="eyebrow">规划轮次</span><h3>${round.title}</h3><p>${U.icon('spark', 14)}${round.model}</p></div><div><time>${U.icon('clock', 14)}${round.time}</time><span>结束于 ${round.ended}</span></div></div>
    <div class="df-audit-stats"><span>${U.icon('layers', 16)}已加载 ${records.length} 条原始演示记录</span><span>${U.icon('code', 16)}${records.filter(isTool).length} 个已加载工具与提案</span><span>${U.icon('warn', 16)}${records.filter(attention).length} 项需要留意</span></div>
    <div class="df-audit-toolbar glass"><label class="df-audit-search">${U.icon('search', 17)}<input type="search" aria-label="搜索已加载审计记录" value="${U.esc(reading.query)}" placeholder="搜索动作、文件或调用标识" data-audit-search></label><details class="select df-audit-filter"><summary role="button" aria-label="审计动作筛选：${filterLabel}" aria-haspopup="listbox" aria-expanded="false"><span>${filterLabel}</span>${U.icon('down', 16)}</summary><div class="select-options" role="listbox" aria-label="审计动作筛选">${filters.map(([id, label]) => `<button type="button" role="option" aria-selected="${reading.filter === id}" data-flow-action="audit-filter" data-flow-value="${id}">${label}${U.icon('check', 16)}</button>`).join('')}</div></details></div>
    <p class="df-audit-scope">搜索与筛选仅作用于已加载记录。加载更多可继续查看；所选详情保持不变。</p>
    <div class="df-audit-mobile-jump">${action('查看所选详情', 'audit-detail-jump', 'down')}</div>
    <div class="df-audit-grid"><div class="df-audit-feed" tabindex="-1" aria-label="已保存的动作" data-flow-scroll="audit-feed-${round.id}"><div class="df-audit-feed-heading"><h3>已保存的动作</h3><span data-audit-visible-count>${visible.length} 项</span></div>${records.map(record => button(`<span class="df-audit-icon">${U.icon(['已失败', '已拒绝'].includes(record.outcome) ? 'close' : isTool(record) ? 'code' : 'file', 17)}</span><span class="df-audit-action-copy"><strong>${record.label}</strong><small>${U.esc(record.parameters.path || record.parameters.reference || record.subject)}</small></span><span class="df-audit-outcome">${record.outcome}</span>${U.icon('chevron', 13)}`, 'audit-entry', record.id, selected.id === record.id, `data-audit-record="${record.id}" ${matches(record, reading) ? '' : 'hidden'}`)).join('')}
      <p class="df-audit-filter-empty" data-audit-filter-empty ${visible.length ? 'hidden' : ''}>已加载记录中没有匹配动作。调整搜索或筛选，或继续加载记录。</p>
      ${records.length < all.length ? action('加载下一片记录', 'audit-more', 'plus') : '<p class="df-caption">当前轮次的演示记录已全部显示。</p>'}
      <details class="df-platform-hints" data-flow-detail="audit-hints"><summary>平台提示 · 2 条</summary><p>源码只读工具已授权。工具未返回凭据内容。</p><p>只显示已保存记录，不推造缺失消息。</p></details>
    </div><section class="df-audit-evidence" tabindex="-1" aria-label="动作详情"><div class="df-audit-mobile-jump">${action('返回动作列表', 'audit-feed-jump', 'arrowLeft')}</div><div class="df-inspector" data-flow-scroll="audit-inspector-${round.id}"><header class="df-audit-inspector-heading"><span class="df-audit-mark">${U.icon(isTool(selected) ? 'code' : 'file', 22)}</span><div><span class="df-caption">${selected.kind}</span><h3>${selected.label}</h3><p class="mono small muted">${selected.subject}</p></div><div class="df-audit-inspector-actions">${U.badge(selected.outcome, tone)}${action('复制原始记录', 'audit-copy', 'copy')}</div></header>
      <div class="df-inline-tabs" role="group" aria-label="审计详情内容">${[['result', '可读结果'], ['parameters', '参数'], ['raw', '原始记录']].map(([id, label]) => button(label, 'audit-tab', id, reading.tab === id)).join('')}</div><div class="df-audit-detail-body" data-flow-scroll="audit-body-${round.id}">${body}</div>
    </div></section></div><details class="df-platform-hints" data-flow-detail="audit-technical"><summary>会话身份</summary>${U.kv('Run ID', `<code>${round.run}</code>`)}${U.kv('Generation', round.generation)}${U.kv('Pod UID', '<code>pod_demo_planner</code>')}${U.kv('源码摘要', '<code>src_demo_e7a2c91</code>')}</details><details class="df-scenario" data-flow-detail="audit-scenario"><summary>审计异常演示</summary><p>仅切换本地保存样例，不再次调用模型或工具。</p>${[['normal', '正常'], ['failed', '工具失败'], ['interrupted', '会话中断'], ['rejected', '请求拒绝'], ['missing', '记录缺失'], ['partial', '消息分片'], ['error', '加载错误'], ['budget', '预算提示']].map(([id, label]) => button(label, 'audit-scenario', id, scenario === id)).join('')}</details>`, 'df-audit');
}
