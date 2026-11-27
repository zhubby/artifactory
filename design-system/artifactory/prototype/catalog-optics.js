/**
 * [INPUT]: 接收展厅容器与生命周期信号，依赖指针和动画偏好。
 * [OUTPUT]: 导出 mountCatalogOptics，标注功能材质并挂载可中断交互。
 * [POS]: 组件库局部光学增强；不向产品页面注册效果。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
const solid = matchMedia('(prefers-reduced-transparency: reduce), (prefers-contrast: more)');
const groups = '.cn-segment,.cn-split,.cd-tabs,.cd-segmented,.cd-pagination,.cd-toc';
const menus = '.cn-menu,.cf-menu-panel,.cd-popover';
export function mountCatalogOptics(root, signal) {
    const active = new Map();
    let frame = 0, pointer;
    const isSolid = () => solid.matches || document.documentElement.dataset.transparency === 'reduce' || document.documentElement.dataset.contrast === 'more';
    const mark = (selector, kind) => root.querySelectorAll(selector).forEach(el => el.dataset.clGlass = kind);
    mark(groups, 'group'); mark(`${menus},.cn-combo-list`, 'menu');
    mark('.cn-fab-stage>.btn,.cn-popover>summary,.cf-menu>summary,.cf-floating-notify', 'regular');
    mark('[data-group=actions] .cn-row>.cn-button,[data-component=actions-toggle-button] .cn-button,[data-component=actions-loading-button] .cn-button', 'regular');
    mark('[data-group=actions] .cn-button.primary', 'prominent');
    root.querySelectorAll('[data-cl-glass=group] [data-cl-glass]').forEach(el => delete el.dataset.clGlass);
    const stop = () => { cancelAnimationFrame(frame);frame=0;active.forEach(a => a.cancel());active.clear();root.querySelectorAll('.cl-active').forEach(el=>el.classList.remove('cl-active')); };
    const animate = (el, frames, options) => {
        active.get(el)?.cancel();
        const a=el.animate(frames,options);active.set(el,a);
        a.finished.then(()=>{if(active.get(el)===a)active.delete(el);},()=>{});
    };
    const on = (type, fn, opts = {}) => root.addEventListener(type, fn, { ...opts, signal });
    on('pointermove', event => {
        if (event.pointerType !== 'mouse' || reduced.matches || isSolid()) return;
        const el = event.target.closest('[data-cl-glass]');if (!el || !root.contains(el)) return;
        pointer={el,x:event.clientX};
        if(frame)return;
        frame=requestAnimationFrame(()=>{frame=0;const {el,x}=pointer;if(!el.isConnected)return;const rect=el.getBoundingClientRect();el.style.setProperty('--cl-pointer-x', `${Math.round((x-rect.left)/rect.width*100)}%`);el.classList.add('cl-active');});
    });
    on('pointerout', event => { const el=event.target.closest('[data-cl-glass]');if(el&&!el.contains(event.relatedTarget)){el.classList.remove('cl-active');if(pointer?.el===el){cancelAnimationFrame(frame);frame=0;}} });
    const press = target => {
        const el = target.closest('button,summary');
        if(el?.matches('.btn,summary'))return;
        if (!el || !el.closest('[data-cl-glass]') || el.disabled || reduced.matches || isSolid() || !el.animate) return;
        animate(el,[{transform:'scale(1)'},{transform:'scale(.965)',offset:.35},{transform:'scale(1)'}],{duration:320,easing:'cubic-bezier(.2,.8,.2,1)'});
    };
    on('pointerdown',e=>press(e.target));on('keydown',e=>{if(e.key==='Enter'||e.key===' ')press(e.target);});
    on('toggle',e=>{
        if(!e.target.open){const panel=e.target.querySelector('[data-cl-glass=menu]');if(panel){active.get(panel)?.cancel();active.delete(panel);}return;}
        if(reduced.matches||isSolid())return;
        const panel=e.target.querySelector(':scope > .cn-menu,:scope > .cf-menu-panel,:scope > .cd-popover');if(!panel)return;
        animate(panel,[{opacity:0,transform:'translateY(-5px) scale(.97)'},{opacity:1,transform:'none'}],{duration:240,easing:'cubic-bezier(.2,.8,.2,1)'});
    },{capture:true});
    const preferences = new MutationObserver(stop);
    preferences.observe(document.documentElement,{attributes:true,attributeFilter:['data-transparency','data-contrast']});
    reduced.addEventListener('change',stop,{signal});solid.addEventListener('change',stop,{signal});
    document.addEventListener('visibilitychange',()=>{if(document.hidden)stop();},{signal});
    signal.addEventListener('abort',()=>{stop();preferences.disconnect();},{once:true});
}
