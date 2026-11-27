/**
 * [INPUT]: 依赖项目列表、版本样例、交付范围列表与完整窗口概览。
 * [OUTPUT]: 导出项目目录、概览、交付列表和独立版本页面。
 * [POS]: 目录只展示 Project 合同字段；版本身份不写为项目当前版本。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { builds } from './data.js';
import { getWorkspaceProjects, isProjectFavorite, getProjectDrafts } from './entry-data.js';
import { renderDashboard } from './entry-dashboard.js';
import { renderReleaseCreate } from './entry-release-create.js';
import { renderProjectBuildList, renderProjectArtifactList } from './entry-project-delivery.js';
const { icon, btn, badge, avatar, esc, panel, sectionTitle } = U;
const projectLink = (p, label, cls = 'text-link') => `<a class="${cls}" href="#project/${p.id}" data-route="project/${p.id}" data-project="${esc(p.id)}">${label}</a>`;
const releaseLink = (p, version, label = '查看版本') => `<a class="text-link" href="#release/${esc(version)}/${esc(p.id)}" data-route="release/${esc(version)}/${esc(p.id)}" data-project="${esc(p.id)}">${label}${icon('arrow', 15)}</a>`;
const projectBuilds = p => builds.filter(build => build[1] === p.name);
const repository = p => `<span class="ep-repo">${icon('git', 15)}<span class="mono">${esc(p.repository_url)}</span></span>`;
const favorite = (p, state) => `<button type="button" class="icon-button ep-favorite ${isProjectFavorite(state, p.id) ? 'is-favorite' : ''}" data-action="entry-favorite" data-project="${esc(p.id)}" aria-label="${isProjectFavorite(state, p.id) ? '取消关注' : '关注'} ${esc(p.name)}" aria-pressed="${isProjectFavorite(state, p.id)}">${icon('star', 19)}</button>`;
const projectWritable = (state, project) => project.lifecycle === 'active' && (state.demoLifecycle?.[`workspace:${state.workspaceId || 'orbital'}`] || 'active') === 'active';
const lifecycleLabels = { active: '活动', archived: '已归档', draining: '排空中', deleting: '删除中' };
export function projectSnapshot(p, state) {
    const last = projectBuilds(p)[0], times = { orbital: '2026-11-26 14:32', atlas: '2026-11-26 13:48', beacon: '2026-11-26 12:35' };
    return { ...p, source_name: p.name, name: state.projectSettings?.[p.id]?.name || p.name, slug: p.slug || (p.id === 'orbital' ? 'orbital-desktop' : p.id), repository_url: state.projectSettings?.[p.id]?.repository_url || p.repository || `https://git.example/${p.repo}.git`, build_count: p.build_count ?? p.builds ?? 0, lifecycle: state.demoLifecycle?.[`project:${p.id}`] || 'active', last_build: p.last_build ?? (last ? { number: last[0], status: last[5], kind: last[6], created_at: times[p.id] } : null) };
}
function card(p) {
    return `<article class="ep-card content-surface"><div class="ep-card-top"><div class="ep-card-identity">${avatar(p.initial, p.color)}<div><span class="ep-kicker">${esc(p.slug)}</span><h2>${projectLink(p, esc(p.name), 'ep-title-link')}</h2></div></div></div><div class="ep-directory-repo">${repository(p)}</div><div class="ep-card-meta">${badge(lifecycleLabels[p.lifecycle], p.lifecycle === 'active' ? 'neutral' : 'warning')}${badge(p.last_build?.status || lifecycleLabels[p.lifecycle], p.last_build?.kind || 'neutral')}<span class="small muted">${p.last_build ? `${esc(p.last_build.number)} · ${p.last_build.created_at}` : '尚无构建'}</span></div><div class="ep-card-bottom"><div><span>累计构建</span><strong>${p.build_count}</strong></div><span class="spacer"></span><a class="icon-button" href="#project-settings/${p.id}" data-route="project-settings/${p.id}" aria-label="管理 ${esc(p.name)}">${icon('settings', 18)}</a>${projectLink(p, icon('arrow', 19) + `<span class="sr-only">进入 ${esc(p.name)}</span>`, 'ep-card-enter')}</div></article>`;
}
function listRow(p) {
    return `<article class="ep-list-item"><div class="ep-list-name">${avatar(p.initial, p.color)}<div><span class="ep-kicker">${esc(p.slug)}</span><h2>${projectLink(p, esc(p.name), 'ep-title-link')}</h2>${repository(p)}</div></div><div class="ep-list-release">${badge(lifecycleLabels[p.lifecycle], p.lifecycle === 'active' ? 'neutral' : 'warning')}${badge(p.last_build?.status || lifecycleLabels[p.lifecycle], p.last_build?.kind || 'neutral')}<small>${p.last_build?.number || '尚无构建'}</small></div><div class="ep-list-numbers"><strong>${p.build_count}</strong><span>次构建</span></div><div class="ep-list-time">${p.last_build?.created_at || '—'}</div>${projectLink(p, `进入项目 ${icon('arrow', 16)}`)}</article>`;
}
function directoryFeature(p) {
    if (!p) return '';
    return `<section class="ep-feature content-surface"><div class="ep-feature-copy"><div class="ep-kicker">RECENT BUILD ACTIVITY / 最近构建活动</div><div class="ep-feature-heading">${avatar(p.initial, p.color)}<div><h2>${esc(p.name)}</h2><p>${repository(p)}</p></div></div><div class="ep-feature-facts">${badge(p.last_build.status, p.last_build.kind)}<span class="mono">${esc(p.last_build.number)}</span><span>${p.last_build.created_at}</span></div>${projectLink(p, `继续进入项目 ${icon('arrow', 16)}`)}</div><div class="ep-feature-object" aria-hidden="true" data-entry-scene><span class="ep-object-orbit"></span><span class="ep-object-tile glass">${icon('folder', 39)}</span><span class="ep-object-node glass">${icon('branch', 21)}</span><span class="ep-object-node last glass">${icon('box', 21)}</span><span class="ep-object-caption">SOURCE TO RELEASE</span></div></section>`;
}
export function renderProjectList(state) {
    const all = getWorkspaceProjects(state).map(p => projectSnapshot(p, state)), query = (state.query || '').trim().toLowerCase(), filter = state.projectFilter || '全部项目';
    const status = { 活动: 'active', 已归档: 'archived', 删除中: 'deleting' }[filter];
    const visible = all.filter(p => `${p.name} ${p.slug} ${p.repository_url}`.toLowerCase().includes(query) && (!status || p.lifecycle === status)).sort(state.projectSort === '名称 A–Z' ? (a, b) => a.name.localeCompare(b.name) : (a, b) => (Date.parse(b.last_build?.created_at || '') || 0) - (Date.parse(a.last_build?.created_at || '') || 0));
    const recent = [...all].filter(p => p.last_build).sort((a, b) => Date.parse(b.last_build.created_at) - Date.parse(a.last_build.created_at))[0], list = state.projectView === 'list';
    return `<div class="ep-directory"><div class="page-heading ep-page-heading"><div><div class="eyebrow">${esc(state.workspace)} / PROJECTS</div><h1>让每个想法，<br class="ep-heading-break">都有抵达的方向。</h1><p>从一份源码，到下一个值得交付的版本。</p></div><div class="page-actions">${btn('新建项目', 'new-project', 'primary', 'plus')}</div></div><div class="ep-directory-summary"><span><strong>${all.length}</strong>个项目</span><span><strong>${all.reduce((sum, p) => sum + p.build_count, 0)}</strong>次累计构建</span><span class="ep-summary-note">当前工作空间 · 项目快照</span></div>${directoryFeature(recent)}<div class="ep-collection-heading"><h2>所有项目 <span>${all.length}</span></h2><p>每个项目保留自己的源码与交付记录。</p></div><div class="ep-toolbar glass toolbar-panel" data-entry-glass><label class="search-field ep-search">${icon('search', 18)}<input class="input" data-search aria-label="搜索项目名称、标识或仓库" placeholder="搜索项目或仓库" value="${esc(state.query || '')}"></label>${U.select('project-filter', filter, ['全部项目', '活动', '已归档', '删除中'])}${U.select('entry-project-sort', state.projectSort || '最近构建', ['最近构建', '名称 A–Z'])}<div class="ep-view-switch" role="group" aria-label="项目显示方式"><button type="button" class="icon-button ${!list ? 'active' : ''}" data-action="entry-project-view" data-value="grid" aria-label="卡片视图" aria-pressed="${!list}">${icon('grid', 17)}</button><button type="button" class="icon-button ${list ? 'active' : ''}" data-action="entry-project-view" data-value="list" aria-label="列表视图" aria-pressed="${list}">${icon('menu', 17)}</button></div></div><p class="ep-result-count" role="status">找到 ${visible.length} 个项目</p>${visible.length ? `<div class="${list ? 'ep-project-list content-surface' : 'ep-project-grid'}">${visible.map(p => list ? listRow(p) : card(p)).join('')}</div>` : panel(U.empty(all.length ? '没有找到这个项目' : '第一个项目，从这里开始。', all.length ? '换一个名称，或清除筛选后再看看。' : '连接源码，为团队准备下一次交付。', all.length ? btn('清除筛选', 'clear-search', 'secondary', 'refresh') : btn('连接第一个项目', 'new-project', 'secondary', 'plus')))}<div class="ep-directory-foot"><span>${icon('shield', 16)}仅展示当前工作空间内已授权的项目。</span><a class="text-link" href="#tenants" data-route="tenants">切换工作空间 ${icon('arrow', 15)}</a></div></div>`;
}
export function comparePrototypeVersions(a, b) {
    const parse = value => { const version = value.split('+')[0], index = version.indexOf('-'); return index === -1 ? [version, ''] : [version.slice(0, index), version.slice(index + 1)]; };
    const [left, leftPre] = parse(a), [right, rightPre] = parse(b);
    const leftCore = left.split('.'), rightCore = right.split('.');
    const numeric = (x, y) => { const first = BigInt(x), second = BigInt(y); return first === second ? 0 : first > second ? 1 : -1; };
    for (let index = 0; index < 3; index++) { const order = numeric(leftCore[index], rightCore[index]); if (order) return order; }
    if (!leftPre && !rightPre) return 0;
    if (!leftPre || !rightPre) return leftPre ? -1 : 1;
    const first = leftPre.split('.'), second = rightPre.split('.');
    for (let index = 0; index < Math.max(first.length, second.length); index++) {
        const x = first[index], y = second[index]; if (x == null || y == null) return x == null ? -1 : 1;
        if (x === y) continue;
        const xNumeric = /^\d+$/.test(x), yNumeric = /^\d+$/.test(y);
        return xNumeric && yNumeric ? numeric(x, y) : xNumeric !== yNumeric ? xNumeric ? -1 : 1 : x < y ? -1 : 1;
    }
    return 0;
}
function releaseEntries(p, state) {
    const drafts = getProjectDrafts(state, p.id), entries = [];
    if (p.version && p.version !== '尚无版本') entries.push({ version: p.version, status: p.status === '已发布' ? 'published' : 'draft', notes: 'Release 示例快照' });
    if (p.id === 'orbital') entries.push({ version: '2.3.9', status: 'draft', notes: '构建 #1044 失败，保留构建证据' }, { version: '2.3.8', status: 'published', notes: '正式交付内容已冻结' });
    drafts.forEach(release => { const existing = entries.find(item => item.version === release.version); if (existing) existing.notes = release.notes; else entries.push({ version: release.version, status: 'draft', notes: release.notes || '' }); });
    return entries.map(entry => {
        const key = `${state.workspaceId || state.workspace || 'default'}/${p.id}/${entry.version}`;
        const view = state.flowReleaseViews?.[key];
        return { ...entry, status: view?.status || (p.id === 'orbital' && entry.version === '2.4.0' && state.published ? 'published' : entry.status), notes: p.id === 'orbital' && entry.version === '2.4.0' ? state.orbitalReleaseNotes || entry.notes : entry.notes, count: projectBuilds({ name: p.source_name }).filter(build => build[2] === entry.version).length };
    });
}
function renderProjectReleases(state, p) {
    const labels = { draft: '草稿', publishing: '发布中', publish_failed: '发布失败', published: '已发布' }, filter = state.releaseListStatus || '全部状态';
    const query = (state.query || '').trim().toLowerCase(), order = state.releaseListOrder || '版本从高到低';
    const entries = releaseEntries(p, state).filter(entry => (filter === '全部状态' || labels[entry.status] === filter) && `${entry.version} ${entry.notes}`.toLowerCase().includes(query)).sort((a, b) => comparePrototypeVersions(a.version, b.version) * (order === '版本从低到高' ? 1 : -1));
    return `<div class="ep-project-releases"><a class="ep-back-link" href="#project/${p.id}" data-route="project/${p.id}">${icon('arrowLeft', 15)}返回项目概览</a>${U.pageHeader(esc(p.name), '版本', 'SemVer 固定版本身份，版本说明可以继续编辑。', projectWritable(state, p) ? btn('新建版本', 'new-release', 'primary', 'plus') : '')}<div class="ep-release-toolbar glass toolbar-panel"><label class="search-field">${icon('search', 18)}<input class="input" data-search aria-label="搜索版本或说明" placeholder="搜索版本或说明" value="${esc(state.query || '')}"></label>${U.select('release-list-status', filter, ['全部状态', '草稿', '发布中', '发布失败', '已发布'])}${U.select('release-list-order', order, ['版本从高到低', '版本从低到高'])}<span class="small muted" role="status">${entries.length} 个版本</span></div>${panel(entries.length ? U.table(['版本号', '状态', '版本说明', '构建次数'], entries.map(entry => [releaseLink(p, entry.version, `<span class="mono">${esc(entry.version)}</span>`), badge(labels[entry.status], entry.status === 'published' ? 'success' : entry.status === 'publish_failed' ? 'danger' : entry.status === 'publishing' ? 'running' : 'neutral'), `<p class="ep-release-notes-preview">${esc(entry.notes || '尚无版本说明')}</p>`, entry.count])) : U.empty('没有匹配的版本', '创建一个独立版本，或调整搜索与筛选。', projectWritable(state, p) ? btn('新建版本', 'new-release', 'secondary', 'plus') : ''))}</div>`;
}
export function renderProject(state) {
    const scoped = getWorkspaceProjects(state), raw = scoped.find(p => p.id === (state.projectId || scoped[0]?.id));
    if (!raw) return U.pageHeader('PROJECT / UNAVAILABLE', '这个项目不在当前工作空间。', '回到项目目录，选择可以访问的项目。', btn('返回项目目录', '', 'primary', 'arrowLeft', 'data-route="projects"'));
    const p = projectSnapshot(raw, state);
    if (state.releaseCreating) return projectWritable(state, p) ? renderReleaseCreate(state, p) : U.pageHeader(esc(p.name), '当前范围只读', '活动工作空间和项目才能创建新版本。', btn('返回版本目录', '', 'secondary', 'arrowLeft', `data-route="project/${p.id}/releases"`));
    if (state.projectTab === 'releases') return renderProjectReleases(state, p);
    if (state.projectTab === 'builds') return renderProjectBuildList(state, p);
    if (state.projectTab === 'artifacts') return renderProjectArtifactList(state, p);
    return `<div class="ep-project"><a class="ep-back-link" href="#projects" data-route="projects">${icon('arrowLeft', 15)}所有项目</a><div class="page-heading ep-project-heading"><div class="ep-project-heading-main">${avatar(p.initial, p.color)}<div><div class="ep-kicker">${esc(state.workspace)} / ${esc(p.slug)}</div><div class="ep-name-row"><h1>${esc(p.name)}</h1>${favorite(p, state)}</div><p>${repository(p)}</p></div></div><div class="page-actions">${btn('管理项目', '', 'secondary', 'settings', `data-route="project-settings/${p.id}"`)}${projectWritable(state, p) ? btn('新建版本', 'new-release', 'primary', 'plus') : ''}</div></div><div class="ep-project-subline">${badge(lifecycleLabels[p.lifecycle], p.lifecycle === 'active' ? 'success' : 'warning')}<span>每次构建独立选择源码与目标</span></div><div class="ep-tab-content" data-entry-panel>${renderDashboard(state, p)}</div></div>`;
}
