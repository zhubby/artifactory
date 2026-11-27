/**
 * [INPUT]: 依赖 ui.js 的文本组件与 charts.js 的曲线和圆弧。
 * [OUTPUT]: 导出 40 项导航、数据、图表与内容组件及局部事件挂载。
 * [POS]: 组件展厅的数据展示分册；内容保留实体底，导航与媒体工具独立成层。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, esc, badge } from './ui.js';
import { smoothPath, chartDefs, donutChart, ringChart, gaugeChart, updateDonut, updateRing, updateGauge, enhanceCharts, stopChartMotion } from './charts.js';

const button = (action, text, attrs = '') => `<button type="button" class="cd-button" data-cd-action="${action}" ${attrs}>${text}</button>`;
const status = (text = '点击控件体验状态。') => `<p class="cd-status" role="status">${text}</p>`;
const demo = (kind, html) => `<div class="cd-demo" data-cd-demo="${kind}">${html}</div>`;
const item = (group, id, name, en, description, render, tags = []) => ({ id: `${group}-${id}`, name, en, description, tags: [group, ...tags], render });
const toolbar = html => `<div class="cd-toolbar">${html}</div>`;
const chartHeading = (label, value, unit) => `<div class="cd-chart-heading"><span>${label}</span><strong>${value}<small>${unit}</small></strong></div>`;
const rows = [['Mac Studio', 'macOS arm64', '68%', 68], ['Windows Builder', 'Windows amd64', '12%', 12], ['Linux Runner', 'Linux amd64', '8%', 8]];
const simpleTable = (mode = 'plain') => `<div class="cd-table-caption"><span>构建设备</span><small>3 台已授权</small></div><div class="cd-table-wrap"><table class="cd-table"><caption class="sr-only">演示构建设备</caption><thead><tr>${mode === 'select' ? '<th><input type="checkbox" data-cd-all aria-label="选择全部设备"></th>' : ''}<th scope="col">设备</th><th scope="col">平台</th><th scope="col" ${mode === 'sort' ? 'aria-sort="none"' : ''}>${mode === 'sort' ? button('sort', 'CPU ↕', 'aria-label="按 CPU 使用率排序"') : 'CPU'}</th>${mode === 'expand' ? '<th scope="col">详情</th>' : ''}</tr></thead><tbody>${rows.map((r, i) => `<tr data-cd-value="${r[3]}">${mode === 'select' ? `<td><input type="checkbox" data-cd-row aria-label="选择 ${r[0]}"></td>` : ''}<th scope="row"><span class="cd-device-name">${icon(i === 0 ? 'monitor' : 'server', 15)}${r[0]}</span></th><td><span class="cd-platform">${r[1]}</span></td><td class="mono"><span class="cd-cpu"><i style="--cpu:${r[3]}%" aria-hidden="true"></i>${r[2]}</span></td>${mode === 'expand' ? `<td>${button('expand-row', icon('plus', 16), `aria-label="展开 ${r[0]}" aria-expanded="false" aria-controls="cd-detail-${i}"`)}</td>` : ''}</tr>${mode === 'expand' ? `<tr id="cd-detail-${i}" hidden><td colspan="4"><div class="cd-row-detail">${icon('shield', 17)} 已授权 Orbital · 签名身份就绪 · 工具链版本已固定</div></td></tr>` : ''}`).join('')}</tbody></table></div>`;
const chartButtons = () => toolbar(button('chart-period', '近 7 天', 'aria-pressed="true"') + button('chart-period', '近 30 天', 'aria-pressed="false"'));
const lineSamples = thirty => thirty ? [5, 7, 6, 9, 7, 10, 9] : [8, 6, 7, 4, 5, 3, 4];
const linePoints = thirty => lineSamples(thirty).map((value, i) => [14 + i * 45, 112 - value * 9]);
const line = (id = 'catalog-line') => {
    const points = linePoints(false), path = smoothPath(points);
    return `<svg class="cd-line" viewBox="0 0 300 140" role="img" aria-label="构建耗时：周一 8、周二 6、周三 7、周四 4、周五 5、周六 3、周日 4 分钟">${chartDefs(id)}<path class="cd-grid-line" d="M14 22H284M14 67H284M14 112H284"/><path class="cd-area" d="${path}L284 112H14Z" fill="url(#${id}-area)"/><path class="cd-series chart-series" d="${path}" fill="none" stroke="url(#${id}-stroke)"/><circle class="cd-line-end" cx="284" cy="76" r="4"/><text class="cd-axis-label" x="14" y="134">周一</text><text class="cd-axis-label" x="149" y="134" text-anchor="middle">周四</text><text class="cd-axis-label" x="284" y="134" text-anchor="end">周日</text></svg>`;
};
const barSamples = thirty => thirty ? [36, 54, 47, 61, 56, 65, 72] : [24, 38, 31, 52, 43, 60, 49];
const bars = () => `<div class="cd-bars" role="img" aria-label="周一至周日构建次数：24、38、31、52、43、60、49">${barSamples(false).map((n, i) => `<div><b>${n}</b><span data-chart-bar style="height:${n * 1.4}px" title="${n} 次"></span><small>${['一', '二', '三', '四', '五', '六', '日'][i]}</small></div>`).join('')}</div>`;

const heat = (calendar = false) => `<div class="cd-heat ${calendar ? 'cd-heat-calendar' : ''}" role="img" aria-label="演示热力图：最近 35 个采样，深色表示较多成功构建">${Array.from({ length: 35 }, (_, i) => `<span style="--level:${(i * 7 + 3) % 5}" title="第 ${i + 1} 天 · ${(i * 7 + 3) % 5 * 8} 次"></span>`).join('')}</div><p class="cd-meta">低 <span class="cd-legend"></span> 高 · 演示采样</p>`;
const paneText = '<p>源码引用在构建时固定。</p><p>签名身份使用凭据引用。</p><p>正式交付保留校验摘要。</p><p>每个目标独立执行。</p><p>离线设备仍保留排队。</p><p>制品证据随版本保存。</p>';
const artwork = (i = 0, decorative = false) => `<div class="cd-artwork cd-art-${i}" ${decorative ? 'aria-hidden="true"' : `role="img" aria-label="Prism 抽象玻璃作品 ${i + 1}"`}><span></span><span></span><strong>PRISM / 0${i + 1}</strong></div>`;
const slides = ['光线，定义层次。', '清晰，承载信息。', '流动，连接操作。'];
const code = 'const release = {\n  version: "2.4.0",\n  immutable: true\n};';

const navigation = [
    item('navigation', 'breadcrumb', '面包屑', 'Breadcrumb', '保留父级路径，当前位置使用文字与语义共同表达。', () => demo('breadcrumb', `<nav class="cd-breadcrumb" aria-label="示例面包屑">${['工作空间', 'Orbital Desktop', '2.4.0'].map((s, i) => `${i ? icon('chevron', 13) : ''}${button('crumb', s, i === 2 ? 'aria-current="page"' : '')}`).join('')}</nav>${status('当前位置：2.4.0')}`), ['路径']),
    item('navigation', 'tabs', '标签导航', 'Tabs', '同层内容共享导航轨道，方向键衔接标签。', () => demo('tabs', `<div class="cd-tabs" role="tablist" aria-label="示例版本视图">${['概览', '制品', '证据'].map((s, i) => button('tab', s, `id="cd-tab-${i}" role="tab" aria-selected="${i === 0}" tabindex="${i === 0 ? 0 : -1}" aria-controls="cd-tab-panel"`)).join('')}</div><div class="cd-tab-panel" id="cd-tab-panel" role="tabpanel" aria-labelledby="cd-tab-0">概览：本次交付包含六个目标。</div><div class="cd-tab-evidence" aria-hidden="true"><span>macOS</span><span>Windows</span><span>Linux</span></div>`), ['页签']),
    item('navigation', 'segmented', '分段控制', 'Segmented Control', '短选项用于同一对象的展示方式。', () => demo('segmented', `<div class="cd-segmented" role="group" aria-label="示例布局">${button('segment', `${icon('grid', 16)} 卡片`, 'aria-pressed="true"')}${button('segment', `${icon('menu', 16)} 列表`, 'aria-pressed="false"')}</div><div class="cd-layout-preview" aria-hidden="true"><span><i></i><b></b><em></em></span><span><i></i><b></b><em></em></span><span><i></i><b></b><em></em></span></div>${status('当前为卡片视图。')}`), ['切换']),
    item('navigation', 'pagination', '分页', 'Pagination', '当前页保持位置，禁用边界操作。', () => demo('pagination', `<nav class="cd-pagination" aria-label="示例分页">${button('page-prev', icon('arrowLeft', 16), 'aria-label="上一页" disabled')}${[1, 2, 3].map(n => button('page', n, `data-cd-page="${n}" ${n === 1 ? 'aria-current="page"' : ''}`)).join('')}${button('page-next', icon('arrow', 16), 'aria-label="下一页"')}</nav>${status('第 1 页，共 3 页 · 21 条记录')}`), ['页码']),
    item('navigation', 'steps', '步骤条', 'Steps', '区分完成、当前与未开始，支持返回已填步骤。', () => demo('steps', `<ol class="cd-steps">${['源码', '目标', '确认'].map((s, i) => `<li>${button('step', `<span>${i + 1}</span>${s}`, `data-cd-step="${i}" ${i === 0 ? 'aria-current="step"' : ''}`)}</li>`).join('')}</ol>${status('步骤 1：选择源码。')}`), ['流程']),
    item('navigation', 'vertical-nav', '垂直导航', 'Vertical Navigation', '图标辅助识别，选中项保留完整标签。', () => demo('vertical-nav', `<nav class="cd-vertical" data-cl-glass="group" aria-label="示例设置导航">${[['settings', '基本设置'], ['shield', '访问策略'], ['bell', '通知渠道']].map(([g, s], i) => button('vertical', `${icon(g, 17)}${s}`, i === 0 ? 'aria-current="page"' : '')).join('')}</nav>${status('基本设置分区。')}`), ['侧边栏']),
    item('navigation', 'tree', '资源树', 'Resource Tree', '用明确的展开器表达层级，选择与展开分离。', () => demo('tree', `<div class="cd-tree" aria-label="项目资源层级"><details open><summary>${icon('folder', 16)} Orbital Desktop</summary><div>${button('tree-select', `${icon('file', 16)} src/main.rs`, 'aria-pressed="true"')}${button('tree-select', `${icon('file', 16)} Cargo.toml`, 'aria-pressed="false"')}<details><summary>${icon('folder', 16)} assets</summary>${button('tree-select', `${icon('file', 16)} brand.svg`, 'aria-pressed="false"')}</details></div></details></div>${status('已选择 src/main.rs。')}`), ['文件树']),
    item('navigation', 'toc', '页内目录', 'Table of Contents', '目录跳转限定在内容区域，焦点与位置同步。', () => demo('toc', `<nav class="cd-toc" aria-label="示例页内目录">${['源码', '签名', '交付'].map((s, i) => button('toc', s, `data-cd-target="cd-section-${i}"`)).join('')}</nav><div class="cd-scroll" tabindex="0" aria-label="示例文档">${['源码', '签名', '交付'].map((s, i) => `<section id="cd-section-${i}" tabindex="-1"><h4>${s}</h4>${paneText}</section>`).join('')}</div>`), ['锚点']),
    item('navigation', 'command-palette', '命令面板', 'Command Palette', '输入过滤命令，方向键选择，Enter 执行本地示例。', () => demo('command', `<label class="cd-search-label">${icon('search', 17)}<input class="input" data-cd-command placeholder="搜索命令" aria-label="过滤示例命令"></label><div class="cd-command-list" data-cl-glass="menu" aria-label="可用命令">${['打开项目', '查看构建', '复制版本号', '查看设备'].map((s, i) => button('command', `${icon(['folder','activity','copy','monitor'][i], 16)}<span>${s}</span><kbd aria-hidden="true">↵</kbd>`)).join('')}</div>${status('4 个命令 · ↑ ↓ 选择')}`), ['快捷搜索']),
    item('navigation', 'back-to-top', '返回顶部', 'Back to Top', '长内容保留明确返回入口，减少动态时直接定位。', () => demo('back-top', `<div class="cd-scroll" tabindex="0" aria-label="可滚动示例内容">${paneText.repeat(3)}</div>${toolbar(button('back-top', `${icon('arrowLeft', 15)} 返回内容顶部`))}${status('可先滚动内容，再返回顶部。')}`), ['滚动']),
];

const data = [
    item('data', 'table', '基础表格', 'Data Table', '正文稳定底色，数据列对齐并保留局部横向滚动。', () => demo('table', simpleTable()), ['数据表']),
    item('data', 'sortable-table', '排序表格', 'Sortable Table', '表头说明排序方向，数据按真实字段重新排列。', () => demo('sort', simpleTable('sort') + status('点击 CPU 表头排序。')), ['排序']),
    item('data', 'selectable-table', '多选表格', 'Selectable Table', '全选保留部分选中状态，选中数量即时更新。', () => demo('select', simpleTable('select') + status('已选择 0 台设备。')), ['批量']),
    item('data', 'expandable-table', '展开行表格', 'Expandable Rows', '详情在对应资源下展开，不改变原行语义。', () => demo('expand', simpleTable('expand')), ['详情']),
    item('data', 'description-list', '描述列表', 'Description List', '标签与值成对呈现，长摘要允许换行。', () => demo('description', `<dl class="cd-description"><div><dt>版本</dt><dd>2.4.0</dd></div><div><dt>源码</dt><dd class="mono">e7a2c91</dd></div><div><dt>交付状态</dt><dd>${badge('校验完成', 'success')}</dd></div></dl>`), ['键值']),
    item('data', 'list', '资源列表', 'Resource List', '搜索与当前结果保留在同一区域。', () => demo('list', `<label class="cd-search-label">${icon('search', 16)}<input class="input" data-cd-list-filter aria-label="搜索示例设备" placeholder="搜索设备"></label><ul class="cd-list">${rows.map(r => `<li>${icon('monitor', 18)}<span><strong>${r[0]}</strong><small>${r[1]}</small></span>${badge('在线', 'success')}</li>`).join('')}</ul>${status('3 台设备')}`), ['搜索列表']),
    item('data', 'infinite-list', '增量列表', 'Load More List', '显式加载更多保留控制权，并报告新增数量。', () => demo('load', `<ol class="cd-list cd-load-list"><li>构建 #1048 · 签名中</li><li>构建 #1047 · 已完成</li><li>构建 #1046 · 已完成</li></ol>${button('load-more', '加载更多记录', 'data-cd-count="3"')}${status('已显示 3 条记录。')}`), ['加载更多']),
    item('data', 'avatar-group', '头像组', 'Avatar Group', '溢出成员通过可操作入口展开。', () => demo('avatars', `<div class="cd-avatars" aria-label="项目成员"><span class="avatar" aria-label="林知远">林</span><span class="avatar purple" aria-label="陈雨">陈</span><span class="avatar green" aria-label="周舟">周</span><details><summary aria-label="+2，显示另外两位成员">+2</summary><div class="cd-popover"><div class="cd-member-row"><span class="avatar">许</span><span><strong>许晨</strong><small>开发者</small></span></div><div class="cd-member-row"><span class="avatar purple">沈</span><span><strong>沈悦</strong><small>只读成员</small></span></div></div></details></div><p class="cd-meta">5 位成员 · 身份与权限独立展示</p>`), ['成员']),
    item('data', 'badge', '徽标与计数', 'Badge & Counter', '计数与状态保持文字含义，减少只靠颜色的判断。', () => demo('badge', `<div class="cd-badge-line">${badge('成功', 'success')}${badge('等待设备', 'warning')}${badge('失败', 'danger')}</div>${toolbar(button('badge-count', `${icon('bell', 17)} 通知 <span class="cd-counter">3</span>`))}${status('点击标记一条通知已读。')}`), ['标记']),
    item('data', 'statistic', '统计数字', 'Statistic', '数字即时更新，单位与统计窗口始终可见。', () => demo('statistic', `<div class="cd-stat"><small>成功交付</small><strong data-cd-stat>128<span> 次</span></strong><p class="positive">↑ 18.6% 较上一周期</p></div>${toolbar(button('stat-period', '近 7 天', 'aria-pressed="true"') + button('stat-period', '近 30 天', 'aria-pressed="false"'))}`), ['指标']),
];

const visualization = [
    item('visualization', 'line-chart', '折线图', 'Line Chart', '绘制趋势，数值摘要保留精确值。', () => demo('line-chart', `<figure>${chartHeading('构建耗时', '5.3', '分钟 / 平均')}${line()}<figcaption class="cd-meta">构建耗时 · 分钟 · 7 天范围</figcaption></figure>${chartButtons()}${status('7 天平均耗时 5.3 分钟。')}`), ['趋势']),
    item('visualization', 'bar-chart', '柱状图', 'Bar Chart', '零基线比较数量，颜色不承担全部含义。', () => demo('bar-chart', `<figure>${chartHeading('构建频次', '297', '次 / 本周')}${bars()}<figcaption class="cd-meta">每日构建次数 · 共 297 次</figcaption></figure>${chartButtons()}${status('周六最多：60 次。')}`), ['比较']),
    item('visualization', 'donut-chart', '环形图', 'Donut Chart', '少量比例保留文字图例与总量。', () => demo('donut', `<div class="cd-donut-row"><div class="cd-donut" role="img" aria-label="成功 72%，失败 18%，取消 10%">${donutChart([72, 18, 10], { id: 'catalog-donut', size: 160, stroke: 16 })}<strong>297<small>次构建</small></strong></div><ul class="cd-legend-list"><li><span><i class="cd-legend-dot"></i>成功</span> <b>72%</b></li><li><span><i class="cd-legend-dot danger"></i>失败</span> <b>18%</b></li><li><span><i class="cd-legend-dot muted"></i>取消</span> <b>10%</b></li></ul></div>${toolbar(button('donut', '切换统计窗口') + button('donut-small', '小比重示例', 'aria-pressed="false"'))}${status('当前窗口：近 7 天。')}`), ['占比']),
    item('visualization', 'heatmap', '矩阵热力图', 'Heatmap', '颜色深浅表达密度，完整范围附文字说明。', () => demo('heatmap', chartHeading('工作日 × 构建池', '35', '次采样') + heat() + status('工作日 × 构建池 · 固定演示采样。')), ['密度']),
    item('visualization', 'sparkline', '迷你趋势', 'Sparkline', '嵌入指标旁的微型趋势保留文字结论。', () => demo('sparkline', `<div class="cd-spark-row"><div><small>平均耗时</small><strong>4分32秒</strong><p class="positive">下降 26.9%</p></div>${line('catalog-spark')}</div><p class="cd-meta">最近 7 次采样：8、6、7、4、5、3、4 分钟。</p>`), ['迷你图']),
    item('visualization', 'progress', '线性进度', 'Progress Bar', '确定进度保留百分比，未知进度使用独立状态。', () => demo('progress', `<div class="cd-progress" role="progressbar" aria-label="示例签名进度" aria-valuemin="0" aria-valuemax="100" aria-valuenow="64"><span style="width:64%"></span></div><div class="cd-progress-label"><strong data-cd-percent>64%</strong><span>签名与校验</span></div>${toolbar(button('progress', '推进演示进度'))}${status('进度为本地演示。')}`), ['完成度']),
    item('visualization', 'progress-ring', '环形进度', 'Progress Ring', '小范围任务使用单一环轨，中心显示真实比例。', () => demo('ring', `<div class="cd-ring" role="progressbar" aria-label="示例目标完成度" aria-valuemin="0" aria-valuemax="100" aria-valuenow="66">${ringChart(66, { id: 'catalog-ring', size: 144, stroke: 12 })}<strong>66%<small>目标完成度</small></strong></div>${toolbar(button('ring', '推进目标'))}${status('4 / 6 个目标完成。')}`), ['圆环']),
    item('visualization', 'gauge', '容量仪表', 'Capacity Gauge', '资源容量同时显示使用量与上限。', () => demo('gauge', `<div class="cd-gauge" role="meter" aria-label="示例构建池容量" aria-valuemin="0" aria-valuemax="16" aria-valuenow="6">${gaugeChart(6, 16, { id: 'catalog-gauge', width: 230, stroke: 16 })}<div><strong>6<span>/16</span></strong><small>正在使用的槽位</small></div></div>${toolbar(button('gauge', '模拟新增任务'))}${status('剩余 10 个槽位。')}`), ['容量']),
    item('visualization', 'timeline', '时间轴', 'Timeline', '时间、事件和状态分开读取，当前事件明确标记。', () => demo('timeline', `<ol class="cd-timeline"><li><time>14:32</time><div><strong>校验通过</strong><p>摘要已写入交付证据。</p></div></li><li><time>14:28</time><div><strong>签名完成</strong><p>使用已授权身份。</p></div></li><li><time>14:21</time><div><strong>构建开始</strong><p>六个目标独立执行。</p></div></li></ol>${button('timeline', '显示更早事件')}${status('按时间倒序排列。')}`), ['活动记录']),
    item('visualization', 'calendar-heatmap', '日历热力图', 'Calendar Heatmap', '连续日期表达频率，日期范围始终可见。', () => demo('calendar-heatmap', `<div class="cd-month-label"><span><strong>11</strong> 月</span><span>共 35 天演示窗口</span></div>${heat(true)}${toolbar(button('calendar', '切换到上一窗口'))}${status('当前窗口：10月24日—11月27日。')}`), ['活跃度']),
];

const content = [
    item('content', 'accordion', '手风琴', 'Accordion', '同组内容按需展开，标题始终可操作。', () => demo('accordion', `<div class="cd-accordion">${[['源码如何固定？', '每次构建解析引用，并保留完整源码清单。'], ['离线设备如何处理？', '对应目标保留排队，等待授权设备上线。'], ['发布后可以覆盖吗？', '正式版本只交付一次完整构建。']].map(([h, p], i) => `<details name="cd-faq" ${i === 0 ? 'open' : ''}><summary>${h}${icon('plus', 16)}</summary><p>${p}</p></details>`).join('')}</div>`), ['折叠']),
    item('content', 'disclosure', '渐进披露', 'Disclosure', '次要参数默认收起，关键约束始终可见。', () => demo('disclosure', `<p>默认目标：macOS arm64</p><details class="cd-disclosure"><summary>${icon('settings', 16)} 高级目标参数</summary><dl class="cd-description"><div><dt>逻辑 key</dt><dd>desktop-macos</dd></div><div><dt>宿主架构</dt><dd>arm64</dd></div></dl></details>`), ['高级参数']),
    item('content', 'scroll-area', '局部滚动区', 'Scroll Area', '长内容局部滚动，保留可见边界与键盘入口。', () => demo('scroll', `<div class="cd-scroll" tabindex="0" aria-label="构建证据示例">${paneText.repeat(3)}</div><p class="cd-meta">聚焦后使用方向键、Page Down 或空格滚动。</p>`), ['溢出']),
    item('content', 'split-pane', '可调分栏', 'Resizable Split Pane', '键盘与拖动共享边界，保留最小阅读宽度。', () => demo('split', `<div class="cd-split" style="--split:45%"><div><strong>源码</strong><p>src/main.rs</p><p>Cargo.toml</p></div><div class="cd-separator" role="separator" tabindex="0" aria-label="调整左右分栏" aria-orientation="vertical" aria-valuemin="25" aria-valuemax="75" aria-valuenow="45">⋮</div><div><strong>预览</strong><p>独立构建计划。</p></div></div><label class="cd-meta">分栏宽度 <input type="range" min="25" max="75" value="45" data-cd-split-range aria-label="左侧分栏百分比"></label>`), ['分隔条']),
    item('content', 'carousel', '手动轮播', 'Carousel', '用户主动切换，当前位置与内容均可读取。', () => demo('carousel', `<div class="cd-gallery"><div class="cd-slide" aria-live="polite">${artwork(0)}<strong data-cd-slide-title>${slides[0]}</strong></div><div class="cd-carousel-nav">${button('slide-prev', icon('arrowLeft', 17), 'aria-label="上一张"')}<span data-cd-slide-index>1 / 3</span>${button('slide-next', icon('arrow', 17), 'aria-label="下一张"')}</div></div>`), ['画廊']),
    item('content', 'image-preview', '图片预览', 'Image Preview', '缩略图明确可放大，关闭后返回触发位置。', () => demo('preview', `<button type="button" class="cd-preview-trigger" data-cd-action="preview">${artwork(1, true)}<span>${icon('eye', 17)} 放大预览</span></button><dialog class="cd-image-dialog" aria-label="Prism 玻璃作品预览"><div>${artwork(1)}${button('preview-close', `${icon('close', 17)} 关闭预览`)}</div></dialog>`), ['媒体']),
    item('content', 'code-block', '代码块', 'Code Block', '等宽字体保留行结构，复制操作提供文本反馈。', () => demo('code', `<div class="cd-code-head"><span><i aria-hidden="true"></i>release.js</span>${button('copy-code', `${icon('copy', 15)} 复制`)}</div><pre class="cd-code"><code>${esc(code)}</code></pre>${status('示例配置 · JavaScript')}`), ['代码']),
    item('content', 'diff-view', '差异查看', 'Diff Viewer', '增删使用符号与色彩双重表达。', () => demo('diff', `${toolbar(button('diff', '切换并排视图', 'aria-pressed="false"'))}<div class="cd-diff"><div><small>之前</small><pre><code><span class="cd-diff-remove">− version: "2.3.9"</span>\n  immutable: true</code></pre></div><div><small>之后</small><pre><code><span class="cd-diff-add">+ version: "2.4.0"</span>\n  immutable: true</code></pre></div></div>${status('统一视图 · 1 行修改')}`), ['比较']),
    item('content', 'markdown', '富文本预览', 'Markdown Preview', '源文与预览可切换，输出限定为安全内容。', () => demo('markdown', `${toolbar(button('markdown', '查看 Markdown 源文', 'aria-pressed="false"'))}<div class="cd-markdown"><h4>交付说明</h4><p>本次版本优化 <strong>离线工作流</strong>。</p><ul><li>保留完整源码证据</li><li>支持六个交付目标</li></ul><blockquote>每次发布都是确定的交付。</blockquote></div><pre class="cd-code" hidden data-cd-markdown-source># 交付说明\n本次版本优化 **离线工作流**。\n- 保留完整源码证据\n- 支持六个交付目标\n> 每次发布都是确定的交付。</pre>`), ['文档']),
    item('content', 'empty-state', '空状态', 'Empty State', '说明对象、原因与下一步，行动后展示真实本地结果。', () => demo('empty', `<div class="cd-empty">${icon('folder', 32)}<h4>还没有构建配方</h4><p>创建配方，保存常用的交付目标。</p>${button('empty-create', `${icon('plus', 16)} 创建示例配方`)}</div>${status('示例数据仅保存在当前组件中。')}`), ['无数据']),
];

export const dataGroups = [
    { id: 'navigation', label: '导航与层级', description: '从位置感到任务路径，始终知道自己在哪里。', items: navigation },
    { id: 'data', label: '数据与资源', description: '表格、列表与指标，让密集内容依然清晰。', items: data },
    { id: 'visualization', label: '图表与进度', description: '趋势、比较、比例和时间，共享可信表达。', items: visualization },
    { id: 'content', label: '内容与媒体', description: '阅读、比较、分栏和预览，按内容选择容器。', items: content },
];

function announce(d, text) { const el = d.querySelector('.cd-status'); if (el) el.textContent = text; }
function choose(buttons, current, attr) { buttons.forEach(b => { if (attr === 'aria-current') { b.removeAttribute(attr); if (b === current) b.setAttribute(attr, 'page'); } else b.setAttribute(attr, String(b === current)); }); }
function pageTo(d, value) {
    const n = Math.max(1, Math.min(3, value));
    d.dataset.page = n;
    d.querySelectorAll('[data-cd-page]').forEach(b => { if (+b.dataset.cdPage === n) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current'); });
    d.querySelector('[data-cd-action="page-prev"]').disabled = n === 1;
    d.querySelector('[data-cd-action="page-next"]').disabled = n === 3;
    announce(d, `第 ${n} 页，共 3 页 · 21 条记录`);
}
function resizeSplit(d, value) {
    const n = Math.max(25, Math.min(75, value));
    d.querySelector('.cd-split').style.setProperty('--split', `${n}%`);
    d.querySelector('.cd-separator').setAttribute('aria-valuenow', n);
    d.querySelector('[data-cd-split-range]').value = n;
}
function selectRows(d, all) {
    const boxes = [...d.querySelectorAll('[data-cd-row]')];
    if (all !== undefined) boxes.forEach(b => b.checked = all);
    const count = boxes.filter(b => b.checked).length;
    const header = d.querySelector('[data-cd-all]');
    header.checked = count === boxes.length;
    header.indeterminate = count > 0 && count < boxes.length;
    boxes.forEach(b => b.closest('tr').classList.toggle('cd-selected', b.checked));
    announce(d, `已选择 ${count} 台设备。`);
}

export function mountData(root, signal) {
    enhanceCharts(root);
    signal?.addEventListener('abort', stopChartMotion, { once: true });
    const on = (event, fn, opts = {}) => root.addEventListener(event, fn, { ...opts, signal });
    signal?.addEventListener('abort', () => root.querySelectorAll('.cd-image-dialog[open]').forEach(d => d.close()), { once: true });
    on('click', async e => {
        const b = e.target.closest('[data-cd-action]');
        if (!b || !root.contains(b)) return;
        const d = b.closest('[data-cd-demo]');
        const action = b.dataset.cdAction;
        if (action === 'crumb' || action === 'vertical') { choose(d.querySelectorAll('[data-cd-action]'), b, 'aria-current'); announce(d, `当前位置：${b.textContent.trim()}`); }
        if (action === 'tab') {
            choose(d.querySelectorAll('[role=tab]'), b, 'aria-selected');
            d.querySelectorAll('[role=tab]').forEach(t => t.tabIndex = t === b ? 0 : -1);
            const p = d.querySelector('[role=tabpanel]'); p.setAttribute('aria-labelledby', b.id);
            p.textContent = { '概览': '概览：本次交付包含六个目标。', '制品': '制品：四个原生安装包与一个双架构镜像。', '证据': '证据：源码、签名与校验摘要已固定。' }[b.textContent];
        }
        if (action === 'segment') { choose(d.querySelectorAll('[data-cd-action]'), b, 'aria-pressed'); const list = b.textContent.includes('列表'); d.querySelector('.cd-layout-preview').classList.toggle('is-list', list); announce(d, `当前为${list ? '列表' : '卡片'}视图。`); }
        if (action === 'page') pageTo(d, +b.dataset.cdPage);
        if (action === 'page-prev' || action === 'page-next') pageTo(d, +(d.dataset.page || 1) + (action === 'page-next' ? 1 : -1));
        if (action === 'step') { d.querySelectorAll('[data-cd-step]').forEach(x => { x.removeAttribute('aria-current'); x.classList.toggle('cd-complete', +x.dataset.cdStep < +b.dataset.cdStep); }); b.setAttribute('aria-current', 'step'); announce(d, `步骤 ${+b.dataset.cdStep + 1}：${['选择源码', '选择目标', '确认发起'][+b.dataset.cdStep]}。`); }
        if (action === 'tree-select') { choose(d.querySelectorAll('[data-cd-action="tree-select"]'), b, 'aria-pressed'); announce(d, `已选择 ${b.textContent.trim()}。`); }
        if (action === 'toc') { const section = d.querySelector(`#${b.dataset.cdTarget}`); const box = d.querySelector('.cd-scroll'); box.scrollTo({ top: section.offsetTop - box.firstElementChild.offsetTop, behavior: matchMedia('(prefers-reduced-motion:reduce)').matches ? 'instant' : 'smooth' }); section.focus({ preventScroll: true }); }
        if (action === 'command') announce(d, `已执行本地示例：${b.textContent.trim()}。`);
        if (action === 'back-top') { d.querySelector('.cd-scroll').scrollTo({ top: 0, behavior: matchMedia('(prefers-reduced-motion:reduce)').matches ? 'instant' : 'smooth' }); announce(d, '已返回内容顶部。'); }
        if (action === 'sort') { const head = b.closest('th'), asc = head.getAttribute('aria-sort') !== 'ascending'; head.setAttribute('aria-sort', asc ? 'ascending' : 'descending'); const body = d.querySelector('tbody'); [...body.rows].sort((a, c) => (+a.dataset.cdValue - +c.dataset.cdValue) * (asc ? 1 : -1)).forEach(r => body.append(r)); announce(d, `CPU 使用率${asc ? '升序' : '降序'}。`); }
        if (action === 'expand-row') { const open = b.getAttribute('aria-expanded') !== 'true'; b.setAttribute('aria-expanded', open); b.setAttribute('aria-label', `${open ? '收起' : '展开'} ${b.closest('tr').querySelector('th').textContent}`); d.querySelector(`#${b.getAttribute('aria-controls')}`).hidden = !open; b.innerHTML = icon(open ? 'close' : 'plus', 16); }
        if (action === 'load-more') { const count = +b.dataset.cdCount; const more = Array.from({ length: 3 }, (_, i) => `<li>构建 #${1048 - count - i} · 已完成</li>`).join(''); d.querySelector('ol').insertAdjacentHTML('beforeend', more); b.dataset.cdCount = count + 3; announce(d, `新增 3 条；已显示 ${count + 3} 条记录。`); if (count >= 9) { b.disabled = true; b.textContent = '已显示全部记录'; } }
        if (action === 'badge-count') { const c = d.querySelector('.cd-counter'); c.textContent = Math.max(0, +c.textContent - 1); announce(d, +c.textContent ? `还有 ${c.textContent} 条未读通知。` : '所有通知已读。'); }
        if (action === 'stat-period') { choose(d.querySelectorAll('[data-cd-action]'), b, 'aria-pressed'); d.querySelector('[data-cd-stat]').innerHTML = `${b.textContent.includes('30') ? 526 : 128}<span> 次</span>`; }
        if (action === 'chart-period') {
            choose(d.querySelectorAll('[data-cd-action]'), b, 'aria-pressed');
            const thirty = b.textContent.includes('30');
            const curve = d.querySelector('.cd-series');
            if (curve) {
                const points = linePoints(thirty), path = smoothPath(points);
                curve.setAttribute('d', path);
                d.querySelector('.cd-area').setAttribute('d', `${path}L284 112H14Z`);
                d.querySelector('.cd-line-end').setAttribute('cy', points.at(-1)[1]);
                d.querySelector('svg').setAttribute('aria-label', `近${thirty ? 30 : 7}天七个采样的耗时：${lineSamples(thirty).join('、')}分钟`);
                d.querySelector('figcaption').textContent = `构建耗时 · 分钟 · ${thirty ? 30 : 7} 天范围`;
                d.querySelector('.cd-chart-heading strong').innerHTML = `${(lineSamples(thirty).reduce((a,n)=>a+n,0)/7).toFixed(1)}<small>分钟 / 平均</small>`;
            }
            const barValues = barSamples(thirty);
            d.querySelectorAll('.cd-bars span').forEach((el, i) => { el.style.height = `${barValues[i] * 1.4}px`; el.title = `${barValues[i]} 次`; el.previousElementSibling.textContent = barValues[i]; });
            if (d.querySelector('.cd-bars')) {
                d.querySelector('.cd-bars').setAttribute('aria-label', `${thirty ? 30 : 7}天窗口七组构建次数：${barValues.join('、')}`);
                d.querySelector('figcaption').textContent = `构建次数 · ${thirty ? 30 : 7} 天范围 · 共 ${barValues.reduce((a, n) => a + n, 0)} 次`;
                d.querySelector('.cd-chart-heading strong').innerHTML = `${barValues.reduce((a,n)=>a+n,0)}<small>次 / ${thirty ? 30 : 7} 天</small>`;
            }
            announce(d, thirty ? '30 天汇总窗口 · 演示数据已切换。' : '7 天汇总窗口 · 演示数据已切换。');
        }
        if (action === 'donut' || action === 'donut-small') {
            const small = action === 'donut-small';
            if (!small) d.dataset.alternate = d.dataset.alternate !== 'true';
            const alternate = d.dataset.alternate === 'true';
            const values = small ? [305, 5, 2] : alternate ? [80, 12, 8] : [72, 18, 10];
            const labels = small ? ['97.8%', '1.6%', '0.6%'] : values.map(n => n + '%');
            const chart = d.querySelector('.cd-donut');
            updateDonut(chart, values);
            chart.setAttribute('aria-label', ['成功', '失败', '取消'].map((name, i) => name + labels[i]).join('，'));
            chart.querySelector('strong').innerHTML = `${small ? 312 : alternate ? 1204 : 297}<small>次构建</small>`;
            d.querySelectorAll('.cd-legend-list b').forEach((x, i) => x.textContent = labels[i]);
            d.querySelector('[data-cd-action=donut-small]').setAttribute('aria-pressed', small);
            announce(d, small ? '微小占比：305 / 5 / 2 次。圆端随分段收窄，角度保持真实比例。' : `当前窗口：近 ${alternate ? 30 : 7} 天。`);
        }
        if (action === 'progress') { const p = d.querySelector('[role=progressbar]'), n = +p.getAttribute('aria-valuenow') >= 100 ? 0 : Math.min(100, +p.getAttribute('aria-valuenow') + 12); p.setAttribute('aria-valuenow', n); p.firstElementChild.style.width = n + '%'; d.querySelector('[data-cd-percent]').textContent = n + '%'; announce(d, n === 100 ? '演示校验已完成。' : `演示进度 ${n}%。`); }
        if (action === 'ring') { const p = d.querySelector('[role=progressbar]'), n = +p.getAttribute('aria-valuenow') >= 100 ? 0 : Math.min(100, +p.getAttribute('aria-valuenow') + 17); p.setAttribute('aria-valuenow', n); updateRing(p, n); p.querySelector('strong').innerHTML = `${n}%<small>目标完成度</small>`; announce(d, `${Math.round(n * 6 / 100)} / 6 个目标完成。`); }
        if (action === 'gauge') { const p = d.querySelector('[role=meter]'), n = (+p.getAttribute('aria-valuenow') + 1) % 17; p.setAttribute('aria-valuenow', n); updateGauge(p, n, 16); p.querySelector('strong').innerHTML = `${n}<span>/16</span>`; announce(d, `剩余 ${16 - n} 个槽位。`); }
        if (action === 'timeline') { d.querySelector('ol').insertAdjacentHTML('beforeend', '<li><time>14:19</time><div><strong>源码已固定</strong><p>解析 release/2.4 并保存清单。</p></div></li>'); b.disabled = true; b.textContent = '已显示全部事件'; announce(d, '已补充最早的源码事件。'); }
        if (action === 'calendar') { const prev = d.dataset.previous !== 'true'; d.dataset.previous = prev; d.querySelectorAll('.cd-heat span').forEach((x, i) => x.style.setProperty('--level', (i * (prev ? 3 : 7) + 3) % 5)); announce(d, prev ? '上一窗口：9月19日—10月23日。' : '当前窗口：10月24日—11月27日。'); b.textContent = prev ? '返回当前窗口' : '切换到上一窗口'; }
        if (action === 'slide-prev' || action === 'slide-next') { const n = ((+(d.dataset.slide || 0)) + (action === 'slide-next' ? 1 : -1) + 3) % 3; d.dataset.slide = n; d.querySelector('.cd-slide').innerHTML = artwork(n) + `<strong data-cd-slide-title>${slides[n]}</strong>`; d.querySelector('[data-cd-slide-index]').textContent = `${n + 1} / 3`; }
        if (action === 'preview') d.querySelector('dialog').showModal();
        if (action === 'preview-close') d.querySelector('dialog').close();
        if (action === 'copy-code') { try { await navigator.clipboard.writeText(code); announce(d, '示例代码已复制。'); } catch { announce(d, '复制不可用。请选择代码后使用系统复制。'); } }
        if (action === 'diff') { const active = b.getAttribute('aria-pressed') !== 'true'; b.setAttribute('aria-pressed', active); d.querySelector('.cd-diff').classList.toggle('cd-diff-split', active); b.textContent = `切换${active ? '统一' : '并排'}视图`; announce(d, `${active ? '并排' : '统一'}视图 · 1 行修改`); }
        if (action === 'markdown') { const source = b.getAttribute('aria-pressed') !== 'true'; b.setAttribute('aria-pressed', source); d.querySelector('.cd-markdown').hidden = source; d.querySelector('[data-cd-markdown-source]').hidden = !source; b.textContent = source ? '查看排版预览' : '查看 Markdown 源文'; }
        if (action === 'empty-create') { d.querySelector('.cd-empty').innerHTML = `${icon('check', 30)}<h4>Desktop Release</h4><p>macOS arm64 · 已保存本地示例配方。</p>${button('empty-reset', '恢复空状态')}`; announce(d, '示例配方已创建。'); }
        if (action === 'empty-reset') { d.querySelector('.cd-empty').innerHTML = `${icon('folder', 32)}<h4>还没有构建配方</h4><p>创建配方，保存常用的交付目标。</p>${button('empty-create', `${icon('plus', 16)} 创建示例配方`)}`; announce(d, '已恢复空状态。'); }
    });
    on('change', e => { const d = e.target.closest('[data-cd-demo]'); if (!d) return; if (e.target.matches('[data-cd-all]')) selectRows(d, e.target.checked); if (e.target.matches('[data-cd-row]')) selectRows(d); });
    on('input', e => {
        const d = e.target.closest('[data-cd-demo]'); if (!d) return;
        if (e.target.matches('[data-cd-list-filter]')) { let n = 0; d.querySelectorAll('li').forEach(li => { li.hidden = !li.textContent.toLowerCase().includes(e.target.value.toLowerCase()); if (!li.hidden) n++; }); announce(d, `${n} 台设备`); }
        if (e.target.matches('[data-cd-command]')) { let n = 0; d.querySelectorAll('[data-cd-action="command"]').forEach(b => { b.hidden = !b.textContent.includes(e.target.value); if (!b.hidden) n++; }); announce(d, n ? `${n} 个命令 · ↑ ↓ 选择` : '没有匹配的命令。'); }
        if (e.target.matches('[data-cd-split-range]')) resizeSplit(d, +e.target.value);
    });
    on('keydown', e => {
        const d = e.target.closest('[data-cd-demo]'); if (!d) return;
        if (d.dataset.cdDemo === 'tabs' && ['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(e.key)) { e.preventDefault(); e.stopPropagation(); const tabs = [...d.querySelectorAll('[role=tab]')], i = tabs.indexOf(e.target); const next = e.key === 'Home' ? 0 : e.key === 'End' ? tabs.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length; tabs[next].click(); tabs[next].focus(); }
        if (d.dataset.cdDemo === 'command' && ['ArrowDown', 'ArrowUp', 'Escape'].includes(e.key)) { e.preventDefault(); e.stopPropagation(); if (e.key === 'Escape') { d.querySelector('input').focus(); return; } const all = [...d.querySelectorAll('[data-cd-action="command"]')].filter(b => !b.hidden), i = all.indexOf(e.target); all[(i + (e.key === 'ArrowDown' ? 1 : -1) + all.length) % all.length]?.focus(); }
        if (e.target.matches('.cd-separator') && ['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) { e.preventDefault(); e.stopPropagation(); resizeSplit(d, e.key === 'Home' ? 25 : e.key === 'End' ? 75 : +e.target.getAttribute('aria-valuenow') + (e.key === 'ArrowRight' ? 5 : -5)); }
    });
    let drag;
    on('pointerdown', e => { const s = e.target.closest('.cd-separator'); if (!s) return; e.preventDefault(); s.setPointerCapture(e.pointerId); drag = { pointer: e.pointerId, d: s.closest('[data-cd-demo]') }; });
    on('pointermove', e => { if (!drag || drag.pointer !== e.pointerId) return; const rect = drag.d.querySelector('.cd-split').getBoundingClientRect(); resizeSplit(drag.d, (e.clientX - rect.left) / rect.width * 100); });
    on('pointerup', () => drag = null);
    on('pointercancel', () => drag = null);
}
