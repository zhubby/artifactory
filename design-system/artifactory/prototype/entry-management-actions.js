/**
 * [INPUT]: 依赖设置与项目字段、范围数据和应用弹层接口。
 * [OUTPUT]: 导出管理动作、具名表单和本地提交分发。
 * [POS]: 原型设置状态写入边界；敏感字段仅展示结构。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { projects } from './data.js';
import { getWorkspaceProjects, getWorkspaces, parseProjectSourceCredentials } from './entry-data.js';
import { projectFields, currentWorkspace } from './entry-management.js';
const { btn, field, input, esc, select, kv, table } = U;
const checks = (name, values, checked = []) => `<div class="entry-grants">${values.map(value => `<label><input class="checkbox" type="checkbox" name="${name}" value="${esc(value)}" ${checked.includes(value) ? 'checked' : ''}>${esc(value)}</label>`).join('')}</div>`;
function form(api, id, title, fields, label = '保存演示') { api.formModal(id, title, fields, label, { wide: true }); }
export function managementAction(name, el, state, api) {
    if (name === 'management-section') { const project = el.dataset.projectScope === 'true'; state[project ? 'projectSettingsSection' : 'managementSection'] = el.dataset.value; api.persist(); api.go(`${project ? `project-settings/${state.projectId}` : 'settings'}/${el.dataset.value}`); return true; }
    if (name === 'new-project') { form(api, 'new-project', '连接下一份源码', `<p class="notice mb">项目属于 ${esc(state.workspace)}。标识由系统生成。</p>${projectFields(state)}`, '连接项目'); return true; }
    if (name === 'manage-invitation' || name === 'invite') { form(api, 'manage-invitation', '创建工作空间邀请', field('工作空间角色', select('role', '成员', ['成员', '管理员'])) + field('有效天数', '<input class="input" name="days" type="number" min="1" max="30" value="7" required>') + '<p class="field-help">创建邀请链接，再由你分享给团队成员。此处不填写邮箱，不自动授予项目权限。</p>', '创建演示邀请链接'); return true; }
    if (name === 'manage-project-member') { form(api, 'manage-project-member', '授权项目成员', field('用户名', input('username', '', '已有工作空间成员的用户名')) + field('项目角色', select('role', '操作员', ['管理员', '操作员', '只读成员'])), '保存项目授权'); return true; }
    if (name === 'manage-webhook') { form(api, 'manage-webhook', '添加 Webhook 端点', `<input type="hidden" name="scope" value="${esc(el.dataset.scope)}">` + field('端点名称', '<input class="input" name="name" maxlength="100" required>') + field('接收地址', '<input class="input" name="url" type="url" placeholder="https://hooks.example/events" required>') + field('事件类型', input('event_types', 'build.status.changed, release.publication.changed')) + '<label class="row"><input class="checkbox" name="enabled" type="checkbox">启用此示例端点</label><p class="field-help">原型不发送请求。一次密钥为无访问能力的示例文本。</p>', '创建演示端点'); return true; }
    if (name === 'manage-token' || name === 'new-token') { const scoped = getWorkspaceProjects(state); form(api, 'manage-token', '创建项目访问令牌', field('令牌名称', '<input class="input" name="name" maxlength="100" required>') + '<fieldset><legend>项目范围</legend>' + checks('projects', scoped.map(project => project.name)) + '</fieldset><fieldset><legend>权限</legend>' + checks('scopes', ['read', 'build', 'release'], ['read']) + '</fieldset>' + field('有效天数', '<input class="input" name="days" type="number" min="1" max="365" value="30" required>') + '<p class="field-help">未选项目时只保留 read。真实权限以项目角色为上限。</p>', '生成一次示例'); return true; }
    if (name === 'test-connection') { api.notify('原型未连接服务，无法验证连通性。请在真实配置页执行验证。'); return true; }
    return false;
}
export function managementSubmit(id, values, state, api, formElement) {
    if (!id.startsWith('manage-')) return false;
    const data = Object.fromEntries(values), project = projects.find(project => project.id === state.projectId);
    const invalid = (fieldName, message) => { const control = formElement.elements.namedItem(fieldName); control?.setCustomValidity(message); control?.reportValidity(); api.notify(message, true); return true; };
    if (id === 'manage-project') {
        if (data.name && new TextEncoder().encode(data.name.trim()).length > 100) return invalid('name', '项目名称不超过 100 UTF-8 字节。');
        if (!project) return invalid('name', '请先选择当前工作空间中的项目。');
        if (data.repository_url) { let url; try { url = new URL(data.repository_url); } catch { return invalid('repository_url', '请输入完整的仓库地址。'); } if (!['https:', 'http:', 'ssh:'].includes(url.protocol) || url.password || (url.protocol !== 'ssh:' && url.username) || url.search || url.hash) return invalid('repository_url', '仓库 URL 不得包含密钥、查询参数或片段。'); }
        const next = { ...state.projectSettings?.[project.id], ...data };
        if (values.has('source_credentials')) { const parsed = parseProjectSourceCredentials(data.source_credentials); if (parsed.error) return invalid('source_credentials', parsed.error); next.source_credentials = parsed.rows; }
        if (formElement.querySelector('[name=node_access_mode]')) { next.node_access_ids = values.getAll('node_access_ids'); next.node_access = { mode: data.node_access_mode, ids: next.node_access_ids }; next.signing_profiles = String(data.signing_profiles || '').split(',').map(value => value.trim()).filter(Boolean); }
        if (data.name) next.name = data.name.trim();
        state.projectSettings = { ...state.projectSettings, [project.id]: next };
    } else if (id === 'manage-workspace') {
        const current = currentWorkspace(state), name = data.name.trim();
        if (!name || name.length > 100) return invalid('name', '请输入不超过 100 个字符的工作空间名称。');
        state.workspaceNames = { ...state.workspaceNames, [current.id]: name }; state.workspace = name;
    } else if (id === 'manage-ai') state.managementValues = { ...state.managementValues, [currentWorkspace(state).id]: { ai: data } };
    else if (id === 'manage-invitation') { state.demoInvitations = [...(state.demoInvitations || []), { id: `demo-${Date.now().toString(36)}`, workspaceId: currentWorkspace(state).id, role: data.role, expires: data.days }]; }
    else if (id === 'manage-project-member') state.demoMembers = [...(state.demoMembers || []), { ...data, projectId: state.projectId }];
    api.persist(); api.closeModal(true); api.render(); api.notify('设置已保存到当前演示会话。'); return true;
}
