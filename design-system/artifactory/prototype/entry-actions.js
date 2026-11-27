/**
 * [INPUT]: 依赖账户、邮件、信任、生命周期与应用弹层接口。
 * [OUTPUT]: 导出入口任务动作、表单提交和草稿关闭保护。
 * [POS]: 将账户和通知交互从应用入口隔离，保持模块小于 800 行。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
import { handleEmailAction } from './entry-email.js';
import { handleTrustAction, submitTrust } from './entry-trust.js';
import { handleLifecycleAction, submitLifecycle } from './entry-lifecycle.js';
import { handleWebhookAction, submitWebhook, clearWebhookSecret } from './entry-webhooks.js';
import { accountBody, submitAccount } from './entry-account.js';
import { notificationList, notificationDetail, notificationItems } from './entry-notifications.js';
import { managementAction, managementSubmit } from './entry-management-actions.js';
const { btn } = U;
let accountDraft = [];
function captureAccount(form) { accountDraft = [...form.elements].filter(input => input.name).map(input => ({ name: input.name, value: input.value, checked: input.checked })); }
export function handleEntryClose(modal, state, api, force = false) {
    if (modal === 'webhook-secret') clearWebhookSecret();
    if (force) return false;
    if (modal === 'account-discard') { openAccount(state, api, true); return true; }
    const current = modal === 'account' && document.querySelector('#overlay-root form[data-dirty=true]');
    if (!current) return false;
    captureAccount(current);
    api.showModal('account-discard', '保留这份未完成的输入。', '<p>当前分区有未保存输入。请返回保存，或明确放弃。</p>', btn('放弃并关闭', 'account-discard', 'danger', 'close', 'data-close="true"') + btn('继续编辑', 'account-resume', 'secondary'));
    return true;
}
function openAccount(state, api, restore = false) {
    api.showModal('account', '你的个人空间', accountBody(state), btn('退出演示', 'logout', 'ghost', 'logout'), { wide: true, eyebrow: 'ACCOUNT' });
    if (restore) for (const saved of accountDraft) { const input = document.querySelector(`#overlay-root [name="${CSS.escape(saved.name)}"]`); if (input) { input.value = saved.value; input.checked = saved.checked; input.form.dataset.dirty = 'true'; } }
    accountDraft = [];
}
export function handleEntryAction(name, el, state, api) {
    if (handleTrustAction(name, el, state, api)) return true;
    if (handleLifecycleAction(name, el, state, api) || handleWebhookAction(name, el, state, api)) return true;
    if (handleEmailAction(name, el, state, api)) return true;
    if (managementAction(name, el, state, api)) return true;
    if (name === 'invite-login' || name === 'invite-register') { state.inviteReturn = state.inviteToken || 'demo'; api.persist(); api.go(name === 'invite-login' ? 'login' : 'register'); return true; }
    if (name === 'invite-signout') { state.demoLoggedOut = true; api.render(); return true; }
    if (name === 'accept-invite') { state.workspace = 'Orbital'; state.workspaceId = 'orbital'; state.projectId = null; state.demoLoggedOut = false; api.persist(); api.go('projects'); api.notify('已演示加入工作空间，项目权限需另行授予。'); return true; }
    if (name === 'account' || name === 'notification-settings' || name === 'account-section') {
        const current = document.querySelector('#overlay-root form[data-dirty=true]');
        if (current && name === 'account-section') {
            captureAccount(current);
            api.showModal('account-discard', '保留这份未完成的输入。', '<p>当前分区有未保存输入。请返回保存，或明确放弃。</p>', btn('放弃并切换', 'account-discard', 'danger', 'close', `data-value="${el.dataset.value}"`) + btn('继续编辑', 'account-resume', 'secondary'));
            return true;
        }
        state.accountSection = name === 'notification-settings' ? 'notifications' : name === 'account-section' ? el.dataset.value : state.accountSection || 'profile';
        openAccount(state, api); return true;
    }
    if (name === 'account-resume') { openAccount(state, api, true); return true; }
    if (name === 'account-discard') { accountDraft = []; if (el.dataset.close) api.closeModal(true); else { state.accountSection = el.dataset.value; openAccount(state, api); } return true; }
    if (name === 'account-email-remove') { state.accountNotifications = { ...state.accountNotifications, email_address: null, email_verified: false, email: {} }; api.persist(); openAccount(state, api); return true; }
    if (name === 'notifications') { api.showModal('notifications', '你的通知', notificationList(state, false), btn('全部已读', 'mark-read', 'secondary', 'check') + btn('完整收件箱', '', 'primary', 'arrow', 'data-route="notifications"'), { drawer: true, eyebrow: 'INBOX' }); return true; }
    if (name === 'mark-read') { state.readNotices = Object.fromEntries(notificationItems(state).map(item => [item.id, item.revision])); api.persist(); api.render(); api.closeModal(true); api.notify('所有通知已标为已读。'); return true; }
    if (name.startsWith('notice-') && name !== 'notice-detail') { if (name === 'notice-more') state.noticePages = (state.noticePages || 1) + 1; if (name === 'notice-revision') { const item = notificationItems(state).find(item => item.id === el.dataset.id); if (item) state.noticeRevisions = { ...state.noticeRevisions, [item.id]: item.revision + 1 }; } if (name === 'notice-error') state.noticeLoadError = true; if (name === 'notice-retry') state.noticeLoadError = false; if (name === 'notice-restore') state.noticeRevoked = {}; if (name === 'notice-revoke') { state.noticeRevoked = { ...state.noticeRevoked, [el.dataset.id]: true }; api.persist(); api.render(); api.showModal('notice-detail', '通知不可用', notificationDetail(state, el.dataset.id), btn('返回收件箱', 'notifications', 'secondary')); return true; } api.persist(); api.render(); if (document.querySelector('#overlay-root .entry-inbox')) handleEntryAction('notifications', el, state, api); return true; }
    if (name === 'notice-detail') { const item = notificationItems(state).find(item => item.id === el.dataset.id); if (item) state.readNotices = { ...state.readNotices, [item.id]: item.revision }; api.persist(); api.render(); api.showModal('notice-detail', '通知详情', notificationDetail(state, el.dataset.id), btn('返回收件箱', 'notifications', 'secondary', 'arrowLeft')); return true; }
    return false;
}
export function handleEntrySubmit(id, values, state, api, form) {
    if (submitTrust(id, values, state, api, form)) return true;
    if (submitLifecycle(id, values, state, api, form) || submitWebhook(id, values, state, api, form)) return true;
    if (managementSubmit(id, values, state, api, form)) return true;
    const result = submitAccount(id, values, state);
    if (!result) return false;
    if (result.error) { const input = form.elements.namedItem(result.field); input?.setCustomValidity(result.error); input?.reportValidity(); api.notify(result.error, true); return true; }
    api.persist(); api.notify(result.message);
    if (result.login) api.go('login'); else openAccount(state, api);
    return true;
}
