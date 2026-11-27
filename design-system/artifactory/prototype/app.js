/**
 * [INPUT]: 依赖项目交付列表、节点池、账户动作与浏览器存储。
 * [OUTPUT]: 提供范围深链、交付向导、任务弹层、键盘连续性与主题光环。
 * [POS]: 独立原型应用入口；所有变更仅作用于演示状态。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { projects, builds as demoBuilds } from './data.js';
import { renderPage } from './pages.js';
import { renderFlow } from './flows.js';
import { bindFlowInteractions, captureFlowState } from './flow-interactions.js';
import { renderBuildWizardBody, captureBuildWizard, validateBuildWizard, buildWizardPayload, bindBuildWizard } from './flow-wizard.js';
import { notificationItems } from './entry-notifications.js';
import { bindDevicePrototype } from './device-detail.js';
import { getPools } from './device-data.js';
import { handleDeviceAction, handleDeviceSubmit, handleDevicePaste, handleDeviceClose, syncDeviceTerminal } from './entry-device-actions.js';
import { renderAdmin } from './admin.js';
import { captureMotion, enhanceMotion, enterOverlay, exitOverlay, replayMaterial } from './motion.js';
import { mountCatalog } from './catalog.js';
import { mountOptics } from './optics.js';
import { entryNavigation, entryNavActive, entryScope, entryBreadcrumb, projectChoices, scopedEmpty, entryCommands, settingsSection, projectContext } from './entry-shell.js';
import { syncProjectResourceMode } from './entry-management.js';
import { renderHeaderTools, renderAccountShell } from './entry-header.js';
import { bindReleaseCreate, handleReleaseCreateAction, handleReleaseCreateSubmit, guardReleaseNavigation, guardReleaseHashChange, releaseCreateBeforeUnload } from './entry-release-create.js';
import { handleEntryAction, handleEntrySubmit, handleEntryClose } from './entry-actions.js';
import { getWorkspaces, getWorkspaceProjects, isProjectFavorite, restoreEntryProjects, createEntryProject, createEntryWorkspace, validateEntryRegistration, getEntryRelease, saveEntryRelease } from './entry-data.js';
const { icon, btn, esc, field, input, select, kv, badge } = U;
const root = document.getElementById('app'), overlayRoot = document.getElementById('overlay-root');
const saved = (() => { try {
    return JSON.parse(sessionStorage.getItem('prism-demo') || '{}');
}
catch {
    return {};
} })();
const state = { workspace: 'Orbital', buildTab: 'process', nodeTab: 'overview', settingsTab: 'general', query: '', ...saved, mobileNav: false, terminalConnected: false };
state.workspaceId ||= getWorkspaces(state).find(workspace => workspace.name === state.workspace)?.id || 'orbital';
restoreEntryProjects(state);
let route = '', modal = null, lastFocus = null, notifyTimer, searchTimer;
const freshWizard = () => ({ step: 1, ref: 'release/2.4', requirements: '构建桌面安装包，使用项目授权的签名身份。保留源码与校验证据。', targets: ['macOS arm64', 'Windows amd64', 'Linux amd64', '容器镜像 amd64', '容器镜像 arm64'], project: 'Orbital Desktop', version: '2.4.0', dirty: false });
let wizard = freshWizard();
const titles = { login: '登录', register: '创建账户', tenants: '工作空间', system: '系统设置', join: '接受邀请', access: '访问权限', overview: '概览', projects: '项目', project: '项目概览', 'project-settings': '项目设置', builds: '构建中心', build: '构建详情', releases: '版本与交付', release: '版本详情', artifacts: '制品库', registry: '镜像仓库', nodes: '构建设备', node: '设备详情', terminal: '设备终端', pools: '节点池', observability: '观测', credentials: '凭据与签名', team: '团队与权限', activity: '审计记录', settings: '设置', notifications: '通知中心', design: '设计语言', onboarding: '开始使用' };
const systemTheme = matchMedia('(prefers-color-scheme: dark)');
let themeIntent = localStorage.getItem('prism-theme') || 'system';
let currentTheme = themeIntent === 'system' ? systemTheme.matches ? 'dark' : 'light' : themeIntent, themeAnimation = null, themeWave = null;
const themeMotion = matchMedia('(prefers-reduced-motion: reduce)');
const themeOptics = matchMedia('(prefers-reduced-transparency: reduce), (prefers-contrast: more), (forced-colors: active)');
document.documentElement.dataset.theme = currentTheme;
document.documentElement.dataset.themePreference = themeIntent;
document.documentElement.dataset.transparency = localStorage.getItem('prism-transparency') || 'normal';
document.documentElement.dataset.contrast = localStorage.getItem('prism-contrast') || 'normal';
function clearThemeWave() {
    themeAnimation?.cancel();
    themeWave?.remove();
    themeAnimation = themeWave = null;
}
function changeTheme(next, trigger) {
    const bounds = trigger?.getBoundingClientRect();
    const x = bounds ? bounds.left + bounds.width / 2 : innerWidth / 2;
    const y = bounds ? bounds.top + bounds.height / 2 : innerHeight / 2;
    themeIntent = next;
    clearThemeWave();
    currentTheme = next === 'system' ? systemTheme.matches ? 'dark' : 'light' : next;
    const html = document.documentElement;
    html.dataset.theme = currentTheme;
    html.dataset.themePreference = next;
    localStorage.setItem('prism-theme', next);
    const authDraft = [...root.querySelectorAll('.ea-form input')].map(input => ({ name: input.name, value: input.value, checked: input.checked, type: input.type }));
    render();
    authDraft.forEach(saved => { const input = root.querySelector(`.ea-form input[name="${CSS.escape(saved.name)}"]`); if (input) { input.value = saved.value; input.checked = saved.checked; input.type = saved.type; } });
    root.querySelectorAll('.ea-password-wrap').forEach(wrap => {
        const toggle = wrap.querySelector('[data-action=entry-password]'), visible = wrap.querySelector('input')?.type === 'text';
        toggle?.setAttribute('aria-pressed', String(visible)); toggle?.setAttribute('aria-label', visible ? '隐藏密码' : '显示密码');
    });
    if (document.hidden || themeMotion.matches || themeOptics.matches || html.dataset.transparency === 'reduce' || html.dataset.contrast === 'more' || !Element.prototype.animate) return;
    const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
    const wave = document.createElement('div');
    wave.className = 'theme-wave';
    wave.setAttribute('aria-hidden', 'true');
    Object.assign(wave.style, { left: `${x - radius}px`, top: `${y - radius}px`, width: `${radius * 2}px`, height: `${radius * 2}px` });
    document.body.append(wave);
    themeWave = wave;
    try {
        // 高光只作装饰，主题与控件在动画开始前已经可用。
        const animation = wave.animate([
            { transform: 'scale(.02)', opacity: 0 },
            { opacity: .75, offset: .22 },
            { transform: 'scale(1)', opacity: 0 },
        ], { duration: 560, easing: 'cubic-bezier(.2,.75,.2,1)' });
        themeAnimation = animation;
        const clean = () => {
            wave.remove();
            if (themeWave === wave) themeAnimation = themeWave = null;
        };
        animation.finished.then(clean, clean);
    } catch { clearThemeWave(); }
}
themeMotion.addEventListener('change', clearThemeWave);
systemTheme.addEventListener('change', () => { if (themeIntent === 'system') changeTheme('system'); });
themeOptics.addEventListener('change', clearThemeWave);
window.addEventListener('blur', clearThemeWave);
document.addEventListener('visibilitychange', () => { if (document.hidden) clearThemeWave(); });
new MutationObserver(() => {
    if (document.documentElement.dataset.transparency === 'reduce' || document.documentElement.dataset.contrast === 'more') clearThemeWave();
}).observe(document.documentElement, { attributes: true, attributeFilter: ['data-transparency', 'data-contrast'] });
function persist() { const { query, mobileNav, terminalConnected, terminalOutput, terminalAuthorizationDeadline, terminalLeaseDeadline, ...rest } = state; sessionStorage.setItem('prism-demo', JSON.stringify(rest)); }
function notify(message, error = false) { clearTimeout(notifyTimer); document.getElementById('notify').innerHTML = `<div class="notify-message ${error ? 'error' : ''}">${icon(error ? 'warn' : 'check', 19)}<span>${esc(message)}</span>${error?'<button class="icon-button" data-action="dismiss-notify" aria-label="关闭提示">×</button>':''}</div>`; if (!error) notifyTimer = setTimeout(() => document.getElementById('notify').innerHTML = '', 4500); }
function go(to) { if (guardReleaseNavigation(to, state, { showModal })) return; closeModal(true); if (location.hash.slice(1) === to) {
    render();
    return;
} location.hash = to; }
function shell(content) { return `<div class="app-shell">${state.mobileNav ? '<div class="mobile-nav-scrim" data-action="close-nav"></div>' : ''}<aside id="workspace-navigation" class="sidebar glass ${state.mobileNav ? 'mobile-open' : ''}" aria-label="工作空间导航"><button type="button" class="icon-button mobile-menu entry-nav-close" data-action="close-nav" aria-label="关闭导航">${icon('close',19)}</button><a class="brand" href="#tenants" data-route="tenants"><img src="assets/brand.svg" alt="">Artifactory <sup>PRISM</sup></a>${entryScope(route, state)}<nav class="nav-scroll">${entryNavigation(route, state).map(g => `<div class="nav-group"><p class="nav-label">${g.group}</p>${g.items.map(([id, glyph, label, count, value]) => `<a class="nav-item ${entryNavActive(id, value, route, state) ? 'active' : ''}" href="#${id}" data-route="${id}" ${value ? `data-project-tab="${value}"` : ''} ${['settings','project-settings'].includes(route) ? `data-action="management-section" data-value="${id.split('/').at(-1)}" data-project-scope="${route === 'project-settings'}"` : ''} ${entryNavActive(id, value, route, state) ? 'aria-current="page"' : ''}>${icon(glyph, 17)}<span>${label}</span>${count ? `<span class="nav-count">${count}</span>` : ''}</a>`).join('')}</div>`).join('')}</nav><div class="sidebar-bottom"><details class="entry-prototype-tools"><summary>原型工具</summary><a class="nav-item ${route === 'design' ? 'active' : ''}" href="#design" data-route="design">${icon('spark', 17)}<span>设计语言</span><span class="nav-count">01</span></a><button type="button" class="nav-item" data-action="tour" style="width:100%;background:none;text-align:left">${icon('help', 17)}<span>产品导览</span>${icon('external', 12)}</button></details><button class="sidebar-footer" data-action="account" style="width:100%;text-align:left;background:none">${U.avatar('林', 'purple')}<span><strong>林知远</strong><small class="muted">个人账户</small></span><span class="spacer"></span>${icon('dots', 17)}</button></div></aside><div class="shell-content"><header class="topbar glass"><button type="button" class="icon-button mobile-menu" data-action="open-nav" aria-controls="workspace-navigation" aria-expanded="${!!state.mobileNav}" aria-label="打开导航">${icon('menu', 19)}</button><div class="breadcrumbs" aria-label="当前位置">${entryBreadcrumb(route, state, titles[route])}</div><div class="topbar-tools"><button class="search-trigger" data-action="search" aria-label="搜索与跳转，快捷键 Command 或 Control K">${icon('search', 17)}<span>搜索与跳转</span><kbd>⌘ K</kbd></button>${renderHeaderTools(state, { mobileAccount: true })}</div></header><main id="main" class="main" tabindex="-1">${content}<footer class="demo-footer"><span class="row"><span class="demo-chip">DESIGN PROTOTYPE</span><span>所有数据与操作均为本地演示</span></span><a href="#design" data-route="design">Prism 设计系统 · 1.4</a><span>ARTIFACTORY / BUILD WITHOUT BOUNDARIES</span></footer></main></div></div>`; }
function render(focus = false) {
    captureFlowState(root, state);
    const active = document.activeElement;
    const focusMain = active?.id === 'main' && root.contains(active);
    const focusAction = root.contains(active) && active.dataset.action;
    const focusValue = active?.dataset.value, focusProject = active?.dataset.project;
    const focusSelect = root.contains(active) && active.closest('.select')?.querySelector('[data-select]')?.dataset.select;
    const previous = captureMotion();
    const requested = decodeURIComponent(location.hash.slice(1) || 'overview');
    const parts = requested.split('/');
    route = parts[0];
    if (['verify', 'email-verify', 'unsubscribe'].includes(route)) state.emailToken = parts[1] || 'demo';
    if (route === 'join') state.inviteToken = parts[1] || 'demo';
    if (['project', 'project-settings'].includes(route) && parts[1]) state.projectId = parts[1];
    if (route === 'project') { state.projectTab = ['releases', 'builds', 'artifacts'].includes(parts[2]) ? parts[2] : 'overview'; state.releaseCreating = parts[2] === 'releases' && parts[3] === 'new'; } else state.releaseCreating = false;
    if (route === 'settings') state.managementSection = settingsSection(state, null, parts[1]);
    if (route === 'project-settings') state.projectSettingsSection = settingsSection(state, projectContext(route,state), parts[2]);
    if (route === 'node' || route === 'terminal') state.nodeId = parts[1] || 'studio';
    if (route === 'nodes') state.devicePoolFilter = parts[1] === 'pool' && getPools(state).some(pool => pool.id === parts[2]) ? parts[2] : '';
    if (route === 'build') state.selectedBuild = parts[1] || '1048';
    if (route === 'release') { state.selectedRelease = parts[1] || '2.4.0'; if (parts[2]) state.projectId = parts[2]; else if (!parts[1] || (!getEntryRelease(state, state.projectId, state.selectedRelease) && !projects.some(p => p.id === state.projectId && p.version === state.selectedRelease))) state.projectId = projects.find(p => p.version === state.selectedRelease)?.id || 'orbital'; }
    if (route === 'build') { const record = demoBuilds.find(b => b[0] === `#${state.selectedBuild}`), name = record?.[1] || state.createdBuild?.project; if (name) state.projectId = projects.find(p => p.name === name)?.id; }
    if (route !== 'terminal') { state.terminalConnected = false; state.terminalOutput = ''; }
    let page = scopedEmpty(route, state) || renderPage(route, state) || renderFlow(route, state) || renderAdmin(route, state);
    if (!page) {
        route = 'not-found';
        page = U.pageHeader('404 / LOST IN SPACE', '这条路径还没有抵达。', '检查地址，或回到工作空间继续。') + U.panel(U.empty('页面不存在', '项目、版本和设备仍在原来的位置。', btn('回到概览', '', 'primary', 'arrow', 'data-route="overview"')));
    }
    root.innerHTML = ['login', 'register', 'tenants', 'join'].includes(route) ? page : ['notifications', 'verify', 'email-verify', 'unsubscribe'].includes(route) ? renderAccountShell(page, state) : shell(page);
    document.title = `${titles[route] || '欢迎'} · Artifactory Prism`;
    root.inert = !!modal;
    if (focus || focusMain) document.getElementById('main')?.focus({ preventScroll: true });
    else if (focusAction) [...root.querySelectorAll('[data-action]')].find(el => el.dataset.action === focusAction && el.dataset.value === focusValue && el.dataset.project === focusProject)?.focus({ preventScroll: true });
    else if (focusSelect) root.querySelector(`[data-select="${CSS.escape(focusSelect)}"]`)?.closest('.select')?.querySelector('summary')?.focus({ preventScroll: true });
    decorate();
    bindFlowInteractions(root, state, { render, persist, notify });
    root.querySelector('.shell-content')?.toggleAttribute('inert', !!state.mobileNav);
    document.body.style.overflow = modal || state.mobileNav ? 'hidden' : '';
    enhanceMotion(previous, focus || !previous.page);
    mountCatalog();
    mountOptics();
    bindDevicePrototype(root, state, { go, syncTerminal: currentRoute => syncDeviceTerminal(currentRoute, state) });
    bindReleaseCreate(root, state, { persist });
}
function decorateScrollRegions() {
    root.querySelectorAll('.table-wrap,.pipeline,.stage-track,.log-content,.code-block,.component-library .cd-table-wrap').forEach(el => {
        if (el.closest('.component-library') && !el.matches('.cd-table-wrap')) return;
        const overflow = el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1;
        el.toggleAttribute('data-scroll-region', overflow);
        if (!overflow) { el.removeAttribute('tabindex'); el.removeAttribute('role'); el.removeAttribute('aria-label'); return; }
        el.tabIndex = 0; el.setAttribute('role', 'region');
        el.setAttribute('aria-label', el.matches('.cd-table-wrap') ? `${el.closest('[data-component]')?.querySelector('h3')?.textContent} · ${el.querySelector('caption')?.textContent}` : el.closest('section')?.querySelector('h2')?.textContent || (el.matches('.pipeline,.stage-track') ? '构建阶段，可横向滚动' : '数据区域，可滚动阅读'));
    });
}
window.addEventListener('resize', decorateScrollRegions);
document.addEventListener('toggle', () => requestAnimationFrame(decorateScrollRegions), true);
function decorate() { syncProjectResourceMode(document); requestAnimationFrame(decorateScrollRegions); document.querySelectorAll('.search-field input:not(.input)').forEach(x => x.classList.add('input')); document.querySelectorAll('[data-search]').forEach(x => x.value = state.query || ''); document.querySelectorAll('.select summary').forEach(s => s.setAttribute('aria-expanded', String(s.parentElement.open))); }
window.addEventListener('hashchange', () => { if (guardReleaseHashChange(state, { showModal })) return; state.query = ''; state.projectBuildStatus = ''; state.projectDeliveryOrder = ''; state.mobileNav = false; state.emailConfirmedKey = null; render(true); window.scrollTo(0, 0); });
window.addEventListener('beforeunload', event => releaseCreateBeforeUnload(event, state));
function closeNavigation() { state.mobileNav = false; render(); root.querySelector('[data-action=open-nav]')?.focus({ preventScroll: true }); }
function saveSettings(form) {
    if (!form || !form.reportValidity()) return;
    const values = Object.fromEntries(new FormData(form));
    form.querySelectorAll('input[type=checkbox][name]').forEach(el => values[el.name] = el.checked);
    if (route === 'project-settings') state.projectSettings = { ...state.projectSettings, [state.projectId || 'orbital']: { ...state.projectSettings?.[state.projectId || 'orbital'], ...values } };
    else state.settingsValues = { ...state.settingsValues, ...values };
    if (values.teamName) state.teamName = values.teamName;
    persist(); notify('设置已保存到当前演示会话。');
}
function closeModal(force = false) { if (!modal)
    return; if (!force && handleDeviceClose(modal, state, { showModal, closeModal })) return; if (handleEntryClose(modal, state, { showModal, closeModal }, force)) return; if (modal === 'wizard' && !force && wizard.dirty) {
    wizardCapture();
    const old = modal;
    showModal('discard', '放弃构建草稿？', '<p>关闭后将丢弃本次填写的构建要求和目标。你也可以返回继续。</p>', btn('继续填写', 'resume-wizard') + btn('放弃草稿', 'discard-wizard', 'danger'));
    modal = old === 'wizard' ? 'discard' : old;
    return;
} exitOverlay(); overlayRoot.innerHTML = ''; modal = null; root.inert = false; document.body.style.overflow = ''; lastFocus?.focus?.({ preventScroll: true }); }
function showModal(id, title, body, footer = '', options = {}) { if (!modal) lastFocus = document.activeElement; modal = id; root.inert = true; document.body.style.overflow = 'hidden'; overlayRoot.innerHTML = `<div class="overlay-backdrop ${options.drawer ? 'drawer-backdrop' : ''}" data-backdrop><section class="dialog glass ${options.wide ? 'wide' : ''} ${options.drawer ? 'drawer' : ''}" role="dialog" aria-modal="true" aria-labelledby="dialog-title"><header class="dialog-header"><div><div class="eyebrow">ARTIFACTORY / ${options.eyebrow || 'LOCAL DEMO'}</div><h2 id="dialog-title">${title}</h2>${options.description ? `<p>${options.description}</p>` : ''}</div><button class="icon-button" data-action="close-modal" aria-label="关闭弹窗">${icon('close', 19)}</button></header><div class="dialog-body">${body}</div>${footer ? `<footer class="dialog-footer"><small>原型演示 · 不调用真实服务</small><div class="row">${footer}</div></footer>` : ''}</section></div>`; decorate(); enterOverlay(); queueMicrotask(() => { const target = overlayRoot.querySelector('input:not([type=hidden]),textarea,button,summary'); target?.focus({ preventScroll: true }); }); }
const submitButton = (label, formId) => `<button class="btn primary" type="submit" form="${formId}">${label}${icon('arrow', 16)}</button>`;
function formModal(id, title, fields, label = '创建', options = {}) { showModal(id, title, `<form id="modal-form" data-form="${id}">${fields}</form>`, btn('取消', 'close-modal', 'ghost') + submitButton(label, 'modal-form'), options); }
function wizardCapture() { captureBuildWizard(overlayRoot.querySelector('#wizard-form'), wizard); }
function showWizard() {
    const s = wizard.step;
    showModal('wizard', '发起一次构建', renderBuildWizardBody(wizard), (s > 1 ? btn('上一步', 'wizard-back', 'ghost', 'arrowLeft') : btn('取消', 'close-modal', 'ghost')) + btn(s === 4 ? '确认并开始构建' : wizard.returnToReview ? '返回确认' : '下一步', s === 4 ? 'wizard-submit' : 'wizard-next', 'primary', 'arrow'), { wide: true, eyebrow: `BUILD / 0${s}`, description: `${wizard.project} · ${wizard.version}` });
    bindBuildWizard(overlayRoot, wizard, showWizard);
    queueMicrotask(() => overlayRoot.querySelector('.wizard-question')?.focus({ preventScroll: true }));
}
function wizardAdvance(final = false) {
    wizardCapture();
    if (!overlayRoot.querySelector('#wizard-form').reportValidity()) return;
    const error = validateBuildWizard(wizard, final);
    if (error) { overlayRoot.querySelector('#wizard-error').textContent = error; return; }
    if (final) {
        const payload = buildWizardPayload(wizard);
        state.newBuild = true;
        state.createdBuild = { ref: wizard.ref, requirements: wizard.requirements, targets: [...wizard.targets], project: wizard.project, projectId: wizard.projectId || state.projectId, workspaceId: wizard.workspaceId || state.workspaceId, version: wizard.version, targetRequirements: payload.target_requirements, parameters: payload.parameters };
        state.selectedBuild = '1049'; state.buildTab = 'process'; persist(); wizard.dirty = false;
        closeModal(true); go('build/1049'); notify('演示构建已创建，已固定本次源码与目标。'); return;
    }
    wizard.step = wizard.returnToReview ? 4 : wizard.step + 1; wizard.returnToReview = false; showWizard();
}
function commandSearch() { showModal('search', '搜索与跳转', `<div class="command-search">${icon('search', 20)}<input data-command-search placeholder="搜索当前范围的页面、项目或设置" aria-label="搜索导航"></div><div class="command-results" id="command-results"></div><div class="command-caption">↑ ↓ 选择 · Enter 打开 · Esc 关闭</div>`, '', { eyebrow: 'QUICK NAVIGATION' }); renderCommands(''); overlayRoot.querySelector('input').focus(); }
function renderCommands(query) {
    const needle = query.trim().toLowerCase(), items = entryCommands(route, state).filter(item => `${item.label} ${item.group}`.toLowerCase().includes(needle));
    document.getElementById('command-results').innerHTML = items.length ? items.map(item => `<a class="command-result" href="${item.href || `#${item.id}`}" ${item.href ? 'target="_blank"' : `data-route="${item.id}"`}>${icon(item.glyph, 17)}<span>${esc(item.label)}</span><small>${esc(item.group)}</small>${icon('chevron', 12)}</a>`).join('') : '<p class="muted" style="padding:20px">当前范围没有匹配结果。</p>';
}
const manifest = () => JSON.stringify({ demo: true, schema_version: 2, project: 'orbital-desktop', version: '2.4.0', selected_build: '#1047', source: { ref: 'release/2.4', commit: 'e7a2c91', immutable: true }, targets: 6, files: 4, image: { platforms: ['linux/amd64', 'linux/arm64'], digest: 'sha256:84fb26c7...a102e9d4' }, published: !!state.published }, null, 2);
function download(name = 'artifactory-demo.json') { const text = /SHA256/.test(name) ? '# Artifactory 演示校验清单，不用于真实文件验证\n6ca9a2b1f82c...  orbital-desktop-2.4.0-macos-arm64.dmg\n' : name.endsWith('.log') ? 'Artifactory demo build #1048\n[PASS] Windows signature verified\n[WAIT] macOS signing\n' : manifest(); const blob = new Blob([text], { type: 'text/plain;charset=utf-8' }), a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = /\.(dmg|exe|AppImage|zip)$/.test(name) ? `${name}.demo.txt` : name; a.click(); setTimeout(() => URL.revokeObjectURL(a.href), 1000); notify('已下载演示文件。安装包在原型中以说明文本提供。'); }
const forms = {
    'new-tenant': () => formModal('new-tenant', '创建你的工作空间', `<p class="muted mb">为团队建立独立的项目、成员和构建资源边界。</p>` + field('工作空间名称', `<input class="input" name="name" placeholder="例如 Northstar" required maxlength="100" autocomplete="organization">`, '创建后直接进入项目目录。工作空间标识由系统生成。'), '创建工作空间'),
    'join': () => formModal('join', '接受工作空间邀请', field('邀请标识', `<input class="input" name="token" placeholder="输入演示邀请标识" required>`) + `<p class="field-help">接受后加入指定工作空间，权限按邀请范围授予。</p>`, '查看邀请')
};
async function action(name, el) {
    if (handleReleaseCreateAction(name, el, state, { go, showModal, closeModal, persist, notify })) return;
    if (handleDeviceAction(name, el, state, { showModal, formModal, closeModal, persist, render, notify, go })) return;
    if (handleEntryAction(name, el, state, { showModal, formModal, closeModal, persist, render, notify, go })) return;
    if (forms[name]) {
        forms[name]();
        return;
    }
    switch (name) {
        case 'entry-password': {
            const input = el.closest('.ea-password-wrap')?.querySelector('input');
            if (!input) break;
            const visible = input.type === 'password'; input.type = visible ? 'text' : 'password';
            el.setAttribute('aria-pressed', String(visible)); el.setAttribute('aria-label', visible ? '隐藏密码' : '显示密码');
            break;
        }
        case 'entry-clear-tenants': state.tenantQuery = ''; render(); break;
        case 'entry-project-view': state.projectView = el.dataset.value; persist(); render(); break;
        case 'entry-project-tab': state.projectTab = el.dataset.value; go(`project/${state.projectId}${state.projectTab === 'releases' ? '/releases' : ''}`); break;
        case 'entry-switch-project': showModal('project-switch', '选择项目', `<div class="stack">${projectChoices(state)}</div>`, btn('所有项目', '', 'ghost', 'folder', 'data-route="projects"')); break;
        case 'entry-choose-project': state.projectId = el.dataset.project; state.projectTab = 'overview'; go(`project/${state.projectId}`); break;
        case 'entry-favorite': {
            const id = el.dataset.project || state.projectId;
            state.projectFavorites = { ...state.projectFavorites, [id]: !isProjectFavorite(state, id) };
            persist(); render(); break;
        }
        case 'new-build': {
            const inProject = ['release', 'project', 'project-settings'].includes(route);
            const record = route === 'release' && demoBuilds.find(b => b[2] === state.selectedRelease && (!state.projectId || projects.find(p => p.id === state.projectId)?.name === b[1]));
            const project = (record ? projects.find(p => p.name === record[1]) : inProject && projects.find(p => p.id === state.projectId)) || projects[0];
            const version = route === 'release' ? state.selectedRelease || project.version : project.version;
            const frozen = !getEntryRelease(state, project.id, version) && ((project.status === '已发布' && version === project.version) || (project.id === 'orbital' && (['2.3.8','2.3.1','2.3.0','2.3.0-rc.1'].includes(version) || (state.published && version === '2.4.0'))));
            if (frozen) {
                notify('此版本已发布。请创建新版本后构建。', true);
                go(`project/${project.id}/releases/new`);
                return;
            }
            wizard = freshWizard();
            wizard.version = version;
            wizard.project = project.name; wizard.projectId = project.id; wizard.workspaceId = state.workspaceId; wizard.repository = project.repository || `https://git.example/${project.repo}.git`; wizard.credential = project.credential || 'git/orbital-readonly';
            wizard.step = 1;
            wizard.dirty = false;
            showWizard();
            break;
        }
        case 'wizard-next':
            wizardAdvance();
            break;
        case 'wizard-submit':
            wizardAdvance(true);
            break;
        case 'wizard-back':
            wizardCapture();
            wizard.step = Math.max(1, wizard.step - 1);
            showWizard();
            break;
        case 'choose-ref':
            overlayRoot.querySelector('[name=ref]').value = el.dataset.value;
            wizard.dirty = true;
            break;
        case 'requirements-template':
            overlayRoot.querySelector('[name=requirements]').value = el.dataset.value === 'container' ? '构建 sync-service 的 linux/amd64 和 linux/arm64 镜像，固定版本与源码标签，保留完整镜像摘要。' : '构建 macOS、Windows 与 Linux 安装包，使用项目授权的签名身份，验证摘要并归档制品。';
            wizard.dirty = true;
            break;
        case 'resume-wizard':
            showWizard();
            break;
        case 'discard-wizard':
            wizard = freshWizard();
            closeModal(true);
            break;
        case 'theme':
            changeTheme(currentTheme === 'light' ? 'dark' : 'light', el);
            break;
        case 'transparency': {
            let d = document.documentElement;
            d.dataset.transparency = d.dataset.transparency === 'reduce' ? 'normal' : 'reduce';
            localStorage.setItem('prism-transparency', d.dataset.transparency);
            document.querySelectorAll('[data-action=transparency]').forEach(control => control.setAttribute(control.getAttribute('role') === 'switch' ? 'aria-checked' : 'aria-pressed', String(d.dataset.transparency === 'reduce')));
            notify(d.dataset.transparency === 'reduce' ? '已使用实色表面，降低透明度。' : '已恢复液态玻璃材质。');
            break;
        }
        case 'contrast': {
            const d = document.documentElement;
            d.dataset.contrast = d.dataset.contrast === 'more' ? 'normal' : 'more';
            localStorage.setItem('prism-contrast', d.dataset.contrast);
            el.setAttribute('aria-checked', String(d.dataset.contrast === 'more'));
            notify(d.dataset.contrast === 'more' ? '已增强边界与对比度。' : '已恢复标准对比度。');
            break;
        }
        case 'search':
            commandSearch();
            break;
        case 'close-modal':
            closeModal();
            break;
        case 'open-nav':
            state.mobileNav = true;
            render();
            queueMicrotask(() => root.querySelector('.sidebar a')?.focus());
            break;
        case 'close-nav':
            closeNavigation();
            break;
        case 'workspace':
            showModal('workspace', '选择工作空间', `<div class="stack">${getWorkspaces(state).map(w => `<button class="workspace-switch" data-action="choose-workspace" data-name="${esc(w.name)}" data-workspace="${esc(w.id)}" style="margin:0">${U.avatar(w.initial, w.color)}<span><strong>${esc(w.name)}</strong><small>${esc(w.id)} · ${esc(w.role)}</small></span>${icon('arrow', 17)}</button>`).join('')}</div>`, btn('查看全部', '', 'ghost', 'grid', 'data-route="tenants"'));
            break;
        case 'choose-workspace':
            state.workspace = el.dataset.name;
            state.workspaceId = el.dataset.workspace || getWorkspaces(state).find(workspace => workspace.name === state.workspace)?.id;
            state.query = ''; state.projectFilter = '全部项目'; state.projectTab = 'overview'; state.projectId = getWorkspaceProjects(state)[0]?.id;
            persist();
            go('projects');
            notify(`已进入 ${state.workspace} 演示工作空间。`);
            break;
        case 'build-tab':
            state.buildTab = el.dataset.value === 'logs' ? 'execution' : el.dataset.value;
            render();
            break;
        case 'node-tab':
            state.nodeTab = el.dataset.value;
            render();
            break;
        case 'settings-tab':
            state.settingsTab = el.dataset.value;
            render();
            break;
        case 'favorite':
            state.favorite = !state.favorite;
            persist();
            render();
            notify(state.favorite ? '已关注项目，将接收重要交付通知。' : '已取消关注项目。');
            break;
        case 'clear-search':
            state.query = '';
            state.projectFilter = '全部项目';
            render();
            break;
        case 'copy':
            try {
                await navigator.clipboard.writeText(el.dataset.copy || manifest());
                notify('已复制到剪贴板。');
            }
            catch {
                showModal('copy', '复制内容', `<textarea readonly rows="8">${esc(el.dataset.copy || manifest())}</textarea>`);
            }
            break;
        case 'download':
            download(el.dataset.file || 'artifactory-demo.json');
            break;
        case 'export':
            download(`${route}-demo-report.json`);
            break;
        case 'refresh':
            notify('已刷新演示快照，当前筛选与阅读位置保持不变。');
            break;
        case 'publish': go(`release/${state.selectedRelease || '2.4.0'}/${state.projectId || 'orbital'}`); notify('请在版本概览选择完整成功构建并明确发布。'); break;
        case 'edit-release':
            formModal('edit-release', '编辑版本说明', field('版本号', `<input class="input mono" value="${esc(state.selectedRelease || '2.4.0')} · 不可修改" readonly>`) + field('版本说明', `<textarea name="notes" rows="7">${esc(getEntryRelease(state, state.projectId, state.selectedRelease)?.notes ?? state.orbitalReleaseNotes ?? '更快的搜索，全新的离线工作流。优化大型工作空间启动时间与内存占用。')}</textarea>`), '保存说明');
            break;
        case 'cancel-build':
            showModal('cancel-build', '取消构建 #1048？', '<p>停止尚在运行与排队的目标。已完成的日志和制品证据会保留。</p>', btn('继续构建', 'close-modal', 'ghost') + btn('确认取消', 'confirm-cancel', 'danger', 'stop'));
            break;
        case 'confirm-cancel':
            state.buildCancelled = true;
            persist();
            closeModal(true);
            render();
            notify('演示构建已取消，历史记录已保留。');
            break;
        case 'retry-build':
            if (state.published) {
                notify('版本已发布，请创建新版本。', true);
                break;
            }
            wizard = freshWizard();
            const original = demoBuilds.find(b => b[0] === `#${state.selectedBuild || '1048'}`);
            if (state.selectedBuild === '1049' && state.createdBuild) { Object.assign(wizard, state.createdBuild); wizard.targetRequirements = wizard.targetRequirements?.map(target => ({ ...target, preset: target.kind === 'image' ? 'image' : `${target.os === 'darwin' ? 'macos' : target.os}-${target.arch}` })); }
            else if (original) {
                wizard.project = original[1]; wizard.version = original[2]; wizard.ref = original[4];
                if (original[0] === '#1048') wizard.targets.splice(1, 0, 'macOS amd64');
            }
            wizard.step = 4;
            showWizard();
            break;
        case 'view-manifest':
            if (el.dataset.image) {
                const image = ({'sync-service':['2.4.0','84fb26c7…a102e9d4','amd64 + arm64'],'atlas-api':['1.8.2','a84fb26c…','amd64 + arm64'],'beacon-gateway':['1.2.0','1dc08be2…','amd64']})[el.dataset.image];
                if (image) showModal('image-manifest', '镜像摘要', `${kv('组件', esc(el.dataset.image))}${kv('版本 / 标签', el.dataset.image === 'sync-service' && !state.published ? 'candidate-1047' : image[0])}${kv('架构', image[2])}${kv('摘要缩写', `<span class="mono">sha256:${image[1]}</span>`)}<p class="field-help mt">此演示未提供完整 OCI 清单。摘要缩写不能用于实际校验。</p>`, btn('关闭','close-modal','primary'));
                break;
            }
            showModal('manifest', '来源与交付清单', `<p class="small muted mb">这是演示清单，可检查固定源码与完整目标身份。</p><pre class="code-block">${esc(manifest())}</pre>`, btn('复制清单', 'copy', 'secondary', 'copy') + btn('下载', 'download', 'primary', 'download', 'data-file="orbital-release-demo.json"'), { wide: true });
            break;
        case 'artifact-detail': {
            const name = el.dataset.artifact || 'orbital-desktop-2.4.0-macos-arm64.dmg';
            const platform = name.includes('windows') ? 'Windows amd64' : name.includes('linux') ? 'Linux amd64' : name.includes('macos-amd64') ? 'macOS amd64' : 'macOS arm64';
            const size = ({'Windows amd64':'97.2 MB','Linux amd64':'112.6 MB','macOS amd64':'135.8 MB','macOS arm64':'128.4 MB'})[platform];
            const signer = platform.startsWith('macOS') ? 'Developer ID · Orbital Labs' : platform.startsWith('Windows') ? 'Authenticode · Orbital Labs' : '未提供签名 · SHA-256 已校验';
            const hash = ({'Windows amd64':'2d7b308c9a41','Linux amd64':'b30fe7a2c914','macOS amd64':'f8c1a306d74e','macOS arm64':'6ca9a2b1f82c'})[platform];
            showModal('artifact', '制品与信任证据', `<h3 class="mono artifact-name">${esc(name)}</h3><p class="small muted mb">构建 #${esc(el.dataset.build || '1047')} · ${size}</p>${kv('平台', platform)}${kv('源码', 'release/2.4 · e7a2c91')}${kv('签名身份', signer)}${kv('校验结果', badge(platform.startsWith('Linux') ? '校验通过' : '签名有效', 'success'))}${kv('文件摘要', `<span class="mono">sha256:${hash}…</span>`)}<p class="field-help mt">摘要为演示缩写。签名与内容校验分别呈现。</p>`, btn('下载演示文件', 'download', 'primary', 'download', `data-file="${esc(name)}"`));
            break;
        }
        case 'distribute':
            formModal('distribute', '按摘要分发镜像', (!state.published ? '<div class="notice warning mb">先正式发布版本，才能开始外部分发。</div>' : '') + field('目标仓库', input('registry', 'ghcr.io/orbital-team')) + field('凭据引用', input('credential', 'registry/ghcr-orbital')) + field('版本标签', '<div class="input mono">2.4.0 · 固定</div>'), '确认演示分发');
            break;
        case 'enroll-node':
            formModal('enroll-node', '让一台设备加入构建', field('设备名称', `<input class="input" name="name" value="My Build Device" required>`) + field('操作系统', select('platform', 'macOS arm64', ['macOS arm64', 'macOS amd64', 'Windows amd64', 'Linux amd64', 'Linux arm64'])) + field('加入构建池', select('pool', 'Apple 原生构建池', ['Apple 原生构建池', 'Windows 原生构建池', 'Linux 原生构建池'])) + `<div class="notice">${icon('shield', 20)}<p>生成 15 分钟有效的一次性注册说明。安装固定平台与签名版本。</p></div>`, '生成安装说明');
            break;
        case 'node-update':
            showModal('node-update', '设备版本已对齐', `${kv('平台版本', 'Artifactory 0.2.5')}${kv('设备版本', 'Emissary 0.2.5')}<p class="field-help">当前无需升级。版本差异只提示，由管理员手动发起升级。</p>`, btn('完成', 'close-modal', 'primary', 'check'));
            break;
        case 'drain-node':
            state.nodeDraining = !state.nodeDraining;
            persist();
            render();
            notify(state.nodeDraining ? '设备已停止接收新任务。当前任务继续完成。' : '设备已恢复接单。');
            break;
        case 'save-settings': {
            saveSettings(root.querySelector('form[data-form=settings]'));
            break;
        }
        case 'audit-detail':
            showModal('audit', '操作审计详情', `${kv('操作', esc(el.dataset.title || '创建构建'))}${kv('对象', esc(el.dataset.object || 'Orbital Desktop · #1048'))}${kv('操作者', '林知远')}${kv('发生时间', '2026-11-26 14:23:36 +08:00')}${kv('结果', badge('成功', 'success'))}${kv('请求标识', '<span class="mono">req_demo_8a42f</span>')}`, btn('关闭', 'close-modal', 'primary'));
            break;
        case 'reset-demo':
            sessionStorage.removeItem('prism-demo');
            location.reload();
            break;
        case 'logout':
            go('login');
            break;
        case 'archive-project':
            showModal('archive', `归档 ${esc(projects.find(p => p.id === state.projectId)?.name || '当前项目')}？`, '<p>归档后停止新构建。已有版本、制品与审计仍可读取。</p>', btn('取消', 'close-modal', 'ghost') + btn('确认归档', 'confirm-archive', 'danger'));
            break;
        case 'confirm-archive':
            closeModal(true);
            notify('项目归档流程已演示。可在生命周期设置中恢复。');
            break;
        case 'dismiss-notify': document.getElementById('notify').innerHTML=''; break;
        case 'material-replay': replayMaterial(); break;
        case 'sample-notify':
            notify('操作已完成，反馈位于顶部中央，不占页面布局。');
            break;
        case 'sample-danger':
            showModal('sample', '确认操作对象', '<p>危险操作需单独确认，并始终提供返回路径。</p>', btn('取消', 'close-modal', 'ghost') + btn('确认演示', 'sample-notify', 'danger'));
            break;
        case 'tour':
            showModal('tour', '一条完整的交付路径', `<div class="stack">${[['onboarding', '从零开始', '创建账户、加入团队、连接第一个项目。'], ['project/orbital', '连接源码与版本', '查看项目资源，再为交付创建版本。'], ['build', '跟随一次构建', '阅读过程、执行日志和 AI 方案。'], ['release', '交付一个确定版本', '选择完整成功构建，确认正式发布。'], ['nodes', '准备构建能力', '查看设备资源、工具链和演示终端。'], ['design', '浏览设计系统', '检查玻璃材质、字体和组件状态。']].map(([r, t, d], i) => `<a class="list-row" href="#${r}" data-route="${r}"><div class="row"><div class="step-circle">${i + 1}</div><div><h3>${t}</h3><p>${d}</p></div></div>${icon('arrow', 17)}</a>`).join('')}</div>`, btn('开始浏览', '', 'primary', 'arrow', 'data-route="onboarding"'), { wide: true });
            break;
        default: notify('此操作在当前演示中保留设计说明。');
    }
}
document.addEventListener('click', e => {
if (e.target.closest('.skip-link')) { e.preventDefault(); const main = document.getElementById('main'); main?.focus({ preventScroll: true }); main?.scrollIntoView({ block: 'start' }); return; }
const to = e.target.closest('[data-route]'); if (to) {
    e.preventDefault();
    state.mobileNav = false;
    if (to.dataset.workspaceContext) { const workspace = getWorkspaces(state).find(item => item.id === to.dataset.workspaceContext); if (!workspace) { notify('通知对象当前不可访问。', true); return; } state.workspaceId = workspace.id; state.workspace = workspace.name; state.projectId = null; persist(); }
    if (to.dataset.build) state.selectedBuild = to.dataset.build.replace('#','');
    else if (to.dataset.route === 'build') state.selectedBuild = '1048';
    if (to.dataset.release) state.selectedRelease = to.dataset.release;
    else if (to.dataset.route === 'release') state.selectedRelease = '2.4.0';
    if (to.dataset.project) state.projectId = to.dataset.project;
    if (to.dataset.route.startsWith('project/')) state.projectTab = to.dataset.projectTab || 'overview';
    go(to.dataset.route);
    return;
} const choice = e.target.closest('[data-select]'); if (choice) {
    const details = choice.closest('.select'), value = choice.dataset.value;
    details.querySelector('summary span').textContent = choice.textContent.trim();
    details.querySelector('input[type=hidden]').value = value;
    details.querySelector('input[type=hidden]').dispatchEvent(new Event('input', { bubbles: true }));
    details.querySelectorAll('[role=option]').forEach(x => x.setAttribute('aria-selected', String(x === choice)));
    details.open = false;
    details.querySelector('summary').focus();
    const name = choice.dataset.select;
    if (name === 'node_access_mode') syncProjectResourceMode(document);
    if (name === 'logTarget') { state.logTarget=value; render(); } if (name === 'planTarget') { state.planTarget=value; render(); } if (name === 'status') {
        state.status = value;
        render();
    }
    if (name === 'project-filter') {
        state.projectFilter = value;
        render();
    }
    if (name === 'entry-project-sort') { state.projectSort = value; render(); }
    if (name === 'release-list-status') { state.releaseListStatus = value; render(); }
    if (name === 'release-list-order') { state.releaseListOrder = value; render(); }
    if (name === 'project-build-status') { state.projectBuildStatus = value; render(); }
    if (name === 'project-delivery-order') { state.projectDeliveryOrder = value; render(); }
    if (name === 'device-pool-filter') { const pool = getPools(state).find(item => item.name === value); state.devicePoolFilter = pool?.id || ''; go(pool ? `nodes/pool/${pool.id}` : 'nodes'); }
    if (name === 'device-filter') {
        state.deviceFilter = value;
        render();
    }
    if (name === 'overview-range') { state.range = value; render(); }
    if (['noticeTenant','noticeType','noticeMode','noticeProject','auditRange'].includes(name)) { state[name] = value; state.noticePages = 1; persist(); render(); if (modal === 'notifications') action('notifications', choice); }
    if (name === 'accountTheme' || name === 'authTheme') {
        changeTheme(value === '跟随系统' ? 'system' : value === '深色' ? 'dark' : 'light', details.querySelector('summary'));
    }
    return;
} const a = e.target.closest('[data-action]'); if (a) {
    action(a.dataset.action, a);
    return;
} if (e.target.hasAttribute('data-backdrop'))
    closeModal(); document.querySelectorAll('.select[open]').forEach(d => { if (!d.contains(e.target))
    d.open = false; }); });
document.addEventListener('paste', e => handleDevicePaste(e, state, { showModal, notify }));
document.addEventListener('input', e => { if (e.target.matches('[data-command-search]'))
    renderCommands(e.target.value); if (e.target.matches('[data-search],[data-log-search]')) {
    const isLog = e.target.hasAttribute('data-log-search'), pos = e.target.selectionStart;
    state[isLog ? 'logQuery' : 'query'] = e.target.value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { render(); const el = root.querySelector(isLog ? '[data-log-search]' : '[data-search]'); el?.focus(); el?.setSelectionRange(pos, pos); }, 160);
} if (modal === 'wizard')
    wizard.dirty = true; });
document.addEventListener('input', e => {
    if (e.target.matches('.ea-form input')) e.target.form.querySelectorAll('input').forEach(input => input.setCustomValidity(''));
    if (e.target.closest('[data-form^="manage-"]')) e.target.form.querySelectorAll('input,textarea').forEach(input => input.setCustomValidity(''));
    if (e.target.closest('[data-form^="account-"]')) { e.target.form.dataset.dirty = 'true'; e.target.form.querySelectorAll('input').forEach(input => input.setCustomValidity('')); }
    if (!e.target.matches('[data-entry-tenant-search]')) return;
    const pos = e.target.selectionStart; state.tenantQuery = e.target.value; clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { render(); const input = root.querySelector('[data-entry-tenant-search]'); input?.focus(); input?.setSelectionRange(pos, pos); }, 160);
});
document.addEventListener('toggle', e => { if (e.target.matches?.('.select'))
    e.target.querySelector('summary').setAttribute('aria-expanded', String(e.target.open)); }, true);
document.addEventListener('submit', e => { const form = e.target.closest('[data-form]'); if (!form)
    return; e.preventDefault(); const d = new FormData(form), id = form.dataset.form;
    if (handleReleaseCreateSubmit(id, d, state, { go, persist, notify }, form)) return;
    if (handleDeviceSubmit(id, d, state, { showModal, formModal, closeModal, persist, render, notify, go }, form)) return;
    if (handleEntrySubmit(id, d, state, { showModal, formModal, closeModal, persist, render, notify, go }, form)) return;
    if (['login', 'register'].includes(id)) {
    if (id === 'register') {
        const result = validateEntryRegistration(state, d);
        if (result.error) {
            const input = result.field && form.elements.namedItem(result.field);
            if (input) { input.setCustomValidity(result.error); input.reportValidity(); input.addEventListener('input', () => input.setCustomValidity(''), { once: true }); }
            notify(result.error, true); return;
        }
    }
    state.demoLoggedOut = false;
    const destination = state.inviteReturn ? `join/${state.inviteReturn}` : /^(verify|email-verify|unsubscribe)\/[A-Za-z0-9%-]+$/.test(state.emailReturn || '') ? state.emailReturn : 'tenants'; state.inviteReturn = null; state.emailReturn = null; persist();
    go(destination);
    notify('已进入演示账户，未发送或保存登录信息。');
    return;
} if (id === 'wizard') {
    wizardAdvance(wizard.step === 4);
    return;
} if (id === 'new-project') {
    const result = createEntryProject(state, d);
    if (result.error) { const input = form.elements.namedItem(result.field); input.setCustomValidity(result.error); input.reportValidity(); input.addEventListener('input', () => input.setCustomValidity(''), { once: true }); return; }
    state.projectId = result.project.id; state.projectTab = 'overview'; persist();
    closeModal(true);
    go(`project/${result.project.id}`);
    notify('项目已加入演示工作空间，尚未连接真实仓库。');
    return;
} if (id === 'edit-release') {
    state.releaseNotes = String(d.get('notes'));
    if (getEntryRelease(state, state.projectId, state.selectedRelease)) saveEntryRelease(state, state.projectId, state.selectedRelease, state.releaseNotes);
    else if (state.projectId === 'orbital' && state.selectedRelease === '2.4.0') state.orbitalReleaseNotes = state.releaseNotes;
    persist();
    closeModal(true);
    render();
    notify('版本说明已保存，交付内容保持固定。');
    return;
} if (id === 'enroll-node') {
    showModal('installation', '设备安装与注册', `<div class="notice warning mb">${icon('shield', 20)}<p>演示说明，不包含可用令牌。生产安装令牌通过环境变量传递。</p></div>${kv('设备名称', esc(d.get('name')))}${kv('平台', esc(d.get('platform')))}${kv('固定版本', 'Artifactory 0.2.5')}<pre class="code-block mt"># 从官方入口获取所选平台的签名安装包\n# 注册令牌有效期为 15 分钟\nARTIFACTORY_ENROLLMENT_TOKEN=&lt;一次性令牌&gt;\nartifactory-emissary register --endpoint devices.example</pre><p class="field-help">注册完成 → 身份校验 → 能力上报 → 加入授权构建池。</p>`, btn('完成', 'close-modal', 'primary', 'check'));
    return;
} if (id === 'new-tenant') {
    const result = createEntryWorkspace(state, d);
    if (result.error) { const input = form.elements.namedItem(result.field); input.setCustomValidity(result.error); input.reportValidity(); input.addEventListener('input', () => input.setCustomValidity(''), { once: true }); return; }
    state.workspace = result.workspace.name; state.workspaceId = result.workspace.id; state.projectId = null; state.query = ''; state.projectFilter = '全部项目';
    persist();
    go('projects');
    notify('演示工作空间已创建。');
    return;
} if (id === 'join') {
    go(`join/${encodeURIComponent(String(d.get('token') || 'demo'))}`);
    return;
} if (id === 'distribute' && !state.published) {
    notify('请先正式发布版本，再分发到外部仓库。', true);
    return;
} if (id === 'settings') {
    saveSettings(form);
    return;
} closeModal(true); notify(id === 'new-pool' ? '演示构建池已创建。' : id === 'new-credential' ? '凭据引用已保存到演示会话。' : '演示操作已完成。'); });
document.addEventListener('keydown', e => { if (document.querySelector('.component-library dialog[open]') || e.defaultPrevented || e.isComposing) return;
if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    if (e.target.closest('.terminal-input,[data-terminal-input]') || (modal && modal !== 'search')) return;
    e.preventDefault();
    commandSearch();
    return;
} const details = e.target.closest('.select'); if (details && ['ArrowDown', 'ArrowUp', 'Home', 'End', 'Escape'].includes(e.key)) {
    const opts = [...details.querySelectorAll('[role=option]')];
    let idx = opts.indexOf(document.activeElement);
    if (e.key === 'Escape') {
        e.preventDefault();
        details.open = false;
        details.querySelector('summary').focus();
        return;
    }
    e.preventDefault();
    details.open = true;
    idx = e.key === 'Home' ? 0 : e.key === 'End' ? opts.length - 1 : (idx + (e.key === 'ArrowDown' ? 1 : -1) + opts.length) % opts.length;
    opts[idx]?.focus();
    return;
} if (e.key === 'Escape') {
    if (modal)
        closeModal();
    else if (state.mobileNav) {
        closeNavigation();
    }
    return;
} if (e.target.closest('.component-library')) return;
const tab = e.target.closest('[role=tab]'); if (tab && ['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(e.key)) {
    e.preventDefault();
    const all = [...tab.parentElement.querySelectorAll('[role=tab]')];
    let i = all.indexOf(tab);
    i = e.key === 'Home' ? 0 : e.key === 'End' ? all.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : -1) + all.length) % all.length;
    all[i].click();
    root.querySelectorAll('[role=tab]')[i]?.focus();
    return;
} if (modal === 'search' && ['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter'].includes(e.key)) {
    e.preventDefault();
    const items = [...overlayRoot.querySelectorAll('.command-result')];
    let i = items.indexOf(document.activeElement);
    if (e.key === 'Enter') (items[i] || items[0])?.click();
    else items[e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : (i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
    return;
} if (e.key === 'Tab' && (modal || state.mobileNav)) {
    const area = modal ? overlayRoot : root.querySelector('.sidebar');
    const els = [...area.querySelectorAll('button:not(:disabled),a[href],input:not([type=hidden]),textarea,summary,[tabindex="0"]')].filter(x => x.getClientRects().length);
    const first = els[0], last = els.at(-1);
    if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last?.focus();
    }
    else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first?.focus();
    }
} });
render();
