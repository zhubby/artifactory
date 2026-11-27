/**
 * [INPUT]: 依赖账户通知修订、共享图标与显示名。
 * [OUTPUT]: 导出文档、通知、账户工具组和账户级页面外壳。
 * [POS]: 大厅与账户工具自带Regular壳；工作空间继承整层顶栏。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, avatar, esc } from './ui.js';
import { notificationItems } from './entry-notifications.js';

export function renderHeaderTools(state, { account = true, mobileAccount = false } = {}) {
    const name = state.demoAccount?.display_name || '林知远';
    return `<div class="topbar-utilities entry-header-tools ${mobileAccount ? '' : 'glass'}" role="group" aria-label="文档、通知与账户"><a href="../README.md" target="_blank" class="icon-button" aria-label="打开文档">${icon('book', 18)}</a><button type="button" class="icon-button" data-action="notifications" aria-label="打开通知中心">${icon('bell', 19)}${notificationItems(state).some(item => item.unread) ? '<span class="unread-dot"></span>' : ''}</button>${account ? `<button type="button" class="icon-button entry-header-account ${mobileAccount ? 'entry-mobile-account' : ''}" data-action="account" aria-label="打开${esc(name)}的个人账户">${avatar(name.slice(0, 1), 'purple')}</button>` : ''}</div>`;
}

export function renderAccountShell(content, state) {
    return `<div class="entry-account-page"><header class="entry-account-header"><a class="brand" href="#tenants" data-route="tenants"><img src="assets/brand.svg" alt="">Artifactory <sup>PRISM</sup></a>${renderHeaderTools(state)}</header><main id="main" class="main entry-account-main" tabindex="-1">${content}<footer class="demo-footer"><span class="demo-chip">DESIGN PROTOTYPE</span><span>账户收件箱 · 跨工作空间</span><a href="#tenants" data-route="tenants">返回工作空间大厅</a></footer></main></div>`;
}
