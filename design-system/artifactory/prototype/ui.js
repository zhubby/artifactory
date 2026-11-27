/**
 * [INPUT]: 接收页面传入的纯文本、SVG 名称与 HTML 片段。
 * [OUTPUT]: 导出图标、表单、标准内容面板和信息层级构造函数。
 * [POS]: 原型无状态视图层；页面依赖此处，事件由 app.js 接收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export const esc = (value = '') => String(value).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const paths = {
    grid: '<rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="3" width="7" height="7" rx="2"/><rect x="3" y="14" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/>',
    box: '<path d="m12 3 9 5-9 5-9-5 9-5Zm-9 5v9l9 5 9-5V8M12 13v9M7.5 5.5l9 5"/>',
    layers: '<path d="m12 3 10 6-10 6L2 9l10-6Zm-9 11 9 5 9-5M3 18l9 5 9-5"/>',
    play: '<path d="m9 5 11 7-11 7V5Z"/>',
    branch: '<circle cx="6" cy="5" r="3"/><circle cx="6" cy="19" r="3"/><circle cx="18" cy="5" r="3"/><path d="M6 8v8m12-8a11 11 0 0 1-12 9"/>',
    arrow: '<path d="M5 12h14m-6-6 6 6-6 6"/>',
    chevron: '<path d="m9 5 7 7-7 7"/>',
    down: '<path d="m6 9 6 6 6-6"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
    bell: '<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9m-12 12a3 3 0 0 0 6 0"/>',
    settings: '<path d="m9 3-.6 2.4-2.2 1.3L3.8 6 2 9l1.8 1.8v2.5L2 15l1.8 3 2.4-.7 2.2 1.3L9 21h6l.6-2.4 2.2-1.3 2.4.7 1.8-3-1.8-1.7v-2.5L22 9l-1.8-3-2.4.7-2.2-1.3L15 3H9Z"/><circle cx="12" cy="12" r="3"/>',
    monitor: '<rect x="2" y="3" width="20" height="14" rx="3"/><path d="M8 21h8m-4-4v4"/>',
    chart: '<path d="M3 3v18h18M7 15l4-5 4 2 6-8"/>',
    shield: '<path d="m12 2 9 4v6c0 5-9 10-9 10S3 17 3 12V6l9-4Z"/><path d="m8 12 3 3 5-6"/>',
    check: '<path d="m5 12 4 4L19 6"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    close: '<path d="m6 6 12 12M6 18 18 6"/>',
    bolt: '<path d="m13 2-9 12h7l-1 8 10-13h-7l1-7Z"/>',
    spark: '<path d="m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3ZM20 2v4m-2-2h4"/>',
    globe: '<circle cx="12" cy="12" r="9"/><ellipse cx="12" cy="12" rx="4" ry="9"/><path d="M3 12h18"/>',
    users: '<circle cx="9" cy="7" r="4"/><path d="M2 21v-3a7 7 0 0 1 14 0v3m0-18a4 4 0 0 1 0 8m3 4a7 7 0 0 1 3 6"/>',
    key: '<circle cx="8" cy="8" r="5"/><path d="m12 12 9 9m-2-2 3-3m-6 0 3-3"/>',
    code: '<path d="m8 6-6 6 6 6m8-12 6 6-6 6m-3-16-2 20"/>',
    terminal: '<path d="m5 5 6 6-6 6m8 1h7"/>',
    copy: '<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M16 8V3H3v13h5"/>',
    download: '<path d="M12 3v12m-5-5 5 5 5-5M3 16v5h18v-5"/>',
    upload: '<path d="M12 16V3m-5 5 5-5 5 5M3 16v5h18v-5"/>',
    external: '<path d="M14 3h7v7m0-7L10 14M10 3H3v18h18v-7"/>',
    dots: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    refresh: '<path d="M20 8a8 8 0 1 0 1 7M20 2v6h-6"/>',
    filter: '<path d="M3 6h18M6 12h12m-8 6h4"/>',
    folder: '<path d="M3 5h7l2 3h9v13H3V5Z"/>',
    star: '<path d="m12 2 3 6 7 1-5 5 1 8-6-4-6 4 1-8-5-5 7-1 3-6Z"/>',
    moon: '<path d="M21 13A9 9 0 0 1 11 3a9 9 0 1 0 10 10Z"/>',
    sun: '<circle cx="12" cy="12" r="4"/><path d="M12 1v2m0 18v2M1 12h2m18 0h2M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2"/>',
    help: '<circle cx="12" cy="12" r="9"/><path d="M9 8a3 3 0 0 1 6 0c0 3-3 2-3 5m0 3h.01"/>',
    book: '<path d="M12 5C8 2 4 3 2 4v16c3-2 7-1 10 1m0-16c4-3 8-2 10-1v16c-3-2-7-1-10 1V5Z"/>',
    lock: '<rect x="4" y="10" width="16" height="12" rx="3"/><path d="M8 10V6a4 4 0 0 1 8 0v4m-4 5v3"/>',
    activity: '<path d="M2 12h5l3-9 4 18 3-9h5"/>',
    git: '<path d="m10 3 11 9-9 10L2 12l8-9Zm-2 3 7 7v5m-4-9v5"/><circle cx="11" cy="8" r="1.5"/><circle cx="11" cy="15" r="1.5"/>',
    mail: '<rect x="2" y="4" width="20" height="16" rx="3"/><path d="m3 6 9 7 9-7"/>',
    warn: '<path d="m12 3 10 18H2L12 3Zm0 6v5m0 3h.01"/>',
    file: '<path d="M5 2h9l5 5v15H5V2Zm9 0v6h5M8 13h8m-8 4h6"/>',
    heart: '<path d="M20 5a5 5 0 0 0-8 1 5 5 0 0 0-8-1c-5 5 1 11 8 16 7-5 13-11 8-16Z"/>',
    menu: '<path d="M3 5h18M3 12h18M3 19h18"/>',
    logout: '<path d="M9 3H3v18h6m6-14 5 5-5 5m-7-5h12"/>',
    server: '<rect x="3" y="2" width="18" height="8" rx="2"/><rect x="3" y="14" width="18" height="8" rx="2"/><path d="M7 6h.01M7 18h.01M12 6h5m-5 12h5"/>',
    tag: '<path d="M3 3h8l11 11-8 8L3 11V3Z"/><circle cx="7" cy="7" r="1"/>',
    eye: '<path d="M2 12s4-8 10-8 10 8 10 8-4 8-10 8S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',
    stop: '<rect x="5" y="5" width="14" height="14" rx="3"/>',
    cpu: '<rect x="5" y="5" width="14" height="14" rx="3"/><path d="M9 1v4m6-4v4M9 19v4m6-4v4M1 9h4m-4 6h4m14-6h4m-4 6h4"/><rect x="9" y="9" width="6" height="6"/>',
    apple: '<path d="M12 7c-5-4-10 1-8 7 2 6 4 8 8 6 4 2 6 0 8-6-4-2-4-5-1-7-2-2-5-1-7 0Zm0-2c0-3 2-4 4-4 0 3-2 5-4 4Z"/>',
    windows: '<path d="m2 5 8-1v7H2V5Zm10-1 10-2v9H12V4ZM2 13h8v7l-8-1v-6Zm10 0h10v9l-10-2v-7Z"/>',
    linux: '<path d="M8 9V6a4 4 0 0 1 8 0v3l4 10-5 2-3-2-3 2-5-2 4-10Z"/><path d="m9 10 3 2 3-2M10 6h.01M14 6h.01"/>',
    cube: '<path d="m12 2 10 5v10l-10 5-10-5V7l10-5Zm0 10v10M2 7l10 5 10-5"/>',
    arrowLeft: '<path d="M19 12H5m6-6-6 6 6 6"/>'
};
export const icon = (name = 'box', size = 20) => `<svg class="icon" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name] || paths.box}</svg>`;
export const btn = (label, action = '', kind = 'secondary', glyph = '', extra = '') => `<button type="button" class="btn ${kind}" ${action ? `data-action="${esc(action)}"` : ''} ${extra}>${glyph ? icon(glyph, 18) : ''}<span>${label}</span></button>`;
export const link = (label, route, cls = 'text-link') => `<a class="${cls}" href="#${route}" data-route="${route}">${label}</a>`;
export const badge = (text, kind = 'neutral') => `<span class="badge ${kind}"><span class="status-dot" aria-hidden="true"></span>${esc(text)}</span>`;
export const panel = (content, cls = '') => `<section class="panel content-surface ${cls}">${content}</section>`;
export const pageHeader = (eyebrow, title, description = '', actions = '') => `<div class="page-heading"><div><div class="eyebrow">${eyebrow}</div><h1>${title}</h1>${description ? `<p>${description}</p>` : ''}</div>${actions ? `<div class="page-actions">${actions}</div>` : ''}</div>`;
export const tabs = (items, active) => `<div class="tabs" role="tablist">${items.map(i => `<button type="button" role="tab" aria-selected="${i.id === active}" tabindex="${i.id === active ? '0' : '-1'}" class="tab ${i.id === active ? 'active' : ''}" ${i.route ? `data-route="${i.route}"` : `data-action="${i.action || 'tab'}" data-value="${i.id}"`}>${i.label}</button>`).join('')}</div>`;
export const field = (label, control, help = '') => `<div class="field"><label>${label}${control}</label>${help ? `<p class="field-help">${help}</p>` : ''}</div>`;
export const input = (name, value = '', placeholder = '', type = 'text') => `<input class="input" name="${esc(name)}" type="${type}" value="${esc(value)}" placeholder="${esc(placeholder)}" autocomplete="off">`;
export const select = (name, value, options) => `<details class="select"><summary role="button" aria-label="${esc(name)}：${esc(value)}" aria-haspopup="listbox" aria-expanded="false"><span>${esc(value)}</span>${icon('down', 16)}</summary><div class="select-options" role="listbox" aria-label="${esc(name)}">${options.map(x => { const o = typeof x === 'string' ? { value: x, label: x } : x; return `<button type="button" role="option" aria-selected="${o.label === value}" data-select="${esc(name)}" data-value="${esc(o.value)}">${esc(o.label)}${o.label === value ? icon('check', 16) : ''}</button>`; }).join('')}</div><input type="hidden" name="${esc(name)}" value="${esc(value)}"></details>`;
export const empty = (title, description, action = '') => `<div class="empty-state"><div class="empty-symbol">${icon('layers', 32)}</div><h2>${title}</h2><p>${description}</p>${action}</div>`;
export const metric = (label, value, note, trend = 'up', points = [14, 22, 18, 30, 24, 37, 28, 46, 40, 53]) => panel(`<div class="metric-label">${label}${icon('arrow', 16)}</div><div class="metric-value">${value}</div><div class="metric-foot"><span class="${trend === 'up' ? 'positive' : 'muted'}">${note}</span>${sparkline(points)}</div>`, 'metric');
export const sparkline = (values = [5, 12, 8, 18, 15, 28, 23, 34], color = 'currentColor') => `<svg class="sparkline" viewBox="0 0 100 36" aria-hidden="true"><path d="${values.map((v, i) => `${i ? 'L' : 'M'}${i * 100 / (values.length - 1)} ${36 - v * 0.52}`).join(' ')}" fill="none" stroke="${color}" stroke-width="2" stroke-linecap="round"/></svg>`;
export const table = (headers, rows, cls = '') => `<div class="table-wrap ${cls}"><table><thead><tr>${headers.map(h => `<th scope="col">${h || '<span class="sr-only">操作</span>'}</th>`).join('')}</tr></thead><tbody>${rows.map(r => `<tr>${r.map(c => `<td>${c}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
export const avatar = (letters, color = 'blue') => `<span class="avatar ${color}" aria-hidden="true">${esc(letters)}</span>`;
export const progress = (value, label = '') => `<div class="progress-track" role="progressbar" aria-label="${esc(label || '进度')}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${value}"><span style="width:${value}%"></span></div>`;
export const sectionTitle = (title, aside = '') => `<div class="section-heading"><h2>${title}</h2>${aside}</div>`;
export const kv = (label, value) => `<div class="kv"><span>${label}</span><strong>${value}</strong></div>`;
