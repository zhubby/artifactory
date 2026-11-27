/**
 * [INPUT]: 依赖当前工作空间项目、交付快照和本地创建构建。
 * [OUTPUT]: 导出限定单个项目的构建与制品列表。
 * [POS]: 补齐项目概览的范围入口，不复用跨项目概念聚合。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { getWorkspaceProjects } from './entry-data.js';
import { prototypeBuildRows, prototypeArtifactFiles } from './flows.js';

const orderOptions = ['最新在前', '最早在前'];
const routeLink = (text, route, cls = 'text-link') => U.link(text, route, cls);
const sourceName = project => project.source_name || project.name;
const projectName = (state, project) => state.projectSettings?.[project.id]?.name || project.name;
const activeProject = (state, project) => getWorkspaceProjects(state).find(item => item.id === project?.id);
function unavailable() {
    return U.pageHeader('PROJECT / UNAVAILABLE', '这个项目不在当前工作空间。', '回到项目目录，选择可以访问的项目。') + U.panel(U.empty('项目不可用', '项目构建与制品保持工作空间隔离。', routeLink('返回项目目录', 'projects')));
}
function listHeading(state, project, title, description) {
    return `<a class="ep-back-link" href="#project/${U.esc(project.id)}" data-route="project/${U.esc(project.id)}">${U.icon('arrowLeft', 15)}返回项目概览</a>${U.pageHeader(U.esc(projectName(state, project)), title, description)}`;
}
function toolbar(state, placeholder, count, status = '') {
    return `<div class="ep-delivery-toolbar glass toolbar-panel"><label class="search-field">${U.icon('search', 18)}<input class="input" data-search aria-label="${placeholder}" placeholder="${placeholder}" value="${U.esc(state.query || '')}"></label>${status}${U.select('project-delivery-order', state.projectDeliveryOrder || orderOptions[0], orderOptions)}<span class="small muted" role="status">${count} 条</span></div>`;
}
function localBuild(state, project) {
    const build = state.createdBuild;
    if (!build || (build.projectId ? build.projectId !== project.id : build.project !== sourceName(project))) return null;
    if (build.workspaceId && build.workspaceId !== (project.workspaceId || state.workspaceId || 'orbital')) return null;
    if (!build.workspaceId && build.workspace && build.workspace !== state.workspace) return null;
    return ['1049', sourceName(project), build.version, build.ref, '未解析', '演示快照', 'neutral', `0 / ${build.targets.length}`, '未执行', '刚刚'];
}
function projectBuilds(state, project) {
    const created = localBuild(state, project);
    return [...(created ? [created] : []), ...prototypeBuildRows.filter(row => row[1] === sourceName(project))].map(row => row[0] === '1048' && state.buildCancelled ? [...row.slice(0, 5), '已取消', 'neutral', ...row.slice(7)] : row);
}
export function renderProjectBuildList(state, project) {
    const scoped = activeProject(state, project);
    if (!scoped) return unavailable();
    const rows = projectBuilds(state, { ...scoped, source_name: project.source_name }), filter = state.projectBuildStatus || '全部状态';
    const query = (state.query || '').trim().toLowerCase();
    const visible = rows.filter(row => (filter === '全部状态' || row[5] === filter) && [row[0], row[2], row[3], row[4], row[5]].some(value => String(value).toLowerCase().includes(query)));
    visible.sort((first, second) => (Number(second[0]) - Number(first[0])) * (state.projectDeliveryOrder === orderOptions[1] ? -1 : 1));
    const status = U.select('project-build-status', filter, ['全部状态', ...new Set(rows.map(row => row[5]))]);
    return `<div class="ep-delivery-page">${listHeading(state, scoped, '构建', '只展示当前项目的构建。每次执行保留自己的版本、源码与目标。')}${toolbar(state, '搜索编号、版本、源码或状态', visible.length, status)}${U.panel(visible.length ? U.table(['构建 / 版本', '本次源码', '状态', '目标', '耗时', '开始时间', ''], visible.map(row => [
        `<strong>${routeLink(`#${row[0]}`, `build/${row[0]}`, 'strong-link')}</strong><div class="small mono">${routeLink(U.esc(row[2]), `release/${row[2]}/${scoped.id}`)}</div>`,
        `<div class="row gap small">${U.icon('branch', 13)}${U.esc(row[3])}</div><div class="mono small muted">${U.esc(row[4])}</div>`,
        U.badge(row[5], row[6]), `<span class="mono small">${U.esc(row[7])}</span>`, `<span class="mono small">${U.esc(row[8])}</span>`, `<span class="small muted">${U.esc(row[9])}</span>`, routeLink(`查看 #${row[0]}`, `build/${row[0]}`),
    ]), 'ep-delivery-table') : U.empty(rows.length ? '没有匹配的构建' : '还没有构建快照', rows.length ? '调整搜索或状态后继续查看。' : '从项目的版本目录创建版本，再发起第一次构建。', routeLink('查看版本', `project/${scoped.id}/releases`))) }<p class="ep-delivery-caption">${visible.length} 条本地样例，全部已显示。编号排序对应此组样例的创建顺序，不推算项目统计。</p></div>`;
}
function orbitalPublished(state, project) {
    const key = `${state.workspaceId || state.workspace || 'default'}/${project.id}/2.4.0`;
    return state.flowReleaseViews?.[key]?.status === 'published' || (!state.flowReleaseViews?.[key] && state.published === true);
}
export function renderProjectArtifactList(state, project) {
    const scoped = activeProject(state, project);
    if (!scoped) return unavailable();
    const known = scoped.id === 'orbital', published = known && orbitalPublished(state, scoped), query = (state.query || '').trim().toLowerCase();
    const files = known ? prototypeArtifactFiles.filter(row => [row[1], row[2], '2.4.0'].some(value => value.toLowerCase().includes(query))) : [];
    const visible = files;
    const imageVisible = published && ['sync-service', '2.4.0', 'linux/amd64', 'linux/arm64'].some(value => value.includes(query));
    const native = visible.length ? U.table(['原生文件 / 目标', '大小', 'SHA-256', '信任证据', ''], visible.map(row => [
        `<button type="button" class="text-link ep-delivery-filename" data-action="artifact-detail" data-artifact="${U.esc(row[1])}" data-build="1047">${U.esc(row[1])}</button><div class="small muted">${U.esc(row[2])} · ${routeLink('2.4.0', `release/2.4.0/${scoped.id}`)}</div>`,
        `<span class="mono small">${row[3]}</span>`, `<span class="mono small" title="演示摘要缩写">${row[5]}…</span>`, U.badge(row[4], 'success'), U.btn('下载', 'download', 'ghost', 'download', `data-file="${U.esc(row[1])}"`),
    ]), 'ep-delivery-table') : U.empty(query && known ? '没有匹配的原生文件' : '尚未提供原生文件快照', query && known ? '换一个文件名、目标或版本继续查找。' : '此项目未附原生制品证据。原型不借用其他项目的文件。');
    const images = imageVisible ? U.table(['正式镜像', '平台', '根摘要', '来源', ''], [[
        '<strong>sync-service</strong><div class="mono small muted">registry.orbital.example/orbital/sync-service:2.4.0</div>', '<span class="small">linux/amd64<br>linux/arm64</span>', '<span class="mono small">sha256:84fb26c7…a102e9d4</span>', routeLink('版本 2.4.0', `release/2.4.0/${scoped.id}`), U.btn('查看摘要', 'view-manifest', 'ghost', 'eye', 'data-image="sync-service"'),
    ]], 'ep-delivery-table') : U.empty(published && query ? '没有匹配的正式镜像' : '正式镜像尚未提供', known && !published ? '候选镜像保留在构建详情。版本正式发布后才进入项目交付目录。' : '此项目未附正式镜像证据。', known && !published ? routeLink('查看候选构建 #1047', 'build/1047') : '');
    return `<div class="ep-delivery-page">${listHeading(state, scoped, '制品', '只展示当前项目的文件和正式镜像，保留版本与来源构建。')}${toolbar(state, '搜索文件名、目标或版本', visible.length + Number(imageVisible))}${U.panel(U.sectionTitle('原生文件', known ? U.badge(published ? '正式交付' : '构建制品', published ? 'success' : 'neutral') : '') + native + (known ? `<p class="ep-delivery-caption">来源 ${routeLink('构建 #1047', 'build/1047')}。同次构建的文件保持清单顺序。文件、签名和摘要为静态样例。</p>` : ''))}${U.panel(U.sectionTitle('正式镜像') + images)}<p class="ep-delivery-caption">当前样例全部已显示。摘要缩写不能用于实际验证或拉取。</p></div>`;
}
