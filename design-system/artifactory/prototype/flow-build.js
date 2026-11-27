/**
 * [INPUT]: 依赖共享内容组件、本地构建状态与独立 AI 审计视图。
 * [OUTPUT]: 导出五标签构建详情、隔离日志分页与连接状态。
 * [POS]: 构建 #1048 的项目内演示；历史缺失证据由 flows.js 保持空态。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { renderBuildAudit } from './flow-audit.js';

export function buildView(state) {
  const key = `${state.workspaceId || state.workspace || 'default'}/${state.projectId || 'orbital'}/${state.selectedBuild || '1048'}`;
  state.flowBuildViews ||= {};
  const view = state.flowBuildViews[key] ||= { tab: state.buildTab === 'logs' ? 'execution' : state.buildTab || 'process', target: 'macos-arm64', attempt: 'attempt-2', step: 'all', logPages: 1, revision: '2', planTarget: 'macos-arm64', scenario: 'running', answer: '', auditRound: 'initial/1', auditPage: 1, auditEntry: 'read-tree', auditTab: 'result', scroll: {}, details: {} };
  view.logPagesByKey ||= {};
  view.logStateByKey ||= {};
  view.key = key;
  return view;
}
export function logView(view){
  const key=`${view.target}/${view.attempt}/${view.step}`;
  view.logStateByKey ||= {};
  return view.logStateByKey[key] ||= {follow:true,connection:'connected',error:false,tick:0};
}
const action = (label, name, value = '', kind = 'secondary', glyph = '') => U.btn(label, '', kind, glyph, `data-flow-action="${name}" data-flow-value="${U.esc(value)}"`);
export const executionTargets = [
  { id: 'macos-arm64', label: 'macOS arm64', device: 'Mac Studio · M2 Ultra', status: '签名中', tone: 'running' },
  { id: 'macos-amd64', label: 'macOS amd64', device: 'Mac mini · Intel', status: '已成功', tone: 'success' },
  { id: 'windows-amd64', label: 'Windows amd64', device: 'Windows Builder', status: '已成功', tone: 'success' },
  { id: 'linux-amd64', label: 'Linux amd64', device: 'Linux Runner', status: '已成功', tone: 'success' },
  { id: 'image', label: '容器镜像', device: 'BuildKit · amd64 + arm64', status: '排队中', tone: 'warning' },
];
const nativeSteps = [['prepare','准备工具链','prepare'],['build','生成原生应用','build'],['sign','签名与校验','sign'],['archive','归档制品','archive']];
const logText = {
  prepare: ['Reading pinned source manifest src_demo_e7a2c91', 'Verified pnpm-lock.yaml', 'Resolved Node.js 24 and Rust stable', 'pnpm install --frozen-lockfile completed'],
  build: ['pnpm tauri build --target aarch64-apple-darwin', 'Compiling orbital-desktop v2.4.0', 'Finished release profile in 3m 42s', 'Bundled Orbital.app'],
  sign: ['Reading credential reference signing/orbital-macos', 'codesign --options runtime --timestamp Orbital.app', 'Code signature applied', 'Verifying sealed resources and designated requirement'],
  archive: ['Waiting for signing verification', 'Artifact upload has not started'],
};
function platformLine(text, target) {
  if(target.id==='windows-amd64')return text.replace('aarch64-apple-darwin','x86_64-pc-windows-msvc').replaceAll('Orbital.app','Orbital.exe').replace('signing/orbital-macos','signing/orbital-windows').replace('codesign --options runtime --timestamp','signtool sign /fd SHA256 /tr timestamp.example').replace('Code signature applied','Authenticode signature applied').replace('Verifying sealed resources and designated requirement','Verifying Authenticode signature');
  if(target.id==='linux-amd64')return text.replace('aarch64-apple-darwin','x86_64-unknown-linux-gnu').replaceAll('Orbital.app','Orbital.AppImage').replace('Reading credential reference signing/orbital-macos','Preparing SHA-256 verification; no signing credential configured').replace('codesign --options runtime --timestamp Orbital.AppImage','sha256sum Orbital.AppImage').replace('Code signature applied','SHA-256 recorded').replace('Verifying sealed resources and designated requirement','Verifying artifact content digest');
  return target.id==='macos-amd64'?text.replace('aarch64-apple-darwin','x86_64-apple-darwin'):text;
}
function planCommand(step, target) {
  if(step==='prepare')return 'pnpm install --frozen-lockfile';
  if(step==='build')return platformLine(logText.build[0],target);
  if(step==='sign')return platformLine(logText.sign[0],target)+'\n'+platformLine(logText.sign[1],target);
  if(target.id==='windows-amd64')return 'Compress-Archive -Path Orbital.exe -DestinationPath Orbital-windows-amd64.zip';
  if(target.id==='linux-amd64')return 'sha256sum Orbital.AppImage > Orbital.AppImage.sha256';
  return `ditto -c -k --sequesterRsrc --keepParent Orbital.app Orbital-${target.id}.zip`;
}
function executionLines(target,attempt) {
  const failed=attempt==='attempt-1',successful=!failed&&target.tone==='success';
  return nativeSteps.flatMap(([step])=>{
    if((failed&&['sign','archive'].includes(step))||(!failed&&!successful&&step==='archive'))return [];
    const content=failed&&step==='build'?[...logText.build.slice(0,2),'error: build tool exited with code 1; attempt ended']:successful&&step==='archive'?['Artifact archive created','SHA-256 verified against uploaded content','Artifact upload completed']:logText[step];
    return content.map((text,index)=>({step,text:`[${target.id}] ${platformLine(text,target)}`,id:`${step}-${index}`}));
  });
}
function process(state, view) {
  const cancelled = !!state.buildCancelled;
  const needsInput = view.scenario === 'needs_input' && !cancelled;
  const stages = [['source','固定源码','已完成'],['planning','AI 规划',needsInput?'需要澄清':'已完成'],['dispatch','调度目标','部分等待'],['execution','执行构建',cancelled?'已取消':'进行中'],['artifacts','归档制品','部分完成']];
  const events = [
    ['execution','提交签名校验','14:32:18','actor=emissary · target=macos-arm64 · attempt=attempt-2 · step=sign'],
    ['artifacts','Windows 制品已校验','14:31:44','actor=storage · target=windows-amd64 · sha256=2d7b308c9a41…'],
    ['dispatch','ARM 镜像保留排队','14:30:10','actor=buildkit · platform=linux/arm64 · reason=pool_unavailable'],
    ['planning','保存计划修订 2','14:25:10','actor=planner · generation=2 · plan_id=plan_demo_2'],
    ['source','工作目录准备完成','14:23:48','actor=source · pod=planner-demo-2 · generation=2'],
    ['source','固定源码清单','14:23:36','actor=source · commit=e7a2c91 · manifest=src_demo_e7a2c91'],
  ].filter(item => !view.stage || item[0] === view.stage);
  return `<div class="grid-main-aside"><div class="stack">${U.panel(`<div class="df-section-head"><div><div class="eyebrow">CURRENT ACTIVITY / LOCAL SNAPSHOT</div><h2>${cancelled?'构建已取消':needsInput?'等待一份明确的回答':'正在为 macOS 制品签名'}</h2><p class="muted">${cancelled?'已完成证据继续保留。':needsInput?'规划已暂停。回答保存在当前构建的本地草稿中。':'原生目标独立执行；ARM 镜像等待构建池恢复。'}</p></div><span class="activity-orb">${U.icon(needsInput?'help':cancelled?'stop':'shield',28)}</span></div>
      <nav class="df-stage-nav" aria-label="按构建阶段筛选事件">${stages.map(([id,label,status],index)=>`<button type="button" data-flow-action="process-stage" data-flow-value="${id}" aria-pressed="${view.stage===id}"><span>${index===0?U.icon('check',14):index+1}</span><strong>${label}<small>${status}</small></strong>${U.icon('chevron',13)}</button>`).join('')}</nav>`)}
      ${U.panel(U.sectionTitle('本代源码准备',U.badge('Generation 2'))+`<p class="muted small">提交固定与本代工作目录分别记录。以下均为演示证据。</p><div class="df-source-grid">${['解析引用','固定提交','检出工作目录','准备子模块','准备 LFS','工作目录就绪'].map(label=>`<span>${U.icon('check',14)}${label}<small>已完成</small></span>`).join('')}</div>${U.kv('源码提交','<code>e7a2c91</code>')}${U.kv('规划 Pod','<code>planner-demo-2</code>')}`)}
      ${U.panel(U.sectionTitle('活动记录',action(view.stage?'显示全部阶段':'查看执行','process-stage',view.stage?'':'execution','ghost','arrow'))+`<div class="df-event-list">${events.map(([stage,title,time,details])=>`<details data-flow-detail="event-${stage}-${time}"><summary><span class="df-event-dot"></span><strong>${title}</strong><time>${time}</time>${U.icon('down',15)}</summary><p class="mono small muted">${details}</p></details>`).join('')||'<p class="muted">此筛选没有演示事件。</p>'}</div>`)}</div><aside class="stack">${U.panel(U.sectionTitle('构建快照')+U.kv('所属项目','Orbital Desktop')+U.kv('交付版本','2.4.0')+U.kv('源码引用','<code>release/2.4</code>')+U.kv('提交','<code>e7a2c91</code>')+U.kv('当前计划','修订 2')+action('阅读计划','build-tab','plan','secondary','arrow'))}${U.panel(`<h3>需要什么，就看什么。</h3><p class="muted small">过程解释当前进展。执行保留目标和日志。计划保留修订与澄清。AI 审计独立展示调用证据。</p>`)}<details class="df-scenario" data-flow-detail="build-scenario"><summary>原型状态演示</summary><p>只改变本地演示，不触发真实构建。</p>${action('执行中','build-scenario','running')}${action('需要澄清','build-scenario','needs_input')}</details></aside></div>`;
}
function execution(state, view) {
  const target = executionTargets.find(item=>item.id===view.target)||executionTargets[0];
  const image = target.id==='image';
  const attempt = view.attempt || 'attempt-2';
  const steps = nativeSteps;
  const failed=attempt==='attempt-1',cancelled=state.buildCancelled&&target.tone!=='success';
  const attemptLabel=failed?'历史失败':cancelled?'已取消':target.tone==='success'?'已成功':'当前执行';
  const attemptTone=failed?'danger':cancelled?'neutral':target.tone;
  const stream=logView(view);
  const lines = [...executionLines(target,attempt),...Array.from({length:stream.tick},(_,index)=>({step:view.step==='all'?'sign':view.step,text:`[${target.id}] local demonstration event ${index+1}; no remote executor connected`,id:`local-${index}`}))].filter(line=>view.step==='all'||line.step===view.step);
  const pages=view.logPagesByKey[`${target.id}/${attempt}/${view.step}`]||1;
  const visible = lines.slice(-(pages*6+stream.tick));
  return `<div class="df-execution"><nav class="df-target-strip" aria-label="执行目标">${executionTargets.map(item=>`<button type="button" data-flow-action="execution-target" data-flow-value="${item.id}" aria-pressed="${target.id===item.id}"><strong>${item.label}</strong>${U.badge(state.buildCancelled&&item.tone!=='success'?'已取消':item.status,item.tone)}</button>`).join('')}</nav>
    ${image?U.panel(U.sectionTitle('镜像执行',U.badge('候选构建'))+`<p class="muted">镜像架构由 BuildKit 构建池执行，不占用原生设备授权。</p>${U.kv('组件','sync-service')}${U.kv('目标平台','linux/amd64 · linux/arm64')}${U.kv('amd64','已成功 · sha256:84fb26c7…')}${U.kv('arm64',state.buildCancelled?'已取消 · 未开始执行':'排队中 · 构建池暂无可用执行器')}<div class="soft-note">${U.icon('clock',17)}<span>正式镜像标签只由 Release 发布创建；此处保留隔离候选。</span></div>`):`<div class="grid-main-aside"><div class="stack">${U.panel(`<div class="df-section-head"><div><h2>${target.label} · 执行记录</h2><p class="muted small">${target.device} · 只读演示快照</p></div>${U.badge(attemptLabel,attemptTone)}</div><nav class="df-inline-tabs" aria-label="执行尝试">${['attempt-1','attempt-2'].map(id=>`<button type="button" data-flow-action="execution-attempt" data-flow-value="${id}" aria-pressed="${attempt===id}">${id==='attempt-1'?'Attempt 1 · 已失败':`Attempt 2 · ${target.tone==='success'?'已成功':cancelled?'已取消':'当前'}`}</button>`).join('')}</nav>${U.kv('执行身份',`<code>${attempt}</code>`)}${U.kv('计划身份','<code>plan_demo_2</code>')}`)}
    ${U.panel(`<div class="df-section-head"><div><h2>${view.step==='all'?'执行日志':steps.find(item=>item[0]===view.step)?.[1]+' · 日志'}</h2><p class="muted small">按目标、attempt 与步骤隔离。向前加载保留阅读锚点。</p></div><span class="mono small muted">${visible.length} / ${lines.length} 行</span></div>${visible.length<lines.length?`<div class="df-log-pagination">${action('加载更早日志','logs-older','','secondary','arrowLeft')}</div>`:''}<div class="df-log-controls"><span role="status">${stream.connection==='disconnected'?'本地断线样例 · 已保存日志仍可阅读':'本地连接样例 · 未连接真实服务'}</span><button type="button" class="btn secondary small" data-flow-action="logs-follow" aria-pressed="${stream.follow}">${stream.follow?'跟随尾部':'暂停跟随'}</button></div>${stream.error?`<div class="df-inline-error" role="alert">日志加载失败样例。当前记录与阅读位置已保留。${action('重试加载','logs-retry','','secondary','refresh')}</div>`:''}<details class="df-scenario" data-flow-detail="log-scenario"><summary>日志连接演示</summary><p>只演示本地连接与加载状态。</p>${action('新增一条演示日志','logs-append')}${action('断线','logs-scenario','disconnected')}${action('加载失败','logs-scenario','error')}${stream.connection==='disconnected'?action('重新连接','logs-retry','','secondary','refresh'):''}</details><div class="log-console df-log-reader" data-flow-scroll="log-${target.id}-${attempt}-${view.step}" role="log" aria-label="${target.label} ${attempt} 日志" aria-live="off" tabindex="0">${!lines.length?`<p class="df-log-boundary">${failed?'本次尝试在构建阶段失败，没有执行此步骤。':'此步骤尚未开始，没有执行日志。'}</p>`:''}${visible.map(line=>`<div class="log-line" data-flow-log-id="${line.id}"><span class="line-number">${String(lines.indexOf(line)+1).padStart(2,'0')}</span><span class="log-level">${line.step.toUpperCase()}</span><code>${U.esc(line.text)}</code></div>`).join('')}<p class="df-log-boundary">本地日志快照 · 未连接真实执行环境</p></div>`)}
    </div><aside class="stack">${U.panel(U.sectionTitle('按步骤查看')+`<div class="df-step-filter">${[['all','全部步骤',''],...steps].map(([id,label,phase],index)=>`<button type="button" data-flow-action="execution-step" data-flow-value="${id}" aria-pressed="${view.step===id}"><span>${index||U.icon('layers',14)}</span><strong>${label}</strong>${phase?`<small>${phase}</small>`:''}</button>`).join('')}</div>`)}${U.panel(`<h3>执行历史持续保留</h3><p class="muted small">失败尝试与重试尝试分开保存。切换目标不会混合日志，切回标签保留原位置。</p>`)}</aside></div>`}</div>`;
}
function plan(view) {
  const revision=view.revision||'2', target=executionTargets.find(item=>item.id===view.planTarget)||executionTargets[0];
  return `<div class="grid-main-aside"><div class="stack">${U.panel(`<div class="df-section-head"><div><div class="eyebrow">PLAN / REVISION ${revision}</div><h2>清晰的方案，确定的交付。</h2><p class="muted small">修订 2 为当前计划。历史修订只读，执行证据保持原计划身份。</p></div>${U.badge(revision==='2'?'当前计划':'历史计划',revision==='2'?'success':'neutral')}</div><nav class="df-inline-tabs" aria-label="计划历史">${['1','2'].map(id=>`<button type="button" data-flow-action="plan-revision" data-flow-value="${id}" aria-pressed="${revision===id}">修订 ${id}${id==='2'?' · 当前':''}</button>`).join('')}</nav><p>${revision==='1'?'首次方案使用原生工具链。签名身份尚需确认。':'确认项目签名引用后更新方案，覆盖四个原生目标与双架构镜像。'}</p>`)}
    ${U.panel(U.sectionTitle('计划目标')+`<nav class="df-target-strip" aria-label="计划目标">${executionTargets.map(item=>`<button type="button" data-flow-action="plan-target" data-flow-value="${item.id}" aria-pressed="${target.id===item.id}">${item.label}</button>`).join('')}</nav>${target.id==='image'?`<div class="callout"><strong>sync-service · BuildKit</strong><p>Dockerfile · context=./ · platforms=linux/amd64,linux/arm64。仅写隔离候选；正式推广受发布租约和摘要准入约束。</p></div>`:nativeSteps.map(([id,label,phase],index)=>`<details class="plan-step" data-flow-detail="plan-${revision}-${target.id}-${id}" ${index===0?'open':''}><summary><span class="step-number">${index+1}</span><div><strong>${label}</strong><p class="muted small">phase=${phase} · ${target.label}</p></div>${U.icon('down',16)}</summary><pre class="code-block"><code>${U.esc(planCommand(id,target))}</code></pre></details>`).join('')}`)}
    ${U.panel(`<div class="df-section-head"><div><h2>澄清与回答</h2><p class="muted small">回答草稿按构建隔离。此演示只保存本地回答。</p></div>${U.badge(view.answerSubmitted?'已保存回答':view.scenario==='needs_input'?'等待回答':'历史澄清',view.scenario==='needs_input'?'warning':'neutral')}</div><p>macOS 产物使用哪个项目签名凭据引用？</p>${view.scenario==='needs_input'?`<label class="field"><span>你的回答</span><textarea data-flow-draft="answer" rows="4" maxlength="8000" placeholder="例如：使用 signing/orbital-macos，不把密钥写入源码。">${U.esc(view.answer||'')}</textarea></label><div class="row wrap">${action('保存演示回答','clarification-save','','primary','check')}<span class="muted small">切换标签后继续填写。最多 8000 个字符。</span></div>`:`<div class="callout"><strong>${view.answerSubmitted?'本地保存的回答':'已记录的澄清回答'}</strong><p>${U.esc(view.answerSubmitted?view.answer:'使用 signing/orbital-macos。Windows 使用项目已有代码签名引用。')}</p></div>`}<details class="df-platform-hints" data-flow-detail="clarification-history"><summary>上一份回答 · 修订 1</summary><p>使用项目凭据 signing/orbital-macos。Windows 使用项目已有代码签名引用。</p></details>`)}</div><aside class="stack">${U.panel(U.sectionTitle('当前规划统计')+U.kv('当前修订','2 · 第 2 代')+U.kv('确认调用','8 次')+U.kv('Token 用量',view.auditScenario==='budget'?'23,600 / 24,000':'18,420 / 24,000')+'<p class="muted small">统计为静态演示；历史修订不覆盖当前运行身份。</p>')}${U.panel(U.sectionTitle('规划上下文')+U.kv('固定提交','<code>e7a2c91</code>')+U.kv('计划身份',`<code>plan_demo_${revision}</code>`)+U.kv('目标覆盖','4 个原生目标 + 双架构镜像')+U.kv('修订来源',revision==='2'?'澄清后修订':'首次规划')+action('查看独立 AI 审计','build-tab','audit','secondary','eye'))}${U.panel(`<h3>正文与证据分开阅读</h3><p class="muted small">计划解释执行方案。AI 审计记录模型正文、工具参数、工具结果和身份关联。</p>`)}</aside></div>`;
}
export function renderBuildWorkspace(state, artifacts) {
  const view=buildView(state);
  const tab=view.tab==='logs'?'execution':view.tab;
  const tabs=[['process','构建过程'],['execution','执行'],['plan','计划'],['audit','AI 审计'],['artifacts','制品']];
  return `<div class="df-build" data-flow-view="build" data-flow-state-key="${U.esc(view.key)}"><div class="breadcrumb-inline">${U.link('项目构建','project/orbital/builds')}${U.icon('chevron',13)}<span>#1048</span></div>${U.pageHeader('ORBITAL / BUILD','构建 <span class="mono">#1048</span>',`Orbital Desktop · Release 2.4.0 ${U.badge(state.buildCancelled?'已取消':view.scenario==='needs_input'?'需要澄清':'执行中',state.buildCancelled?'neutral':view.scenario==='needs_input'?'warning':'running')}`,!state.buildCancelled?U.btn('取消构建','cancel-build','ghost','stop'):'')}<div class="build-source-bar"><span class="mono">${U.icon('branch',14)} release/2.4 · e7a2c91</span><span class="muted small">静态执行证据 · 本地交互演示</span></div><div class="tabs df-build-tabs" role="tablist" aria-label="构建详情">${tabs.map(([id,label])=>`<button type="button" class="tab ${tab===id?'active':''}" id="df-tab-${id}" role="tab" aria-controls="df-build-panel" aria-selected="${tab===id}" tabindex="${tab===id?0:-1}" data-flow-action="build-tab" data-flow-value="${id}">${label}</button>`).join('')}</div><section id="df-build-panel" role="tabpanel" aria-labelledby="df-tab-${tab}">${tab==='execution'?execution(state,view):tab==='plan'?plan(view):tab==='audit'?renderBuildAudit(view):tab==='artifacts'?artifacts():process(state,view)}</section></div>`;
}
