/**
 * [INPUT]: 依赖工作空间标识、导航边界、项目索引与入口范围数据。
 * [OUTPUT]: 导出唯一分组设置导航、范围命令、面包屑与范围外空态。
 * [POS]: 对齐生产工作空间和项目层级；由 app.js 组合玻璃外壳。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { projects, builds } from './data.js';
import { getWorkspaces, getWorkspaceProjects } from './entry-data.js';
import { icon, esc, pageHeader, panel, empty, btn } from './ui.js';

export function projectContext(route, state) {
    if (!['project', 'project-settings', 'release', 'build'].includes(route)) return null;
    const build = route === 'build' && builds.find(row => row[0] === `#${state.selectedBuild}`);
    const name = build?.[1] || (route === 'build' && state.selectedBuild === '1049' ? state.createdBuild?.project : null);
    const scoped = getWorkspaceProjects(state);
    return scoped.find(project => name ? project.name === name : project.id === state.projectId) || (!state.projectId && !name ? scoped[0] : null);
}

export function entryNavigation(route, state) {
    const p = projectContext(route, state);
    if (['settings', 'project-settings'].includes(route)) return entrySettingsGroups(state, p);
    if (p) return [{ group: '项目', items: [
        [`project/${p.id}`, 'grid', '项目概览'],
        [`project/${p.id}/releases`, 'layers', '版本'],
        [`project-settings/${p.id}`, 'settings', '管理项目'],
    ] }];
    return [
        { group: '工作空间', items: [['overview', 'grid', '概览'], ['projects', 'folder', '项目'], ['nodes', 'monitor', '共享设备'], ['pools', 'server', '节点池']] },
        { group: '设置', items: [[workspaceAdmin(state) ? 'settings/general' : 'settings/tokens', 'settings', workspaceAdmin(state) ? '管理工作空间' : '访问令牌']] },
    ];
}

const workspaceAdmin = state => ['所有者', '管理员'].includes(getWorkspaces(state).find(item => item.id === state.workspaceId)?.role || '所有者');
export function entrySettingsGroups(state, project = null) {
    const base = project ? `project-settings/${project.id}` : 'settings';
    const rows = values => values.map(([id, glyph, label]) => [`${base}/${id}`, glyph, label]);
    const basics = { group: '基础设置', items: rows([['general','settings','基本信息'],['members','users','成员与邀请']]) };
    const danger = { group: '危险操作', items: rows([['lifecycle','folder','生命周期']]) };
    if (project) return [basics, { group: '构建与交付', items: rows([['resources','monitor','资源与凭据'],['signing','shield','默认签名'],['integrations','external','Webhook']]) }, danger];
    const tokens = ['tokens','key','访问令牌'];
    if (!workspaceAdmin(state)) return [{ group: '访问', items: rows([tokens]) }];
    return [basics, { group: '构建与交付', items: rows([['ai','spark','AI 规划'],['emissary','refresh','Emissary 升级'],['integrations','external','Webhook'],['signing','shield','签名']]) }, { group: '访问', items: rows([tokens,['credentials','book','Git 凭据'],['audit','shield','审计']]) }, danger];
}

export function settingsSection(state, project = null, requested) {
    const groups = entrySettingsGroups(state, project), fallback = project || workspaceAdmin(state) ? 'general' : 'tokens';
    return groups.flatMap(group => group.items).some(([id]) => id.split('/').at(-1) === requested) ? requested : fallback;
}

export function entryCommands(route, state) {
    if (['tenants','notifications','verify','email-verify','unsubscribe'].includes(route)) return [{id:'tenants',glyph:'grid',label:'所有工作空间',group:'账户'}, {id:'notifications',glyph:'bell',label:'通知中心',group:'账户'}];
    const project = projectContext(route, state), scope = project ? state.projectSettings?.[project.id]?.name || project.name : state.workspace;
    const navigation = entryNavigation(project ? 'project' : 'projects', state), settings = entrySettingsGroups(state, project);
    const items = [...navigation.flatMap(group => group.items), ...settings.flatMap(group => group.items)].map(([id, glyph, label]) => ({ id, glyph, label, group: scope }));
    items.push(...getWorkspaceProjects(state).map(item => ({ id: `project/${item.id}`, glyph: 'folder', label: state.projectSettings?.[item.id]?.name || item.name, group: '当前工作空间的项目' })));
    if (project && !['archived','draining','deleting'].includes(state.demoLifecycle?.[`project:${project.id}`])) items.push({ id: `project/${project.id}/releases/new`, glyph: 'plus', label: '新建版本', group: scope });
    items.push({ id: 'projects', glyph: 'arrowLeft', label: '返回项目目录', group: state.workspace }, { id: 'tenants', glyph: 'grid', label: '所有工作空间', group: '账户' }, { id: 'notifications', glyph: 'bell', label: '通知中心', group: '账户' }, { id: 'docs', href: '../README.md', glyph: 'book', label: '文档', group: 'Artifactory' });
    return items.filter((item, index) => items.findIndex(other => other.id === item.id) === index);
}

export function entryNavActive(id, value, route, state) {
    const p = projectContext(route, state);
    if (['settings', 'project-settings'].includes(route)) return id === `${route}${p ? `/${p.id}` : ''}/${p ? state.projectSettingsSection : state.managementSection}`;
    if (!p) return route === id || (['node', 'terminal'].includes(route) && id === 'nodes');
    if (id.startsWith('project-settings/')) return route === 'project-settings';
    return id.endsWith('/releases') ? ['release', 'build'].includes(route) || (route === 'project' && state.projectTab === 'releases') : route === 'project' && state.projectTab !== 'releases';
}

export function entryScope(route, state) {
    const project = projectContext(route, state), workspace = getWorkspaces(state).find(w => state.workspaceId ? w.id === state.workspaceId : w.name === state.workspace);
    const name = (project && state.projectSettings?.[project.id]?.name) || project?.name || state.workspace || 'Orbital';
    const label = project ? '项目' : '工作空间';
    const settings = ['settings','project-settings'].includes(route), back = settings ? project ? `project/${project.id}` : 'projects' : project ? 'projects' : 'tenants', backLabel = settings ? project ? '返回项目' : '返回项目目录' : project ? '返回工作空间' : '所有工作空间';
    return `<a class="entry-scope-back text-link" href="#${back}" data-route="${back}">${icon('arrowLeft', 14)}${backLabel}</a><button type="button" class="workspace-switch" data-action="${project ? 'entry-switch-project' : 'workspace'}" title="${esc(name)} · 切换${label}"><span class="avatar ${project?.color || workspace?.color || 'blue'}">${esc(project?.initial || workspace?.initial || name[0])}</span><span><strong>${esc(name)}</strong><small>${label}${project ? ` · ${esc(state.workspace)}` : ` · ${esc(workspace?.role || '所有者')}`}</small></span>${icon('down', 15)}</button>`;
}

export function entryBreadcrumb(route, state, title) {
    const p = projectContext(route, state), parts = [`<a href="#tenants" data-route="tenants" class="crumb-tenant">${esc(state.workspace)}</a>`];
    if (p) parts.push(`<a href="#project/${esc(p.id)}" data-route="project/${esc(p.id)}" data-project="${esc(p.id)}">${esc(state.projectSettings?.[p.id]?.name || p.name)}</a>`);
    if (route === 'build') {
        const record = builds.find(row => row[0] === `#${state.selectedBuild}`), version = record?.[2] || state.createdBuild?.version;
        if (version && p) parts.push(`<a href="#release/${esc(version)}/${esc(p.id)}" data-route="release/${esc(version)}/${esc(p.id)}" data-project="${esc(p.id)}" data-release="${esc(version)}">${esc(version)}</a>`);
        parts.push(`<strong>#${esc(state.selectedBuild || '1048')}</strong>`);
    } else if (route === 'release') parts.push(`<strong>${esc(state.selectedRelease || '2.4.0')}</strong>`);
    else if (['settings','project-settings'].includes(route)) { const base = p ? `project-settings/${p.id}` : 'settings', section = p ? state.projectSettingsSection : state.managementSection; parts.push(`<a href="#${base}" data-route="${base}">${p ? '管理项目' : '管理工作空间'}</a>`); parts.push(`<strong>${esc(entrySettingsGroups(state,p).flatMap(group=>group.items).find(([id])=>id===`${base}/${section}`)?.[2] || '设置')}</strong>`); }
    else if (route === 'project' && state.releaseCreating && p) { parts.push(`<a href="#project/${p.id}/releases" data-route="project/${p.id}/releases">版本</a>`); parts.push('<strong>新建版本</strong>'); }
    else parts.push(`<strong>${esc(route === 'project' ? ({ releases: '版本', builds: '构建记录', artifacts: '制品' }[state.projectTab] || title || '工作台') : title || '工作台')}</strong>`);
    return parts.join(`<span aria-hidden="true">${icon('chevron', 12)}</span>`);
}

export function projectChoices(state) {
    return getWorkspaceProjects(state).map(p => `<button class="workspace-switch" data-action="entry-choose-project" data-project="${esc(p.id)}" style="margin:0"><span class="avatar ${p.color}">${esc(p.initial)}</span><span><strong>${esc(state.projectSettings?.[p.id]?.name || p.name)}</strong><small>${esc(p.repo)}</small></span>${icon('arrow', 17)}</button>`).join('');
}

/* --- 非演示资源范围保留空状态，不借用 Orbital 快照 --- */
export function scopedEmpty(route, state) {
    const scoped = getWorkspaceProjects(state);
    if (['project-settings', 'release', 'build'].includes(route) && !projectContext(route, state)) return pageHeader('PROJECT / UNAVAILABLE', '这个对象不在当前工作空间。', '返回项目目录，选择有权访问的项目。') + panel(empty('对象不可用', '工作空间之间的项目与构建保持隔离。', btn('返回项目目录', '', 'primary', 'folder', 'data-route="projects"')));
    if ((state.workspaceId || getWorkspaces(state).find(w => w.name === state.workspace)?.id) === 'orbital') return '';
    const names = { overview: '工作空间概览', nodes: '共享设备', node: '共享设备', terminal: '设备终端', pools: '构建池', builds: '构建记录', releases: '版本记录', artifacts: '制品', registry: '镜像仓库', observability: '观测' };
    if (!names[route]) return '';
    const label = names[route];
    return pageHeader('WORKSPACE / ' + route.toUpperCase(), route === 'overview' ? esc(state.workspace) : label, route === 'overview' ? `当前工作空间共有 ${scoped.length} 个项目。` : `${esc(state.workspace)} · 当前工作空间`) + panel(empty(route === 'overview' ? '下一次交付，从项目开始。' : `尚无${label}演示记录`, '此工作空间尚未提供此类数据。', btn(scoped.length ? '查看项目' : '连接第一个项目', scoped.length ? '' : 'new-project', 'primary', 'folder', scoped.length ? 'data-route="projects"' : '')));
}
