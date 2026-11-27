/**
 * [INPUT]: 依赖三个组件分册、局部光学交互、共享 SVG 与浏览器 DOM。
 * [OUTPUT]: 导出 120 项组件目录、分类检索与局部交互装配。
 * [POS]: 设计展厅组件工作台；检索栏使用功能材质，标本使用内容材质。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, esc } from './ui.js';
import { inputGroups, mountInputs } from './catalog-inputs.js';
import { dataGroups, mountData } from './catalog-data.js';
import { feedbackGroups, mountFeedback } from './catalog-feedback.js';
import { mountCatalogOptics } from './catalog-optics.js';

export const componentGroups = [...inputGroups, ...dataGroups, ...feedbackGroups];
const components = componentGroups.flatMap(group => group.items.map(item => ({ ...item, group: group.id, groupLabel: group.label })));
const preferences = { category: 'all', query: '', density: 'comfortable' };
let controller;
const regularItems = new Set(['actions-button','actions-icon-button','actions-split-button','actions-button-group','actions-toggle-button','actions-loading-button','selection-select','selection-multi-select','files-date-picker','files-date-range','files-time-picker','files-cascader','navigation-pagination','navigation-vertical-nav','navigation-command-palette','feedback-notification','overlays-popover','overlays-dropdown','overlays-context-menu','overlays-coachmark','organization-workspace-switcher']);
const mixedItems = new Set(['forms-unit-input','selection-switch','selection-combobox','selection-slider','selection-range-slider','navigation-tabs','navigation-segmented','navigation-toc','data-avatar-group','overlays-dialog','overlays-drawer','overlays-confirm-dialog','overlays-sheet','organization-member-card']);
const materialLabels = { regular:'REGULAR GLASS',standard:'STANDARD',mixed:'STANDARD + GLASS',media:'STANDARD + CLEAR',clear:'CLEAR / MEDIA',prominent:'STANDARD + PROMINENT' };
function materialFor(id) {
    if(regularItems.has(id))return 'regular';
    if(mixedItems.has(id))return 'mixed';
    if(['content-carousel','content-image-preview'].includes(id))return 'media';
    if(id==='overlays-lightbox')return 'mixed';
    if(['actions-fab','content-empty-state'].includes(id))return 'prominent';
    return 'standard';
}
const scenes = { actions:'操作 / 明确下一步', forms:'输入 / 保持专注', selection:'选择 / 触感回应', files:'组织 / 有序归档', navigation:'导航 / 连续空间', data:'数据 / 清晰阅读', visualization:'图表 / 精确与柔和', content:'内容 / 保留原形', feedback:'反馈 / 温和确定', overlays:'浮层 / 有源展开', delivery:'交付 / 证据连贯', organization:'组织 / 身份与边界' };

function specimen(item, index) {
    const material = materialFor(item.id);
    return `<article class="cl-specimen" id="component-${esc(item.id)}" data-component="${esc(item.id)}" data-group="${esc(item.group)}" data-material="${material}" data-search="${esc([item.name, item.en, item.description, ...(item.tags || []), item.groupLabel].join(' ').toLowerCase())}">
      <header class="cl-specimen-head"><span class="cl-number">${String(index + 1).padStart(3, '0')}</span><div><h3>${esc(item.name)}</h3><span>${esc(item.en)}</span></div><button type="button" class="cl-anchor" data-cl="permalink" data-id="${esc(item.id)}" aria-label="定位${esc(item.name)}">${icon('external', 15)}</button></header>
      <div class="cl-demo"><div class="cl-scene-label" aria-hidden="true"><span>${esc(scenes[item.group])}</span><span class="cl-scene-mark"></span></div>${item.render()}</div><footer class="cl-specimen-foot"><p>${esc(item.description)}</p><span>${esc(item.groupLabel)}</span><small class="cl-material-label">${materialLabels[material]}</small></footer>
    </article>`;
}

export function renderCatalog() {
    return `<section class="component-library" id="component-library" aria-labelledby="cl-title" data-density="${preferences.density}">
      <div class="cl-heading"><div><div class="eyebrow">PRISM / COMPONENT ATLAS</div><h2 id="cl-title">让操作有触感。<br>让内容有秩序。</h2><p>${components.length} 项组件 · ${componentGroups.length} 个类别。圆润轮廓、清晰内容与随手势回应的玻璃，共同构成 Prism 的组件语言。</p></div><div class="cl-total"><strong>${components.length}</strong><span>COMPONENTS</span></div></div>
      <div class="cl-toolbar glass"><label class="cl-search">${icon('search', 18)}<input type="search" data-cl-search placeholder="搜索组件、英文名或使用场景" aria-label="搜索组件" value="${esc(preferences.query)}"><kbd>/</kbd></label><div class="cl-density" role="group" aria-label="展厅密度"><button type="button" data-cl="density" data-value="comfortable" aria-pressed="${preferences.density === 'comfortable'}">${icon('grid', 16)}<span>舒展</span></button><button type="button" data-cl="density" data-value="compact" aria-pressed="${preferences.density === 'compact'}">${icon('menu', 16)}<span>紧凑</span></button></div></div>
      <div class="cl-layout"><aside class="cl-categories" aria-label="组件分类"><button type="button" data-cl="category" data-value="all" aria-pressed="${preferences.category === 'all'}"><span>全部组件</span><span>${components.length}</span></button>${componentGroups.map(group => `<button type="button" data-cl="category" data-value="${group.id}" aria-pressed="${preferences.category === group.id}"><span>${esc(group.label)}</span><span>${group.items.length}</span></button>`).join('')}<div class="cl-category-note">LIGHT / DARK<br>LIQUID / SOLID<br>POINTER / KEYBOARD</div></aside>
      <div class="cl-results"><div class="cl-result-heading"><span data-cl-count role="status" aria-live="polite">${components.length} 项组件</span><button type="button" class="text-link" data-cl="reset">清除筛选</button></div><div class="cl-grid">${components.map(specimen).join('')}</div><div class="cl-empty" hidden><span>${icon('search', 30)}</span><h3>没有找到这个组件</h3><p>尝试「表格」「选择」「上传」或英文名称。</p><button type="button" class="btn secondary" data-cl="reset">查看全部组件</button></div></div></div>
      <div class="cl-bottom-note"><span>${icon('shield', 18)}所有组件均为本地交互演示。</span><span>键盘可达 · 明暗主题 · 减少动态效果 · 降低透明度</span></div>
    </section>`;
}

function applyFilter(root) {
    const query = preferences.query.trim().toLowerCase();
    let count = 0;
    root.querySelectorAll('[data-component]').forEach(card => {
        const match = (preferences.category === 'all' || preferences.category === card.dataset.group) && query.split(/\s+/).every(word => card.dataset.search.includes(word));
        card.hidden = !match;
        if (match) count++;
    });
    root.querySelector('[data-cl-count]').textContent = `${count} / ${components.length} 项组件`;
    root.querySelector('.cl-empty').hidden = count > 0;
    root.querySelectorAll('[data-cl="category"]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.value === preferences.category)));
}

export function mountCatalog() {
    controller?.abort();
    const root = document.querySelector('.component-library');
    if (!root) return;
    controller = new AbortController();
    const { signal } = controller;
    mountInputs(root, signal);
    mountData(root, signal);
    mountFeedback(root, signal);
    mountCatalogOptics(root, signal);
    applyFilter(root);
    root.addEventListener('input', event => {
        if (!event.target.matches('[data-cl-search]')) return;
        preferences.query = event.target.value;
        applyFilter(root);
    }, { signal });
    root.addEventListener('click', event => {
        const button = event.target.closest('[data-cl]');
        if (!button) return;
        if (button.dataset.cl === 'category') {
            preferences.category = button.dataset.value;
            applyFilter(root);
        }
        if (button.dataset.cl === 'reset') {
            preferences.category = 'all'; preferences.query = '';
            root.querySelector('[data-cl-search]').value = '';
            applyFilter(root);
        }
        if (button.dataset.cl === 'density') {
            preferences.density = button.dataset.value;
            root.dataset.density = preferences.density;
            root.querySelectorAll('[data-cl="density"]').forEach(b => b.setAttribute('aria-pressed', String(b === button)));
        }
        if (button.dataset.cl === 'permalink') {
            const url = new URL(location.href); url.hash = `design/components/${button.dataset.id}`;
            history.replaceState(null, '', url);
            button.closest('[data-component]').classList.add('cl-highlight');
            setTimeout(() => button.closest('[data-component]')?.classList.remove('cl-highlight'), 1200);
        }
    }, { signal });
    root.addEventListener('keydown', event => {
        if (event.key === '/' && !event.target.matches('input,textarea,[contenteditable=true]')) {
            event.preventDefault(); root.querySelector('[data-cl-search]').focus();
        }
        if (event.target.matches('[data-cl-search]') && event.key === 'Escape') {
            preferences.query = ''; event.target.value = ''; applyFilter(root);
        }
    }, { signal });
    const deepId = location.hash.split('/')[2];
    if (!deepId && location.hash.split('/')[1] === 'components') requestAnimationFrame(() => root.scrollIntoView({ block: 'start' }));
    if (deepId) {
        const card = document.getElementById(`component-${deepId}`);
        if (card) {
            preferences.category = 'all'; preferences.query = '';
            root.querySelector('[data-cl-search]').value = '';
            applyFilter(root);
            requestAnimationFrame(() => { card.scrollIntoView({ block: 'center' }); card.classList.add('cl-highlight'); });
        }
    }
}
