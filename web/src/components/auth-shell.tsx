/**
 * [INPUT]: 依赖品牌、偏好控件及账户上下文。
 * [OUTPUT]: 提供 AuthShell 认证页面框架。
 * [POS]: 登录与注册共享入口布局。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import type { ReactNode } from "react";
import { GitBranch, Package, ShieldCheck } from "lucide-react";
import { BrandMark } from "@/components/brand-mark";
import { PreferencesControls } from "@/components/preferences-controls";
import { useUI } from "@/components/providers";
import { useAccount } from "@/components/account";

export function AuthShell({ title, description, notice, children }: {
  title: string; description: string; notice: string; children: ReactNode;
}) {
  const { t } = useUI();
  const account = useAccount();
  return <div className="login-grid">
    {!account.data && <div className="login-preferences"><PreferencesControls /></div>}
    <aside className="login-story">
      <a href="/" className="brand"><span className="brand-mark"><BrandMark /></span>Artifactory</a>
      <div className="story-body">
        <h1>{t("login.headlineFirst")}<br />{t("login.headlineSecond")}</h1>
        <p>{t("login.descriptionFirst")} {t("login.descriptionSecond")}</p>
        <div className="mt-8 max-w-sm">
          {[{ icon: GitBranch, label: "source" }, { icon: ShieldCheck, label: "signing" }, { icon: Package, label: "release" }].map(({ icon: Icon, label }) =>
            <div className="pipeline-line" key={label}><Icon size={16} className="text-muted-foreground" aria-hidden="true" /><span className="text-xs">{t("login." + label)}</span></div>)}
        </div>
      </div>
      <footer>Artifactory · {t("login.pipeline")}</footer>
    </aside>
    <main className="login-form">
      <div className="login-form-content">
        <h2>{title}</h2>
        <p className="mt-3 mb-7 text-sm leading-6 text-muted-foreground">{description}</p>
        {children}
        <div className="mt-8 flex gap-2 border-t border-border pt-5 text-xs leading-6 text-muted-foreground">
          <ShieldCheck className="mt-1 size-4 shrink-0" aria-hidden="true" /><span>{notice}</span>
        </div>
      </div>
    </main>
  </div>;
}
