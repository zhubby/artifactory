/**
 * [INPUT]: 依赖 WebhookEndpoint、WebhookDelivery 合同与当前本地范围。
 * [OUTPUT]: 导出端点和令牌提交、一次示例清理与投递重试任务。
 * [POS]: Webhook 本地交互适配；不发送请求，示例密钥不进入存储。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
const { btn, esc, kv, badge, table } = U;
let transientSecret = null, secretTitle = '示例密钥';
export function clearWebhookSecret() { transientSecret = null; }
function secretView(api, visible = false) {
    if (!transientSecret) { api.showModal('webhook-secret', '示例已清理', '<p>一次示例已经移除。历史记录不再显示完整值。</p>', btn('完成', 'close-modal', 'primary')); return; }
    api.showModal('webhook-secret', `仅此一次的${secretTitle}`, `<p>此字符串没有访问能力，不是服务端密钥。关闭或确认后从内存清理。</p><pre class="code-block mt">${visible ? esc(transientSecret) : '••••••••••••••••••••••••'}</pre><div class="row wrap">${btn(visible ? '隐藏示例' : '显示一次示例', visible ? 'webhook-hide' : 'webhook-reveal', 'secondary', 'eye')}</div>`, btn('已记录并清理', 'webhook-clear', 'primary', 'check'));
}
function openHistory(state, api, id) {
    const endpoint = state.demoWebhooks?.find(item => item.id === id); if (!endpoint) return;
    const rows = (state.webhookDeliveries || []).filter(item => item.endpoint_id === id), shown = rows.slice(0, (state.webhookPages?.[id] || 1) * 3);
    const deliveries = shown.map(item => `<section class="panel"><div class="row between">${badge(({pending:'等待投递',failed:'投递失败',delivered:'投递完成'})[item.status], item.status === 'failed' ? 'danger' : item.status === 'delivered' ? 'success' : 'warning')}<code>${esc(item.event_type)}</code></div>${kv('尝试次数', item.attempts)}${kv('下一次尝试', item.next_attempt_at || '无')}${item.last_error ? `<p class="notice warning">${esc(item.last_error)}</p>` : ''}<p class="small muted">${item.created_at} · 本地事件 ${esc(item.id)}</p><div class="row wrap mt">${item.status === 'failed' ? btn('重试相同投递', 'webhook-retry', 'secondary', 'refresh', `data-id="${item.id}" data-endpoint="${id}"`) : item.status === 'pending' ? btn('演示接收成功', 'webhook-outcome', 'secondary', 'check', `data-id="${item.id}" data-endpoint="${id}" data-value="delivered"`) + btn('演示接收失败', 'webhook-outcome', 'ghost', 'warn', `data-id="${item.id}" data-endpoint="${id}" data-value="failed"`) : ''}</div></section>`).join('');
    api.showModal('webhook-history', `${esc(endpoint.name)} · 投递记录`, `<p class="notice mb">测试只创建本地投递记录。不会请求 ${esc(endpoint.url)}，不会把点击测试视为送达。</p>${kv('端点状态', endpoint.enabled ? '已开启 · 示例' : '默认关闭')}${btn('创建一条测试投递', 'webhook-test', 'secondary', 'play', `data-id="${id}"`)}<div class="stack mt">${deliveries || U.empty('没有投递记录', '创建测试事件，再显式预览接收成功或失败。')}</div>${shown.length < rows.length ? btn('加载更多投递', 'webhook-more', 'secondary', 'down', `data-id="${id}"`) : ''}`, btn('完成', 'close-modal', 'primary'), { wide: true });
}
export function handleWebhookAction(name, el, state, api) {
    if (['webhook-reveal','webhook-hide','webhook-clear'].includes(name)) { if (name === 'webhook-clear') { clearWebhookSecret(); api.closeModal(true); } else secretView(api, name === 'webhook-reveal'); return true; }
    if (name === 'webhook-history') { openHistory(state, api, el.dataset.id); return true; }
    if (!['webhook-test','webhook-outcome','webhook-retry','webhook-more'].includes(name)) return false;
    const id = el.dataset.endpoint || el.dataset.id;
    if (name === 'webhook-test') state.webhookDeliveries = [{ id: `delivery-${crypto.randomUUID()}`, endpoint_id: id, event_type: 'webhook.test', status: 'pending', attempts: 1, next_attempt_at: null, last_error: null, created_at: new Date().toISOString() }, ...(state.webhookDeliveries || [])];
    if (name === 'webhook-more') state.webhookPages = { ...state.webhookPages, [id]: (state.webhookPages?.[id] || 1) + 1 };
    const delivery = state.webhookDeliveries?.find(item => item.id === el.dataset.id);
    if (delivery && name === 'webhook-outcome') { delivery.status = el.dataset.value; delivery.last_error = delivery.status === 'failed' ? '本地失败样例：接收端返回 HTTP 503。' : null; delivery.next_attempt_at = null; }
    if (delivery && name === 'webhook-retry' && delivery.status === 'failed') { delivery.status = 'pending'; delivery.attempts++; delivery.last_error = null; delivery.next_attempt_at = '等待本次显式结果演示'; }
    api.persist(); openHistory(state, api, id); return true;
}
export function submitWebhook(id, values, state, api, form) {
    if (id === 'manage-token') { const data = Object.fromEntries(values), selected = values.getAll('projects'), scopes = selected.length ? values.getAll('scopes') : ['read']; state.demoTokens = [...(state.demoTokens || []), { name: data.name, days: data.days, workspaceId: state.workspaceId, projects: selected, scopes }]; api.persist(); secretTitle = '示例令牌'; transientSecret = `demo_only_no_service_access_${crypto.randomUUID()}`; secretView(api); return true; }
    if (id !== 'manage-webhook') return false;
    const data = Object.fromEntries(values); let url;
    try { url = new URL(data.url); if (!['https:','http:'].includes(url.protocol) || url.username || url.password) throw Error(); }
    catch { const input = form.elements.namedItem('url'); input.setCustomValidity('请输入不含访问凭据的 HTTP 或 HTTPS 地址。'); input.reportValidity(); input.addEventListener('input',()=>input.setCustomValidity(''),{once:true}); return true; }
    const endpoint = { ...data, id: `hook-${crypto.randomUUID()}`, enabled: values.has('enabled'), generation: 1, event_types: data.event_types.split(',').map(value => value.trim()).filter(Boolean), url_display: data.url, last_status: null, created_at: new Date().toISOString() };
    state.demoWebhooks = [...(state.demoWebhooks || []), endpoint]; api.persist();
    secretTitle = '示例密钥'; transientSecret = `demo_only_no_service_access_${crypto.randomUUID()}`; secretView(api); return true;
}
