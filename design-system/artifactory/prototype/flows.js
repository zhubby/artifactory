/**
 * [INPUT]: 依赖构建与发布视图、工作空间项目范围和入口版本草稿。
 * [OUTPUT]: 导出交付页面与项目范围列表复用的构建、制品快照。
 * [POS]: 组合交付路由；范围列表复用快照，跨项目聚合明确保留概念边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { renderBuildWorkspace } from './flow-build.js';
import { renderReleaseWorkspace } from './flow-release.js';
import { getEntryRelease, getWorkspaceProjects, getWorkspaces } from './entry-data.js';
import { builds as buildIndex, projects } from './data.js';
export const prototypeBuildRows = [...buildIndex.map((r, i) => [r[0].replace('#', ''), ...r.slice(1, 5), ({running:'签名中',success:'已成功',warning:'等待设备',danger:'已失败'})[r[6]], ...r.slice(6, 9), ['刚刚','24 分钟前','48 分钟前','1 小时前','2 小时前'][i]]),
    ['1043', 'Atlas API', '1.8.1', 'main', '2f90ae1', '已取消', 'neutral', '0 / 2', '00:43', '昨天'],
];
const buildRows = prototypeBuildRows;
export const prototypeArtifactFiles = [
    ['apple', 'orbital-desktop-2.4.0-macos-arm64.dmg', 'macOS · arm64', '128.4 MB', '已签名', '6ca9a2b1f82c'],
    ['apple', 'orbital-desktop-2.4.0-macos-amd64.dmg', 'macOS · amd64', '135.8 MB', '已签名', 'f8c1a306d74e'],
    ['windows', 'orbital-desktop-2.4.0-windows-amd64.exe', 'Windows · amd64', '97.2 MB', '已签名', '2d7b308c9a41'],
    ['linux', 'orbital-desktop-2.4.0-linux-amd64.AppImage', 'Linux · amd64', '112.6 MB', '已校验', 'b30fe7a2c914'],
];
const files = prototypeArtifactFiles;
const heading = (title, description, actions = '') => U.pageHeader('ORBITAL WORKSPACE / DELIVERY', title, description, actions);
const conceptBoundary = () => `<div class="notice"><span>${U.icon('layers',20)}</span><p><strong>概念聚合 · 跨项目</strong><br><span class="small muted">此入口演示跨项目浏览。项目范围的构建和制品从项目概览进入。</span></p></div>`;
const projectId = name => projects.find(p => p.name === name)?.id || 'orbital';
const objectLink = (label, type, id, cls = 'text-link', project = 'orbital') => `<a class="${cls}" href="#${type}/${U.esc(id)}${type === 'release' ? '/' + U.esc(project) : ''}" data-route="${type}/${U.esc(id)}${type === 'release' ? '/' + U.esc(project) : ''}" data-${type}="${U.esc(id)}" data-project="${U.esc(project)}">${label}</a>`;
const routeButton = (label, route, glyph = 'arrow', extra = '') => U.btn(label, '', 'secondary', glyph, `data-route="${route}" ${extra}`);
const toolbar = (placeholder, status = '全部状态') => `<div class="list-toolbar"><label class="search-field">${U.icon('search', 18)}<input data-search placeholder="${placeholder}" aria-label="${placeholder}"></label><div class="row gap">${U.select('status', status, ['全部状态', '运行中', '已成功', '已失败', '等待设备', '已取消'])}${U.btn('导出', 'export', 'ghost', 'download')}</div></div>`;
function scopedCreatedBuild(state) {
    const build=state.createdBuild;
    if(!build)return null;
    const workspace=getWorkspaces(state).find(item=>state.workspaceId?item.id===state.workspaceId:item.name===(state.workspace||'Orbital'));
    if(build.workspaceId&&build.workspaceId!==workspace?.id)return null;
    if(!build.workspaceId&&build.workspace&&build.workspace!==workspace?.name)return null;
    return getWorkspaceProjects(state).some(project=>build.projectId?project.id===build.projectId:project.name===build.project)?build:null;
}
function builds(state) {
    const query = (state.query || '').toLowerCase();
    const status = state.status || '全部状态';
    const created = scopedCreatedBuild(state), names=new Set(getWorkspaceProjects(state).map(project=>project.name));
    const rows = [...(created ? [['1049', created.project, created.version, created.ref, '待解析', '演示快照', 'neutral', `0 / ${created.targets.length}`, '未执行', '刚刚']] : []), ...buildRows].map(r => r[0] === '1048' && state.buildCancelled ? [...r.slice(0,5),'已取消','neutral',...r.slice(7)] : r).filter(r => names.has(r[1]) && r.join(' ').toLowerCase().includes(query) && (status === '全部状态' || (status === '运行中' && r[6] === 'running') || r[5] === status));
    return heading('构建中心', '从每一次提交，到每一个平台。所有构建都有迹可循。', U.btn('发起构建', 'new-build', 'primary', 'plus')) + conceptBoundary() +
        `<div class="grid-4">${U.metric('运行中的构建', '3', '跨 2 个项目', 'neutral')}${U.metric('今日构建', '28', '较昨日 +6')}${U.metric('构建成功率', '96.4%', '近 7 天 +2.1%')}${U.metric('平均用时', '8m 24s', '较上周缩短 18%')}</div>` +
        U.panel(U.sectionTitle('全部构建', `<span class="muted small">近 30 天 · 128 次构建</span>`) + toolbar('搜索构建编号、项目或提交', status) +
            (rows.length ? U.table(['构建 / 项目', '版本与源码', '状态', '目标', '耗时', '开始时间', ''], rows.map(r => [
                `<div class="row gap"><span class="list-icon ${r[6]}">${U.icon(r[6] === 'running' ? 'play' : r[6] === 'danger' ? 'warn' : 'check', 18)}</span><div>${objectLink(`#${r[0]}`, 'build', r[0], 'strong-link', projectId(r[1]))}<div class="muted small">${r[1]}</div></div></div>`,
                `<div>${objectLink(r[2], 'release', r[2], 'text-link', projectId(r[1]))}<div class="mono small muted">${r[3]} · ${r[4]}</div></div>`, U.badge(r[0] === '1048' && state.buildCancelled ? '已取消' : r[5], r[0] === '1048' && state.buildCancelled ? 'neutral' : r[6]), `<span class="mono">${r[7]}</span>`, `<span class="mono">${r[8]}</span>`, `<span class="muted small">${r[9]}</span>`, routeButton('查看', `build/${r[0]}`, 'chevron', `data-build="${r[0]}" data-project="${projectId(r[1])}"`)
            ])) : U.empty('没有匹配的构建', '换一个编号或状态，继续查找。')) +
            `<div class="table-footer"><span class="muted small">显示 ${rows.length} 条演示构建</span><span class="small muted">数据更新时间 · 14:32</span></div>`);
}
function artifactTable(buildId = '1047') {
    return U.table(['制品', '平台', '大小', '校验', ''], files.filter((r, i) => buildId !== '1048' || i > 0).map(r => [
        `<div class="row gap"><span class="list-icon">${U.icon(r[0], 20)}</span><div><button type="button" class="text-link artifact-name" data-action="artifact-detail" data-artifact="${r[1]}" data-build="${buildId}">${r[1]}</button><div class="small mono muted">sha256:${r[5]}…</div></div></div>`, r[2], `<span class="mono small">${r[3]}</span>`, U.badge(r[4], 'success'), U.btn('下载', 'download', 'ghost', 'download', `data-file="${r[1]}"`)
    ]));
}
function build(state) {
    const active = state.buildTab === 'logs' ? 'execution' : state.buildTab || 'process';
    const id = String(state.selectedBuild || '1048').replace('#','');
    if (id !== '1048') return buildSnapshot(state, id, active);
    return renderBuildWorkspace(state, () => U.panel(U.sectionTitle('已归档制品') + '<p class="muted">只显示此构建已完成的原生文件。完整镜像和正式交付分别记录。</p>' + artifactTable('1048')) + U.panel(U.sectionTitle('镜像候选') + U.kv('sync-service · amd64', '已完成 · sha256:84fb26c7…') + U.kv('sync-service · arm64', '排队中 · 尚无根摘要') + '<p class="muted small">镜像架构全部完成后才产生完整镜像索引。正式版本标签由 Release 发布创建。</p>'));
}
function buildSnapshot(state, id, active) {
    const created = id === '1049' ? scopedCreatedBuild(state) : null;
    const record = buildRows.find(r => r[0] === id);
    if (!created && !record) return heading(`构建 <span class="mono">#${U.esc(id)}</span>`, '当前演示未包含此构建。') + U.panel(U.empty('没有这个构建的快照', '返回当前项目，选择已提供的演示记录。', routeButton('返回项目构建', `project/${state.projectId || 'orbital'}/builds`, 'arrowLeft')));
    const project = created?.project || record[1], version = created?.version || record[2], ref = created?.ref || record[3];
    const kind = created ? 'neutral' : record[6], status = created ? '演示快照 · 未执行' : record[5];
    const summary = created ? '已保存本次选择。原型未解析源码或连接执行环境。' : kind === 'success' ? '本次构建已成功。候选制品与正式发布分别记录。' : kind === 'danger' ? '本次构建已失败。演示记录只包含状态，缺失的失败原因不作推测。' : kind === 'warning' ? '本次构建等待授权设备恢复。离线目标保留排队。' : '本次构建已取消。历史执行记录继续保留。';
    const tabs = U.tabs([{id:'process',label:'构建过程',action:'build-tab'},{id:'execution',label:'执行',action:'build-tab'},{id:'plan',label:'计划',action:'build-tab'},{id:'audit',label:'AI 审计',action:'build-tab'},{id:'artifacts',label:'制品',action:'build-tab'}], active);
    let detail;
    if (active === 'process') detail = U.panel(U.sectionTitle('构建状态', U.badge(status, kind)) + `<p>${summary}</p>` + (created ? U.kv('交付目标', created.targets.map(t => `<span class="chip">${U.esc(t)}</span>`).join(' ')) + U.kv('构建要求', U.esc(created.requirements)) : U.kv('完成目标', record[7]) + U.kv('耗时', record[8])));
    else if (active === 'artifacts' && id === '1047') detail = U.panel(U.sectionTitle('候选制品 · 构建 #1047') + '<p class="muted">4 个原生制品与 1 个双架构镜像已完成，正式发布后固定交付身份。</p>' + artifactTable());
    else detail = U.panel(U.empty(({execution:'未提供执行日志',plan:'未提供计划',audit:'未提供 AI 审计',artifacts:'未提供制品证据'})[active], created ? '本地快照未执行，因此没有日志、计划与制品。确认的源码引用、要求和目标保留在构建过程。' : '此演示记录没有保存这部分证据。构建身份和状态保持不变。'));
    return `<div class="breadcrumb-inline">${U.link('项目构建',`project/${created?.projectId || projectId(project)}/builds`)}${U.icon('chevron',13)}<span>#${U.esc(id)}</span></div>` + heading(`构建 <span class="mono">#${U.esc(id)}</span>`, `${U.esc(project)} · 版本 ${U.esc(version)} ${U.badge(status,kind)}`) + `<div class="build-source-bar"><span class="mono">${U.icon('branch',14)} ${U.esc(ref)}</span><span class="mono muted">${created ? '提交尚未解析' : U.esc(record[4])}</span></div>` + tabs + `<div class="grid-main-aside"><div class="stack">${detail}</div><aside class="stack">${U.panel(U.sectionTitle('构建快照') + U.kv('所属项目', U.link(U.esc(project),`project/${projectId(project)}`)) + U.kv('交付版本', objectLink(U.esc(version),'release',version,'text-link',projectId(project))) + U.kv('源码引用',U.esc(ref)) + U.kv('提交',created ? '未解析' : `<span class="mono">${U.esc(record[4])}</span>`) + U.kv('数据边界',created ? '仅浏览器本地保存' : '静态演示历史'))}</aside></div>`;
}
function releases(state) {
    const added = state.releaseVersion;
    const rows = [...(added ? [[added, '草稿', 'neutral', '尚未构建', '—', '刚刚']] : []), ['2.4.0', state.published ? '已发布' : '准备中', state.published ? 'success' : 'running', '2 次构建', '6 个目标', '今天'], ['2.3.8', '已发布', 'success', '构建 #1042', '6 个目标', '11 月 25 日'], ['2.3.1', '已发布', 'success', '构建 #1029', '6 个目标', '11 月 22 日'], ['2.3.0', '已发布', 'success', '构建 #1012', '6 个目标', '11 月 18 日'], ['2.3.0-rc.1', '已发布', 'success', '构建 #1006', '4 个目标', '11 月 16 日']];
    return heading('版本交付', '把一次可靠构建，交付为一个确定版本。', U.btn('创建版本', 'new-release', 'primary', 'plus')) +
        `<div class="release-feature content-surface panel"><div class="release-feature-icon">${U.icon('layers', 40)}</div><div><div class="eyebrow">NEXT RELEASE</div><h2>Orbital Desktop <span class="mono">2.4.0</span></h2><p class="muted">更快的搜索，全新的离线工作流。准备好下一次出发。</p><div class="row gap wrap">${U.badge(state.published ? '已发布' : '正在构建', state.published ? 'success' : 'running')}<span class="small muted">6 个交付目标 · 2 次构建</span></div></div>${routeButton('进入版本', 'release')}</div>` +
        U.panel(U.sectionTitle('版本历史', `<span class="muted small">Orbital Desktop · ${rows.length} 个版本</span>`) + U.table(['版本', '状态', '构建', '交付目标', '创建时间', ''], rows.map(r => [`<div>${objectLink(r[0], 'release', r[0], 'version-link')}<div class="muted small">${r[0].includes('rc') ? '预发布版本' : '稳定版本'}</div></div>`, U.badge(r[1], r[2]), r[3], r[4], `<span class="muted small">${r[5]}</span>`, routeButton('查看', `release/${r[0]}`, 'chevron', `data-release="${r[0]}" data-project="orbital"`)])) +
            `<div class="table-footer"><span class="muted small">版本号创建后固定。发布后交付内容不可覆盖。</span>${U.icon('lock', 16)}</div>`);
}
function release(state) {
    if ((state.projectId && state.projectId !== 'orbital') || (state.selectedRelease && state.selectedRelease !== '2.4.0')) return releaseSnapshot(state);
    return renderReleaseWorkspace(state, artifactTable);
}
function releaseSnapshot(state) {
    const version = state.selectedRelease, draft = getEntryRelease(state, state.projectId || 'orbital', version);
    const history = {'2.3.8':['1042','6','11 月 25 日'],'2.3.1':['1029','6','11 月 22 日'],'2.3.0':['1012','6','11 月 18 日'],'2.3.0-rc.1':['1006','4','11 月 16 日']};
    const known = (!state.projectId || state.projectId === 'orbital') && history[version], scope = projects.find(p => p.id === state.projectId);
    const current = scope ? (scope.version === version ? scope : null) : projects.find(p => p.version === version);
    const candidate = buildRows.find(r => r[2] === version && (!scope || r[1] === scope.name));
    const owner = known ? projects.find(p => p.id === 'orbital') : draft ? scope : current || projects.find(p => p.name === candidate?.[1]) || scope;
    const project = owner?.name || 'Orbital Desktop';
    const record = buildRows.find(r => r[2] === version && r[1] === project);
    const frozen = !!known || (!draft && current?.status === '已发布');
    const linkedBuild = known?.[0] || record?.[0];
    const targetCount = known?.[1] || record?.[7]?.split('/').at(-1).trim();
    const localBuild=scopedCreatedBuild(state);
    const created = draft && localBuild?.version === version && localBuild?.project === project ? localBuild : null;
    const releaseDirectory = `project/${owner?.id || state.projectId || 'orbital'}/releases`;
    const context = `<div class="breadcrumb-inline">${U.link('版本目录',releaseDirectory)}${U.icon('chevron',13)}<span>${U.esc(version)}</span></div>`;
    let body;
    if (frozen) body = U.panel(U.sectionTitle('已发布交付',U.badge('内容已冻结','success')) + '<p>版本已发布，交付内容不可覆盖。此样例未附完整证据，因此不提供替代清单。</p>' + U.kv('来源构建',linkedBuild ? `<span class="mono">#${linkedBuild}</span>` : '未提供') + U.kv('交付目标',targetCount ? `${targetCount} 个目标` : '未提供') + U.kv('发布时间',known?.[2] || '未提供') + U.kv('交付明细','此快照未提供') + (record ? objectLink('查看来源构建','build',linkedBuild,'text-link',projectId(project)) : ''));
    else if (draft) body = U.panel(U.sectionTitle('版本说明',U.btn('编辑说明','edit-release','ghost','file')) + `<p>${U.esc(draft.notes || '为下一次交付准备。')}</p>`) + (created ? U.panel(U.sectionTitle('构建快照') + '<p>本次选择已在浏览器保存，未执行真实构建。</p>' + objectLink('查看构建 #1049','build','1049','text-link',projectId(project))) : U.panel(U.empty('从一次构建开始交付','这个版本还没有构建。选择源码并描述目标，生成第一份构建方案。')));
    else body = U.panel(U.sectionTitle('版本快照',U.badge(record?.[5] || '未提供',record?.[6] || 'neutral')) + `<p>${record ? '此版本尚未发布。构建状态和对象身份来自本地演示记录。' : '当前原型未提供此版本的交付与构建证据。'}</p>` + (record ? objectLink(`查看构建 #${record[0]}`,'build',record[0],'text-link',projectId(project)) : routeButton('返回版本列表',releaseDirectory,'arrowLeft')));
    return context + heading(`${U.esc(project)} <span class="mono">${U.esc(version)}</span>`,frozen ? '已发布 · 正式交付不可覆盖。' : draft ? '草稿 · 版本号已固定，构建独立选择源码与目标。' : '版本与构建状态分别记录。',draft ? U.btn('发起构建','new-build','primary','plus') : '') + body;
}
function artifacts(state) {
    return heading('制品库', '每一个文件，都有来源、有签名、有校验。', U.btn('下载校验清单', 'download', 'primary', 'download', 'data-file="orbital-2.4.0-SHA256SUMS"')) + conceptBoundary() +
        `<div class="grid-3">${U.metric('已归档制品', '164', '覆盖 8 个项目')}${U.metric('制品总大小', '18.6 GB', '近 30 天 +2.4 GB', 'neutral')}${U.metric('已验证签名', '100%', '本次版本全部通过')}</div>` +
        U.panel(U.sectionTitle('Orbital Desktop · 2.4.0', `<div class="row gap">${U.badge(state.published ? '正式交付' : '构建候选', state.published ? 'success' : 'neutral')}${U.select('artifactVersion', '2.4.0', ['2.4.0'])}</div>`) + artifactTable() + `<div class="table-footer"><span class="small muted">4 个原生制品 · 合计 474.0 MB</span>${U.link('查看所属版本', 'release')}</div>`) +
        `<div class="grid-2">` + U.panel(`<div class="row gap">${U.icon('shield', 26)}<h2>可以验证的信任</h2></div><p class="muted">文件以 SHA-256 标识。签名验证结果与源码提交一同保留，让交付过程清晰可查。</p><div class="row gap wrap">${U.badge('Developer ID', 'success')}${U.badge('Authenticode', 'success')}${U.badge('SHA-256', 'success')}</div>`) + U.panel(`<div class="row gap">${U.icon('file', 26)}<h2>完整交付清单</h2></div><p class="muted">下载版本清单与校验文件，在本地复核文件身份、版本和来源。</p><div class="row gap">${U.btn('版本清单', 'view-manifest', 'secondary', 'code')}${U.btn('下载 SHA256SUMS', 'download', 'ghost', 'download', 'data-file="SHA256SUMS"')}</div>`) + `</div>`;
}
function registry(state) {
    return heading('镜像仓库', '从多架构构建，到摘要固定的可靠分发。', U.btn('连接外部仓库', 'distribute', 'primary', 'plus')) + conceptBoundary() +
        `<div class="grid-3">${U.metric('镜像组件', '12', '跨 5 个项目')}${U.metric('存储用量', '8.4 GB', '近 30 天 +1.2 GB', 'neutral')}${U.metric('今日拉取', '326', '较昨日 +24%')}</div>` +
        U.panel(U.sectionTitle('内建私有仓库', `<span class="row gap small">${U.icon('lock', 15)} 仅授权成员可拉取</span>`) + `<div class="registry-address"><code>registry.orbital.example / orbital</code>${U.btn('复制地址', 'copy', 'ghost', 'copy', 'data-copy="registry.orbital.example/orbital"')}</div>` +
            U.table(['组件与标签', '架构', '状态', '大小', '更新时间', ''], [
                [`<div class="row gap"><span class="list-icon blue">${U.icon('cube', 22)}</span><div><strong>sync-service</strong><p class="mono small muted">${state.published ? '2.4.0' : 'candidate-1047'} · sha256:84fb26c7…</p></div></div>`, `<div class="row gap">${U.badge('amd64')}${U.badge('arm64')}</div>`, U.badge(state.published ? '正式版本' : '候选镜像', state.published ? 'success' : 'neutral'), '<span class="mono">84.2 MB</span>', '24 分钟前', U.btn('查看摘要', 'view-manifest', 'ghost', 'eye', 'data-image="sync-service"')],
                [`<div class="row gap"><span class="list-icon">${U.icon('cube', 22)}</span><div><strong>atlas-api</strong><p class="mono small muted">1.8.2 · sha256:a84fb26c…</p></div></div>`, `<div class="row gap">${U.badge('amd64')}${U.badge('arm64')}</div>`, U.badge('正式版本', 'success'), '<span class="mono">42.8 MB</span>', '48 分钟前', U.btn('查看摘要', 'view-manifest', 'ghost', 'eye', 'data-image="atlas-api"')],
                [`<div class="row gap"><span class="list-icon">${U.icon('cube', 22)}</span><div><strong>beacon-gateway</strong><p class="mono small muted">1.2.0 · sha256:1dc08be2…</p></div></div>`, U.badge('amd64'), U.badge('正式版本', 'success'), '<span class="mono">31.6 MB</span>', '昨天', U.btn('查看摘要', 'view-manifest', 'ghost', 'eye', 'data-image="beacon-gateway"')]
            ])) +
        `<div class="grid-2">` + U.panel(U.sectionTitle('拉取镜像', U.btn('复制命令', 'copy', 'ghost', 'copy', `data-copy="docker pull registry.orbital.example/orbital/sync-service:${state.published ? '2.4.0' : 'candidate-1047'}"`)) + `<pre class="code-block"><code>docker pull registry.orbital.example/\
orbital/sync-service:${state.published ? '2.4.0' : 'candidate-1047'}</code></pre><p class="small muted">使用项目范围的只读访问令牌登录仓库。演示地址不提供真实服务。</p>`) +
        U.panel(U.sectionTitle('外部仓库', U.btn('配置分发', 'distribute', 'ghost', 'globe')) + `<div class="row gap"><span class="list-icon">${U.icon('globe', 24)}</span><div><strong>团队容器仓库</strong><p class="small muted">ghcr.io/orbital-team · 凭据引用已配置</p></div>${U.badge('已连接', 'success')}</div><p class="muted small">只分发已发布版本。分发结果独立记录，保留原始镜像摘要。</p>`) + `</div>`;
}
export function renderFlow(route, state = {}) {
    const pages = { builds, build, releases, release, artifacts, registry };
    return pages[route] ? pages[route](state) : null;
}
