/**
 * [INPUT]: 依赖浏览器 Canvas、SVG 位移滤镜与动效偏好。
 * [OUTPUT]: 提供可拖动透镜、三档材质和受控边缘折射实验。
 * [POS]: 独立光学实验；参考研究见 LIQUID-GLASS-REFERENCE.md。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon } from './ui.js';
const presets = { pill: [10, 38, 58], control: [16, 62, 25], card: [26, 100, 30] };
const ns = 'http://www.w3.org/2000/svg';
let controller;

export function renderOptics() {
    return `<section class="panel content-surface optics-panel" aria-labelledby="optics-title"><div class="section-heading"><div><div class="eyebrow">OPTICAL STUDY / INSPIRED BY LIQUID-GLASS</div><h2 id="optics-title">让背景，在边缘流动。</h2></div><a class="text-link" href="https://github.com/xiaojiaenen/liquid-glass" target="_blank" rel="noreferrer">参考仓库 ${icon('external',14)}</a></div><div class="optics-layout"><div class="optics-stage"><div class="optics-scene" aria-hidden="true"><span class="optics-grid"></span><span class="optics-disc blue"></span><span class="optics-disc peach"></span><div class="optics-type">BUILD.<br>BEYOND.</div><span class="optics-caption">SOURCE → EVERY PLATFORM</span></div><button type="button" class="optics-lens glass" aria-label="Prism lens，移动玻璃透镜，使用方向键；Enter 触发回弹" aria-describedby="optics-help"><span class="optics-lens-layer" aria-hidden="true"></span><span class="optics-lens-label">${icon('spark',26)}<span>PRISM LENS</span></span></button></div><div class="optics-controls"><h3>一片玻璃，三种尺度。</h3><p id="optics-help">拖动透镜看边缘折射。使用 Tab 聚焦，方向键移动，Enter 回弹。</p><div class="optics-presets" role="group" aria-label="玻璃材质预设">${['pill','control','card'].map((name,i)=>`<button type="button" data-optics-preset="${name}" aria-pressed="${i===2}">${['胶囊','控件','大透镜'][i]}<small>${name.toUpperCase()}</small></button>`).join('')}</div><label class="optics-range">折射强度 <output data-optics-value>62%</output><input type="range" min="0" max="100" value="62" data-optics-strength></label><div class="optics-measures"><span>棱镜宽 <strong data-optics-bezel>26px</strong></span><span>虚拟厚度 <strong data-optics-thickness>100</strong></span><span>折射率 <strong>1.5</strong></span></div><button type="button" class="btn secondary" data-optics-reset>${icon('refresh',17)}重置透镜位置</button><p class="optics-status" role="status" data-optics-status>正在准备本地光学实验。</p></div></div></section>`;
}

/* --- 独立重写的圆角距离场与折射轮廓 --- */
function displacement(width, height, radius, bezel, thickness) {
    const canvas = document.createElement('canvas'); canvas.width = width; canvas.height = height;
    const context = canvas.getContext('2d');
    const pixels = context.createImageData(width, height);
    const profile = t => Math.pow(Math.max(0, 1 - Math.pow(1 - t, 4)), .25);
    const curve = Array.from({ length:128 }, (_,i) => {
        const t = (i + .5) / 128;
        const slope = (profile(Math.min(1,t+.001)) - profile(Math.max(0,t-.001))) / .002;
        const incidence = Math.atan(slope), refraction = Math.asin(Math.sin(incidence) / 1.5);
        return Math.tan(incidence-refraction) * thickness * (1-t);
    });
    const peak = Math.max(...curve, 1);
    for (let y=0; y<height; y++) for (let x=0; x<width; x++) {
        const qx = Math.abs(x-width/2)-(width/2-radius), qy = Math.abs(y-height/2)-(height/2-radius);
        const ox=Math.max(qx,0),oy=Math.max(qy,0),hyp=Math.hypot(ox,oy);
        const distance=hyp+Math.min(Math.max(qx,qy),0)-radius;
        let nx=0,ny=0;
        if(hyp>0){nx=ox/hyp*Math.sign(x-width/2);ny=oy/hyp*Math.sign(y-height/2);}
        else if(qx>qy)nx=Math.sign(x-width/2);else ny=Math.sign(y-height/2);
        const inside=-distance, index=Math.min(127,Math.max(0,Math.floor(inside/bezel*128)));
        const amount=inside>=0&&inside<bezel?curve[index]/peak:0;
        const p=(y*width+x)*4;
        pixels.data[p]=Math.round(128+nx*amount*100);pixels.data[p+1]=Math.round(128+ny*amount*100);pixels.data[p+2]=128;pixels.data[p+3]=255;
    }
    context.putImageData(pixels,0,0); return canvas.toDataURL();
}

export function mountOptics() {
    controller?.abort();
    const root=document.querySelector('.optics-panel');if(!root)return;
    controller=new AbortController();const {signal}=controller;
    const stage=root.querySelector('.optics-stage'),lens=root.querySelector('.optics-lens'),layer=root.querySelector('.optics-lens-layer');
    const motion=matchMedia('(prefers-reduced-motion: reduce)'),transparency=matchMedia('(prefers-reduced-transparency: reduce)'),forced=matchMedia('(forced-colors: active)'),contrast=matchMedia('(prefers-contrast: more)');
    const chromium=/Chrome|Chromium|Edg\//.test(navigator.userAgent);
    let mode='card',strength=.62,x=0,y=0,drag=null,frame=0,resizeTimer,animation;
    const svg=document.createElementNS(ns,'svg');svg.classList.add('optics-definitions');svg.setAttribute('aria-hidden','true');
    svg.innerHTML='<defs><filter id="prism-optics-filter" x="-30%" y="-30%" width="160%" height="160%" color-interpolation-filters="sRGB"><feImage result="field" preserveAspectRatio="none"/><feDisplacementMap in="SourceGraphic" in2="field" xChannelSelector="R" yChannelSelector="G" scale="16"/></filter></defs>';
    root.prepend(svg);
    const image=svg.querySelector('feImage'),map=svg.querySelector('feDisplacementMap');
    const opaque=()=>transparency.matches||forced.matches||contrast.matches||document.documentElement.dataset.transparency==='reduce'||document.documentElement.dataset.contrast==='more';
    const bounds=()=>({x:Math.max(0,(stage.clientWidth-lens.offsetWidth)/2-12),y:Math.max(0,(stage.clientHeight-lens.offsetHeight)/2-12)});
    function position(){const b=bounds();x=Math.max(-b.x,Math.min(b.x,x));y=Math.max(-b.y,Math.min(b.y,y));lens.style.setProperty('--lens-x',`${x}px`);lens.style.setProperty('--lens-y',`${y}px`);const rect=lens.getBoundingClientRect(),s=stage.getBoundingClientRect();layer.style.backgroundSize=`${stage.clientWidth}px ${stage.clientHeight}px`;layer.style.backgroundPosition=`${-(rect.left-s.left)}px ${-(rect.top-s.top)}px`;}
    function update(){
        const [bezel,thickness,radius]=presets[mode];lens.style.borderRadius=`${radius}px`;
        const sw=stage.clientWidth,sh=stage.clientHeight;
        const scene=`<svg xmlns="http://www.w3.org/2000/svg" width="${sw}" height="${sh}"><defs><linearGradient id="g"><stop stop-color="#ccdfff"/><stop offset="1" stop-color="#ffdfcf"/></linearGradient><pattern id="p" width="24" height="24" patternUnits="userSpaceOnUse"><path d="M24 0H0V24" fill="none" stroke="#6887b5" stroke-opacity=".32"/></pattern></defs><rect width="100%" height="100%" fill="url(#g)"/><circle cx="${sw-15}" cy="25" r="110" fill="#8aafff" fill-opacity=".65"/><circle cx="110" cy="${sh+35}" r="110" fill="#ffbca0"/><rect width="100%" height="100%" fill="url(#p)"/><g font-family="Arial,sans-serif" font-weight="700" font-size="52" letter-spacing="-2" fill="#17243b"><text x="26" y="88">BUILD.</text><text x="26" y="144">BEYOND.</text></g><text x="26" y="${sh-24}" fill="#3e577a" font-family="monospace" font-size="9" letter-spacing="2">SOURCE → EVERY PLATFORM</text></svg>`;
        const sceneUrl=`url("data:image/svg+xml,${encodeURIComponent(scene)}")`;
        root.querySelector('.optics-scene').style.backgroundImage=sceneUrl;
        layer.style.backgroundImage=sceneUrl;
        root.querySelector('[data-optics-bezel]').textContent=`${bezel}px`;root.querySelector('[data-optics-thickness]').textContent=thickness;
        const enabled=chromium&&!opaque()&&!motion.matches;
        root.dataset.optics=enabled?'refract':opaque()?'solid':'frosted';
        if(enabled){image.setAttribute('href',displacement(lens.offsetWidth,lens.offsetHeight,radius,bezel,thickness));image.setAttribute('width',lens.offsetWidth);image.setAttribute('height',lens.offsetHeight);map.setAttribute('scale',String(strength*32));layer.style.filter='url(#prism-optics-filter)';}
        else layer.style.filter='none';
        root.querySelector('[data-optics-status]').textContent=opaque()?'已降低透明度，使用实色透镜。':motion.matches?'已减少动态效果，保留静态玻璃。':chromium?'边缘位移折射已启用。拖动不会改变文字形状。':'当前浏览器使用毛玻璃与镜面高光降级。';position();
    }
    function reset(){x=0;y=0;position();}
    lens.addEventListener('pointerdown',e=>{drag={id:e.pointerId,x:e.clientX,y:e.clientY,startX:x,startY:y};lens.setPointerCapture(e.pointerId);lens.classList.add('is-dragging');},{signal});
    lens.addEventListener('pointermove',e=>{if(!drag||drag.id!==e.pointerId)return;x=drag.startX+e.clientX-drag.x;y=drag.startY+e.clientY-drag.y;if(!frame)frame=requestAnimationFrame(()=>{frame=0;position();});},{signal});
    const end=()=>{drag=null;lens.classList.remove('is-dragging');};lens.addEventListener('pointerup',end,{signal});lens.addEventListener('pointercancel',end,{signal});lens.addEventListener('lostpointercapture',end,{signal});
    lens.addEventListener('keydown',e=>{if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();const n=e.shiftKey?24:12;x+=e.key==='ArrowLeft'?-n:e.key==='ArrowRight'?n:0;y+=e.key==='ArrowUp'?-n:e.key==='ArrowDown'?n:0;position();},{signal});
    lens.addEventListener('click',()=>{if(motion.matches)return;animation?.cancel();animation=lens.querySelector('.optics-lens-label').animate([{transform:'scale(1)'},{transform:'scale(.86)',offset:.3},{transform:'scale(1)'}],{duration:420,easing:'cubic-bezier(.2,1.15,.3,1)'});},{signal});
    root.addEventListener('click',e=>{const p=e.target.closest('[data-optics-preset]');if(p){mode=p.dataset.opticsPreset;root.querySelectorAll('[data-optics-preset]').forEach(b=>b.setAttribute('aria-pressed',String(b===p)));update();}if(e.target.closest('[data-optics-reset]'))reset();},{signal});
    root.addEventListener('input',e=>{if(!e.target.matches('[data-optics-strength]'))return;strength=Number(e.target.value)/100;map.setAttribute('scale',String(strength*32));root.querySelector('[data-optics-value]').textContent=`${e.target.value}%`;},{signal});
    const resize=new ResizeObserver(()=>{clearTimeout(resizeTimer);resizeTimer=setTimeout(update,100);});resize.observe(stage);
    const observer=new MutationObserver(update);observer.observe(document.documentElement,{attributes:true,attributeFilter:['data-transparency','data-contrast']});
    for(const pref of [motion,transparency,forced,contrast])pref.addEventListener('change',()=>{animation?.cancel();end();update();},{signal});
    document.addEventListener('visibilitychange',()=>{if(document.hidden){animation?.cancel();end();}},{signal});
    signal.addEventListener('abort',()=>{resize.disconnect();observer.disconnect();clearTimeout(resizeTimer);cancelAnimationFrame(frame);animation?.cancel();},{once:true});
    update();
}
