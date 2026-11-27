/**
 * [INPUT]: 依赖授权项目索引、DashboardData 窗口、深链和 ui.js。
 * [OUTPUT]: 导出窗口指标、圆角结果柱、柔和周转曲线及近期活动。
 * [POS]: 工作空间与项目概览共用示例聚合，不从截断列表推算统计。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { builds } from './data.js';
import { getWorkspaceProjects } from './entry-data.js';
const { panel, sectionTitle, metric, badge, esc, icon, select } = U;
const category = value => ({ 成功: 'succeeded', 失败: 'failed', 构建中: 'ongoing', 等待设备: 'ongoing' })[value] || 'canceled';
export function dashboardSnapshot(state, project) {
    const scoped = getWorkspaceProjects(state).filter(item => !project || item.id === project.id), knownWeights = { orbital: 1, atlas: .7, beacon: .3, docs: .2 };
    const days = ({ '近 7 天': 7, '近 90 天': 90 })[state.range] || 30, factor = scoped.reduce((sum, item) => sum + (knownWeights[item.id] || 0), 0);
    const daily = Array.from({ length: days }, (_, i) => {
        const date = new Date(Date.UTC(2026, 10, 28 - days + i)).toISOString().slice(0, 10), succeeded = Math.round((6 + i % 5) * factor), failed = factor && i % 6 === 0 ? 1 : 0, canceled = factor && i % 9 === 0 ? 1 : 0, ongoing = factor && i === days - 1 ? 1 : 0;
        return { date, succeeded, failed, canceled, ongoing, total: succeeded + failed + canceled + ongoing, median_turnaround_seconds: !factor || i % 8 === 4 ? null : Math.round(330 - i * 3 + Math.sin(i * .8) * 26) };
    });
    const sum = key => daily.reduce((sum, day) => sum + day[key], 0), succeeded = sum('succeeded'), failed = sum('failed'), canceled = sum('canceled'), completed = succeeded + failed;
    const source = builds.filter(build => scoped.some(item => item.name === build[1]));
    const recentProjects = [...new Map([...source].reverse().map(build => [build[1], build])).values()].map(build => ({ project_name: build[1], project_id: { 'Orbital Desktop': 'orbital', 'Atlas API': 'atlas', 'Beacon Mobile': 'beacon' }[build[1]], number: build[0].slice(1), release_version: build[2], label: build[5], kind: build[6], activity_at: '2026-11-26 14:32' }));
    return { window: { days, timezone: 'UTC', start: daily[0].date, end: daily.at(-1).date, generated_at: '2026-11-27T00:00:00Z' }, summary: { total_projects: scoped.length, active_projects: scoped.filter(item => knownWeights[item.id]).length, builds_created: sum('total'), succeeded, failed, canceled, ongoing: sum('ongoing'), current_ongoing: sum('ongoing'), success_rate: completed ? succeeded / completed * 100 : null, median_turnaround_seconds: succeeded ? 292 : null }, daily, recent_projects: recentProjects, recent_builds: source.slice(0, 4).map((build, i) => ({ number: build[0].slice(1), project_name: build[1], release_version: build[2], status: category(build[5]), label: build[5], kind: build[6], activity_at: ['2026-11-26 14:32', '2026-11-26 14:12', '2026-11-26 13:48', '2026-11-26 12:35'][i] })) };
}
function resultChart(data, id) {
    const width = 560, step = 510 / data.daily.length, barWidth = Math.min(24, step * .62), max = Math.max(...data.daily.map(day => day.total), 1), scale = 112 / max;
    const series = [['succeeded', 'var(--chart-succeeded)', '成功'], ['failed', 'var(--chart-failed)', '失败'], ['canceled', 'var(--chart-canceled)', '取消'], ['ongoing', 'var(--chart-ongoing)', '进行中']];
    const columns = data.daily.map((day, i) => { let y = 130; return `<g>${series.map(([key,,label]) => { const height = day[key] * scale, radius = Math.min(8, barWidth / 2, height / 2); y -= height; return height ? `<rect x="${30 + i * step + (step - barWidth) / 2}" y="${y}" width="${barWidth}" height="${height}" rx="${radius}" ry="${radius}" fill="url(#${id}-${key})"><title>${day.date} · ${label} ${day[key]}</title></rect>` : ''; }).join('')}</g>`; }).join('');
    return `<div class="chart-legend">${series.map(([,color,label]) => `<span><i class="legend-dot" style="background:${color}"></i>${label}</span>`).join('')}</div><svg class="chart" viewBox="0 0 ${width} 160" role="img" aria-label="${data.window.days} 天构建结果"><defs>${series.map(([key,color]) => `<linearGradient id="${id}-${key}" x1="0" y1="0" x2="0" y2="1"><stop stop-color="color-mix(in srgb,${color} 82%,white)"/><stop offset="1" stop-color="${color}"/></linearGradient>`).join('')}</defs>${[0,.5,1].map(value => `<path d="M28 ${130 - value * 112}H550" class="chart-grid"/><text x="0" y="${134 - value * 112}" class="chart-label">${Math.round(max * value)}</text>`).join('')}${columns}<text x="30" y="151" class="chart-label">${data.daily[0].date.slice(5)}</text><text x="550" y="151" text-anchor="end" class="chart-label">${data.daily.at(-1).date.slice(5)}</text></svg><details class="small"><summary>查看逐日结果数据</summary>${U.table(['日期', '成功', '失败', '取消', '进行中', '合计'], data.daily.map(day => [day.date, day.succeeded, day.failed, day.canceled, day.ongoing, day.total]))}</details>`;
}
/* --- 限制相邻切线，保留采样范围与缺口 --- */
function trendPath(points) {
    if (points.length < 2) return '';
    const slopes = points.slice(1).map((p, i) => (p.y - points[i].y) / (p.x - points[i].x));
    const tangents = points.map((_, i) => i === 0 ? slopes[0] : i === points.length - 1 ? slopes.at(-1) : (slopes[i - 1] + slopes[i]) / 2);
    slopes.forEach((slope, i) => {
        if (!slope) { tangents[i] = tangents[i + 1] = 0; return; }
        if (tangents[i] / slope < 0) tangents[i] = 0;
        if (tangents[i + 1] / slope < 0) tangents[i + 1] = 0;
        const magnitude = Math.hypot(tangents[i] / slope, tangents[i + 1] / slope);
        if (magnitude > 3) { tangents[i] *= 3 / magnitude; tangents[i + 1] *= 3 / magnitude; }
    });
    return `M${points[0].x} ${points[0].y}` + points.slice(1).map((point, i) => {
        const previous = points[i], dx = (point.x - previous.x) / 3;
        return `C${previous.x + dx} ${previous.y + dx * tangents[i]} ${point.x - dx} ${point.y - dx * tangents[i + 1]} ${point.x} ${point.y}`;
    }).join('');
}
function turnaroundChart(data, id) {
    if (data.daily.every(day => day.median_turnaround_seconds === null)) return U.empty('暂无成功周转样本', '完成成功构建后，再形成真实趋势。');
    const max = Math.max(1, ...data.daily.map(day => day.median_turnaround_seconds || 0)), points = data.daily.map((day, i) => day.median_turnaround_seconds === null ? null : { x: 30 + i / Math.max(1, data.daily.length - 1) * 510, y: 130 - day.median_turnaround_seconds / max * 110 }), segments = [];
    let current = []; for (const point of points) { if (point) current.push(point); else if (current.length) { segments.push(current); current = []; } } if (current.length) segments.push(current);
    const paths = segments.map(segment => segment.length === 1 ? `<circle cx="${segment[0].x}" cy="${segment[0].y}" r="3" fill="var(--chart-turnaround)"/>` : `<path d="${trendPath(segment)}" fill="none" stroke="url(#${id}-trend)" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>`).join('');
    return `<p class="chart-summary">成功构建的周转中位数 · 分钟</p><svg class="chart" viewBox="0 0 560 160" role="img" aria-label="周转中位数趋势，缺少数据的日期保留断点"><defs><linearGradient id="${id}-trend" gradientUnits="userSpaceOnUse" x1="30" y1="0" x2="540" y2="0"><stop stop-color="color-mix(in srgb,var(--chart-turnaround) 84%,white)"/><stop offset=".55" stop-color="var(--chart-turnaround)"/><stop offset="1" stop-color="var(--chart-turnaround)"/></linearGradient></defs>${[0,.5,1].map(value => `<path d="M28 ${130 - value * 110}H550" class="chart-grid"/><text x="0" y="${134 - value * 110}" class="chart-label">${(max * value / 60).toFixed(1)}</text>`).join('')}${paths}<text x="30" y="151" class="chart-label">${data.daily[0].date.slice(5)}</text><text x="550" y="151" text-anchor="end" class="chart-label">${data.daily.at(-1).date.slice(5)}</text></svg><details class="small"><summary>查看逐日周转数据</summary>${U.table(['日期', '周转中位数 · 秒'], data.daily.map(day => [day.date, day.median_turnaround_seconds ?? '无样本']))}</details><p class="field-help">柔和曲线不跨过缺失日期。仅显示成功构建样本。</p>`;
}
export function renderDashboard(state, project) {
    const data = dashboardSnapshot(state, project), summary = data.summary, id = `dashboard-${project?.id || 'workspace'}`;
    const projectMetric = metric('周转中位数', summary.median_turnaround_seconds === null ? '—' : `${(summary.median_turnaround_seconds / 60).toFixed(1)}<small>分钟</small>`, '创建至结束 · 成功构建', 'neutral', []);
    const workspaceMetric = metric('活跃项目', `${summary.active_projects}<small> / ${summary.total_projects}</small>`, '期间有构建创建或完成的项目', 'neutral', []);
    const metrics = `${!project ? workspaceMetric : ''}${metric('构建次数', summary.builds_created, `${data.window.days} 天完整聚合`, 'neutral', [])}${metric('成功率', summary.success_rate === null ? '—' : `${summary.success_rate.toFixed(1)}<small>%</small>`, '成功 ÷（成功 + 失败），排除取消', 'neutral', [])}${project ? projectMetric : ''}${metric('当前进行中', summary.current_ongoing, '当前所有非终态构建，不限日期', 'neutral', [])}`;
    const activity = project ? data.recent_builds : data.recent_projects;
    const activityRows = activity.map(build => `<a class="list-row" href="#${project ? `build/${build.number}` : `project/${build.project_id}`}" data-route="${project ? `build/${build.number}` : `project/${build.project_id}`}"><div><h3>${esc(project ? `构建 #${build.number}` : build.project_name)}</h3><p>${project ? '' : `#${build.number} · `}${esc(build.release_version)} · ${build.activity_at}</p></div>${badge(build.label, build.kind)}${icon('arrow', 16)}</a>`).join('');
    return `<div class="stack entry-dashboard"><div class="row between wrap"><p class="small muted">${data.window.start}–${data.window.end} · UTC · 按自然日统计，包含今天</p>${select('overview-range', state.range || '近 30 天', ['近 7 天', '近 30 天', '近 90 天'])}</div><div class="metric-grid">${metrics}</div><div class="dashboard-body">${panel(sectionTitle('每日构建结果') + (summary.builds_created ? resultChart(data, id) : U.empty('还没有构建结果', '新的构建会出现在这个窗口。')))}${panel(sectionTitle('成功周转趋势') + turnaroundChart(data, id))}</div>${panel(sectionTitle(project ? '近期构建活动' : '近期活跃项目', project ? U.link('全部构建', `project/${project.id}/builds`) : U.link('全部项目', 'projects')) + (activity.length ? activityRows : U.empty('还没有构建活动', '创建版本后发起第一次构建。')) + (project ? `<div class="entry-dashboard-footer">${U.link('制品', `project/${project.id}/artifacts`)}</div>` : ''))}<p class="field-help">示例按 DashboardData 合同提供完整窗口，指标不从上方近期列表推算。</p></div>`;
}
