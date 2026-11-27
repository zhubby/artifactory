/**
 * [INPUT]: 依赖本地邀请示例、认证状态和 ui.js。
 * [OUTPUT]: 导出邀请预览、失效与登录恢复页面。
 * [POS]: 加入工作空间的账户入口；不自动授予项目角色。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
const { btn, panel, pageHeader, kv, esc, icon } = U;
export function renderInvitation(state) {
    const token = state.inviteToken || 'demo', expired = token === 'expired', loggedOut = state.demoLoggedOut;
    return `<main id="main" class="tenant-page" tabindex="-1"><a class="brand" href="#tenants" data-route="tenants"><img src="assets/brand.svg" alt="">Artifactory <sup>PRISM</sup></a>${pageHeader('WORKSPACE / INVITATION', expired ? '这份邀请已失效。' : '下一次交付，与团队同行。', '确认工作空间与角色后，再决定是否加入。')}<div style="max-width:580px;margin:36px auto">${panel(`${kv('工作空间', 'Orbital')}${kv('角色', '成员')}${kv('邀请状态', expired ? '已过期或不可用' : '示例可用 · 有效至 2026-11-29')}${kv('邀请标识', `<span class="mono">${esc(token)}</span>`)}<p class="field-help mt">接受邀请只加入工作空间。项目和设备权限由管理员另行授予。</p><div class="row wrap mt">${expired ? '<p role="status">请联系工作空间管理员获取新的邀请链接。</p>' : loggedOut ? btn('登录后继续', 'invite-login', 'primary', 'arrow') + (state.registrationEnabled === false ? '' : btn('注册账户', 'invite-register', 'secondary', 'plus')) : btn('接受演示邀请', 'accept-invite', 'primary', 'check')}${btn('返回大厅', '', 'ghost', 'arrowLeft', 'data-route="tenants"')}</div><details class="mt"><summary>预览其他状态</summary><div class="row wrap mt"><a class="text-link" href="#join/expired" data-route="join/expired">失效邀请 ${icon('arrow', 14)}</a>${btn('未登录状态', 'invite-signout', 'ghost', 'logout')}</div></details>`)}</div></main>`;
}
