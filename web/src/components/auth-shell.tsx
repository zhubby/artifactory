/**
 * [INPUT]: 依赖品牌、偏好控件、账户上下文和 Prism 认证样式。
 * [OUTPUT]: 提供登录、注册和邀请共用的 AuthShell。
 * [POS]: 以玻璃交付雕塑承载品牌；珠光阅读面承载真实认证表单。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useId, type ReactNode } from "react";
import { Apple, ArrowRight, GitBranch, LockKeyhole, PanelsTopLeft, ShieldCheck, Sparkles, Terminal, UsersRound } from "lucide-react";
import { BrandMark } from "@/components/brand-mark";
import { PreferencesControls } from "@/components/preferences-controls";
import { useUI } from "@/components/providers";
import { useAccount } from "@/components/account";

function DeliverySculpture() {
  return (
    <div className="prism-auth-sculpture" aria-hidden="true">
      <svg className="prism-auth-orbits" viewBox="0 0 560 370" fill="none">
        <ellipse cx="278" cy="185" rx="239" ry="84" transform="rotate(-24 278 185)" />
        <ellipse cx="278" cy="185" rx="210" ry="119" transform="rotate(24 278 185)" />
        <path d="M40 240C165 252 183 71 302 72C405 72 445 175 518 145" strokeDasharray="3 8" />
      </svg>
      <div className="prism-auth-stack prism-auth-stack-back" />
      <div className="prism-auth-stack prism-auth-stack-mid" />
      <div className="prism-auth-core prism-auth-glass">
        <span className="prism-auth-core-light" />
        <BrandMark />
        <span className="prism-auth-core-caption">ONE SOURCE</span>
        <span className="prism-auth-core-stroke" />
      </div>
      <div className="prism-auth-platform prism-auth-platform-mac prism-auth-glass"><Apple size={23} /></div>
      <div className="prism-auth-platform prism-auth-platform-windows prism-auth-glass"><PanelsTopLeft size={22} /></div>
      <div className="prism-auth-platform prism-auth-platform-linux prism-auth-glass"><Terminal size={22} /></div>
      <div className="prism-auth-source prism-auth-glass"><GitBranch size={15} /><span>source → possibility</span><i /></div>
      <span className="prism-auth-art-note">BUILD ONCE. REACH FURTHER.</span>
    </div>
  );
}

export function AuthShell({ title, description, notice, children, variant = "invite" }: {
  title: string; description: string; notice: string; children: ReactNode;
  variant?: "login" | "register" | "invite";
}) {
  const { t } = useUI();
  const account = useAccount();
  const headingId = useId();
  const storyId = useId();
  const HeadingIcon = variant === "register" ? Sparkles : variant === "invite" ? UsersRound : ArrowRight;

  return (
    <div className="login-grid prism-auth" data-auth-variant={variant}>
      <header className="prism-auth-header">
        <a href="/" className="brand prism-auth-brand"><span className="brand-mark"><BrandMark /></span><span>Artifactory<small>PRISM</small></span></a>
        <div className="prism-auth-header-end">
          <p>{t("prismAuth.headerNote")}</p>
          {!account.data && <div className="login-preferences prism-auth-glass"><PreferencesControls /></div>}
        </div>
      </header>
      <div className="prism-auth-layout">
        <aside className="login-story" aria-labelledby={storyId}>
          <div className="story-body">
            <div className="prism-auth-story-copy">
              <p className="prism-auth-kicker"><span />FROM SOURCE TO EVERYWHERE</p>
              <h2 id={storyId}>{t("prismAuth.headlineFirst")}<br />{t("prismAuth.headlineLead")}<span>{t("prismAuth.headlineAccent")}</span></h2>
              <p className="prism-auth-intro">{t("prismAuth.descriptionFirst")}<br />{t("prismAuth.descriptionSecond")}</p>
            </div>
            <DeliverySculpture />
          </div>
          <footer><span className="prism-auth-story-rule" /><p>{t("prismAuth.storyNote")}</p><span>SOURCE · BUILD · DELIVER</span></footer>
        </aside>
        <main className="login-form" aria-labelledby={headingId}>
          <div className="prism-auth-account">
            <div className="prism-auth-account-top"><span>01 <i>/</i> {t("prismAuth.account")}</span><span><LockKeyhole size={13} aria-hidden="true" />{t("prismAuth.privateNote")}</span></div>
            <div className="login-form-content">
              <div className="prism-auth-form-heading">
                <span className="prism-auth-form-mark"><HeadingIcon size={22} aria-hidden="true" /></span>
                <div><p>{variant === "register" ? "A NEW BEGINNING" : variant === "invite" ? "BETTER TOGETHER" : "WELCOME BACK"}</p><h1 id={headingId}>{title}</h1></div>
              </div>
              <p className="prism-auth-form-intro">{description}</p>
              {children}
            </div>
            <p className="prism-auth-account-note"><ShieldCheck size={15} aria-hidden="true" /><span>{notice}</span></p>
          </div>
        </main>
      </div>
      <footer className="prism-auth-footer"><span>© Artifactory · {t("prismAuth.productName")}</span><span>{t("prismAuth.footerNote")}</span></footer>
    </div>
  );
}
