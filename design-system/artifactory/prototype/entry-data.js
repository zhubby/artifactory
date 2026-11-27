/**
 * [INPUT]: 依赖 data.js 项目索引与应用保存的工作空间和关注状态。
 * [OUTPUT]: 导出入口校验、范围创建、源码凭据验证与版本草稿。
 * [POS]: 登录到项目路径的本地演示数据；页面不写入共享索引。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { projects } from './data.js';

const workspaces = [
    { id: 'orbital', name: 'Orbital', initial: 'O', color: 'blue', description: '让代码与团队，抵达同一个方向。', role: '所有者', projectIds: ['orbital', 'atlas', 'beacon', 'docs'] },
    { id: 'northstar', name: 'Northstar Studio', initial: 'N', color: 'purple', description: '设计系统与体验工程的共同工作空间。', role: '成员', projectIds: [] },
];

export function getWorkspaces(state = {}) {
    return [...workspaces, ...(state.customWorkspaces || [])].map(workspace => ({ ...workspace, name: state.workspaceNames?.[workspace.id] || workspace.name }));
}

export function validateEntryRegistration(state, values) {
    const username = String(values.get('username') || '').trim(), password = String(values.get('password') || '');
    const bytes = new TextEncoder();
    if (!username || bytes.encode(username).length > 100) return { field: 'username', error: '请输入 1–100 字节的用户名。' };
    const length = bytes.encode(password).length;
    if (length < 12 || length > 256) return { field: 'password', error: '密码需要 12–256 字节，中文占多个字节。' };
    if (password !== String(values.get('confirmPassword') || '')) return { field: 'confirmPassword', error: '两次输入的密码不一致。' };
    if (state.registrationEnabled === false) return { error: '当前实例已关闭公开注册。' };
    return {};
}

export function createEntryWorkspace(state, values) {
    const name = String(values.get('name') || '').trim();
    if (!name || name.length > 100) return { field: 'name', error: '请输入不超过 100 个字符的工作空间名称。' };
    // 本地标识只用于原型路由；生产标识由服务端生成。
    const base = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 48).replace(/-$/, '') || 'workspace';
    const existing = getWorkspaces(state); let id = base, suffix = 2;
    while (existing.some(workspace => workspace.id === id)) id = `${base}-${suffix++}`;
    const workspace = { id, name, initial: name[0].toUpperCase(), color: 'green', description: '属于你和团队的下一段交付旅程。', role: '所有者', projectIds: [] };
    state.customWorkspaces = [...(state.customWorkspaces || []), workspace];
    return { workspace };
}

export function getWorkspaceProjects(state = {}, workspace = state.workspaceId || state.workspace || 'Orbital') {
    const spaces = getWorkspaces(state), owner = spaces.find(item => item.id === workspace) || spaces.find(item => item.name === workspace);
    if (!owner) return [];
    return projects.filter(project => project.workspaceId ? project.workspaceId === owner.id : owner.projectIds?.includes(project.id) || (project.workspace === owner.name && spaces.find(item => item.name === project.workspace)?.id === owner.id));
}

export function isProjectFavorite(state, projectId) {
    return state.projectFavorites?.[projectId] ?? (projectId === 'orbital' ? state.favorite ?? true : false);
}

export function restoreEntryProjects(state) {
    for (const project of state.entryProjects || []) {
        if (!projects.some(item => item.id === project.id)) projects.push(project);
    }
}

export function parseProjectSourceCredentials(raw) {
    let rows; try { rows = JSON.parse(String(raw || '[]')); } catch { return { error: '额外源码凭据需为 JSON 数组。' }; }
    if (!Array.isArray(rows)) return { error: '额外源码凭据需为 JSON 数组。' };
    for (const row of rows) { if (!row || typeof row.repository_url !== 'string' || typeof row.credential_id !== 'string' || !row.credential_id.trim()) return { error: '每个绑定需要 repository_url 与 credential_id。' }; let url; try { url = new URL(row.repository_url); } catch { return { error: '源码绑定需要完整仓库地址。' }; } if (!['https:', 'http:', 'ssh:'].includes(url.protocol) || url.password || url.protocol !== 'ssh:' && url.username || url.search || url.hash) return { error: '源码绑定 URL 不得包含密钥、查询参数或片段。' }; }
    return { rows: rows.map(row => ({ repository_url: row.repository_url, credential_id: row.credential_id.trim() })) };
}

export function createEntryProject(state, values) {
    const sourceBindings = parseProjectSourceCredentials(values.get('source_credentials'));
    if (sourceBindings.error) return { field: 'source_credentials', error: sourceBindings.error };
    const name = String(values.get('name') || '').trim();
    const repository = String(values.get('repository_url') || '').trim();
    if (!name) return { field: 'name', error: '请输入项目名称。' };
    if (new TextEncoder().encode(name).length > 100) return { field: 'name', error: '项目名称不超过 100 UTF-8 字节。' };
    let url;
    try { url = new URL(repository); } catch { return { field: 'repository_url', error: '请输入完整的仓库地址。' }; }
    if (!['https:', 'http:', 'ssh:'].includes(url.protocol) || (url.protocol !== 'ssh:' && (url.username || url.password)) || url.password || url.search || url.hash) return { field: 'repository_url', error: '使用不含密钥、查询参数和片段的 HTTP、HTTPS 或 SSH 仓库地址。' };
    const base = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 48).replace(/-$/, '') || 'project';
    let slug = base, suffix = 2; while (getWorkspaceProjects(state).some(project => (project.slug || project.id) === slug)) slug = `${base}-${suffix++}`;
    const project = { id: `local-${Date.now().toString(36)}-${crypto.randomUUID().slice(0, 8)}`, slug, createdAt: Date.now(), workspace: state.workspace || 'Orbital', workspaceId: state.workspaceId || getWorkspaces(state).find(item => item.name === state.workspace)?.id || 'orbital', name, initial: name.slice(0, 1).toUpperCase(), color: 'purple', repo: url.pathname.replace(/^\//, '').replace(/\.git$/, ''), repository, credential: String(values.get('credential_id') || ''), version: '尚无版本', status: 'active', kind: 'success', platforms: ['待配置'], builds: 0, build_count: 0, last_build: null };
    state.projectSettings = { ...state.projectSettings, [project.id]: { ...Object.fromEntries(values), node_access_ids: values.getAll('node_access_ids'), source_credentials: sourceBindings.rows, node_access: { mode: values.get('node_access_mode') || 'pools', ids: values.getAll('node_access_ids') }, signing_profiles: String(values.get('signing_profiles') || '').split(',').map(value => value.trim()).filter(Boolean) } };
    state.entryProjects = [...(state.entryProjects || []), project];
    projects.push(project);
    return { project };
}

export function getProjectDrafts(state, projectId) {
    const drafts = (state.entryReleases || []).filter(r => r.projectId === projectId);
    if (state.releaseVersion && state.releaseProjectId === projectId && !drafts.some(r => r.version === state.releaseVersion)) drafts.push({ projectId, version: state.releaseVersion, notes: state.releaseNotes || '' });
    return drafts;
}

export function getEntryRelease(state, projectId, version) {
    return getProjectDrafts(state, projectId).find(r => r.version === version);
}

export function saveEntryRelease(state, projectId, version, notes) {
    const previous = getWorkspaces(state).flatMap(w => getWorkspaceProjects(state, w.id)).flatMap(p => getProjectDrafts(state, p.id));
    state.entryReleases = [{ projectId, version, notes }, ...previous.filter(r => r.projectId !== projectId || r.version !== version)];
}
