/**
 * [INPUT]: 依赖浏览器动画与可见性接口、用户偏好和页面 DOM。
 * [OUTPUT]: 导出材质增强、页面动效、弹层动效与位置快照。
 * [POS]: 连接页面与入口场景变化；功能层响应操作，内容入场后静止。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
const reducedTransparency = matchMedia('(prefers-reduced-transparency: reduce)');
const increasedContrast = matchMedia('(prefers-contrast: more)');
const forcedColors = matchMedia('(forced-colors: active)');
const finePointer = matchMedia('(hover: hover) and (pointer: fine)');
const surfaceSelector = '.sidebar,.topbar.glass,.search-trigger.glass,.topbar-utilities.glass,.toolbar-panel,.cl-toolbar,.liquid-lab-orb,[data-entry-glass],[data-entry-scene] .glass';
const activeAnimations = new Map();
const hoverGroups = '.nav-scroll,.tabs,.topbar-utilities';
const hoverStates = new Map();
const pressSelector = '.btn,.icon-button,.workspace-switch,.nav-item,.tab,.check-card,.select summary';
const spring = 'cubic-bezier(.2,1.15,.3,1)';
const ease = 'cubic-bezier(.2,.8,.2,1)';
let pointerFrame = 0, pointerSample, pointerSurface;
let dataObserver;

function animate(element, keyframes, options, key = 'interaction') {
    if (!element?.isConnected || document.hidden || reducedMotion.matches || !element.animate) return;
    let running = activeAnimations.get(element);
    if (!running) activeAnimations.set(element, running = new Map());
    running.get(key)?.cancel();
    const animation = element.animate(keyframes, { fill: 'backwards', ...options });
    running.set(key, animation);
    const clean = () => {
        if (running.get(key) !== animation) return;
        running.delete(key);
        if (!running.size) activeAnimations.delete(element);
    };
    animation.finished.then(clean, clean);
    return animation;
}
function stopMotion() {
    activeAnimations.forEach(animations => animations.forEach(animation => animation.cancel()));
    activeAnimations.clear();
    dataObserver?.disconnect();
    if (pointerFrame) cancelAnimationFrame(pointerFrame);
    pointerFrame = 0;
    resetPointer();
    clearHoverPreviews();
    document.querySelectorAll('.liquid-ripple,.motion-ghost').forEach(el => el.remove());
    /* --- 失焦时同时结束材质复位产生的 CSS 过渡 --- */
    document.getAnimations().forEach(animation => {
        if (typeof CSSTransition !== 'undefined' && animation instanceof CSSTransition) animation.cancel();
    });
}
function materialEnabled() {
    return !reducedTransparency.matches && !increasedContrast.matches && !forcedColors.matches && document.documentElement.dataset.transparency !== 'reduce' && document.documentElement.dataset.contrast !== 'more';
}
function resetPointer() {
    if (!pointerSurface) return;
    pointerSurface.classList.remove('liquid-pointer');
    pointerSurface.style.removeProperty('transform');
    pointerSurface.style.removeProperty('--rim-angle');
    pointerSurface = null;
}
function cancelDetached() {
    activeAnimations.forEach((animations, el) => {
        if (el.isConnected) return;
        animations.forEach(a => a.cancel());
        activeAnimations.delete(el);
    });
    resetPointer();
    clearHoverPreviews();
}
function addReflection(element) {
    element.classList.add('liquid-surface');
    addRim(element);
    if (!element.querySelector(':scope > .liquid-reflection')) {
        const reflection = document.createElement('span');
        reflection.className = 'liquid-reflection';
        reflection.setAttribute('aria-hidden', 'true');
        element.prepend(reflection);
    }
    if (!element.matches('.cl-toolbar') && !element.querySelector(':scope > .liquid-caustic')) {
        const caustic = document.createElement('span');
        caustic.className = 'liquid-caustic';
        caustic.setAttribute('aria-hidden', 'true');
        const light = document.createElement('span');
        light.className = 'liquid-caustic-light';
        caustic.append(light);
        element.prepend(caustic);
    }
}
function playCaustic(surface) {
    if (!materialEnabled()) return;
    animate(surface.querySelector(':scope > .liquid-caustic > .liquid-caustic-light'), [
        { transform: 'translateX(-110%)', opacity: 0 },
        { opacity: .7, offset: .45 },
        { transform: 'translateX(110%)', opacity: 0 },
    ], { duration: 720, easing: ease }, 'caustic');
}
/* --- 预览层独立于选中层；连续移动只改变装饰的矩阵 --- */
function hoverTarget(target) {
    const group = target?.closest?.(hoverGroups);
    if (!group || group.closest('.component-library,[inert]')) return null;
    const selector = group.matches('.nav-scroll') ? '.nav-item' : group.matches('.tabs') ? '.tab' : '.icon-button';
    const control = target.closest(selector);
    return control && group.contains(control) && !control.disabled ? { group, control } : null;
}
function cancelPreview(state) {
    const running = activeAnimations.get(state.indicator);
    running?.get('hover')?.cancel();
}
function showHoverPreview(target) {
    if (document.hidden || reducedMotion.matches || !materialEnabled()) return;
    const next = hoverTarget(target);
    if (!next) return;
    const { group, control } = next;
    let state = hoverStates.get(group);
    if (state?.control === control && state.visible) return;
    const oldRect = state?.visible ? state.indicator.getBoundingClientRect() : null;
    const oldOpacity = state?.visible ? getComputedStyle(state.indicator).opacity : '0';
    if (!state) {
        const indicator = document.createElement('span');
        indicator.className = 'glass-hover-indicator';
        indicator.setAttribute('aria-hidden', 'true');
        group.prepend(indicator);
        state = { indicator };
        hoverStates.set(group, state);
    }
    cancelPreview(state);
    const rect = control.getBoundingClientRect(), bounds = group.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return;
    const x = rect.left - bounds.left + group.scrollLeft - group.clientLeft;
    const y = rect.top - bounds.top + group.scrollTop - group.clientTop;
    const transform = `translate(${x}px,${y}px)`;
    Object.assign(state.indicator.style, {
        width: `${rect.width}px`, height: `${rect.height}px`, transform,
        opacity: '1', transformOrigin: 'left top',
    });
    state.control = control;
    state.visible = true;
    const from = oldRect ? {
        transform: `translate(${x + oldRect.left - rect.left}px,${y + oldRect.top - rect.top}px) scale(${oldRect.width / rect.width},${oldRect.height / rect.height})`, opacity: oldOpacity,
    } : { transform: `${transform} scale(.94,.9)`, opacity: 0 };
    animate(state.indicator, [from, { transform, opacity: 1 }], { duration: oldRect ? 260 : 180, easing: ease }, 'hover');
}
function hideHoverPreview(group) {
    const state = hoverStates.get(group);
    if (!state?.visible) return;
    const opacity = getComputedStyle(state.indicator).opacity;
    cancelPreview(state);
    state.visible = false;
    state.control = null;
    state.indicator.style.opacity = '0';
    animate(state.indicator, [{ opacity }, { opacity: 0 }], { duration: 160, easing: ease }, 'hover');
}
function clearHoverPreviews() {
    hoverStates.forEach(state => {
        cancelPreview(state);
        state.indicator.remove();
    });
    hoverStates.clear();
}
function addRim(element) {
    element.classList.add('liquid-rimmed');
    if(element.querySelector(':scope > .liquid-rim')) return;
    const rim=document.createElement('span');rim.className='liquid-rim';rim.setAttribute('aria-hidden','true');element.prepend(rim);
}
function measure(element) {
    if (!element) return null;
    const { left, top, width, height } = element.getBoundingClientRect();
    return { left, top, width, height };
}
export function captureMotion() {
    const activeTab = document.querySelector('[role=tab][aria-selected=true]');
    return {
        route: location.hash,
        page: !!document.querySelector('#main'),
        nav: measure(document.querySelector('.nav-scroll .nav-item.active')),
        tab: activeTab ? { label: activeTab.textContent, rect: measure(activeTab) } : null,
        charts: [...document.querySelectorAll('.main svg.chart,.main svg.chart-svg')].map(el => el.getAttribute('aria-label')).join('|'),
        scrollY,
    };
}
function placeIndicator(parent, active, className, previous) {
    if (!parent || !active) return;
    parent.querySelector(`:scope > .${className}`)?.remove();
    parent.style.position = 'relative';
    const rect = active.getBoundingClientRect(), bounds = parent.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1 || bounds.width < 1) return;
    const indicator = document.createElement('span');
    indicator.className = className;
    indicator.setAttribute('aria-hidden', 'true');
    Object.assign(indicator.style, {
        left: `${rect.left - bounds.left + parent.scrollLeft}px`,
        top: `${rect.top - bounds.top + parent.scrollTop}px`,
        width: `${rect.width}px`, height: `${rect.height}px`,
    });
    parent.prepend(indicator);
    if (previous?.width > 0 && previous?.height > 0) {
        const dx = previous.left - rect.left, dy = previous.top - rect.top;
        if (Math.abs(dx) + Math.abs(dy) + Math.abs(previous.width - rect.width) < 1) return;
        animate(indicator, [
            { transform: `translate(${dx}px,${dy}px) scale(${previous.width / rect.width},${previous.height / rect.height})`, borderRadius: '18px' },
            { transform: 'translate(0,0) scale(1.025,.985)', offset: .78 },
            { transform: 'translate(0,0) scale(1,1)' },
        ], { duration: 460, easing: ease }, 'indicator');
    }
}
function revealPage() {
    const main = document.querySelector('.main');
    if (!main) {
        const groups = document.querySelectorAll('.ea-story-copy,.ea-account-top,.ea-form-card,.et-hero-copy,.et-workspace-card,.tenant-page');
        groups.forEach((el, i) => animate(el, [{ opacity: 0, transform: 'translateY(16px)' }, { opacity: 1, transform: 'translateY(0)' }], { duration: 520, delay: Math.min(i * 45, 180), easing: ease }, 'entry'));
        revealScenes();
        return;
    }
    const heading = main.querySelector('.page-heading');
    animate(heading, [{ opacity: 0, transform: 'translateY(12px)' }, { opacity: 1, transform: 'translateY(0)' }], { duration: 440, easing: ease }, 'entry');
    /* --- 分组入场不改变滚动容器与固定功能层的合成 --- */
    if (main.querySelector('.component-library')) return;
    const cards = [...main.querySelectorAll('.content-surface,.tabs')].filter(el => !el.parentElement.closest('.content-surface') && el.getBoundingClientRect().top < innerHeight);
    cards.slice(0, 12).forEach((el, i) => animate(el, [
        { opacity: 0, transform: 'translateY(18px) scale(.992)' },
        { opacity: 1, transform: 'translateY(0) scale(1)' },
    ], { duration: 520, delay: 35 + Math.min(i * 35, 175), easing: ease }, 'entry'));
    revealScenes();
    const art = main.querySelector('.orbital-art');
    animate(art, [{ opacity: .1, transform: 'translateY(12px) scale(.93)' }, { opacity: 1, transform: 'translateY(0) scale(1)' }], { duration: 760, delay: 120, easing: spring }, 'entry');
}
function revealScenes() {
    document.querySelectorAll('[data-entry-scene] .glass').forEach((el, i) => {
        const base = getComputedStyle(el).transform;
        animate(el, [{ opacity: .1, transform: `translateY(${16 + i * 3}px) ${base === 'none' ? '' : base}` }, { opacity: 1, transform: base }], { duration: 760, delay: 100 + i * 50, easing: spring }, 'entry');
        if (materialEnabled()) animate(el.querySelector(':scope > .liquid-rim'), [{ opacity: .7 }, { opacity: .18 }], { duration: 900, delay: 120 + i * 50, easing: ease }, 'material');
    });
}
function drawChart(chart) {
    chart.querySelectorAll('path[fill="none"],polyline,.sparkline path').forEach(el => {
        if (!el.getTotalLength) return;
        const length = el.getTotalLength();
        animate(el, [{ strokeDasharray: `${length}`, strokeDashoffset: length }, { strokeDasharray: `${length}`, strokeDashoffset: 0 }], { duration: 860, delay: 100, easing: ease }, 'data');
    });
    const groups = chart.querySelectorAll('g[data-total],.bar-stack');
    const bars = groups.length ? groups : chart.querySelectorAll('rect[fill^="url(#bars"],.result-chart rect');
    bars.forEach((el, i) => {
        /* --- 整段缩放保持圆角裁切与小分段比例一致 --- */
        animate(el, [{ opacity: .1, transform: 'scaleY(.04)' }, { opacity: 1, transform: 'scaleY(1)' }], { duration: 680, delay: 90 + Math.min(i * 18, 240), easing: ease }, 'data');
    });
    chart.querySelectorAll('path[fill^="url(#area"],circle').forEach(el => animate(el, [{ opacity: 0 }, { opacity: 1 }], { duration: 500, delay: 330, easing: ease }, 'data'));
}
function drawData(replay = true) {
    if (reducedMotion.matches) return;
    const targets = [...document.querySelectorAll('.main .ep-progress>span,.main .progress-track>span,.main .resource-bar>span,.main .stage-meter>span,.main svg.chart,.main svg.chart-svg,.main svg.sparkline')].filter(el => !el.closest('.component-library'));
    const draw = el => {
        if (el.matches('svg')) drawChart(el);
        else animate(el, [{ transform: 'scaleX(.04)', transformOrigin: 'left' }, { transform: 'scaleX(1)', transformOrigin: 'left' }], { duration: 680, delay: 100, easing: ease }, 'data');
    };
    dataObserver?.disconnect();
    dataObserver = new IntersectionObserver(entries => entries.forEach(entry => {
        if (!entry.isIntersecting) return;
        dataObserver.unobserve(entry.target);
        draw(entry.target);
    }), { threshold: .12 });
    targets.forEach(el => {
        const rect = el.getBoundingClientRect();
        if (rect.top < innerHeight && rect.bottom > 0) { if (replay) draw(el); }
        else dataObserver.observe(el);
    });
}
export function enhanceMotion(previous = null, routeEntry = false) {
    cancelDetached();
    document.querySelectorAll(surfaceSelector).forEach(addReflection);
    document.querySelectorAll('.btn,.icon-button,.workspace-switch,.select summary').forEach(el => el.classList.add('liquid-control'));
    document.querySelectorAll('.page-actions>.btn.primary,.optics-lens').forEach(addRim);
    placeIndicator(document.querySelector('.nav-scroll'), document.querySelector('.nav-scroll .nav-item.active'), 'glass-nav-indicator', previous?.nav);
    const tabs = document.querySelector('.tabs');
    const tab = tabs?.querySelector('[aria-selected=true]');
    placeIndicator(tabs, tab, 'glass-tab-indicator', previous?.tab?.rect);
    const tabChanged = previous?.tab && tab && previous.tab.label !== tab.textContent;
    if (routeEntry) revealPage();
    else if (tabChanged) {
        const next = tabs.parentElement.matches('.ep-project-tabs') ? tabs.parentElement.nextElementSibling : tabs.nextElementSibling;
        animate(next, [{ opacity: .45, transform: 'translateY(8px)' }, { opacity: 1, transform: 'translateY(0)' }], { duration: 240, easing: ease }, 'tab');
    }
    const charts = [...document.querySelectorAll('.main svg.chart,.main svg.chart-svg')].map(el => el.getAttribute('aria-label')).join('|');
    drawData(routeEntry || tabChanged || previous?.charts !== charts);
}
export function enterOverlay() {
    cancelDetached();
    document.querySelectorAll('.dialog').forEach(addReflection);
    const dialog = document.querySelector('.dialog');
    if (!dialog) return;
    const origin = measure(dialog);
    const prior = enterOverlay.previous;
    const bodyOnly = prior && prior.title === dialog.querySelector('h2')?.textContent;
    enterOverlay.previous = { title: dialog.querySelector('h2')?.textContent, rect: origin };
    const anchor = lastInteraction?.isConnected ? lastInteraction.getBoundingClientRect() : null;
    const rect = dialog.getBoundingClientRect();
    const originX = anchor ? Math.max(10, Math.min(90, ((anchor.left + anchor.width / 2 - rect.left) / rect.width) * 100)) : 50;
    const originY = anchor ? Math.max(10, Math.min(90, ((anchor.top + anchor.height / 2 - rect.top) / rect.height) * 100)) : 50;
    dialog.style.transformOrigin = `${originX}% ${originY}%`;
    const drawer = dialog.classList.contains('drawer');
    if (!bodyOnly) {
        animate(document.querySelector('.overlay-backdrop'), [{ opacity: 0 }, { opacity: 1 }], { duration: 240, easing: ease }, 'overlay');
        animate(dialog, [
            { opacity: .05, transform: drawer ? 'translateX(58px) scale(.975)' : 'translateY(22px) scale(.91)', borderRadius: '40px' },
            { opacity: 1, transform: drawer ? 'translateX(-2px) scale(1)' : 'translateY(-2px) scale(1.008)', offset: .78 },
            { opacity: 1, transform: 'translate(0,0) scale(1)' },
        ], { duration: 480, easing: ease }, 'overlay');
    } else if (prior.rect?.height && Math.abs(prior.rect.height - rect.height) > 2) {
        dialog.style.transformOrigin = 'center';
        const shift = Math.max(-12, Math.min(12, (prior.rect.top - rect.top) * .2));
        animate(dialog, [{ transform: `translateY(${shift}px)`, borderRadius: '34px' }, { transform: 'translateY(0)' }], { duration: 340, easing: ease }, 'overlay');
    }
    animate(dialog.querySelector('.dialog-body'), [{ opacity: 0, transform: 'translateY(10px)' }, { opacity: 1, transform: 'translateY(0)' }], { duration: 340, delay: bodyOnly ? 35 : 90, easing: ease }, 'content');
    playCaustic(dialog);
    if (materialEnabled()) animate(dialog.querySelector(':scope > .liquid-rim'), [{ opacity: .85 }, { opacity: .2 }], { duration: 700, easing: ease }, 'material');
}
export function exitOverlay() {
    enterOverlay.previous = null;
    const dialog = document.querySelector('.dialog');
    if (!dialog || reducedMotion.matches) return;
    const rect = dialog.getBoundingClientRect();
    const ghost = document.createElement('div');
    ghost.className = 'motion-ghost glass';
    ghost.setAttribute('aria-hidden', 'true');
    ghost.inert = true;
    Object.assign(ghost.style, { left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
    document.body.append(ghost);
    const animation = animate(ghost, [{ opacity: .65, transform: 'scale(1)' }, { opacity: 0, transform: dialog.classList.contains('drawer') ? 'translateX(22px)' : 'translateY(8px) scale(.975)' }], { duration: 180, easing: 'ease-in' });
    if (animation) animation.finished.then(() => ghost.remove(), () => ghost.remove()); else ghost.remove();
}
export function replayMaterial() {
    const orb = document.querySelector('.liquid-lab-orb');
    if (!orb) return;
    const label = document.querySelector('.lab-state');
    if (reducedMotion.matches) {
        if (label) label.textContent = '已跟随系统减少动态效果偏好。';
        return;
    }
    const rect = orb.getBoundingClientRect();
    ripple(orb, rect.left + rect.width / 2, rect.top + rect.height / 2);
    animate(orb, [
        { transform: 'rotate(0) scale(1)', borderRadius: '38px' },
        { transform: 'rotate(-12deg) scale(.91)', borderRadius: '48px', offset: .22 },
        { transform: 'rotate(7deg) scale(1.08)', borderRadius: '54px', offset: .58 },
        { transform: 'rotate(0) scale(1)', borderRadius: '38px' },
    ], { duration: 780, easing: ease }, 'material');
    if (label) label.textContent = '形变、回弹与折射，随每一次操作发生。';
}
let lastInteraction;
function ripple(control, clientX, clientY) {
    if (document.hidden || reducedMotion.matches || !materialEnabled()) return;
    const rect = control.getBoundingClientRect(), size = Math.max(rect.width, rect.height) * 1.6;
    const wave = document.createElement('span');
    wave.className = 'liquid-ripple';
    wave.setAttribute('aria-hidden', 'true');
    Object.assign(wave.style, { width: `${size}px`, height: `${size}px`, left: `${clientX - rect.left - size / 2}px`, top: `${clientY - rect.top - size / 2}px` });
    control.querySelector('.liquid-ripple')?.remove();
    control.append(wave);
    const a = animate(wave, [{ opacity: .6, transform: 'scale(.04)' }, { opacity: 0, transform: 'scale(1)' }], { duration: 520, easing: ease }, 'ripple');
    if (a) a.finished.then(() => wave.remove(), () => wave.remove()); else wave.remove();
}
function pressControl(control, clientX, clientY) {
    ripple(control, clientX, clientY);
    animate(control, [
        { transform: 'scale(1)' },
        { transform: 'scale(.965,.94)', offset: .24 },
        { transform: 'scale(1.018,1.025)', offset: .65 },
        { transform: 'scale(1)' },
    ], { duration: 380, easing: ease }, 'press');
}
document.addEventListener('pointerdown', e => {
    const control = e.target.closest(pressSelector);
    lastInteraction = control || e.target;
    if (!control || control.disabled || e.button !== 0 || control.closest('[inert]')) return;
    pressControl(control, e.clientX, e.clientY);
}, { passive: true });
document.addEventListener('keydown', e => {
    if (!['Enter', ' '].includes(e.key) || e.repeat) return;
    const control = e.target.closest(pressSelector);
    if (!control || control.disabled || control.closest('[inert]')) return;
    lastInteraction = control;
    const rect = control.getBoundingClientRect();
    pressControl(control, rect.left + rect.width / 2, rect.top + rect.height / 2);
});
document.addEventListener('pointerover', e => {
    if (document.hidden || !finePointer.matches || reducedMotion.matches || !materialEnabled()) return;
    const surface = e.target.closest('.liquid-surface');
    if (surface && !surface.closest('[inert]') && !surface.contains(e.relatedTarget)) playCaustic(surface);
    if (!hoverTarget(e.target)) return;
    hoverStates.forEach((_, group) => { if (!group.contains(e.target)) hideHoverPreview(group); });
    showHoverPreview(e.target);
}, { passive: true });
document.addEventListener('focusin', e => {
    hoverStates.forEach((_, group) => { if (!group.contains(e.target)) hideHoverPreview(group); });
    showHoverPreview(e.target);
});
document.addEventListener('focusout', e => {
    const previous = hoverTarget(e.target);
    if (previous && !previous.group.contains(e.relatedTarget)) hideHoverPreview(previous.group);
});
document.addEventListener('pointermove', e => {
    if (document.hidden || !finePointer.matches || reducedMotion.matches || !materialEnabled()) return;
    const surface = e.target.closest('.liquid-surface,.liquid-rimmed');
    if (!surface || surface.closest('[inert]')) { resetPointer(); return; }
    if (pointerSurface !== surface) { resetPointer(); pointerSurface = surface; surface.classList.add('liquid-pointer'); }
    pointerSample = { x: e.clientX, y: e.clientY, surface };
    if (pointerFrame) return;
    pointerFrame = requestAnimationFrame(() => {
        pointerFrame = 0;
        const { x, y, surface: el } = pointerSample;
        if (!el.isConnected || el !== pointerSurface) return;
        const rect = el.getBoundingClientRect();
        el.style.setProperty('--pointer-x', `${x - rect.left}px`);
        el.style.setProperty('--pointer-y', `${y - rect.top}px`);
        const angle=Math.atan2(y-rect.top-rect.height/2,x-rect.left-rect.width/2)*180/Math.PI+90;
        el.style.setProperty('--rim-angle',`${angle}deg`);

    });
}, { passive: true });
document.addEventListener('pointerout', e => {
    if (!e.relatedTarget || !pointerSurface?.contains(e.relatedTarget)) resetPointer();
    hoverStates.forEach((_, group) => { if (!group.contains(e.relatedTarget)) hideHoverPreview(group); });
}, { passive: true });
let resizeFrame = 0;
function refreshIndicators() {
    if (resizeFrame) return;
    resizeFrame = requestAnimationFrame(() => {
        resizeFrame = 0;
        clearHoverPreviews();
        placeIndicator(document.querySelector('.nav-scroll'), document.querySelector('.nav-scroll .nav-item.active'), 'glass-nav-indicator');
        placeIndicator(document.querySelector('.tabs'), document.querySelector('.tabs [aria-selected=true]'), 'glass-tab-indicator');
    });
}
window.addEventListener('resize', refreshIndicators, { passive: true });
window.addEventListener('blur', stopMotion);
document.addEventListener('visibilitychange', () => { if (document.hidden) stopMotion(); });
document.addEventListener('change', e => {
    const card = e.target.closest('.check-card,.switch');
    if (card) animate(card, [{ transform: 'scale(.975)' }, { transform: 'scale(1)' }], { duration: 300, easing: spring }, 'select');
});
document.addEventListener('toggle', e => {
    if (!e.target.matches?.('.select[open]')) return;
    const menu = e.target.querySelector('.select-options');
    if (!menu) return;
    const trigger = e.target.querySelector('summary')?.getBoundingClientRect();
    const rect = menu.getBoundingClientRect();
    const x = trigger ? Math.max(0, Math.min(rect.width, trigger.left + trigger.width / 2 - rect.left)) : rect.width / 2;
    menu.style.transformOrigin = `${x}px ${trigger && trigger.top > rect.bottom ? '100%' : '0%'}`;
    animate(menu, [
        { opacity: 0, transform: 'translateY(-7px) scale(.96,.93)' },
        { opacity: 1, transform: 'translateY(1px) scale(1.006,1.012)', offset: .78 },
        { opacity: 1, transform: 'translateY(0) scale(1)' },
    ], { duration: 300, easing: ease }, 'popover');
}, true);
reducedMotion.addEventListener('change', stopMotion);
reducedTransparency.addEventListener('change', stopMotion);
increasedContrast.addEventListener('change', stopMotion);
forcedColors.addEventListener('change', stopMotion);
new MutationObserver(() => { if (!materialEnabled()) stopMotion(); }).observe(document.documentElement, { attributes: true, attributeFilter: ['data-transparency','data-contrast'] });
