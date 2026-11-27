/**
 * [INPUT]: 依赖入口、账户、管理、完整窗口视图与组件目录。
 * [OUTPUT]: 导出页面分发、范围概览、真实设置结构与设计系统。
 * [POS]: 产品入口视图；认证提供主地标，业务动作由 app.js 接收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { renderCatalog } from './catalog.js';
import { renderOptics } from './optics.js';
import { projects, builds } from './data.js';
import { renderAuth } from './entry-auth.js';
import { renderNotificationPage } from './entry-notifications.js';
import { renderEmailConfirmation } from './entry-email.js';
import { renderManagement } from './entry-management.js';
import { renderDashboard } from './entry-dashboard.js';
import { renderInvitation } from './entry-invitation.js';
import { renderTenants } from './entry-tenants.js';
import { renderProjectList, renderProject } from './entry-projects.js';
const { icon, btn, link, panel, badge, avatar, metric, pageHeader, sectionTitle, table, kv, esc, field, input } = U;
const buildLink = (label, build) => {
    const id = build[0].slice(1), project = projects.find(p => p.name === build[1]);
    return `<a class="text-link" href="#build/${id}" data-route="build/${id}" data-build="${id}" data-project="${project?.id || ''}">${label}</a>`;
};
const releaseLink = (label, project) => `<a class="text-link" href="#release/${project.version}" data-route="release/${project.version}" data-release="${project.version}" data-project="${project.id}">${label}</a>`;
export const orbitArt = () => `<div class="orbital-art" aria-hidden="true"><div class="art-orbit"></div><div class="art-orbit second"></div><div class="art-platform"><img src="assets/brand.svg" alt=""></div><span class="art-node n1">${icon('apple', 23)}</span><span class="art-node n2">${icon('windows', 23)}</span><span class="art-node n3">${icon('linux', 23)}</span><span class="art-node n4">${icon('cube', 23)}</span><span class="art-caption">ONE SOURCE. EVERY PLATFORM.</span></div>`;

function materialPlayground() {
    return panel(sectionTitle('交互实验 / Liquid in motion', '<span class="small muted">指针移动 · 点击 · 键盘 Enter</span>')+`<div class="liquid-playground"><div><div class="eyebrow">MATERIAL RESPONDS TO YOU</div><h3>看见光，也感受到回应。</h3><p>导航与工具层回应指针，内容保持稳定。点击透镜感受形变与回弹；选中项平滑移动，弹层从操作位置展开。</p><div class="row wrap">${btn('触发液态回弹','material-replay','primary','spark')}${btn('切换透明度','transparency','secondary','layers')}</div><p class="lab-state" role="status">所有效果都可以被下一次操作打断。</p></div><div class="liquid-lab-stage"><span class="lab-orbit" aria-hidden="true"></span><span class="lab-orbit second" aria-hidden="true"></span><button type="button" class="liquid-lab-orb glass" data-action="material-replay" aria-label="Touch the light，触发液态玻璃回弹">${icon('spark',36)}<small>TOUCH THE LIGHT</small></button><span class="lab-caption">REFRACTION / RESPONSE / RHYTHM</span></div></div>`);
}
function applePrinciples() {
    return panel(sectionTitle('内容在前，功能浮于其上。', '<a class="text-link" href="../APPLE-LIQUID-GLASS.md" target="_blank">Apple 设计准则 '+icon('external',14)+'</a>')+`<div class="material-hierarchy"><div><span class="hierarchy-layer">01 / Standard</span><h3>安静的内容层</h3><p>表格、图表、项目与日志使用稳定底色。精确阅读，保持原形。</p></div><div><span class="hierarchy-layer">02 / Regular Glass</span><h3>轻盈的功能层</h3><p>导航与重要工具共享一层玻璃。选中项连续移动，色彩只强调下一步。</p></div><div><span class="hierarchy-layer">03 / Temporary</span><h3>有来处的临时层</h3><p>菜单贴近触发位置。长任务增加不透明度，关闭后回到原来的焦点。</p></div></div>`, 'apple-principles');
}
function design(state) {
    return `<div class="stack"><section class="spec-hero content-surface"><div><div class="eyebrow">ARTIFACTORY DESIGN SYSTEM / 03</div><h1>Prism.<br>内容在前，<br>玻璃随行。</h1><p>以 Apple Liquid Glass 为最高设计准则。120 项组件，12 个分类：导航轻盈浮起，数据安静呈现，动效连接每次操作。</p><div class="row wrap mt">${btn('浏览全部组件', '', 'primary', 'grid', 'data-route="design/components"')}${btn('切换明暗', 'theme', 'secondary', 'sun')}${btn('降低透明度', 'transparency', 'secondary', 'layers')}</div></div><div class="material-stage"><div class="material-sample glass"><div class="row between">${icon('spark', 24)}<small>PRISM / 03</small></div><div><h2>Clarity.<br>In motion.</h2><small>120 COMPONENTS / ONE LANGUAGE</small></div></div></div></section>${applePrinciples()}${materialPlayground()}${renderOptics()}${renderCatalog()}<details class="cl-foundations"><summary>材质、语义配色与字体规范 ${icon('down',18)}</summary><div class="stack">${panel(sectionTitle('01 / 材质公式') + `<div class="grid-2"><div><h3>功能浮起，内容清晰。</h3><p class="mt">Regular 玻璃用于导航、工具组和临时浮层。正文、表格和日志采用标准材质。清透透镜仅用于媒体与光学实验；下列数值是本项目的 Web 调校。</p><div class="mt">${kv('Backdrop', 'blur(10px) saturate(210%)')}${kv('Navigation radius', '30px · 内层按间距递减')}${kv('Chromatic rim', '1px · cyan / violet / apricot')}${kv('Inner highlight', '0 1px 1px rgba(255,255,255,.95)')}</div></div><pre class="code-block">backdrop-filter: blur(10px) saturate(210%);\nborder-radius: 30px;\nbox-shadow:\n  inset 0 1px 1px rgba(255,255,255,.95),\n  0 20px 52px -28px rgba(34,57,102,.30);\n\n/* 仅功能层；内容面板使用标准材质 */</pre></div>`)}${panel(sectionTitle('02 / 语义配色') + `<div class="grid-4">${[['var(--text)', '墨色 / Text', '#17243B'], ['var(--primary)', '钴蓝 / Action', '#2459E0'], ['var(--success)', '松绿 / Success', '#18734C'], ['var(--warning)', '琥珀 / Attention', '#85550D']].map(a => `<div><div class="swatch" style="background:${a[0]}"></div><div class="swatch-label">${a[1]}<code>${a[2]} · Light</code></div></div>`).join('')}</div>`)}${panel(sectionTitle('03 / 字体与层级', `<span class="small muted">Inter · 苹方 · IBM Plex Mono</span>`) + [['44 / 52 · 600', '交付，清晰可见。', '44px'], ['32 / 40 · 600', '让代码自由抵达', '32px'], ['20 / 28 · 600', '每一步都有确定的方向', '20px'], ['16 / 24 · 400', '正文保留从容的阅读节奏。', '16px'], ['14 / 22 · 500', '构建 #1048 · 六个目标，独立执行', '14px'], ['12 / 18 · 400', '辅助信息 · 更新于 14:32', '12px']].map(a => `<div class="type-sample"><span style="font-size:${a[2]};line-height:1.35">${a[1]}</span><small class="mono">${a[0]}</small></div>`).join(''))}</div></details>${panel(sectionTitle('规范与交付') + `<p>组件目录包含 120 项可浏览标本。三个分册分别记录输入、数据、反馈与业务组件的尺寸、状态和键盘规则。</p><div class="document-links mt"><a href="../APPLE-LIQUID-GLASS.md" target="_blank">Apple 最高准则 ${icon('external',14)}</a><a href="../MASTER.md" target="_blank">视觉语言 ${icon('external',14)}</a><a href="../COMPONENTS.md" target="_blank">核心模式 ${icon('external',14)}</a><a href="../COMPONENTS-INPUTS.md" target="_blank">输入与操作 ${icon('external',14)}</a><a href="../COMPONENTS-DATA.md" target="_blank">数据与导航 ${icon('external',14)}</a><a href="../COMPONENTS-FEEDBACK.md" target="_blank">反馈与业务 ${icon('external',14)}</a><a href="../tokens.json" target="_blank">设计变量 ${icon('external',14)}</a></div>`)}</div>`;
}
function systemSettings(state) { const values = state.settingsValues || {}; return '<form data-form="settings">' + pageHeader('PLATFORM / SYSTEM', '为整个团队，保持运行。', '系统管理员视图 · 平台运行配置与设备公开入口', btn('保存系统配置', 'save-settings', 'primary', 'check')) + `<div class="stack"><div class="grid-3">${metric('平台版本', '0.2.5', '全部一方组件统一版本', 'neutral')}${metric('服务健康', '正常', '控制、网关与存储可用')}${metric('活动工作空间', '2', '按授权隔离工作空间', 'neutral')}</div><div class="grid-2">${panel(sectionTitle('设备公开入口') + field('连接地址', input('deviceEndpoint', values.deviceEndpoint ?? 'devices.orbital.example:443')) + `<p class="field-help">仅系统管理员修改公开地址。工作空间注册设备时使用此入口。</p>` + kv('连接身份', 'mTLS · 主动出站') + kv('安装版本', '0.2.5 · 固定签名包'))}${panel(sectionTitle('平台策略') + kv('公开注册', badge('已开启', 'success')) + kv('默认外发通知', badge('关闭', 'neutral')) + kv('正式交付', '不可变 SemVer') + kv('原生设备执行', '仅授权的可信项目') + kv('设备终端', '默认关闭 · 显式授权'))}</div>${panel(sectionTitle('运行组件') + table(['组件', '状态', '版本'], [['Control', badge('健康', 'success'), '0.2.5'], ['Gateway', badge('健康', 'success'), '0.2.5'], ['Planning', badge('健康', 'success'), '0.2.5'], ['Image Build', badge('健康', 'success'), '0.2.5'], ['Telemetry', badge('健康', 'success'), '0.2.5']]))}<div class="notice">${icon('shield', 22)}<p>此页为平台管理员的概念视图。实际产品由独立系统权限校验，工作空间管理员不自动获得此范围。</p></div></div></form>`; }
export function renderPage(route, state) { if (route === 'notifications') return renderNotificationPage(state);
    if (['email-verify', 'verify', 'unsubscribe'].includes(route)) return renderEmailConfirmation(route, state); if (route === 'system')
    return systemSettings(state); if (route === 'join')
    return renderInvitation(state); if (route === 'access')
    return pageHeader('403 / ACCESS REQUIRED', '这里需要一份授权。', '你的账户尚未获得此项目的访问权限。') + panel(U.empty('联系项目管理员', '请求访问后，再从原链接返回。现有工作空间仍可使用。', btn('返回工作空间', '', 'primary', 'arrowLeft', 'data-route="overview"'))); if (route === 'overview')
    return pageHeader('WORKSPACE / OVERVIEW', esc(state.workspace), '按授权范围展示完整窗口。项目、版本和构建保留各自身份。') + renderDashboard(state); if (route === 'projects')
    return renderProjectList(state); if (route === 'project')
    return renderProject(state); if (route === 'project-settings')
    return renderManagement(state, (() => { const project = projects.find(project => project.id === state.projectId); return project ? { ...project, name: state.projectSettings?.[project.id]?.name || project.name } : null; })()); if (route === 'design')
    return design(state); if (['login', 'register'].includes(route))
    return renderAuth(route, state); if (route === 'tenants')
    return renderTenants(state); if (route === 'onboarding')
    return pageHeader('GET STARTED', '你的第一次交付，由此开始。', '完成四个步骤，把想法交给每一个平台。') + panel(`<div class="stack">${[['folder', '连接一个项目', '使用只读 Git 凭据，保持仓库整洁。', 'new-project', '连接项目'], ['monitor', '准备构建资源', '加入原生设备，配置容器构建池。', 'enroll-node', '加入设备'], ['layers', '创建第一个版本', '版本只定义身份，每次构建独立选择源码。', 'new-release', '创建版本'], ['play', '描述目标并构建', 'AI 生成可审阅的执行方案。', 'new-build', '发起构建']].map((x, i) => `<div class="list-row"><div class="row"><div class="step-circle">${i + 1}</div><div><h2>${x[2 - 1]}</h2><p>${x[2]}</p></div></div>${btn(x[4], x[3], 'secondary', x[0])}</div>`).join('')}</div>`); return null; }
