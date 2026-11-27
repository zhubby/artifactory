/**
 * [INPUT]: 依赖设备演示快照、统一平台版本与 ui.js。
 * [OUTPUT]: 提供有限操作和升级视图，以及本地阶段推进。
 * [POS]: 设备任务原型；请求与结果分离，阶段不伪造百分比。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { platformVersion } from './device-data.js';
const { esc, btn, badge, panel, sectionTitle, kv, table, progress } = U;
const terminalStates = new Set(['succeeded', 'failed', 'canceled', 'expired', 'interrupted', 'rejected']);
const labels = { requested: '已请求', accepted: '设备已接收', running: '执行中', succeeded: '已完成', canceled: '已取消', failed: '失败', expired: '已过期', interrupted: '已中断', rejected: '已拒绝' };
const kinds = { collect_diagnostics: '收集诊断', refresh_inventory: '刷新清单' };
export const upgradePhases = [
    ['queued', '排队', '等待设备接收固定版本请求。'],
    ['offered', '已下发', '设备已接收升级请求。'],
    ['space_check', '空间检查', '检查可用空间与候选包要求。'],
    ['download', '下载', '字节进度仅表示安装包传输。'],
    ['verify', '验证', '校验候选包摘要与签名。'],
    ['verified', '已验证', '候选包等待安全切换。'],
    ['drain', '排空', '等待执行、日志、终端与有限操作归零。'],
    ['activating', '切换版本', '启用已验证的候选版本。'],
    ['health', '健康检查', '确认新进程与控制连接。'],
    ['succeeded', '完成', '设备上报新版本并通过健康检查。']
];
function records(state, device) {
    state.deviceOperations ||= {};
    state.deviceOperations[device.key] ||= [
        { id: 'operation-demo-03', request_id: '9fab2e6c-92e0-4620-806a-901ec69cfa43', kind: 'refresh_inventory', state: 'succeeded', delivery_state: 'confirmed', created_at: '14:27:18', finished_at: '14:27:19', result: { inventory: { observation_id: 'obs-c7e2', revision: 7, delivery: 'confirmed' } } },
        { id: 'operation-demo-02', request_id: 'd5486d4a-d887-4f63-a582-55ab50595791', kind: 'collect_diagnostics', state: 'succeeded', delivery_state: 'confirmed', created_at: '14:20:04', finished_at: '14:20:06', result: { probes: [{ id: 'git', status: 'available', observed_at: '14:20:05', duration_ms: 0, probe_version: '1', version: '2.47.1' }, { id: 'docker', status: 'unsupported', observed_at: '14:20:06', duration_ms: 4, error_code: 'probe_not_supported' }, { id: 'signing', status: 'permission_denied', observed_at: '14:20:06', duration_ms: 2, error_code: 'access_denied' }], skipped: ['secret_contents'], truncated: false, omitted: 0 } },
        { id: 'operation-demo-01', request_id: '928c33d4-fb9c-46f5-8171-e6b6a9845536', kind: 'collect_diagnostics', state: 'canceled', delivery_state: 'confirmed', created_at: '14:10:00', finished_at: '14:10:02', cancel_requested_at: '14:10:01' },
        { id: 'operation-demo-00', request_id: '7324cb81-8e0e-4c89-b78e-983c2b3f55e2', kind: 'collect_diagnostics', state: 'expired', delivery_state: 'confirmed', created_at: '13:56:10', finished_at: '13:57:10', reason: '执行期限已过' }
    ];
    return state.deviceOperations[device.key];
}
function resultView(operation) {
    if (!operation.result) return `<p class="field-help">${operation.cancel_requested_at ? `取消请求 ${operation.cancel_requested_at}；${operation.state === 'canceled' ? '设备已确认取消。' : '等待设备确认取消。'}` : '尚无结果。只有设备确认结果后才显示观测值。'}</p>`;
    const result = operation.result;
    if (result.inventory) return kv('观测 ID', `<code>${esc(result.inventory.observation_id)}</code>`) + kv('修订', esc(result.inventory.revision)) + kv('交付', esc(result.inventory.delivery)) + '<p class="field-help">此操作刷新观测清单，不改变启动时固定的调度快照。</p>';
    const statuses = { available: '可用', unsupported: '不支持', permission_denied: '无权限', timeout: '超时', error: '错误' };
    return table(['探测', '结果', '观测 / 耗时', '版本或错误'], result.probes.map(probe => [esc(probe.id), esc(statuses[probe.status] || probe.status), `${esc(probe.observed_at)} / ${esc(probe.duration_ms)} ms`, esc(probe.version ?? probe.error_code ?? '未知')])) + kv('跳过', esc(result.skipped.join('、') || '无')) + kv('结果截断', String(result.truncated)) + kv('省略项数', esc(result.omitted));
}
export function renderFiniteOperations(state, device) {
    const all = records(state, device), page = Math.min(state.deviceOperationPage || 0, Math.max(0, Math.ceil(all.length / 3) - 1));
    const entries = all.slice(page * 3, page * 3 + 3);
    return panel(sectionTitle('有限诊断操作', badge('node-operations-v1', 'neutral')) + '<p class="muted small">仅执行预定义探测。此原型由你逐步推进样例，不请求真实设备。</p>' + `<div class="row wrap device-task-controls">${btn('收集诊断', 'device-operation-request', 'secondary', 'activity', 'data-kind="collect_diagnostics"')}${btn('刷新清单', 'device-operation-request', 'secondary', 'refresh', 'data-kind="refresh_inventory"')}</div>` + entries.map(operation => `<article class="device-operation"><div class="row spread wrap"><strong>${kinds[operation.kind]}</strong>${badge(labels[operation.state] || operation.state, operation.state === 'succeeded' ? 'success' : terminalStates.has(operation.state) ? 'neutral' : 'running')}</div><p class="mono small device-request">request ${esc(operation.request_id)}</p>${kv('请求时间', esc(operation.created_at))}${kv('交付状态', esc(operation.delivery_state))}${operation.finished_at ? kv('结束时间', esc(operation.finished_at)) : ''}${operation.reason ? kv('原因', esc(operation.reason)) : ''}<details data-device-detail="operation-${operation.id}"><summary>结果与观测证据</summary>${resultView(operation)}</details>${!terminalStates.has(operation.state) ? `<div class="row wrap device-task-controls">${btn(operation.cancel_requested_at ? '演示设备确认取消' : '推进演示阶段', 'device-operation-next', 'secondary', 'arrow', `data-id="${esc(operation.id)}"`)}${operation.cancel_requested_at ? badge('取消待确认', 'warning') : btn('请求取消', 'device-operation-cancel', 'ghost', 'stop', `data-id="${esc(operation.id)}"`)}</div>` : ''}</article>`).join('') + `<div class="row spread wrap device-pagination"><span class="small muted">第 ${page + 1} 页 · 共 ${all.length} 条请求</span><div class="row">${btn('更新记录', 'device-operation-page', 'ghost', 'arrowLeft', `data-page="${page - 1}" ${page === 0 ? 'disabled' : ''}`)}${btn('更早记录', 'device-operation-page', 'ghost', 'arrow', `data-page="${page + 1}" ${(page + 1) * 3 >= all.length ? 'disabled' : ''}`)}</div></div>`);
}
export function requestOperation(state, device, kind) {
    const all = records(state, device), request = crypto.randomUUID();
    const operation = { id: `op-${request}`, request_id: request, kind, state: 'requested', delivery_state: 'pending', created_at: new Date().toLocaleTimeString('zh-CN', { hour12: false }) };
    all.unshift(operation); state.deviceOperationPage = 0;
    return operation;
}
export function advanceOperation(state, device, id, cancel = false) {
    const operation = records(state, device).find(item => item.id === id);
    if (!operation || terminalStates.has(operation.state)) return;
    if (cancel) {
        operation.cancel_requested_at = new Date().toLocaleTimeString('zh-CN', { hour12: false });
        return;
    }
    if (operation.cancel_requested_at) { operation.state = 'canceled'; operation.finished_at = new Date().toLocaleTimeString('zh-CN', { hour12: false }); operation.delivery_state = 'confirmed'; return; }
    const next = ({ requested: 'accepted', accepted: 'running', running: 'succeeded' })[operation.state];
    operation.state = next; operation.delivery_state = 'confirmed';
    if (next !== 'succeeded') return;
    operation.finished_at = new Date().toLocaleTimeString('zh-CN', { hour12: false });
    if (operation.kind === 'refresh_inventory') {
        state.deviceMetadataRevision = (state.deviceMetadataRevision || 7) + 1;
        state.deviceMetadataObserved = operation.finished_at;
        state.deviceMetadataObservationId = `obs-${operation.request_id.slice(0, 8)}`;
        operation.result = { inventory: { observation_id: state.deviceMetadataObservationId, revision: state.deviceMetadataRevision, delivery: 'confirmed' } };
    } else operation.result = { probes: [{ id: 'git', status: 'available', observed_at: operation.finished_at, duration_ms: 0, probe_version: '1', version: '2.47.1' }, { id: 'docker', status: 'unsupported', observed_at: operation.finished_at, duration_ms: 3, probe_version: '1', error_code: 'probe_not_supported' }, { id: 'codesign', status: 'available', observed_at: operation.finished_at, duration_ms: 12, probe_version: '1', version: '15.2' }], skipped: ['secret_contents'], truncated: false, omitted: 0 };
}
export function renderUpgrade(state, device) {
    const upgrade = state.deviceUpgrade?.device === device.key ? state.deviceUpgrade : null;
    const phase = upgrade && upgradePhases[upgrade.index];
    const canceled = upgrade?.canceled, outcome = upgrade?.outcome, complete = phase?.[0] === 'succeeded';
    const actionable = !upgrade || canceled || complete || outcome;
    const canCancel = phase && ['queued', 'offered', 'download', 'drain'].includes(phase[0]) && !canceled && !outcome;
    return panel(sectionTitle('Emissary 升级', badge(`平台 ${platformVersion}`, 'neutral')) + kv('设备上报版本', esc(device.version ?? '未知')) + kv('固定目标版本', platformVersion) + '<p class="field-help">统一版本由管理员手动确认。版本差异不会自动执行升级。设备证书、配置与身份保留。</p>' + (actionable ? `<div class="device-task-controls">${btn(device.version === platformVersion ? '与平台版本一致' : '检查并确认升级', 'device-upgrade', 'secondary', 'download', device.version === platformVersion ? 'disabled' : '')}</div>` : '') + (upgrade ? `<div class="device-upgrade-status"><div class="row spread"><strong>${canceled ? '已取消' : outcome === 'rolled_back' ? '已回滚' : outcome === 'failed' ? '升级失败' : phase[1]}</strong>${badge(canceled ? '取消已确认' : outcome === 'rolled_back' ? '原版本已恢复' : outcome === 'failed' ? '可重新确认重试' : complete ? '已完成样例' : `阶段 ${upgrade.index + 1} / ${upgradePhases.length}`, canceled ? 'neutral' : outcome ? 'warning' : complete ? 'success' : 'running')}</div><p class="muted small">${canceled ? '已停止此本地升级请求，设备版本保持原值。' : outcome === 'rolled_back' ? '健康检查失败，启动器恢复原版本。证书、配置与设备身份保留。' : outcome === 'failed' ? '候选包验证失败，设备继续使用原版本。重新确认后可以创建新请求。' : phase[2]}</p><p class="mono small device-request">request ${esc(upgrade.request_id)}</p>${phase[0] === 'download' && !canceled && !outcome ? kv('安装包传输', '24 MiB / 60 MiB') + progress(40, '安装包下载，24 MiB / 60 MiB') + '<p class="field-help">40% 仅为下载字节比例，不表示整体升级进度。</p>' : ''}${phase[0] === 'drain' && !canceled && !outcome ? table(['等待项', '剩余'], [['执行器', 0], ['日志记录', 0], ['终端记录', 0], ['待传日志字节', 0], ['终端会话', 0], ['有限操作', 0]]) : ''}${!canceled && !complete && !outcome ? `<div class="row wrap device-task-controls">${btn('推进演示阶段', 'device-upgrade-next', 'secondary', 'arrow')}${canCancel ? btn('取消升级', 'device-upgrade-cancel', 'ghost', 'stop') : ''}${phase[0] === 'verify' ? btn('演示验证失败', 'device-upgrade-fail', 'ghost', 'warn') : ''}${phase[0] === 'health' ? btn('演示健康失败并回滚', 'device-upgrade-rollback', 'ghost', 'refresh') : ''}</div>` : ''}<details data-device-detail="upgrade-stages"><summary>阶段记录</summary><ol class="device-upgrade-timeline">${upgradePhases.slice(0, upgrade.index + 1).map(([, title, detail]) => `<li><strong>${title}</strong><span class="muted small">${detail}</span></li>`).join('')}</ol></details></div>` : ''));
}
