/**
 * [INPUT]: 依赖共享视图函数和按构建隔离的本地审计阅读状态。
 * [OUTPUT]: 导出审计轮次、分片阅读、异常记录和预算提示。
 * [POS]: 构建详情只读证据层；不执行工具，不读取真实模型会话。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';

export const auditRounds = [
  { id: 'initial/1', title: '首次规划', purpose: 'initial', model: '项目配置模型', status: '已完成', time: '14:23:48', run: 'run_demo_initial', generation: 1 },
  { id: 'clarification/1', title: '澄清后规划', purpose: 'clarification', model: '项目配置模型', status: '已完成', time: '14:24:08', run: 'run_demo_clarification', generation: 1 },
  { id: 'revision/2', title: '计划修订 2 · 第 2 代', purpose: 'revision', model: '项目配置模型', status: '已完成', time: '14:25:10', run: 'run_demo_revision', generation: 2 },
];
export const auditRecords = [
  { id: 'read-tree', label: '读取源码目录', kind: '工具调用', subject: 'repository.list_tree', outcome: '已完成', parameters: { path: '.', depth: 2 }, result: { entries: ['Cargo.toml', 'src-tauri/', 'package.json', 'Dockerfile'], truncated: false } },
  { id: 'read-file', label: '读取构建配置', kind: '工具调用', subject: 'repository.read_file', outcome: '已完成', parameters: { path: 'src-tauri/tauri.conf.json', start_line: 1, end_line: 48 }, result: { version: '2.4.0', bundle: { active: true, targets: ['dmg', 'nsis', 'appimage'] } } },
  { id: 'proposal', label: '生成目标方案', kind: '结构化方案', subject: 'plan.propose · revision 2', outcome: '已接受', parameters: { source_manifest: 'src_demo_e7a2c91', targets: 6 }, result: { native_targets: 4, image_platforms: ['linux/amd64', 'linux/arm64'], validation: 'passed' } },
  { id: 'check', label: '验证签名引用', kind: '工具调用', subject: 'credentials.inspect_reference', outcome: '已完成', parameters: { reference: 'signing/orbital-macos' }, result: { available: true, secret_material: 'not returned', scope: 'project' } },
  { id: 'summary', label: '保存规划结论', kind: '模型正文', subject: '固定源码与全部选定目标', outcome: '已完成', parameters: {}, result: { summary: '使用锁定依赖执行原生构建。签名凭据仅通过引用传入。镜像只写入候选，正式标签由 Release 发布。' } },
];
function recordsForRound(round) {
  if(round.purpose==='revision')return auditRecords;
  if(round.purpose==='clarification')return [
    { id:'answer',label:'读取澄清回答',kind:'用户回答',subject:'planning.clarification',outcome:'已保存',parameters:{question:'macOS 产物使用哪个项目签名凭据引用？'},result:{answer:'使用 signing/orbital-macos。Windows 使用项目已有代码签名引用。'} },
    auditRecords[3],
    { ...auditRecords[4],result:{summary:'签名引用已确认。重新规划将在第 2 代源码工作目录生成修订 2。'} },
  ];
  return [
    ...auditRecords.slice(0,2),
    { id:'question',label:'请求签名澄清',kind:'模型正文',subject:'planning.needs_input',outcome:'需要回答',parameters:{},result:{question:'macOS 产物使用哪个项目签名凭据引用？',plan_revision:1} },
  ];
}
const button = (label, action, value, selected = false) => `<button type="button" class="df-choice ${selected ? 'selected' : ''}" data-flow-action="${action}" data-flow-value="${U.esc(value)}" aria-pressed="${selected}">${label}</button>`;

export function renderBuildAudit(view) {
  const round = auditRounds.find(item => item.id === view.auditRound) || auditRounds[0];
  const scenario=view.auditScenario||'normal';
  const exception={failed:{id:'failed-tool',label:'读取构建配置失败',kind:'工具调用',subject:'repository.read_file',outcome:'已失败',parameters:{path:'missing-build.toml'},result:{error:'文件不存在',retryable:true}},rejected:{id:'rejected-tool',label:'工具请求被拒绝',kind:'工具调用',subject:'credentials.read_secret',outcome:'已拒绝',parameters:{reference:'signing/orbital-macos'},result:{reason:'规划仅授权凭据引用，不返回密钥'}},missing:{id:'missing-result',label:'工具结果未保存',kind:'缺失记录',subject:'repository.read_file',outcome:'记录缺失',parameters:{path:'Cargo.toml'},result:{notice:'未保存结果。原型不推造缺失内容。'}}}[scenario];
  const allRecords=exception?[exception,...recordsForRound(round)]:recordsForRound(round);
  const count = Math.min(allRecords.length, (view.auditPage || 1) * 2);
  const records = allRecords.slice(0, count);
  const selected = records.find(item => item.id === view.auditEntry) || records[0];
  const tab = view.auditTab || 'result';
  return U.panel(`<header class="df-section-head"><div><div class="eyebrow">PLANNING / AUDIT</div><h2>每次规划，都有可读证据。</h2><p class="muted small">独立只读审计。此处展示本地分片样例，不连接模型或执行工具。</p></div>${U.badge(`${auditRounds.length} 个轮次`)}</header>
    ${scenario==='interrupted'?'<p class="notice" role="status">会话已中断。已保存记录继续可读，缺失尾部不会补写。</p>':''}${scenario==='error'?'<div class="df-inline-error" role="alert">审计分片加载失败。已有记录继续保留。</div>':''}${scenario==='budget'?'<p class="soft-note" role="status">本地预算样例：已使用 23,600 / 24,000 Token。接近上限，下一次调用等待预算调整。</p>':''}
    ${['error','failed','interrupted'].includes(scenario)?button('重试本地读取','audit-retry','',false):''}
    <div class="df-audit-stats"><span>当前修订 2 · 第 2 代</span><span>8 次确认调用</span><span>${scenario==='budget'?'23,600':'18,420'} / 24,000 Token</span></div>
    <nav class="df-rounds" aria-label="规划审计轮次">${auditRounds.map(item => button(`<strong>${item.title}</strong><span>${item.time} · ${item.status}</span>`, 'audit-round', item.id, round.id === item.id)).join('')}</nav>
    <div class="df-audit-session"><span>${round.title} · ${round.model}</span><span>${count} / ${allRecords.length} 条演示记录</span></div>
    <div class="df-audit-grid"><div class="df-audit-feed" data-flow-scroll="audit-feed-${round.id}"><h3>动作记录</h3>${records.map(record => button(`<span class="df-audit-icon">${U.icon(record.kind === '工具调用' ? 'code' : 'file', 17)}</span><span><strong>${record.label}</strong><small>${record.subject}</small></span><span class="df-audit-outcome">${record.outcome}</span>`, 'audit-entry', record.id, selected.id === record.id)).join('')}
      ${count < allRecords.length ? button('加载下一片记录', 'audit-more', '', false) : '<p class="df-caption">当前轮次记录已全部显示。</p>'}
      <details class="df-platform-hints" data-flow-detail="audit-hints"><summary>平台提示 · 2 条</summary><p>源码只读工具已授权。工具未返回凭据内容。</p><p>只显示已保存的审计记录，不推造缺失消息。</p></details>
    </div><section class="df-inspector" data-flow-scroll="audit-inspector-${round.id}"><div class="row between wrap"><div><span class="df-caption">${selected.kind}</span><h3>${selected.label}</h3></div>${U.badge(selected.outcome, ['需要回答','记录缺失'].includes(selected.outcome)?'warning':['已失败','已拒绝'].includes(selected.outcome)?'danger':'success')}</div><p class="mono small muted">${selected.subject}</p>
      <div class="df-inline-tabs" role="group" aria-label="审计详情内容">${[['result','结果'],['parameters','参数'],['raw','原始记录']].map(([id,label]) => button(label,'audit-tab',id,tab===id)).join('')}</div>
      <pre class="code-block"><code>${U.esc(JSON.stringify(tab === 'parameters' ? selected.parameters : tab === 'raw' ? { demo: true, run_id: round.run, generation: round.generation, sequence: allRecords.indexOf(selected) + 1, ...selected } : selected.result, null, 2))}</code></pre>
    </section></div><details class="df-platform-hints" data-flow-detail="audit-technical"><summary>会话身份</summary>${U.kv('Run ID', `<code>${round.run}</code>`)}${U.kv('Generation', round.generation)}${U.kv('Pod UID', '<code>pod_demo_planner</code>')}${U.kv('源码摘要', '<code>src_demo_e7a2c91</code>')}</details><details class="df-scenario" data-flow-detail="audit-scenario"><summary>审计异常演示</summary><p>仅切换本地保存样例，不再次调用模型或工具。</p>${[['normal','正常'],['failed','工具失败'],['interrupted','会话中断'],['rejected','请求拒绝'],['missing','记录缺失'],['error','加载错误'],['budget','预算提示']].map(([id,label])=>button(label,'audit-scenario',id,scenario===id)).join('')}</details>`, 'df-audit');
}
