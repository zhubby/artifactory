/**
 * [INPUT]: 依赖项目范围深链、共享视图、隔离发布状态与正式镜像分发视图。
 * [OUTPUT]: 导出草稿、发布中、失败重试与冻结交付的 Release 工作台。
 * [POS]: 只演示单个完整成功构建的发布；不调用仓库或发布服务。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import {renderDistribution} from './flow-distribution.js';

export function releaseView(state) {
  const key = `${state.workspaceId || state.workspace || 'default'}/${state.projectId || 'orbital'}/${state.selectedRelease || '2.4.0'}`;
  state.flowReleaseViews ||= {};
  const view = state.flowReleaseViews[key] ||= { status: state.published ? 'published' : 'draft', tab: 'overview', selected: '', frozen: state.published ? '1047' : '', confirmation: false, invalidated: false, error: '', scroll: {}, details: {} };
  view.key = key;
  return view;
}
const action = (label,name,value='',kind='secondary',glyph='') => U.btn(label,'',kind,glyph,`data-flow-action="${name}" data-flow-value="${U.esc(value)}"`);
const buildLink = (id,label=`#${id}`) => U.link(label,`build/${id}`,'strong-link');
const statusLabels = { draft:['草稿','neutral'],publishing:['发布中','running'],publish_failed:['发布失败','danger'],published:['已发布','success'] };

export function renderReleaseWorkspace(state, artifactTable) {
  const view=releaseView(state), status=view.status;
  const [label,tone]=statusLabels[status]||statusLabels.draft;
  const frozen=['publishing','publish_failed','published'].includes(status);
  const selected=frozen?view.frozen:view.selected;
  const eligible=!view.invalidated;
  const tab=view.tab||'overview';
  const notes=state.orbitalReleaseNotes||'更流畅的跨平台体验，以及即使离线也能继续的工作方式。\n优化工作空间启动速度，修复高分屏下的字体清晰度问题。';
  const header=U.pageHeader('ORBITAL / RELEASE','<span class="mono">2.4.0</span>','版本号固定。每次构建独立选择源码和目标。',U.btn('编辑说明','edit-release','secondary','file')+(status==='draft'?U.btn('发起构建','new-build','primary','plus'):''));
  const selection=`<section class="panel content-surface df-release-selection"><div class="df-section-head"><div><h2>${frozen?'已选定交付构建':'选择一次完整成功构建'}</h2><p class="muted small">${status==='publish_failed'?'失败重试保持同一构建和摘要。':status==='published'?'正式交付不可覆盖。版本说明仍可编辑。':'全部原生目标与镜像架构成功后才能形成一次完整交付。'}</p></div>${U.badge(frozen?'交付身份已固定':'单构建交付',frozen?'success':'neutral')}</div>
    ${frozen?`<div class="callout ${status==='publish_failed'?'':'success'}"><strong>${U.icon('lock',18)} 构建 #${U.esc(view.frozen||'1047')}</strong><p>固定源码 e7a2c91 · 6 / 6 目标 · 4 个原生文件 + 1 个双架构镜像。</p></div>`:`<div class="df-candidate-list"><label class="df-candidate unavailable"><input type="radio" name="flow-release-candidate" value="1048" disabled><span><strong>构建 #1048</strong><small>执行中 · 4 / 6 目标。尚未完整成功。</small></span>${U.badge('不可选择','warning')}</label><label class="df-candidate ${eligible?'':'unavailable'}"><input type="radio" name="flow-release-candidate" value="1047" data-flow-candidate ${selected==='1047'?'checked':''} ${!eligible?'disabled':''}><span><strong>构建 #1047</strong><small>${eligible?'已成功 · 6 / 6 目标 · commit e7a2c91':'候选已失效。请等待有效构建后重新选择。'}</small></span>${U.badge(eligible?'完整成功':'已失效',eligible?'success':'danger')}</label></div>`}
    ${view.error?`<p class="df-inline-error" role="alert">${U.esc(view.error)}</p>`:''}
    ${status==='publishing'?`<div class="notice"><span class="df-static-progress">${U.icon('upload',20)}</span><p>正在演示按摘要核对、原生归档和镜像推广。页面返回后仍保留发布中状态。</p></div><div class="df-demo-outcome"><span>选择本地演示结果</span>${action('成功完成','publish-outcome','published','secondary','check')}${action('模拟发布失败','publish-outcome','publish_failed','ghost','warn')}</div>`:status==='published'?`<p class="df-caption">交付清单、校验文件与正式镜像引用已冻结。此页面只演示，不提供真实文件。</p>`:view.confirmation?`<div class="df-publish-confirm"><h3>${status==='publish_failed'?'重试同一份交付':'确认交付构建 #1047'}</h3><p class="muted small">${status==='publish_failed'?'继续使用构建 #1047，不重新选择候选。':'发布后固定此构建、源码清单与摘要。当前候选将在确认时再次校验。'}</p><label class="row small"><input type="checkbox" data-flow-publish-ack ${view.acknowledged?'checked':''}>我理解正式版本不可覆盖。</label><div class="row wrap">${action('返回检查','publish-close','','ghost')}${action(status==='publish_failed'?'确认重试发布':'确认正式发布','publish-confirm','','primary','upload')}</div></div>`:`<div class="row wrap">${action(status==='publish_failed'?'重试同一构建':'正式发布','publish-open','','primary','upload')}<span class="df-caption">${status==='publish_failed'?'候选 #1047 已冻结。':'只接受当前有效的完整成功构建。'}</span></div>`}
  </section>`;
  const overview=U.panel(U.sectionTitle('版本说明')+`<p class="df-release-notes">${U.esc(notes)}</p>`)+selection;
  const history=U.panel(U.sectionTitle('构建历史',U.badge('2 次演示构建'))+U.table(['构建','源码引用','结果','目标',''],[
    [buildLink('1048'),'<code>release/2.4 · e7a2c91</code>',U.badge(state.buildCancelled?'已取消':'执行中',state.buildCancelled?'neutral':'running'),'4 / 6',U.link('查看构建','build/1048')],
    [buildLink('1047'),'<code>release/2.4 · e7a2c91</code>',U.badge('已成功','success'),'6 / 6',U.link('查看构建','build/1047')],
  ])+`<p class="df-caption">构建的源码和目标属于本次执行。版本号与说明不承载这些选择。</p>`);
  const delivery=status!=='published'?U.panel(U.empty('正式交付尚未生成','完整成功构建的候选制品保留在构建详情。Release 发布完成后才提供正式交付。',U.link('查看候选构建 #1047','build/1047'))):U.panel(U.sectionTitle('正式原生制品',U.badge('来源构建 #1047','success'))+artifactTable('1047'))+U.panel(U.sectionTitle('正式容器镜像')+U.kv('组件','sync-service')+U.kv('固定标签','<code>registry.orbital.example/orbital/sync-service:2.4.0</code>')+U.kv('根摘要','<code>sha256:84fb26c7…a102e9d4</code>')+U.kv('平台','linux/amd64 · linux/arm64')+'<p class="muted small">摘要为静态缩写。不能用于实际验证或拉取。</p>')+renderDistribution(view);
  return `<div class="df-release" data-flow-view="release" data-flow-state-key="${U.esc(view.key)}"><div class="breadcrumb-inline">${U.link('版本目录',`project/${state.projectId || 'orbital'}/releases`)}${U.icon('chevron',13)}<span>2.4.0</span></div>${header}<div class="row wrap df-release-meta">${U.badge(label,tone)}<span class="small muted">2 次构建 · ${status==='published'?'4 个正式制品':'尚未形成正式交付'}</span></div><div class="tabs" role="tablist" aria-label="版本详情">${[['overview','概览'],['builds','构建历史'],['delivery','交付内容']].map(([id,text])=>`<button type="button" class="tab ${tab===id?'active':''}" id="df-release-tab-${id}" role="tab" aria-controls="df-release-panel" aria-selected="${tab===id}" tabindex="${tab===id?0:-1}" data-flow-action="release-tab" data-flow-value="${id}">${text}</button>`).join('')}</div><section id="df-release-panel" role="tabpanel" aria-labelledby="df-release-tab-${tab}" class="grid-main-aside"><div class="stack">${tab==='builds'?history:tab==='delivery'?delivery:overview}</div><aside class="stack">${U.panel(`<h3>${U.icon('lock',20)} 一次发布，永久确定</h3><p class="muted small">全部交付来自一个完整成功构建。失败重试保持同一构建。候选失效时清空选择，要求重新选择。</p>${U.kv('版本号','2.4.0 · 固定')}${U.kv('当前候选',selected?`#${U.esc(selected)}`:'尚未选择')}${U.kv('源码',selected?'<code>e7a2c91</code>':'随候选构建固定')}`)}<details class="df-scenario" data-flow-detail="release-scenario"><summary>原型状态演示</summary><p>以下只切换本地发布样例。</p>${action('草稿','release-scenario','draft')}${action('发布中','release-scenario','publishing')}${action('发布失败','release-scenario','publish_failed')}${action('已发布','release-scenario','published')}${status==='draft'?action(eligible?'使当前候选失效':'恢复候选样例','candidate-toggle','','ghost'):''}</details></aside></section></div>`;
}
