/**
 * [INPUT]: 依赖设备快照、详情侧面板与真实分区设置。
 * [OUTPUT]: 导出按节点池筛选的设备目录、详情、终端与节点池。
 * [POS]: 资源与组织视图层；由共享设备快照保持身份一致。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { renderManagement } from './entry-management.js';
import { getDevices, getDevice, getPools, known } from './device-data.js';
import { renderDeviceDrawer } from './device-detail.js';
const { icon, esc, btn, link, badge, panel, pageHeader, select, metric, table, progress, sectionTitle } = U;
const sub = (title, detail) => `<strong>${title}</strong><div class="muted small">${detail}</div>`;
const row = (left, right) => `<div class="list-row"><div>${left}</div><div class="row gap">${right}</div></div>`;
const note = (glyph, title, detail, kind = '') => `<div class="notice ${kind}">${icon(glyph, 22)}<div><strong>${title}</strong><p class="muted small">${detail}</p></div></div>`;
function deviceCard(device, state) {
    const pool = getPools(state).find(pool => pool.id === device.pool);
    const used = device.active, capacity = device.slots;
    return panel(`<div class="row spread"><div class="app-icon ${device.online ? '' : 'gray'}">${icon(device.glyph, 27)}</div>${badge(device.online ? '在线' : '离线', device.online ? 'success' : 'neutral')}</div><div class="device-card-identity"><h2>${link(esc(device.name), `node/${device.key}`, 'plain-link')}</h2><p class="muted small mono">${esc(device.hostname)}</p><p class="muted small mono">${device.id.slice(0, 8)}</p></div><p>${esc(device.chip)} · ${device.memory} GB</p><p class="muted small">${esc(device.os)} · ${device.arch}</p><div class="device-slot-snapshot"><div class="row spread small"><span class="muted">任务槽位 · 使用 / 容量</span><strong>${known(used)} / ${capacity}</strong></div>${used == null ? '<p class="field-help">设备离线，当前用量未知</p>' : progress(used / capacity * 100, `使用 ${used} / ${capacity} 个任务槽位`)}</div><div class="row gap wrap">${device.tags.map(tag => `<span class="chip">${esc(tag)}</span>`).join('')}</div><div class="card-footer row spread"><span class="small muted">${esc(pool?.name ?? '节点池未知')}</span>${btn('查看详情', '', 'ghost', 'arrow', `data-route="node/${device.key}"`)}</div>`, 'device-card');
}
function nodes(state) {
    const all = getDevices(state), query = (state.query || '').trim().toLowerCase(), pools = getPools(state);
    const shown = all.filter(device => `${device.name} ${device.hostname} ${device.id} ${device.os} ${device.chip} ${device.tags.join(' ')}`.toLowerCase().includes(query) && (!state.devicePoolFilter || device.pool === state.devicePoolFilter));
    return pageHeader('BUILD FLEET', '每一种平台，都有归处。', '工作空间的设备目录 · 硬件与槽位为明确标记的演示快照。', btn('加入设备', 'enroll-node', 'primary', 'plus')) +
        panel(`<div class="row spread wrap"><div class="row gap wrap"><label class="search-field">${icon('search', 18)}<input name="query" data-search placeholder="搜索设备、主机名、型号、CPU 或 ID" value="${esc(state.query || '')}" aria-label="搜索设备"></label>${select('device-pool-filter', pools.find(pool => pool.id === state.devicePoolFilter)?.name || '全部节点池', ['全部节点池', ...pools.map(pool => pool.name)])}${state.devicePoolFilter ? btn('清除节点池筛选', 'device-clear-pool', 'ghost', 'close') : ''}</div><div class="row"><span class="small muted">${shown.length} 台设备</span>${link('查看节点池', 'pools')}</div></div>`, 'toolbar-panel') +
        `<div class="grid-2 device-directory">${shown.length ? shown.map(device => deviceCard(device, state)).join('') : panel(U.empty('没有匹配的设备', '更换名称、系统或节点池筛选。', btn('清除筛选', 'device-clear-filters', 'secondary', 'refresh')))}</div>` +
        note('shield', '设备能力与项目授权分别定义', '只有项目授权的系统与架构设备才接收对应目标。离线设备保留排队，不自动更换平台。');
}
function smoothPath(points) {
    const slopes = points.slice(1).map((p, i) => (p.y - points[i].y) / (p.x - points[i].x));
    const tangents = points.map((_, i) => i === 0 ? slopes[0] : i === points.length - 1 ? slopes.at(-1) : slopes[i - 1] * slopes[i] <= 0 ? 0 : 2 / (1 / slopes[i - 1] + 1 / slopes[i]));
    return points.slice(1).reduce((path, p, i) => {
        const start = points[i], step = (p.x - start.x) / 3;
        return `${path} C${start.x + step},${start.y + step * tangents[i]} ${p.x - step},${p.y - step * tangents[i + 1]} ${p.x},${p.y}`;
    }, `M${points[0].x},${points[0].y}`);
}
function resourceChart(title, unit, values, color = 'var(--primary)') {
    const minutes = [15, 16, 18, 19, 20, 22, 23, 25, 26, 27, 29, 30];
    const samples = values.map((value, i) => ({ value, time: `14:${minutes[i]}`, x: 40 + (minutes[i] - 15) / 15 * 407, y: 170 - value * 1.5 }));
    const last = samples.at(-1);
    return panel(sectionTitle(title, `<span class="muted small">14:15–14:30 · ${unit}</span>`) + `<svg viewBox="0 0 480 210" class="chart-svg resource-chart" role="img" aria-label="${title}，14:15 至 14:30，最新 ${last.value} ${unit}，完整采样见下表"><g stroke="var(--line)" stroke-dasharray="3 6"><path d="M40 20h407M40 70h407M40 120h407M40 170h407"/></g><g fill="var(--muted)" font-size="12"><text x="8" y="24">100</text><text x="16" y="74">67</text><text x="16" y="124">33</text><text x="22" y="174">0</text><text x="40" y="199">14:15</text><text x="230" y="199" text-anchor="middle">14:22</text><text x="447" y="199" text-anchor="end">14:30</text></g><path d="${smoothPath(samples)}" fill="none" stroke="${color}" stroke-width="3" stroke-linecap="round"/><circle cx="${last.x}" cy="${last.y}" r="4" fill="${color}"/></svg><details class="small"><summary>查看全部 ${samples.length} 次采样</summary>${table(['采样时间 · UTC+08:00', `${title} · ${unit}`], samples.map(s => [s.time, `${s.value} ${unit}`]))}</details>`);
}

function node(state) { return nodes(state) + renderDeviceDrawer(state); }
function terminal(state) {
    const device = getDevice(state);
    if (!device || device.key !== 'studio') return pageHeader('DEVICE / TERMINAL', '此设备未开放终端演示', '返回设备详情查看快照。', btn('返回设备', '', 'secondary', 'arrowLeft', `data-route="node/${esc(state.nodeId || 'studio')}"`));
    const heading = pageHeader('DEVICE / TERMINAL', `${esc(device.name)} · 临时终端`, '固定当前登录 owner · 5 秒授权截止 · 45 秒租约', btn('返回设备详情', '', 'secondary', 'arrowLeft', 'data-route="node/studio"'));
    if (!state.terminalConnected) return heading + panel(U.empty('建立一次有边界的连接', '设备终端默认关闭。每次连接明确授权；此处只模拟浏览器输出。', btn('申请临时连接', 'terminal-connect', 'primary', 'terminal')));
    return heading + panel(`<div class="row spread wrap">${sectionTitle('独立终端会话')}${badge('模拟会话 · 林知远', 'running')}${btn('明确结束会话', 'terminal-disconnect', 'secondary', 'stop')}</div><pre class="code-block terminal-output" aria-live="polite">${esc(state.terminalOutput || 'Artifactory · 演示终端\n固定 owner：linzhiyuan；设备用户：orbital-builder\n输入 help 查看可用演示命令。\n')}</pre><form data-form="terminal" class="row gap"><label class="terminal-input" style="flex:1"><span class="sr-only">演示命令</span><input class="input mono" name="command" placeholder="help、pwd、uname -a 或 clear" autocomplete="off" required></label><button class="btn primary" type="submit">${icon('arrow', 18)}发送</button></form><p class="field-help">固定 owner · 45 秒租约。离开页面或明确结束即回收。粘贴先确认，再明确发送。</p>`);
}
function pools(state) {
    const all = getPools(state);
    return pageHeader('NODE POOLS', '为设备建立清楚的归属。', '节点池只有名称。设备计数来自节点池字段，零值与未知分别保留。', btn('创建节点池', 'new-pool', 'primary', 'plus')) + `<div class="grid-2">${all.map(pool => panel(`<div class="row gap"><div class="app-icon">${icon('layers', 26)}</div><div><h2>${esc(pool.name)}</h2><p class="mono small muted">${esc(pool.id)}</p></div>${pool.is_default ? badge('默认', 'neutral') : ''}</div><dl class="device-pool-metrics"><div><dt>设备总数</dt><dd>${known(pool.node_count)}</dd></div><div><dt>在线设备</dt><dd>${known(pool.online_count)}</dd></div></dl>${pool.node_count == null || pool.online_count == null ? '<p class="field-help">汇总尚未提供。未知不表示零台设备。</p>' : ''}<div class="card-footer row wrap">${btn('查看设备', 'device-filter-pool', 'ghost', 'arrow', `data-id="${esc(pool.id)}"`)}${btn('重命名', 'rename-pool', 'secondary', 'file', `data-id="${esc(pool.id)}"`)}${btn('删除', 'delete-pool', 'ghost', 'close', `data-id="${esc(pool.id)}" ${pool.is_default || (pool.node_count ?? 0) > 0 ? 'disabled' : ''}`)}</div>${pool.is_default ? '<p class="field-help">默认节点池不能删除。</p>' : (pool.node_count ?? 0) > 0 ? '<p class="field-help">先移出全部设备，再删除节点池。</p>' : ''}`)).join('')}</div>` + note('shield', '资源授权由项目明确选择', '节点池不推导系统、架构或项目授权。BuildKit 镜像目标单独配置。');
}
function observation() {
  const results = [
    { day: '周五', success: 34, failed: 1, cancelled: 0 },
    { day: '周六', success: 43, failed: 0, cancelled: 1 },
    { day: '周日', success: 30, failed: 1, cancelled: 0 },
    { day: '周一', success: 48, failed: 1, cancelled: 0 },
    { day: '周二', success: 39, failed: 0, cancelled: 0 },
    { day: '周三', success: 57, failed: 2, cancelled: 0 },
    { day: '周四', success: 54, failed: 0, cancelled: 1 }
  ];
  const kinds = [['success', '成功', 'var(--success)'], ['failed', '失败', 'var(--danger)'], ['cancelled', '取消', 'var(--muted)']];
  const totals = Object.fromEntries(kinds.map(([key]) => [key, results.reduce((sum, d) => sum + d[key], 0)]));
  const bars = results.map((day, i) => {
    let base = 170;
    return kinds.map(([key]) => {
      const height = day[key] * 2.1;
      base -= height;
      return height ? `<rect x="${66 + i * 55}" y="${base}" width="26" height="${height}" rx="${Math.min(7, height / 2)}" fill="url(#result-${key})"/>` : '';
    }).join('') + `<text x="${79 + i * 55}" y="199" text-anchor="middle" class="result-axis">${day.day}</text>`;
  }).join('');
  const resultChart = `<p class="field-help">构建次数 / 天 · 完整 7 日窗口</p><svg class="chart-svg result-chart" viewBox="0 0 480 215" role="img" aria-label="近7天共312次构建：成功305次、失败5次、取消2次。完整逐日数据见下表。"><defs>${kinds.map(([key, , color]) => `<linearGradient id="result-${key}" x1="0" y1="1" x2="0" y2="0"><stop stop-color="${color}" stop-opacity=".72"/><stop offset="1" stop-color="${color}"/></linearGradient>`).join('')}</defs>${[0, 20, 40, 60].map(n => `<path d="M40 ${170 - n * 2.1}H447" class="result-grid"/><text x="28" y="${174 - n * 2.1}" text-anchor="end" class="result-axis">${n}</text>`).join('')}${bars}</svg><div class="result-legend">${kinds.map(([key, label, color]) => `<span><i style="background:${color}"></i>${label} ${totals[key]}</span>`).join('')}</div><details class="small"><summary>查看逐日结果数据</summary>${table(['日期', '成功', '失败', '取消', '合计'], results.map(d => [d.day, d.success, d.failed, d.cancelled, d.success + d.failed + d.cancelled]))}</details>`;
  const stages = [['源码准备', '12秒', 12, 'var(--primary)'], ['AI 规划', '38秒', 38, 'var(--purple)'], ['排队等待', '8秒', 8, 'var(--muted)'], ['编译与打包', '3分06秒', 186, 'var(--primary)'], ['签名与校验', '28秒', 28, 'var(--success)']];
  return pageHeader('OBSERVABILITY', '让每次交付，都更从容。', '近 7 天 · Orbital 全部已授权项目 · 演示统计窗口', btn('导出报告', 'export', 'secondary', 'download')) +
    `<div class="metric-grid">${metric('构建成功率', '97.8%', '较上周 +2.4 个百分点', 'up')}${metric('构建耗时 P50', '4m 32s', '较上周缩短 18%', 'up')}${metric('队列等待 P95', '42s', '较上周缩短 12%', 'up')}${metric('交付版本', '24', '本周新增 6 个', 'up')}</div>` +
    `<div class="grid-2">${panel(sectionTitle('构建结果', '<span class="chip">最近 7 天</span>') + resultChart)}${resourceChart('设备平均 CPU', '%', [24, 22, 31, 45, 40, 56, 42, 50, 36, 34, 28, 32])}</div>` +
    panel(sectionTitle('从等待到交付', '<span class="muted small">各阶段 P50 · 秒</span>') + '<p class="field-help">所有横条共用 0–186 秒标尺。各阶段分位数独立计算。</p>' + `<div class="stage-breakdown">${stages.map(([name, value, seconds, color]) => `<div class="stage-row"><div class="row spread small"><strong>${name}</strong><span class="mono muted">${value}</span></div><div class="stage-meter" aria-hidden="true"><span style="width:${seconds / 186 * 100}%;--stage-color:${color}"></span></div></div>`).join('')}</div>`) +
    panel(sectionTitle('值得关注', link('查看审计', 'activity')) + row(sub('macOS 签名等待略有增加', '最近 24 小时 · P95 从 42 秒上升至 1 分 18 秒'), badge('建议关注', 'warning')) + row(sub('Windows 构建稳定', '连续 42 次成功 · 过去 7 天未出现签名失败'), badge('运行良好', 'success')) + row(sub('Mac mini 已离线 2 小时', '1 个目标等待设备，任务保持排队'), link('查看设备', 'nodes')));
}

export function renderAdmin(route, state = {}) {
    if (['settings', 'team', 'credentials', 'activity'].includes(route)) return renderManagement({ ...state, managementSection: ({ team: 'members', credentials: 'credentials', activity: 'audit' })[route] || state.managementSection });
    const pages = { nodes, node, terminal, pools, observability: observation };
    return pages[route] ? pages[route](state) : null;
}
