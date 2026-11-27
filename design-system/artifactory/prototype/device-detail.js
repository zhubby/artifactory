/**
 * [INPUT]: 依赖设备快照、有限操作、ui.js 与应用导航接口。
 * [OUTPUT]: 提供五标签设备侧面板和焦点、阅读位置生命周期。
 * [POS]: 设备详情内容层；Regular 只用于边框工具和标签。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { getDevice, getPools, known } from './device-data.js';
import { renderFiniteOperations, renderUpgrade } from './device-operations.js';
const { esc, icon, btn, badge, panel, sectionTitle, kv, table, metric, tabs, progress } = U;
const tabItems = [['overview', '设备概览'], ['resources', '资源趋势'], ['tools', '工具链'], ['metadata', '元数据'], ['operations', '设备运维']];
function overview(state, device) {
    const pool = getPools(state).find(pool => pool.id === device.pool);
    return `<div class="device-summary-grid">${[[ 'cpu', '处理器', `${device.cores} 核`, device.chip], ['layers', '物理内存', `${device.memory} GB`, '设备上报容量'], ['activity', '执行槽位', `${known(device.active)} / ${device.slots}`, '当前使用 / 容量'], ['monitor', '系统与架构', device.arch, device.os]].map(([glyph, title, value, detail]) => `<div><span class="device-summary-label">${icon(glyph, 17)}${title}</span><strong>${esc(value)}</strong><small>${esc(detail)}</small></div>`).join('')}</div>` + panel(sectionTitle('身份与连接') + kv('设备名称', esc(device.name)) + kv('主机名称', `<code>${esc(device.hostname)}</code>`) + kv('节点池', esc(pool?.name ?? '未知')) + kv('Emissary', esc(device.version ?? '未知')) + kv('最近上报', esc(device.observed ?? '未知 · 设备离线')) + `<details data-device-detail="identity"><summary>完整设备身份</summary><p class="mono device-request">${device.id}</p><p class="field-help">设备身份由证书绑定，重命名不改变设备身份。</p></details>`) + panel(sectionTitle('能力标签') + `<div class="row wrap">${device.tags.map(tag => `<span class="chip">${esc(tag)}</span>`).join('')}</div><p class="field-help">能力由设备上报。项目授权与签名凭据分别配置。</p>`) + (device.key === 'studio' ? panel(sectionTitle('网络接口') + table(['接口', '地址', '连接'], [['en0', '<code>192.168.10.24</code>', '主动出站 · mTLS']])) : '');
}
function smoothPath(points) {
    if (points.length === 1) return `M${points[0].x},${points[0].y}`;
    const slopes = points.slice(1).map((p, index) => (p.y - points[index].y) / (p.x - points[index].x));
    const tangents = points.map((_, index) => !index ? slopes[0] : index === points.length - 1 ? slopes.at(-1) : slopes[index - 1] * slopes[index] <= 0 ? 0 : 2 / (1 / slopes[index - 1] + 1 / slopes[index]));
    return points.slice(1).reduce((path, p, index) => { const start = points[index], distance = (p.x - start.x) / 3; return `${path} C${start.x + distance},${start.y + distance * tangents[index]} ${p.x - distance},${p.y - distance * tangents[index + 1]} ${p.x},${p.y}`; }, `M${points[0].x},${points[0].y}`);
}
function trend(title, values, id, color) {
    const minutes = [15, 16, 18, 19, 20, 22, 23, 25, 26, 27, 29, 30];
    const samples = values.map((value, index) => ({ value, time: `14:${minutes[index]}`, x: 42 + (minutes[index] - 15) / 15 * 405, y: value == null ? null : 166 - value * 1.4 }));
    const segments = []; let segment = [];
    samples.forEach(sample => { if (sample.value == null) { if (segment.length) segments.push(segment); segment = []; } else segment.push(sample); });
    if (segment.length) segments.push(segment);
    return panel(sectionTitle(title, '<span class="small muted">14:15–14:30 · %</span>') + `<svg viewBox="0 0 480 205" class="chart-svg device-trend" role="img" aria-label="${title}，14:22 缺失采样保留断点；零值保留。完整采样见下表。"><defs><linearGradient id="${id}-line" gradientUnits="userSpaceOnUse" x1="42" y1="0" x2="447" y2="0"><stop stop-color="${color}" stop-opacity=".65"/><stop offset=".6" stop-color="${color}"/><stop offset="1" stop-color="${color}" stop-opacity=".85"/></linearGradient><linearGradient id="${id}-area" gradientUnits="userSpaceOnUse" x1="0" y1="26" x2="0" y2="166"><stop stop-color="${color}" stop-opacity=".14"/><stop offset="1" stop-color="${color}" stop-opacity="0"/></linearGradient></defs>${[0, 50, 100].map(value => `<path d="M42 ${166 - value * 1.4}H447" class="result-grid"/><text x="32" y="${170 - value * 1.4}" text-anchor="end" class="result-axis">${value}</text>`).join('')}${segments.map(points => `<path d="${smoothPath(points)} L${points.at(-1).x},166 L${points[0].x},166 Z" fill="url(#${id}-area)"/><path d="${smoothPath(points)}" fill="none" stroke="url(#${id}-line)" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>${points.length === 1 ? `<circle cx="${points[0].x}" cy="${points[0].y}" r="3" fill="${color}"/>` : ''}`).join('')}<text x="42" y="195" class="result-axis">14:15</text><text x="447" y="195" text-anchor="end" class="result-axis">14:30</text></svg><details data-device-detail="${id}"><summary>查看 ${samples.length} 次采样</summary>${table(['时间 · UTC+08:00', title], samples.map(sample => [sample.time, sample.value == null ? '未知 · 缺失采样' : `${sample.value}%`]))}</details>`);
}
function resources(device) {
    if (device.key !== 'studio') return panel(sectionTitle('资源快照') + kv('CPU', '未知') + kv('可用内存', '未知') + kv('磁盘可用字节', '未知') + '<p class="field-help">此设备没有资源采样样例。列表硬件容量不会替代实时遥测。</p>');
    return `<div class="device-resource-metrics">${metric('CPU 利用率', '68%', '最近采样 14:30:12', 'flat')}${metric('可用内存', '111.4 GB', '总量 192 GB · 可用 58%', 'flat')}${metric('磁盘可用', '1.24 TB', '磁盘总量未知', 'flat')}</div>` + panel(sectionTitle('当前容量') + '<div class="device-capacity"><div><span>CPU 利用率 · 68%</span>' + progress(68, 'CPU 利用率 68%') + '</div><div><span>内存可用比例 · 58%</span>' + progress(58, '可用内存占总量 58%') + '</div></div><p class="field-help">磁盘没有总量，不推算百分比。GPU 利用率未知。</p>') + trend('CPU 利用率', [18, 22, 0, 36, 43, null, 58, 72, 65, 80, 74, 68], 'device-cpu', 'var(--primary)') + trend('内存使用率', [25, 25, 28, 31, 34, null, 37, 39, 40, 41, 42, 42], 'device-memory', 'var(--purple)');
}
function tools(device) {
    if (device.key !== 'studio') return panel(sectionTitle('工具链') + kv('工具版本与路径', '未知') + '<p class="field-help">能力标签不等同于已验证的工具版本。</p>');
    const entries = [['Xcode', '16.2', '/Applications/Xcode.app'], ['Swift', '6.0.3', '/usr/bin/swift'], ['Rust', '1.95.0', '/opt/homebrew/bin/cargo'], ['Node.js', '24.11.0', '/opt/homebrew/bin/node'], ['Git', '2.47.1', '/usr/bin/git'], ['codesign', '15.2', '/usr/bin/codesign']];
    return panel(sectionTitle('工具链快照', badge('6 项', 'neutral')) + entries.map(([name, version, path], index) => `<article class="device-tool"><div class="row spread"><strong>${name}</strong><code>${version}</code></div><details data-device-detail="tool-${index}"><summary>工具路径</summary><code class="device-request">${path}</code></details></article>`).join('') + '<p class="field-help">版本与路径属于设备上报快照。此处不读取签名材料。</p>');
}
function metadata(state, device) {
    if (device.key !== 'studio') return panel(sectionTitle('元数据快照') + kv('观测修订', '未知') + kv('设备上报 JSON', '未知'));
    const revision = state.deviceMetadataRevision || 7;
    const data = { os: 'darwin', arch: 'arm64', hostname: device.hostname, location: '上海工作室', purpose: '桌面应用构建', retry_count: 0, experimental: false, gpu_utilization: null };
    return panel(sectionTitle('元数据观测', badge(`revision ${revision}`, 'neutral')) + kv('快照时间', esc(state.deviceMetadataObserved || device.observed)) + kv('观测 ID', `<code>obs-${esc((state.deviceMetadataObservationId || 'obs-c7e2').replace(/^obs-/, ''))}</code>`) + '<p class="field-help">刷新清单生成新的观测修订。启动调度快照保持固定。</p>' + btn('请求刷新快照', 'device-metadata-refresh', 'secondary', 'refresh')) + panel(sectionTitle('设备上报 JSON') + table(['键', '值', '类型'], Object.entries(data).map(([key, value]) => [esc(key), value === null ? '未知 · null' : esc(value), value === null ? 'null' : typeof value])) + `<details data-device-detail="metadata-json"><summary>查看完整 JSON</summary><pre class="code-block">${esc(JSON.stringify(data, null, 2))}</pre></details>`);
}
function operations(state, device) {
    if (device.key !== 'studio') return panel(sectionTitle('设备运维') + kv('设备版本', esc(device.version ?? '未知')) + kv('操作能力', '此设备未开放运维样例') + '<p class="field-help">设备离线与能力未知时，不创建可执行任务。</p>');
    return renderUpgrade(state, device) + renderFiniteOperations(state, device) + panel(sectionTitle('设备终端', badge('默认关闭', 'neutral')) + '<p class="muted small">仅工作空间 owner/admin 的原登录 Session。每次连接明确授权，固定 owner，5 秒授权截止，45 秒租约。</p><div class="device-task-controls">' + btn('申请临时终端', 'terminal-connect', 'secondary', 'terminal') + '</div>');
}
export function renderDeviceDrawer(state) {
    const device = getDevice(state);
    if (!device) return `<div class="device-detail-backdrop" data-action="device-close"><section class="device-detail-drawer" role="dialog" aria-modal="true" aria-labelledby="device-detail-title"><header class="device-detail-header"><h2 id="device-detail-title">设备不存在</h2>${btn('返回设备', 'device-close', 'secondary', 'arrowLeft')}</header></section></div>`;
    const active = tabItems.some(([key]) => key === state.nodeTab) ? state.nodeTab : 'overview';
    const views = { overview, resources: (_, item) => resources(item), tools: (_, item) => tools(item), metadata, operations };
    return `<div class="device-detail-backdrop" data-device-backdrop><section class="device-detail-drawer" role="dialog" aria-modal="true" aria-labelledby="device-detail-title" aria-describedby="device-detail-description"><header class="device-detail-header glass" data-entry-glass><div class="row gap"><span class="app-icon">${icon(device.glyph, 25)}</span><div><div class="eyebrow">DEVICE / ${device.id.slice(0, 8)}</div><h2 id="device-detail-title">${esc(device.name)}</h2></div></div><div class="row">${badge(device.online ? '在线' : '离线', device.online ? 'success' : 'neutral')}${btn('重命名', 'device-rename', 'ghost', 'file')}<button type="button" class="icon-button" data-action="device-close" aria-label="关闭设备详情">${icon('close', 20)}</button></div></header><p id="device-detail-description" class="device-detail-description">${esc(device.hostname)} · ${esc(device.os)} · ${device.arch}</p><div class="device-detail-tabs glass">${tabs(tabItems.map(([id, label]) => ({ id, label, action: 'node-tab' })), active).replace('role="tablist"', 'role="tablist" aria-label="设备详情分区"').replace(/data-value="([^"]+)"/g, 'data-value="$1" id="device-tab-$1" aria-controls="device-panel-$1"')}</div><div class="device-detail-body" id="device-panel-${active}" role="tabpanel" aria-labelledby="device-tab-${active}" data-device-scroll>${views[active](state, device)}</div><footer class="device-detail-footer"><span>设备快照 · 本地演示</span><span>缺失值保留未知</span></footer></section></div>`;
}
let cleanup = () => {}, priorDrawer = '', returnDevice = '';
export function bindDevicePrototype(root, state, api) {
    cleanup();
    const drawer = root.querySelector('.device-detail-drawer');
    const background = [...root.querySelectorAll('.sidebar,.topbar,.main > :not(.device-detail-backdrop)')].filter(element => !element.closest('.device-detail-backdrop'));
    const overlayRoot = document.getElementById('overlay-root');
    const route = location.hash.slice(1).split('/')[0];
    api.syncTerminal?.(route);
    if (!drawer) {
        if (priorDrawer && route === 'nodes') root.querySelector(`[data-route="node/${CSS.escape(returnDevice)}"]`)?.focus({ preventScroll: true });
        priorDrawer = ''; return;
    }
    const id = state.nodeId || 'studio', tab = state.nodeTab || 'overview', key = `${id}/${tab}`;
    const opening = priorDrawer !== id;
    returnDevice = id; priorDrawer = id;
    let nestedOpen = false, lastDrawerFocus = drawer.contains(document.activeElement) ? document.activeElement : drawer.querySelector('[data-action=device-close]');
    const rememberFocus = event => { if (drawer.contains(event.target)) lastDrawerFocus = event.target; };
    const lock = () => {
        const nested = !!overlayRoot?.children.length;
        background.forEach(element => element.inert = true);
        drawer.inert = nested;
        drawer.setAttribute('aria-modal', String(!nested));
        document.body.style.overflow = 'hidden';
        if (nestedOpen && !nested) queueMicrotask(() => { if (drawer.isConnected && !drawer.contains(document.activeElement)) lastDrawerFocus?.focus({ preventScroll: true }); });
        nestedOpen = nested;
    };
    lock();
    const scroll = drawer.querySelector('[data-device-scroll]');
    if (scroll) scroll.scrollTop = state.deviceReading?.[key] || 0;
    const rememberScroll = () => { state.deviceReading ||= {}; state.deviceReading[key] = scroll.scrollTop; };
    scroll?.addEventListener('scroll', rememberScroll, { passive: true });
    drawer.querySelectorAll('[data-device-detail]').forEach(detail => {
        detail.open = !!state.deviceExpanded?.[`${id}/${detail.dataset.deviceDetail}`];
        detail.addEventListener('toggle', () => { state.deviceExpanded ||= {}; state.deviceExpanded[`${id}/${detail.dataset.deviceDetail}`] = detail.open; });
    });
    const focusable = () => [...drawer.querySelectorAll('a[href],button:not(:disabled),input:not(:disabled),summary,[tabindex="0"]')].filter(element => element.getClientRects().length && !element.closest('[inert]'));
    const keyboard = event => {
        if (overlayRoot?.children.length) return;
        if (event.key === 'Escape') { event.preventDefault(); event.stopImmediatePropagation(); api.go('nodes'); }
        if (event.key !== 'Tab') return;
        const items = focusable(), first = items[0], last = items.at(-1);
        if (!drawer.contains(document.activeElement)) { event.preventDefault(); (event.shiftKey ? last : first)?.focus(); }
        else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    const observer = new MutationObserver(lock);
    if (overlayRoot) observer.observe(overlayRoot, { childList: true });
    document.addEventListener('keydown', keyboard, true);
    document.addEventListener('focusin', rememberFocus, true);
    const backdrop = root.querySelector('[data-device-backdrop]');
    const closeBackdrop = event => { if (event.target === backdrop) api.go('nodes'); };
    backdrop?.addEventListener('click', closeBackdrop);
    queueMicrotask(() => { if (opening || !drawer.contains(document.activeElement)) drawer.querySelector(opening ? '[data-action=device-close]' : '[role=tab][aria-selected=true]')?.focus({ preventScroll: true }); });
    cleanup = () => {
        observer.disconnect(); document.removeEventListener('keydown', keyboard, true); document.removeEventListener('focusin', rememberFocus, true); scroll?.removeEventListener('scroll', rememberScroll); backdrop?.removeEventListener('click', closeBackdrop);
        background.forEach(element => element.inert = false); drawer.inert = false;
    };
}
