/**
 * [INPUT]: 依赖凭据与签名公开元数据、当前工作空间和 ui.js。
 * [OUTPUT]: 导出虚构材料检查、元数据保存、版本停用与默认方案演示。
 * [POS]: 信任资源原型；固定无访问能力材料，不读取文件或保存秘密。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
const { btn, esc, field, select, badge, table } = U;
let draft = { kind: 'git', variant: 'https' };
const defaults = { git: ['https','ssh'], image: ['registry','secret'], signing: ['macos','windows','android'] };
function scope(state) { return `${state.workspaceId || 'orbital'}/${draft.kind === 'image' ? state.projectId || 'orbital' : 'workspace'}`; }
function renderTrust(state, api) {
    const kind = draft.kind, variant = draft.variant, label = {git:'Git 凭据',image:'镜像凭据与 Secret',signing:'签名材料与方案'}[kind], records = (state.trustSamples || []).filter(item => item.scope === scope(state) && item.kind === kind);
    let fields = field('显示名称', `<input class="input" name="name" maxlength="100" value="${esc(draft.name || '')}" placeholder="例如 本地演示材料" required>`);
    if (kind === 'git') fields += variant === 'https' ? field('用户名', '<input class="input" name="username" value="demo-user" required>') + field('访问密钥样例', '<input class="input mono" type="password" value="DEMO_ONLY_NO_ACCESS_TOKEN" readonly>', '固定无访问能力文本，不保存。') : field('SSH 私钥样例', '<textarea class="mono" readonly rows="2">DEMO_ONLY_NO_VALID_PRIVATE_KEY</textarea>') + field('known_hosts', '<textarea name="known_hosts" rows="2" required>git.example ssh-ed25519 DEMO_INVALID_PUBLIC_KEY</textarea>');
    if (kind === 'image') fields += variant === 'registry' ? field('仓库地址', '<input class="input" name="endpoint" type="url" value="https://registry.example.invalid" required>') + field('用户名', '<input class="input" name="username" value="demo-push-user" required>') + field('密码样例', '<input class="input" type="password" value="DEMO_ONLY_NO_ACCESS" readonly>') : field('BuildKit Secret 样例', '<input class="input" type="password" value="DEMO_ONLY_NO_SECRET_VALUE" readonly>');
    if (kind === 'signing') fields += field('公开签名身份', `<input class="input" name="identity" value="${variant === 'macos' ? 'Developer ID Application: Demo Team' : variant === 'windows' ? 'Demo Authenticode Publisher' : 'Demo Android Release'}" required>`) + field('材料样例', `<textarea class="mono" readonly rows="3">${variant === 'android' ? 'DEMO_ONLY_NOT_A_KEYSTORE' : 'DEMO_ONLY_NOT_A_CERTIFICATE'}\n不读取真实证书、私钥或文件。</textarea>`) + field('方案引用', '<input class="input mono" name="profile" value="signing/demo-release" required>');
    const rows = records.map(item => `<div class="list-row"><div><h3>${esc(item.name)}</h3><p>${esc(item.variant)} · 版本 ${item.version} · ${esc(item.id)}</p>${item.identity ? `<p>${esc(item.identity)} · ${esc(item.profile)}</p>` : ''}</div>${badge(item.enabled ? '本地元数据已保存' : '已停用', item.enabled ? 'success' : 'neutral')}${item.enabled ? btn('停用样例版本','trust-disable','ghost','close',`data-id="${item.id}"`) : ''}${kind === 'signing' && item.enabled ? btn('设为项目默认样例','trust-default','secondary','check',`data-id="${item.id}"`) : ''}</div>`).join('');
    api.showModal('trust-samples', label, `<p class="notice warning mb">这是一组固定虚构材料。保存仅生成公开元数据演示，不是真实导入或安全验证。</p><div class="row wrap mb">${defaults[kind].map(value => btn(value,'trust-variant',variant === value ? 'secondary' : 'ghost','',`data-value="${value}"`)).join('')}</div><form id="trust-form" data-form="trust-sample">${fields}<div class="row wrap mt">${btn('模拟材料检查失败','trust-invalid','ghost','warn')}<button class="btn primary" type="submit">保存虚构元数据</button></div><p id="trust-error" class="error-text" role="alert"></p></form><section class="mt"><h3>本地样例记录</h3>${rows || '<p class="field-help">尚无本地元数据。真实材料不会进入原型存储。</p>'}${kind === 'signing' ? `<p class="field-help">项目默认方案：${esc(state.trustDefaults?.[state.projectId || 'orbital'] || '尚未设置本地样例')}</p>` : ''}</section>`, btn('完成', 'close-modal', 'secondary'), { wide: true });
}
export function handleTrustAction(name, el, state, api) {
    const kind = ({'manage-credential':'git','new-credential':'git','manage-image-credentials':'image','manage-signing':'signing'})[name];
    if (kind) { draft = { kind, variant: defaults[kind][0] }; renderTrust(state, api); return true; }
    if (!name.startsWith('trust-')) return false;
    if (name === 'trust-variant') { draft.variant = el.dataset.value; renderTrust(state, api); }
    if (name === 'trust-invalid') { const error = document.getElementById('trust-error'); if (error) error.textContent = '本地检查失败样例：材料格式与所选平台不匹配。请切回正确类型后重新保存。'; }
    if (name === 'trust-disable') { const item = state.trustSamples?.find(item => item.id === el.dataset.id); if (item) item.enabled = false; api.persist(); renderTrust(state, api); }
    if (name === 'trust-default') { state.trustDefaults = { ...state.trustDefaults, [state.projectId || 'orbital']: el.dataset.id }; api.persist(); renderTrust(state, api); }
    return true;
}
export function submitTrust(id, values, state, api) {
    if (id !== 'trust-sample') return false;
    const data = Object.fromEntries(values), record = { id: `sample-${crypto.randomUUID()}`, scope: scope(state), kind: draft.kind, variant: draft.variant, version: 1, enabled: true, name: data.name.trim(), ...(data.identity ? { identity: data.identity, profile: data.profile } : {}), ...(data.endpoint ? { endpoint: data.endpoint } : {}) };
    state.trustSamples = [...(state.trustSamples || []), record]; api.persist(); renderTrust(state, api); api.notify('已保存虚构公开元数据，未导入真实材料。'); return true;
}
