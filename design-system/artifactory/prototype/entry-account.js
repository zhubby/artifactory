/**
 * [INPUT]: 依赖账户字段合同、通知偏好视图与 ui.js。
 * [OUTPUT]: 导出个人资料、安全、偏好和通知的账户弹层正文。
 * [POS]: 独立账户任务；密码不进入状态或浏览器存储。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { subscriptionFields, notificationCategories, notificationCategoryIds } from './entry-notifications.js';
const { btn, field, esc, icon, select } = U;
const sections = [['profile', '个人资料', 'users'], ['security', '账户安全', 'shield'], ['preferences', '外观偏好', 'sun'], ['notifications', '通知订阅', 'bell']];
export function accountBody(state) {
    const active = state.accountSection || 'profile', account = state.demoAccount || { display_name: '林知远', username: 'linzhiyuan' };
    const nav = `<nav class="entry-settings-nav glass" aria-label="个人设置分区">${sections.map(([id, label, glyph]) => `<button type="button" class="btn ${id === active ? 'secondary' : 'ghost'}" data-action="account-section" data-value="${id}" ${id === active ? 'aria-current="page"' : ''}>${icon(glyph, 16)}${label}</button>`).join('')}</nav>`;
    let body;
    if (active === 'profile') body = `<div class="row mb">${U.avatar(account.display_name.slice(0, 1), 'purple')}<div><h3>${esc(account.display_name)}</h3><p class="small muted">@${esc(account.username)}</p></div></div><form data-form="account-profile">${field('显示名称', `<input class="input" name="display_name" value="${esc(account.display_name)}" maxlength="100" autocomplete="name" required>`)}${field('用户名', `<input class="input" value="${esc(account.username)}" readonly autocomplete="username">`, '用户名保持不变。')}<button class="btn primary" type="submit">保存资料 ${icon('check', 16)}</button></form>`;
    else if (active === 'security') body = `<form data-form="account-password"><p class="notice mb">修改密码后返回登录。请只使用虚构密码。</p>${[['current_password', '当前密码', 'current-password'], ['new_password', '新密码', 'new-password'], ['confirm_password', '确认新密码', 'new-password']].map(([name, label, autocomplete]) => field(label, `<input class="input" type="password" name="${name}" autocomplete="${autocomplete}" maxlength="256" required>`)).join('')}<button class="btn primary" type="submit">更新密码 ${icon('lock', 16)}</button></form>`;
    else if (active === 'notifications') body = subscriptionFields(state);
    else body = `<div class="stack"><p>外观与语言属于个人账户。当前原型正文以简体中文展示。</p>${field('界面主题', select('accountTheme', ({system:'跟随系统',dark:'深色',light:'浅色'})[document.documentElement.dataset.themePreference || 'system'], ['跟随系统', '浅色', '深色']))}<div class="list-row"><div><h3>降低透明度</h3><p>关闭背景滤镜与色散。</p></div><button type="button" class="toggle" role="switch" aria-checked="${document.documentElement.dataset.transparency === 'reduce'}" data-action="transparency" aria-label="降低透明度"></button></div><div class="list-row"><div><h3>增强对比度</h3><p>使用实色表面与明确边界。</p></div><button type="button" class="toggle" role="switch" aria-checked="${document.documentElement.dataset.contrast === 'more'}" data-action="contrast" aria-label="增强对比度"></button></div><div class="list-row"><div><h3>重置原型</h3><p>清除本地演示对象与草稿。</p></div>${btn('重置', 'reset-demo', 'danger')}</div></div>`;
    return `<div class="entry-account-layout">${nav}<div class="entry-account-content">${body}</div></div>`;
}
export function submitAccount(id, values, state) {
    if (!id.startsWith('account-')) return null;
    if (id === 'account-profile') {
        const display_name = String(values.get('display_name') || '').trim();
        if (!display_name) return { error: '请输入显示名称。', field: 'display_name' };
        state.demoAccount = { username: state.demoAccount?.username || 'linzhiyuan', display_name };
        return { message: '个人资料已保存到本地演示。' };
    }
    if (id === 'account-password') {
        const length = new TextEncoder().encode(String(values.get('new_password') || '')).length;
        if (length < 12 || length > 256) return { error: '新密码需要 12–256 UTF-8 字节。', field: 'new_password' };
        if (values.get('new_password') !== values.get('confirm_password')) return { error: '两次新密码不一致。', field: 'confirm_password' };
        return { message: '密码更新流程已演示，未保存密码。', login: true };
    }
    if (id === 'account-email') {
        state.accountNotifications = { ...state.accountNotifications, email_address: String(values.get('address')), email_verified: false };
        return { message: '已保存虚构邮箱，请预览明确验证流程。' };
    }
    if (id === 'account-subscriptions') {
        const preference = state.accountNotifications || {};
        state.accountNotifications = { ...preference, in_app: Object.fromEntries(notificationCategories.map((_, i) => [notificationCategoryIds[i], values.has(`in_app.${i}`)])), email: Object.fromEntries(notificationCategories.map((_, i) => [notificationCategoryIds[i], !!preference.email_verified && values.has(`email.${i}`)])), locale: values.get('locale') };
        return { message: '个人通知订阅已保存。邮件和 Webhook 分别配置。' };
    }
    return null;
}
