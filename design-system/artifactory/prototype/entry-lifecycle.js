/**
 * [INPUT]: 依赖工作空间、项目稳定身份与 LifecycleOperation 合同。
 * [OUTPUT]: 导出紧凑影响预览、显式确认、等待取消和失败重试演示。
 * [POS]: 生命周期的本地任务边界；密码不进入状态或浏览器存储。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { getWorkspaces, getWorkspaceProjects } from './entry-data.js';
const { btn, field, kv, esc, badge } = U;
function identity(state, scope, projectScope = false) {
    const workspace = getWorkspaces(state).find(item => item.id === state.workspaceId), project = projectScope ? getWorkspaceProjects(state).find(item => item.id === scope) : null;
    return { id: scope, workspaceId: workspace?.id, projectId: project?.id || null, slug: project?.slug || (scope === 'orbital' && project ? 'orbital-desktop' : project?.id) || workspace?.id || scope, name: state.projectSettings?.[project?.id]?.name || project?.name || workspace?.name || scope };
}
function impact(state, scope, projectScope = false) { const project = projectScope ? getWorkspaceProjects(state).find(item => item.id === scope) : null; return { projects: project ? 1 : getWorkspaceProjects(state).length, builds: project ? project.build_count ?? project.builds ?? 0 : getWorkspaceProjects(state).reduce((sum,item) => sum + (item.build_count ?? item.builds ?? 0),0), active_builds: scope === 'orbital' ? 1 : 0, releases: project?.version && project.version !== '尚无版本' ? 1 : 0 }; }
function impactSummary(counts) {
    const labels = { projects: '项目', builds: '构建', active_builds: '活动构建', releases: '版本' };
    return `<section class="entry-lifecycle-impact" aria-label="当前影响，本地快照"><h3>当前影响 · 本地快照</h3><dl>${Object.entries(counts).map(([key, value]) => `<div><dt>${labels[key] || esc(key)}</dt><dd>${esc(value)}</dd></div>`).join('')}</dl></section>`;
}
function buttons(scope, op) { return `${op.state === 'waiting' ? btn('取消等待操作', 'lifecycle-cancel', 'secondary', 'close', `data-scope="${scope}"`) : ''}${op.state === 'failed' ? btn('重试同一操作', 'lifecycle-retry', 'primary', 'refresh', `data-scope="${scope}"`) : ''}${['waiting','running'].includes(op.state) ? btn('演示本次执行', 'lifecycle-run', 'secondary', 'play', `data-scope="${scope}"`) + btn('演示执行失败', 'lifecycle-fail', 'ghost', 'warn', `data-scope="${scope}"`) : ''}`; }
function openOperation(state, api, scope) {
    const op = state.lifecycleOperations?.[scope]; if (!op) return;
    api.showModal('lifecycle-operation', '生命周期操作记录', `<div class="stack">${kv('对象', esc(op.name))}${kv('标识', esc(op.slug))}${kv('动作', ({archive:'归档',restore:'恢复',delete:'删除'})[op.action])}${kv('操作状态', badge(({waiting:'等待排空',running:'执行中',failed:'失败',succeeded:'已完成',canceled:'已取消'})[op.state], op.state === 'failed' ? 'danger' : op.state === 'succeeded' ? 'success' : 'warning'))}${impactSummary(op.impact)}${op.error ? `<p class="notice warning" role="alert">${esc(op.error)}</p>` : ''}<p class="field-help">所有状态均为本地样例。等待期间可以取消。失败重试沿用相同操作标识与动作，不创建另一个请求。生产按当前权限和真实任务决定排空。</p><div class="row wrap">${buttons(scope, op)}</div><details><summary>状态历史</summary>${op.history.map(item => `<p class="small">${esc(item)}</p>`).join('')}</details></div>`, btn('返回设置', 'close-modal', 'ghost'));
}
export function handleLifecycleAction(name, el, state, api) {
    if (name === 'manage-lifecycle') {
        const rawScope = el.dataset.scope, projectScope = el.dataset.projectScope === 'true', scope = `${projectScope ? 'project' : 'workspace'}:${rawScope}`, target = identity(state, rawScope, projectScope), verb = el.dataset.verb, counts = impact(state, rawScope, projectScope), existing = state.lifecycleOperations?.[scope];
        if (existing && ['waiting','running','failed'].includes(existing.state)) { openOperation(state, api, scope); return true; }
        api.formModal('lifecycle-confirm', `确认${({archive:'归档',restore:'恢复',delete:'删除'})[verb]}`, `<input type="hidden" name="scope" value="${esc(scope)}"><input type="hidden" name="raw_scope" value="${esc(rawScope)}"><input type="hidden" name="project_scope" value="${projectScope}"><input type="hidden" name="verb" value="${esc(verb)}">${kv('对象', esc(target.name))}${kv('稳定标识', `<code>${esc(target.slug)}</code>`)}${impactSummary(counts)}<p class="notice warning mt">${verb === 'delete' ? '删除请求先进入排空，不会立即抹除历史。请只输入虚构密码。' : '已有任务先保留，后续状态在操作记录中展示。'}</p>${verb === 'delete' ? field(`输入 ${target.slug} 以确认`, '<input class="input mono" name="slug" autocomplete="off" required>') + field('当前密码', '<input class="input" type="password" name="current_password" maxlength="256" autocomplete="current-password" placeholder="只填写虚构密码" required>') : ''}`, '提交本地操作'); return true;
    }
    if (name === 'lifecycle-current') { openOperation(state, api, el.dataset.scope); return true; }
    if (!name.startsWith('lifecycle-')) return false;
    const scope = el.dataset.scope, op = state.lifecycleOperations?.[scope]; if (!op) return true;
    if (name === 'lifecycle-cancel' && op.state === 'waiting') { op.state = 'canceled'; state.demoLifecycle[scope] = op.previous; op.history.push('等待操作已明确取消；对象恢复先前状态。'); }
    if (name === 'lifecycle-fail' && ['waiting','running'].includes(op.state)) { op.state = 'failed'; op.error = '本地失败样例：排空协调暂时中断。可以重试同一操作。'; op.history.push('示例执行失败，保留相同操作标识。'); }
    if (name === 'lifecycle-retry' && op.state === 'failed') { op.state = 'waiting'; op.error = ''; op.history.push('已重试同一操作，重新等待排空。'); }
    if (name === 'lifecycle-run' && op.state === 'waiting') { op.state = 'running'; op.history.push('进入本地执行阶段；生产不允许此时取消。'); }
    else if (name === 'lifecycle-run' && op.state === 'running') { op.state = 'succeeded'; state.demoLifecycle[scope] = op.action === 'archive' ? 'archived' : op.action === 'restore' ? 'active' : 'deleting'; op.history.push(op.action === 'delete' ? '本地排空演示完成，保持删除中；未声称真实数据已删除。' : '本地状态演示完成。'); }
    api.persist(); api.render(); openOperation(state, api, scope); return true;
}
export function submitLifecycle(id, values, state, api, form) {
    if (id !== 'lifecycle-confirm') return false;
    const scope = String(values.get('scope')), action = String(values.get('verb')), target = identity(state, String(values.get('raw_scope')), values.get('project_scope') === 'true');
    if (action === 'delete' && values.get('slug') !== target.slug) { const input = form.elements.namedItem('slug'); input.setCustomValidity('请输入一致的对象标识。'); input.reportValidity(); input.addEventListener('input',()=>input.setCustomValidity(''),{once:true}); return true; }
    const previous = state.demoLifecycle?.[scope] || 'active';
    state.lifecycleOperations = { ...state.lifecycleOperations, [scope]: { ...target, id: `operation-${crypto.randomUUID()}`, action, state: 'waiting', previous, impact: impact(state, String(values.get('raw_scope')), values.get('project_scope') === 'true'), history: ['已提交本地操作，等待排空。'], created_at: new Date().toISOString() } };
    state.demoLifecycle = { ...state.demoLifecycle, [scope]: action === 'restore' ? 'archived' : 'draining' }; api.persist(); api.render(); openOperation(state, api, scope); return true;
}
