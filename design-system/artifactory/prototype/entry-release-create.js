/**
 * [INPUT]: 依赖项目授权快照、版本草稿与应用导航弹层接口。
 * [OUTPUT]: 提供独立版本创建页、SemVer 校验与未保存保护。
 * [POS]: 项目版本入口；版本和说明归属 Release，构建字段独立。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { getWorkspaceProjects, getProjectDrafts, saveEntryRelease } from './entry-data.js';
const { esc, icon, btn, field, panel } = U;
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const getProject = state => getWorkspaceProjects(state).find(project => project.id === state.projectId);
const routeFor = project => `project/${project}/releases/new`;
const formDraft = (state, projectId) => state.releaseCreateDrafts?.[projectId] || { version: '', notes: '' };
let activeRoute = '', previousRoute = '', pendingRoute = '', bypassRoute = '', activeScope = null, pendingScope = null;
const scopeSnapshot = state => ({ projectId: state.projectId, projectTab: state.projectTab, releaseCreating: state.releaseCreating, workspace: state.workspace, workspaceId: state.workspaceId });
export function validateReleaseCreate(state, projectId, version) {
    const project = getWorkspaceProjects(state).find(item => item.id === projectId);
    if (!project) return '此项目不在当前工作空间。';
    const existing = [project.version, ...(projectId === 'orbital' ? ['2.4.0', '2.3.9', '2.3.8', '2.3.1', '2.3.0', '2.3.0-rc.1'] : []), ...getProjectDrafts(state, projectId).map(release => release.version)];
    if (version.length > 100 || !semver.test(version)) return '请输入有效的 SemVer，例如 2.5.0 或 2.5.0-rc.1。';
    if (existing.includes(version)) return '此版本已存在。版本号不能重复使用。';
    return '';
}
export function renderReleaseCreate(state, project) {
    const draft = formDraft(state, project.id), name = state.projectSettings?.[project.id]?.name || project.name;
    return `<div class="ep-release-create"><a class="ep-back-link" href="#project/${project.id}/releases" data-route="project/${project.id}/releases">${icon('arrowLeft', 15)}返回版本目录</a><div class="page-heading"><div><div class="eyebrow">${esc(name)} / NEW RELEASE</div><h1>新建版本</h1><p>固定一个版本号，记录这次交付的改变。</p></div></div>${panel(`<form data-form="release-create" data-project="${esc(project.id)}" class="ep-release-form" ${draft.dirty ? 'data-dirty="true"' : ''}><div class="ep-release-form-intro"><span class="app-icon">${icon('layers', 24)}</span><div><h2>为下一次交付命名</h2><p class="field-help">版本创建后不可改名。版本说明可以继续编辑。</p></div></div>${field('版本号', `<input class="input mono" name="version" value="${esc(draft.version)}" placeholder="例如 2.5.0 或 2.5.0-rc.1" required maxlength="100" aria-describedby="release-version-help release-create-error">`, '<span id="release-version-help">使用 SemVer。支持预发布标识与构建元数据。</span>')}${field('版本说明', `<textarea class="input" name="notes" rows="9" placeholder="这个版本为用户带来了什么？">${esc(draft.notes)}</textarea>`, '支持多行说明。源码引用、构建要求和目标在发起构建时选择。')}<p id="release-create-error" class="field-error" role="alert"></p><div class="ep-release-form-actions"><span class="small muted">${draft.dirty ? '有尚未保存的修改' : '只创建版本，尚未发起构建'}</span><div class="row">${btn('取消', 'release-create-cancel', 'ghost')}<button type="submit" class="btn primary">${icon('plus', 18)}创建版本</button></div></div></form>`, 'ep-release-create-panel')}</div>`;
}
function capture(form, state) {
    const projectId = form.dataset.project;
    state.releaseCreateDrafts ||= {};
    const draft = { version: form.elements.version.value, notes: form.elements.notes.value, dirty: true };
    state.releaseCreateDrafts[projectId] = draft; form.dataset.dirty = 'true';
    return draft;
}
export function bindReleaseCreate(root, state, api) {
    const form = root.querySelector('[data-form=release-create]');
    activeRoute = form ? routeFor(form.dataset.project) : '';
    activeScope = form ? { ...scopeSnapshot(state), projectId: form.dataset.project, projectTab: 'releases', releaseCreating: true } : null;
    previousRoute = location.hash.slice(1);
    if (!form) return;
    form.addEventListener('input', () => {
        capture(form, state); api.persist();
        form.querySelector('#release-create-error').textContent = '';
        form.querySelector('.ep-release-form-actions>span').textContent = '有尚未保存的修改';
        form.elements.version.setCustomValidity('');
    });
}
function dirty(state) { const projectId = /^project\/([^/]+)\/releases\/new$/.exec(activeRoute)?.[1]; return projectId && !!formDraft(state, projectId).dirty; }
function askLeave(to, api) {
    pendingRoute = to;
    api.showModal('release-create-leave', '离开版本草稿？', '<p>这个版本尚未创建。可以继续编辑，或放弃本次草稿后离开。</p>', btn('继续编辑', 'release-create-stay', 'secondary') + btn('放弃草稿并离开', 'release-create-discard', 'danger'));
}
export function guardReleaseNavigation(to, state, api) {
    if (to === bypassRoute) { bypassRoute = ''; return false; }
    if (!activeRoute || to === activeRoute || !dirty(state)) return false;
    pendingScope = scopeSnapshot(state);
    Object.assign(state, activeScope);
    askLeave(to, api); return true;
}
export function guardReleaseHashChange(state, api) {
    const to = location.hash.slice(1);
    if (!activeRoute || to === activeRoute || to === bypassRoute || !dirty(state)) { if (to === bypassRoute) bypassRoute = ''; return false; }
    const from = activeRoute || previousRoute;
    pendingScope = scopeSnapshot(state);
    Object.assign(state, activeScope);
    history.pushState(null, '', `#${from}`); askLeave(to, api); return true;
}
export function releaseCreateBeforeUnload(event, state) {
    if (!activeRoute || !dirty(state)) return;
    event.preventDefault(); event.returnValue = '';
}
export function handleReleaseCreateAction(name, el, state, api) {
    if (name === 'new-release') { const project = getProject(state); if (!project) { api.notify('先选择当前工作空间内的项目。', true); return true; } api.go(routeFor(project.id)); return true; }
    if (name === 'release-create-cancel') { api.go(`project/${state.projectId}/releases`); return true; }
    if (name === 'release-create-stay') { pendingRoute = ''; pendingScope = null; api.closeModal(true); document.querySelector('[data-form=release-create] [name=version]')?.focus(); return true; }
    if (name === 'release-create-discard') {
        const projectId = /^project\/([^/]+)\/releases\/new$/.exec(activeRoute)?.[1];
        if (projectId && state.releaseCreateDrafts) delete state.releaseCreateDrafts[projectId];
        const to = pendingRoute || `project/${state.projectId}/releases`; pendingRoute = ''; activeRoute = ''; bypassRoute = to;
        if (pendingScope) Object.assign(state, pendingScope); pendingScope = null;
        api.persist(); api.closeModal(true); api.go(to); return true;
    }
    return false;
}
export function handleReleaseCreateSubmit(id, values, state, api, form) {
    if (id !== 'release-create') return false;
    const project = getProject(state), version = String(values.get('version') || ''), notes = String(values.get('notes') || '');
    if (!project) return true;
    const lifecycle = state.demoLifecycle?.[`project:${project.id}`] || 'active';
    const workspaceStatus = state.demoLifecycle?.[`workspace:${state.workspaceId || 'orbital'}`] || 'active';
    const error = lifecycle !== 'active' || workspaceStatus !== 'active' ? '当前范围只读，不能创建版本。' : validateReleaseCreate(state, project.id, version);
    if (error) { form.querySelector('#release-create-error').textContent = error; form.elements.version.focus(); return true; }
    saveEntryRelease(state, project.id, version, notes);
    state.releaseVersion = version; state.releaseProjectId = project.id; state.selectedRelease = version; state.releaseNotes = notes;
    if (state.releaseCreateDrafts) delete state.releaseCreateDrafts[project.id];
    activeRoute = ''; pendingRoute = ''; api.persist(); api.go(`release/${version}/${project.id}`); api.notify(`演示版本 ${version} 已创建。`); return true;
}
