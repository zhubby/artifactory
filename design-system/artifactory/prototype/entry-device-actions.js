/**
 * [INPUT]: 依赖设备快照、任务状态、浏览器表单与应用弹层。
 * [OUTPUT]: 提供重命名、有限操作、升级、终端与节点池动作。
 * [POS]: 设备原型的交互边界；所有阶段均为浏览器演示。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { getDevice, getPools, platformVersion, validateDeviceName } from './device-data.js';
import { requestOperation, advanceOperation, upgradePhases } from './device-operations.js';
const { btn, field, esc, kv } = U;
let pendingPaste = '', leaseTimer, authorizationTimer, authorizationRequest = '';
const clearAuthorization = () => { clearTimeout(authorizationTimer); authorizationRequest = ''; };
function endTerminal(state) {
    clearTimeout(leaseTimer); clearAuthorization(); state.terminalConnected = false;
    state.terminalOutput = ''; state.terminalLeaseDeadline = null; state.terminalAuthorizationDeadline = null;
}
function lease(state, api) {
    clearTimeout(leaseTimer); state.terminalLeaseDeadline = Date.now() + 45000;
    leaseTimer = setTimeout(() => { if (!state.terminalConnected) return; endTerminal(state); api.render(); api.notify('45 秒演示租约已到期，会话已结束。'); }, 45000);
}
export function syncDeviceTerminal(route, state) {
    if (route !== 'terminal' && !document.querySelector('[data-action=confirm-terminal]')) endTerminal(state);
}
function renameFields(device, draft = device.name, error = '') {
    const checked = validateDeviceName(draft);
    return `<input type="hidden" name="id" value="${device.key}">${field('设备名称', `<input class="input" name="name" value="${esc(draft)}" required aria-describedby="device-name-help device-name-error">`, `<span id="device-name-help">最多 100 个 UTF-8 字节；保存时移除 Unicode 首尾空白。</span>`)}<p id="device-name-error" class="field-error" role="alert">${esc(error)}</p><p class="field-help" id="device-name-bytes">${checked.bytes} / 100 字节</p>${btn('使用主机名', 'device-name-hostname', 'ghost', 'monitor')}<p class="field-help">主机名只填入草稿，仍需明确保存。</p>`;
}
function showRename(state, api, draft, error) {
    const device = getDevice(state); if (!device) return;
    state.deviceNameDraft = draft ?? device.name;
    api.formModal('device-name', `重命名 ${esc(device.name)}`, renameFields(device, state.deviceNameDraft, error), '保存名称');
    queueMicrotask(() => {
        const input = document.querySelector('[data-form=device-name] [name=name]');
        input?.focus(); input?.addEventListener('input', () => { state.deviceNameDraft = input.value; document.getElementById('device-name-bytes').textContent = `${new TextEncoder().encode(input.value).length} / 100 字节`; });
    });
}
export function handleDeviceClose(id, state, api) {
    if (id === 'terminal-connect') clearAuthorization();
    if (id === 'device-name-discard') { showRename(state, api, state.deviceNameDraft); return true; }
    if (id !== 'device-name') return false;
    const draft = document.querySelector('[data-form=device-name] [name=name]')?.value ?? state.deviceNameDraft;
    if (draft === getDevice(state)?.name) { state.deviceNameDraft = undefined; return false; }
    state.deviceNameDraft = draft;
    api.showModal('device-name-discard', '放弃名称草稿？', '<p>当前名称尚未保存。可以返回继续修改。</p>', btn('继续修改', 'device-name-resume', 'secondary') + btn('放弃修改', 'device-name-discard', 'danger'));
    return true;
}
export function handleDeviceAction(name, el, state, api) {
    const device = getDevice(state);
    if (name === 'device-close') { api.go('nodes'); return true; }
    if (name === 'node-tab') { state.nodeTab = el.dataset.value; api.render(); return true; }
    if (name === 'device-rename' || name === 'device-name-resume') { showRename(state, api, name === 'device-name-resume' ? state.deviceNameDraft : undefined); return true; }
    if (name === 'device-name-hostname') { const input = document.querySelector('[data-form=device-name] [name=name]'); if (input && device) { input.value = device.hostname; input.dispatchEvent(new Event('input', { bubbles: true })); input.focus(); } return true; }
    if (name === 'device-name-discard') { state.deviceNameDraft = undefined; api.closeModal(true); return true; }
    if (name === 'device-clear-filters' || name === 'device-clear-pool') { state.devicePoolFilter = null; if (name === 'device-clear-filters') state.query = ''; api.go('nodes'); return true; }
    if (name === 'device-filter-pool') { state.devicePoolFilter = el.dataset.id; api.go(`nodes/pool/${encodeURIComponent(el.dataset.id)}`); return true; }
    if (name === 'device-metadata-refresh' || name === 'device-operation-request') {
        if (device?.key !== 'studio') return true;
        const kind = name === 'device-metadata-refresh' ? 'refresh_inventory' : el.dataset.kind;
        if (!['refresh_inventory', 'collect_diagnostics'].includes(kind)) return true;
        requestOperation(state, device, kind); state.nodeTab = 'operations'; api.persist(); api.render(); api.notify('已创建本地请求；推进阶段查看设备确认与结果。'); return true;
    }
    if (name === 'device-operation-next' || name === 'device-operation-cancel') { if (!device) return true; advanceOperation(state, device, el.dataset.id, name.endsWith('cancel')); api.persist(); api.render(); return true; }
    if (name === 'device-operation-page') { state.deviceOperationPage = Math.max(0, Number(el.dataset.page)); api.render(); return true; }
    if (name === 'device-upgrade' || name === 'node-update') {
        if (device?.key !== 'studio' || device.version === platformVersion) { api.notify('设备与平台版本一致。'); return true; }
        state.deviceUpgradeConfirmation = { device: device.key, target: platformVersion, current: device.version };
        api.showModal('device-upgrade', '确认固定版本升级', `<p class="field-help mb">这是手动升级样例。确认后创建固定版本请求，不会连接设备。</p>${kv('设备', esc(device.name))}${kv('当前版本', esc(device.version ?? '未知'))}${kv('目标产品 / 版本', `Artifactory / ${platformVersion}`)}${kv('身份与配置', '保留设备证书、配置与设备标识')}${kv('过程', '空间检查 → 下载 → 验证 → 排空 → 切换 → 健康检查')}`, btn('取消', 'close-modal', 'ghost') + btn('确认并创建升级请求', 'device-upgrade-confirm', 'primary', 'download')); return true;
    }
    if (name === 'device-upgrade-confirm') {
        const confirmation = state.deviceUpgradeConfirmation;
        if (!device || confirmation?.device !== device.key || confirmation.target !== platformVersion || confirmation.current !== device.version) { api.notify('确认条件已改变，请重新检查版本。', true); return true; }
        state.deviceUpgrade = { device: device.key, target: platformVersion, request_id: crypto.randomUUID(), index: 0 };
        api.closeModal(true); api.persist(); api.render(); api.notify('已创建本地升级请求。'); return true;
    }
    if (name === 'device-upgrade-next') {
        const upgrade = state.deviceUpgrade;
        if (!upgrade || upgrade.canceled || upgrade.outcome || upgrade.device !== device?.key || upgrade.index >= upgradePhases.length - 1) return true;
        upgrade.index++;
        if (upgradePhases[upgrade.index][0] === 'succeeded') { state.deviceVersions ||= {}; state.deviceVersions[device.key] = upgrade.target; }
        api.persist(); api.render(); return true;
    }
    if (name === 'device-upgrade-fail' || name === 'device-upgrade-rollback') {
        const upgrade = state.deviceUpgrade, phase = upgrade && upgradePhases[upgrade.index][0];
        if (upgrade && !upgrade.canceled && !upgrade.outcome && ((name.endsWith('fail') && phase === 'verify') || (name.endsWith('rollback') && phase === 'health'))) { upgrade.outcome = name.endsWith('rollback') ? 'rolled_back' : 'failed'; api.persist(); api.render(); api.notify('已呈现失败样例，原设备版本保留。'); } return true;
    }
    if (name === 'device-upgrade-cancel') { const upgrade = state.deviceUpgrade; if (upgrade && ['queued', 'offered', 'download', 'drain'].includes(upgradePhases[upgrade.index][0])) { upgrade.canceled = true; api.persist(); api.render(); api.notify('本地升级取消已确认。'); } return true; }
    if (name === 'terminal-disconnect') { endTerminal(state); api.render(); api.notify('演示终端已结束。'); return true; }
    if (name === 'terminal-connect') {
        if (state.terminalConnected) { api.go('terminal/studio'); return true; }
        if (device?.key !== 'studio') return true;
        clearAuthorization(); authorizationRequest = crypto.randomUUID(); const request = authorizationRequest;
        state.terminalAuthorizationDeadline = Date.now() + 5000;
        api.showModal('terminal-connect', '授权一次临时终端连接', `<p class="notice warning mb">仅工作空间 owner/admin 的原登录 Session 可申请。此原型只模拟浏览器输出。</p>${kv('设备', `${esc(device.name)} · a7e4b912`)}${kv('固定 owner', 'linzhiyuan · 当前登录会话')}${kv('授权截止', '<span id="terminal-auth-deadline">请在 5 秒内确认；到期需重新申请。</span>')}${kv('会话租约', '45 秒；离开终端或明确结束即回收')}`, btn('取消', 'close-modal', 'ghost') + btn('明确授权并连接', 'confirm-terminal', 'primary', 'terminal'));
        authorizationTimer = setTimeout(() => { if (request !== authorizationRequest) return; const button = document.querySelector('[data-action=confirm-terminal]'); if (button) { button.disabled = true; document.getElementById('terminal-auth-deadline').textContent = '授权已过期。关闭后重新申请。'; } }, 5000); return true;
    }
    if (name === 'confirm-terminal') {
        if (!authorizationRequest || Date.now() >= state.terminalAuthorizationDeadline) { api.notify('授权已过期，请重新申请。', true); return true; }
        clearAuthorization(); state.nodeId = 'studio'; state.terminalConnected = true; state.terminalOutput = ''; api.closeModal(true); lease(state, api); api.go('terminal/studio'); return true;
    }
    if (name === 'terminal-paste') { api.closeModal(true); const input = document.querySelector('[data-form=terminal] [name=command]'); if (input) { input.value = pendingPaste; input.focus(); } pendingPaste = ''; return true; }
    if (name === 'new-pool' || name === 'rename-pool') {
        const pool = getPools(state).find(pool => pool.id === el.dataset.id);
        api.formModal('device-pool', pool ? '重命名节点池' : '创建节点池', `<input type="hidden" name="id" value="${esc(pool?.id || '')}">${field('节点池名称', `<input class="input" name="name" value="${esc(pool?.name || '')}" required aria-describedby="device-pool-error">`, '只保存名称。最多 100 个 UTF-8 字节。')}<p id="device-pool-error" class="field-error" role="alert"></p>`, '保存名称'); return true;
    }
    if (name === 'delete-pool' || name === 'confirm-delete-pool') {
        const pool = getPools(state).find(pool => pool.id === el.dataset.id);
        if (!pool || pool.is_default || (pool.node_count ?? 0) > 0) { api.notify('默认节点池或仍有设备的节点池不能删除。', true); return true; }
        if (name === 'delete-pool') { api.showModal('delete-pool', `删除 ${esc(pool.name)}？`, '<p>只删除当前本地节点池条目。设备身份与历史构建保留。</p>', btn('取消', 'close-modal', 'ghost') + btn('删除本地节点池', 'confirm-delete-pool', 'danger', 'close', `data-id="${esc(pool.id)}"`)); return true; }
        state.demoPools = state.demoPools.filter(item => item.id !== pool.id); api.persist(); api.closeModal(true); api.render(); api.notify('本地节点池已删除。'); return true;
    }
    return false;
}
export function handleDeviceSubmit(id, values, state, api, form) {
    if (id === 'device-name' || id === 'device-pool') {
        const checked = validateDeviceName(values.get('name'));
        const error = form.querySelector(id === 'device-name' ? '#device-name-error' : '#device-pool-error');
        if (checked.error) { if (error) error.textContent = checked.error; form.querySelector('[name=name]')?.focus(); return true; }
        if (id === 'device-name') {
            const device = getDevice(state, values.get('id')); if (!device) return true;
            state.deviceNames ||= {}; state.deviceNames[device.key] = checked.name; state.deviceNameDraft = undefined;
        } else {
            const key = values.get('id') || `pool-${crypto.randomUUID()}`, pool = getPools(state).find(item => item.id === key);
            state.demoPools = pool ? state.demoPools.map(item => item.id === key ? { ...item, name: checked.name } : item) : [...state.demoPools, { id: key, name: checked.name, is_default: false, node_count: 0, online_count: 0 }];
        }
        api.persist(); api.closeModal(true); api.render(); api.notify('名称已保存到当前本地演示。'); return true;
    }
    if (id !== 'terminal') return false;
    if (!state.terminalConnected || Date.now() > state.terminalLeaseDeadline) { endTerminal(state); api.render(); api.notify('演示会话已结束，请重新授权。', true); return true; }
    const command = String(values.get('command')).trim(), output = { help: '演示命令：help / pwd / uname -a / clear\n输出由浏览器模拟。', pwd: '/Users/orbital-builder/workspaces', 'uname -a': 'Darwin orbital-studio 24.2.0 arm64' };
    state.terminalOutput = command === 'clear' ? '' : `${state.terminalOutput || 'Artifactory · 本地演示终端\n'}\n$ ${command}\n${output[command] || '此模拟器仅支持 help、pwd、uname -a 与 clear。'}\n`;
    lease(state, api); api.render(); document.querySelector('[data-form=terminal] [name=command]')?.focus(); return true;
}
export function handleDevicePaste(event, state, api) {
    if (!event.target.matches('[data-form=terminal] [name=command]')) return;
    event.preventDefault(); pendingPaste = event.clipboardData.getData('text/plain').slice(0, 4096);
    api.showModal('terminal-paste', '先确认粘贴内容', `<p class="field-help mb">确认只填入输入框。仍需明确发送，不自动执行。</p><pre class="code-block">${esc(pendingPaste)}</pre>`, btn('取消', 'close-modal', 'ghost') + btn('填入命令框', 'terminal-paste', 'primary', 'check'));
}
