/**
 * [INPUT]: 依赖账户邮件偏好、本地令牌场景和 ui.js。
 * [OUTPUT]: 导出验证、退订、过期、已使用和登录恢复页面。
 * [POS]: 邮件链接的显式确认任务；不自动消费令牌或发送邮件。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
const { btn, kv, esc, panel, pageHeader } = U;
export const emailRoutes = ['verify', 'email-verify', 'unsubscribe'];
export function emailTokenKey(route, token) { return `${route}/${token || 'demo'}`; }
export function renderEmailConfirmation(route, state) {
    const token = state.emailToken || 'demo', key = emailTokenKey(route, token), kind = ['expired', 'used', 'error'].includes(token) ? token : state.emailTokenScenario || 'valid', unsubscribe = route === 'unsubscribe', complete = state.emailConfirmedKey === key;
    let title = complete ? '设置已更新。' : state.demoLoggedOut ? '登录后，继续这次确认。' : kind === 'expired' ? '链接已经过期。' : kind === 'used' || state.emailUsedTokens?.[key] ? '这份链接已使用。' : kind === 'error' ? '暂时无法读取链接。' : unsubscribe ? '确认停止邮件订阅。' : '确认这个接收邮箱。';
    let body;
    if (complete) body = `<p role="status">${unsubscribe ? '本地演示的邮件订阅已全部关闭。' : '本地演示邮箱已验证。'}</p>${btn('回到账户订阅', 'notification-settings', 'primary', 'bell')}`;
    else if (state.demoLoggedOut) body = `<p>当前链接已保留。使用演示账户登录后返回此页，再明确确认。</p>${btn('登录并返回', 'email-login', 'primary', 'arrow', `data-kind="${route}"`)}${state.registrationEnabled === false ? '' : btn('创建账户后返回', 'email-register', 'secondary', 'plus', `data-kind="${route}"`)}`;
    else if (kind === 'expired') body = '<p role="alert">有效期已结束，当前链接不能继续使用。请重新申请一份链接。</p>' + btn('重新申请演示链接', 'email-renew', 'primary', 'refresh', `data-kind="${route}"`);
    else if (kind === 'used' || state.emailUsedTokens?.[key]) body = '<p role="status">此令牌已经完成一次确认。重复打开不会再次更改设置。</p>' + btn('查看账户订阅', 'notification-settings', 'secondary', 'bell');
    else if (kind === 'error') body = '<p role="alert">本地读取失败样例。设置尚未更改，当前链接仍保留。</p>' + btn('重试读取', 'email-retry', 'primary', 'refresh', `data-kind="${route}"`);
    else body = `<p>请确认当前账户与接收邮箱。打开链接本身不会验证邮箱或停止订阅。</p>${kv('当前账户', esc(state.demoAccount?.username || 'linzhiyuan'))}${kv('接收邮箱', esc(state.accountNotifications?.email_address || 'demo@example.com'))}${btn(unsubscribe ? '确认退订邮件' : '确认验证邮箱', 'email-confirm', 'primary', 'check', `data-kind="${route}"`)}`;
    const scenarios = `<details class="mt"><summary>预览链接状态</summary><div class="row wrap mt">${[['demo','有效'],['expired','已过期'],['used','已使用'],['error','读取失败']].map(([id, label]) => `<a class="text-link" href="#${route}/${id}" data-route="${route}/${id}">${label}</a>`).join('')}${btn('退出后预览', 'email-signout', 'ghost', 'logout')}</div></details>`;
    return pageHeader('ACCOUNT / EMAIL', title, '一次性链接 · 需明确确认 · 本地演示') + panel(`<div class="stack">${body}<p class="field-help">状态只在浏览器演示，不发送邮件。真实系统会复核账户、有效期和令牌使用状态。</p><a class="text-link" href="#notifications" data-route="notifications">返回通知中心</a>${scenarios}</div>`, 'entry-confirmation');
}
export function handleEmailAction(name, el, state, api) {
    if (!name.startsWith('email-')) return false;
    const route = emailRoutes.includes(el.dataset.kind) ? el.dataset.kind : location.hash.slice(1).split('/')[0], token = state.emailToken || 'demo';
    if (name === 'email-login' || name === 'email-register') { state.emailReturn = `${route}/${encodeURIComponent(token)}`; api.persist(); api.go(name === 'email-login' ? 'login' : 'register'); return true; }
    if (name === 'email-signout') { state.demoLoggedOut = true; api.persist(); api.render(); return true; }
    if (name === 'email-renew' || name === 'email-retry') { state.emailConfirmedKey = null; state.emailTokenScenario = 'valid'; api.go(`${route}/${name === 'email-renew' ? `renewed-${Date.now().toString(36)}` : `retry-${Date.now().toString(36)}`}`); api.notify(name === 'email-renew' ? '已生成新的本地演示链接，未发送邮件。' : '链接已重新读取，请明确确认。'); return true; }
    if (name !== 'email-confirm') return false;
    const key = emailTokenKey(route, token);
    if (state.demoLoggedOut || ['expired','used','error'].includes(token) || state.emailUsedTokens?.[key]) { api.notify('当前链接不能确认，请检查账户与链接状态。', true); return true; }
    state.emailUsedTokens = { ...state.emailUsedTokens, [key]: true }; state.emailConfirmedKey = key;
    state.accountNotifications = { ...state.accountNotifications, ...(route === 'unsubscribe' ? { email: {} } : { email_verified: true }) };
    api.persist(); api.render(); api.notify(route === 'unsubscribe' ? '本地演示邮件订阅已关闭。' : '本地演示邮箱已验证。'); return true;
}
