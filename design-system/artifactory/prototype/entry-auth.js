/**
 * [INPUT]: 依赖 ui.js 的图标与转义能力，读取主题与注册开关。
 * [OUTPUT]: 导出登录和注册入口 renderAuth。
 * [POS]: 账户入口的无状态视图；提交与密码显隐由 app.js 接收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, esc, select } from './ui.js';

const brand = () => `<a class="ea-brand" href="#login" data-route="login"><img src="assets/brand.svg" alt=""><span>Artifactory<small>PRISM</small></span></a>`;
const sculpture = () => `<div class="ea-sculpture" aria-hidden="true" data-entry-scene>
  <svg class="ea-orbits" viewBox="0 0 560 370" fill="none"><ellipse cx="278" cy="185" rx="239" ry="84" transform="rotate(-24 278 185)"/><ellipse cx="278" cy="185" rx="210" ry="119" transform="rotate(24 278 185)"/><path d="M40 240C165 252 183 71 302 72C405 72 445 175 518 145" stroke-dasharray="3 8"/></svg>
  <div class="ea-stack ea-stack-back"></div><div class="ea-stack ea-stack-mid"></div>
  <div class="ea-core glass"><span class="ea-core-light"></span><img src="assets/brand.svg" alt=""><span class="ea-core-caption">ONE SOURCE</span><span class="ea-core-stroke"></span></div>
  <div class="ea-platform ea-platform-mac glass">${icon('apple', 23)}</div>
  <div class="ea-platform ea-platform-windows glass">${icon('windows', 22)}</div>
  <div class="ea-platform ea-platform-linux glass">${icon('linux', 22)}</div>
  <div class="ea-source glass">${icon('branch', 15)}<span>source → possibility</span><span class="ea-source-dot"></span></div>
  <span class="ea-art-note">BUILD ONCE. REACH FURTHER.</span>
</div>`;

export function renderAuth(route, state = {}) {
  const register = route === 'register';
  const registrationEnabled = state.registrationEnabled !== false;
  const theme = ({system:'跟随系统',light:'浅色',dark:'深色'})[document.documentElement.dataset.themePreference || 'system'];
  const id = register ? 'ea-register' : 'ea-login';
  return `<main id="main" class="ea-page" tabindex="-1">
    <header class="ea-header">${brand()}<div class="ea-header-end"><span class="ea-header-note">你的下一次交付，从这里开始。</span><div class="ea-tools glass" data-entry-glass role="group" aria-label="外观与帮助">${select('authTheme',theme,['跟随系统','浅色','深色'])}<a class="icon-button" href="../README.md" target="_blank" aria-label="打开原型说明">${icon('help', 19)}</a></div></div></header>
    <div class="ea-layout">
      <section class="ea-story" aria-labelledby="ea-story-title">
        <div class="ea-story-copy"><p class="ea-kicker"><span></span>FROM SOURCE TO EVERYWHERE</p><h2 id="ea-story-title">好想法，<br>值得<span>自由抵达。</span></h2><p class="ea-intro">连接一份源码，开启无限可能。<br>让构建、签名与交付，在同一处自然发生。</p></div>
        ${sculpture()}
        <div class="ea-story-footer"><div class="ea-story-rule"></div><p>每一份交付，都有清晰来处。</p><span>SOURCE · BUILD · DELIVER</span></div>
      </section>
      <section class="ea-account" aria-labelledby="${id}-title">
        <div class="ea-account-top"><span class="ea-step">01 <i>/</i> 账户</span><span class="ea-private">${icon('lock', 13)} 你的工作，始终有序</span></div>
        <div class="ea-form-card content-surface">
          <div class="ea-form-heading"><span class="ea-form-mark">${icon(register ? 'spark' : 'arrow', 22)}</span><div><p class="ea-form-kicker">${register ? 'A NEW BEGINNING' : 'WELCOME BACK'}</p><h1 id="${id}-title">${register ? '开启你的交付旅程。' : '欢迎回来。'}</h1></div></div>
          <p class="ea-form-intro">${register ? '创建一个账户，连接团队与下一个好想法。' : '登录账户，继续创造下一个值得交付的版本。'}</p>
          ${register && !registrationEnabled ? '<p class="ea-registration-note" role="status">当前实例已关闭公开注册。请返回登录，或联系管理员创建账户。</p>' : `<form class="ea-form" data-form="${register ? 'register' : 'login'}">
            <div class="ea-field"><label for="${id}-username">用户名</label><div class="ea-input-wrap">${icon('users', 18)}<input id="${id}-username" class="input" name="username" autocomplete="username" placeholder="输入用户名" required${register ? ` maxlength="100" aria-describedby="${id}-username-help"` : ''}></div>${register ? `<p id="${id}-username-help" class="ea-field-note">使用 1–100 字节的用户名。中文占多个字节。</p>` : ''}</div>
            <div class="ea-field"><label for="${id}-password">密码</label><div class="ea-password-wrap ea-input-wrap">${icon('lock', 18)}<input id="${id}-password" class="input" name="password" type="password" autocomplete="${register ? 'new-password' : 'current-password'}" placeholder="${register ? '创建密码' : '输入密码'}" required${register ? ` maxlength="256" aria-describedby="${id}-password-help"` : ''}><button type="button" class="icon-button ea-password-toggle" data-action="entry-password" aria-label="显示密码" aria-pressed="false" aria-controls="${id}-password">${icon('eye', 18)}</button></div>${register ? `<p id="${id}-password-help" class="ea-field-note">使用 12–256 字节的密码，支持粘贴与密码管理器。</p>` : ''}</div>
            ${register ? `<div class="ea-field"><label for="${id}-confirm-password">确认密码</label><div class="ea-password-wrap ea-input-wrap">${icon('lock', 18)}<input id="${id}-confirm-password" class="input" name="confirmPassword" type="password" autocomplete="new-password" placeholder="再次输入密码" maxlength="256" required><button type="button" class="icon-button ea-password-toggle" data-action="entry-password" aria-label="显示密码" aria-pressed="false" aria-controls="${id}-confirm-password">${icon('eye', 18)}</button></div></div>` : ''}
            <p class="ea-registration-note">${register ? '账户创建后，即可选择工作空间，连接项目与团队。' : '一个账户，自由切换你有权限访问的工作空间。'}</p>
            <button class="btn primary ea-submit" type="submit"><span>${register ? '创建账户' : '登录工作空间'}</span>${icon('arrow', 18)}</button>
          </form>`}
          ${register || registrationEnabled ? `<p class="ea-switch">${register ? '已有账户？' : '第一次来到这里？'} <a class="text-link" href="#${register ? 'login' : 'register'}" data-route="${register ? 'login' : 'register'}">${register ? '登录账户' : '创建账户'}${icon('arrow', 14)}</a></p>` : ''}
        </div>
        <p class="ea-account-note">${icon('shield', 14)} 从账户到工作空间，权限边界始终清晰。</p>
      </section>
    </div>
    <footer class="ea-footer"><span>© Artifactory · ${esc('Prism 流光工作台')}</span><span>产品交互原型 · 请使用虚构账户信息</span><a href="#tenants" data-route="tenants">浏览工作空间 ${icon('arrow', 14)}</a></footer>
  </main>`;
}
