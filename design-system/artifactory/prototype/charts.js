/**
 * [INPUT]: 接收有序采样、比例和容量；依赖主题图表变量与浏览器动画接口。
 * [OUTPUT]: 导出保形曲线、SVG 渐变、圆环和容量弧及更新函数。
 * [POS]: 仅供组件展厅使用的图形几何层；不改变产品页面。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
const animations = new Map();
const sectorMotions = new WeakMap();
let sequence = 0, observer;
const bound = (n, min, max) => Math.max(min, Math.min(max, Number(n) || 0));
const number = n => Number(n.toFixed(3));
const key = id => String(id || `prism-chart-${++sequence}`).replace(/[^a-zA-Z0-9_-]/g, '-');
const toneTokens = {
    blue: ['--chart-start', '--chart-end'],
    danger: ['--chart-danger-start', '--chart-danger-end'],
    neutral: ['--chart-neutral-start', '--chart-neutral-end'],
    success: ['--chart-success-start', '--chart-success-end'],
    purple: ['--chart-purple-start', '--chart-purple-end'],
};

// --- 保形插值：每段限制切线，曲线不超出相邻采样范围。 ---
export function smoothPath(points) {
    if (!points.length) return '';
    const chunks = []; let chunk = [];
    points.forEach(p => {
        if (p?.length >= 2 && p.every(Number.isFinite)) chunk.push(p);
        else if (chunk.length) { chunks.push(chunk); chunk = []; }
    });
    if (chunk.length) chunks.push(chunk);
    if (chunks.length > 1 || chunks[0]?.length !== points.length) return chunks.map(smoothPath).join(' ');
    const clean = chunks[0] || [];
    if (!clean.length) return '';
    let path = `M${clean[0].map(number).join(' ')}`;
    if (clean.length < 2) return path;
    const slopes = clean.slice(1).map((p, i) => (p[1] - clean[i][1]) / (p[0] - clean[i][0]));
    const tangents = clean.map((_, i) => i === 0 ? slopes[0] : i === clean.length - 1 ? slopes.at(-1) : (slopes[i - 1] + slopes[i]) / 2);
    for (let i = 0; i < slopes.length; i++) {
        const slope = slopes[i];
        if (!Number.isFinite(slope)) { tangents[i] = tangents[i + 1] = 0; continue; }
        if (!slope) { tangents[i] = tangents[i + 1] = 0; continue; }
        if (tangents[i] / slope < 0) tangents[i] = 0;
        if (tangents[i + 1] / slope < 0) tangents[i + 1] = 0;
        const magnitude = Math.hypot(tangents[i] / slope, tangents[i + 1] / slope);
        if (magnitude > 3) {
            const scale = 3 / magnitude;
            tangents[i] *= scale;
            tangents[i + 1] *= scale;
        }
    }
    for (let i = 1; i < clean.length; i++) {
        const a = clean[i - 1], b = clean[i], third = (b[0] - a[0]) / 3;
        if (b[0] <= a[0]) { path += `L${b.map(number).join(' ')}`; continue; }
        path += `C${number(a[0] + third)} ${number(a[1] + third * tangents[i - 1])} ${number(b[0] - third)} ${number(b[1] - third * tangents[i])} ${number(b[0])} ${number(b[1])}`;
    }
    return path;
}

export function chartDefs(id, tone = 'blue') {
    const name = key(id), [start, end] = toneTokens[tone] || toneTokens.blue;
    const area = tone === 'blue' ? '--chart-area' : start;
    return `<defs><linearGradient id="${name}-stroke" x1="0%" y1="0%" x2="100%" y2="100%"><stop stop-color="var(${start})"/><stop offset=".5" stop-color="var(${start})"/><stop offset="1" stop-color="var(${end})"/></linearGradient><linearGradient id="${name}-area" x1="0%" y1="0%" x2="0%" y2="100%"><stop stop-color="var(${area})" stop-opacity=".2"/><stop offset=".85" stop-color="var(${area})" stop-opacity=".03"/><stop offset="1" stop-color="var(${area})" stop-opacity="0"/></linearGradient></defs>`;
}
function trackCircle(size, stroke) {
    return `<circle class="chart-track" cx="${size / 2}" cy="${size / 2}" r="${(size - stroke - 12) / 2}" fill="none" stroke-width="${stroke}"/>`;
}
export function ringChart(value, { id, size = 144, stroke = 12 } = {}) {
    const name = key(id), n = bound(value, 0, 100), r = (size - stroke - 12) / 2;
    return `<svg class="chart-radial chart-ring" data-chart-kind="ring" viewBox="0 0 ${size} ${size}" aria-hidden="true">${chartDefs(name)}${trackCircle(size, stroke)}<circle class="chart-arc" data-chart-arc cx="${size / 2}" cy="${size / 2}" r="${r}" transform="rotate(-90 ${size / 2} ${size / 2})" fill="none" stroke="url(#${name}-stroke)" stroke-width="${stroke}" stroke-linecap="round" pathLength="100" stroke-dasharray="100 100" stroke-dashoffset="${100 - n}" opacity="${n ? 1 : 0}"/></svg>`;
}
export function gaugeChart(value, max, { id, width = 230, stroke = 16 } = {}) {
    const name = key(id), n = bound(value / Math.max(Number(max) || 1, 1) * 100, 0, 100);
    return `<svg class="chart-radial chart-gauge" data-chart-kind="gauge" viewBox="0 0 240 150" style="max-width:${width}px" aria-hidden="true">${chartDefs(name)}<path class="chart-track" d="M25 120A95 95 0 0 1 215 120" fill="none" stroke-width="${stroke}" stroke-linecap="round"/><path class="chart-arc" data-chart-arc d="M25 120A95 95 0 0 1 215 120" fill="none" stroke="url(#${name}-stroke)" stroke-width="${stroke}" stroke-linecap="round" pathLength="100" stroke-dasharray="100 100" stroke-dashoffset="${100 - n}" opacity="${n ? 1 : 0}"/><text class="chart-end-label" x="25" y="145" text-anchor="middle">0</text><text class="chart-end-label" x="215" y="145" text-anchor="middle">${bound(max, 0, Number.MAX_SAFE_INTEGER)}</text></svg>`;
}
// --- 环图按真实角度分区；圆角和间隙随小分段收敛。 ---
function proportions(values) {
    const nums = values.map(v => bound(v, 0, Number.MAX_SAFE_INTEGER));
    const sum = nums.reduce((a, b) => a + b, 0);
    let cursor = -Math.PI / 2;
    return nums.map(n => {
        const angle = sum ? n / sum * Math.PI * 2 : 0;
        const result = { start: cursor, angle };
        cursor += angle;
        return result;
    });
}
function segmentGeometry(size, stroke, start, angle) {
    const radius = (size - stroke - 12) / 2, c = size / 2;
    if (angle < .000001) return { d: '', width: 0 };
    const point = a => [number(c + radius * Math.cos(a)), number(c + radius * Math.sin(a))].join(' ');
    if (angle > Math.PI * 2 - .000001) return { d: `M${point(start)}A${radius} ${radius} 0 1 1 ${point(start + Math.PI)}A${radius} ${radius} 0 1 1 ${point(start)}`, width: stroke };
    // 圆帽占据真实角度范围；小分段收窄为短胶囊，不拉长占比。
    const gap = Math.min(.035, angle * .14), available = angle - gap;
    const width = Math.min(stroke, 2 * radius * Math.sin(Math.min(available / 2, Math.PI / 2)) * .84);
    const capAngle = Math.asin(width / (2 * radius));
    const begin = start + gap / 2 + capAngle, end = start + angle - gap / 2 - capAngle;
    return { d: `M${point(begin)}A${radius} ${radius} 0 ${end - begin > Math.PI ? 1 : 0} 1 ${point(end)}`, width: number(width) };
}
function segmentFrame(size, stroke, start, angle) {
    const { d, width } = segmentGeometry(size, stroke, start, angle);
    return { d: `path("${d}")`, strokeWidth: String(width) };
}
export function donutChart(values, { id, size = 160, stroke = 16 } = {}) {
    const name = key(id), tones = ['blue', 'danger', 'neutral'];
    const arcs = proportions(values);
    return `<svg class="chart-radial chart-donut" data-chart-kind="donut" data-chart-size="${size}" data-chart-stroke="${stroke}" viewBox="0 0 ${size} ${size}" aria-hidden="true">${values.map((_, i) => chartDefs(`${name}-${i}`, tones[i % tones.length])).join('')}${trackCircle(size, stroke)}${arcs.map(({start, angle}, i) => {
        const { d, width } = segmentGeometry(size, stroke, start, angle);
        return `<path class="chart-sector" data-chart-arc data-chart-segment="${i}" data-start="${start}" data-angle="${angle}" d="${d}" fill="none" stroke="url(#${name}-${i}-stroke)" stroke-width="${width}" stroke-linecap="round"/>`;
    }).join('')}</svg>`;
}
function cancel(element) { animations.get(element)?.cancel(); animations.delete(element); }
function tween(element, frames, duration = 650) {
    cancel(element);
    if (reduced.matches || document.hidden || !element.isConnected || !element.animate) return;
    const a = element.animate(frames, { duration, easing: 'cubic-bezier(.22,.75,.2,1)' });
    animations.set(element, a);
    a.finished.then(() => { if (animations.get(element) === a) animations.delete(element); }, () => {});
    return a;
}
function svgFor(root, kind) { return root?.matches?.(`[data-chart-kind=${kind}]`) ? root : root?.querySelector(`[data-chart-kind=${kind}]`); }
function updateSingle(root, kind, percent) {
    const arc = svgFor(root, kind)?.querySelector('[data-chart-arc]');
    if (!arc) return;
    const old = getComputedStyle(arc).strokeDashoffset, next = 100 - bound(percent, 0, 100);
    arc.setAttribute('stroke-dashoffset', next);
    arc.setAttribute('opacity', next === 100 ? 0 : 1);
    tween(arc, [{ strokeDashoffset: old }, { strokeDashoffset: String(next) }]);
}
export const updateRing = (root, value) => updateSingle(root, 'ring', value);
export const updateGauge = (root, value, max) => updateSingle(root, 'gauge', value / Math.max(Number(max) || 1, 1) * 100);
export function updateDonut(root, values) {
    const svg = svgFor(root, 'donut');
    if (!svg) return;
    const size = Number(svg.dataset.chartSize), stroke = Number(svg.dataset.chartStroke);
    proportions(values).forEach(({ start, angle }, i) => {
        const arc = svg.querySelector(`[data-chart-segment="${i}"]`);
        if (!arc) return;
        let oldStart = Number(arc.dataset.start), oldAngle = Number(arc.dataset.angle);
        const previous = sectorMotions.get(arc), progress = animations.get(arc)?.effect?.getComputedTiming().progress;
        if (previous && progress != null) {
            oldStart = previous.start + (previous.nextStart - previous.start) * progress;
            oldAngle = previous.angle + (previous.nextAngle - previous.angle) * progress;
        }
        arc.dataset.start = start; arc.dataset.angle = angle;
        const geometry = segmentGeometry(size, stroke, start, angle);
        arc.setAttribute('d', geometry.d);
        arc.setAttribute('stroke-width', geometry.width);
        const frames = Array.from({ length: 31 }, (_, step) => {
            const t = step / 30;
            return { offset: t, ...segmentFrame(size, stroke, oldStart + (start - oldStart) * t, oldAngle + (angle - oldAngle) * t) };
        });
        // 从当前呈现角度续接；语义值与最终几何已先提交。
        sectorMotions.set(arc, { start: oldStart, angle: oldAngle, nextStart: start, nextAngle: angle });
        tween(arc, frames);
    });
}
export function stopChartMotion() {
    animations.forEach(a => a.cancel()); animations.clear();
}
export function enhanceCharts(root) {
    observer?.disconnect();
    animations.forEach((_, el) => { if (!el.isConnected) cancel(el); });
    observer = new IntersectionObserver(entries => entries.forEach(entry => {
        if (!entry.isIntersecting) return;
        const el = entry.target; observer.unobserve(el);
        if (el.dataset.chartDrawn) return;
        el.dataset.chartDrawn = 'true';
        if (el.matches('[data-chart-arc]')) {
            if (el.getAttribute('opacity') !== '0' && el.getAttribute('d') !== '') tween(el, [{ opacity: 0 }, { opacity: 1 }], 700);
        }
        else if (el.matches('[data-chart-bar]')) tween(el, [{ transform: 'scaleY(.02)', opacity: .25 }, { transform: 'scaleY(1)', opacity: 1 }], 600);
        else if (el.getTotalLength) {
            const length = el.getTotalLength();
            tween(el, [{ strokeDasharray: `${length} ${length}`, strokeDashoffset: length }, { strokeDasharray: `${length} ${length}`, strokeDashoffset: 0 }], 800);
        }
    }), { threshold: .15 });
    root.querySelectorAll('.chart-series,[data-chart-arc],[data-chart-bar]').forEach(el => observer.observe(el));
}
reduced.addEventListener('change', () => { if (reduced.matches) stopChartMotion(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) stopChartMotion(); });
