/**
 * [INPUT]: 依赖认证 API、邮箱返回地址校验、Prism 认证框架和密码显隐控件。
 * [OUTPUT]: 提供 Login 登录页面。
 * [POS]: 保留登录草稿；验证返回地址经白名单校验，GET 不消费令牌。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Link } from "@/lib/tenant";
import { useEffect, useState, type FormEvent } from "react";
import { ArrowRight, UserRound } from "lucide-react";
import { AuthShell } from "@/components/auth-shell";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PasswordInput } from "@/components/ui/password-input";
import { ErrorBox } from "@/components/common";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { notificationReturnTo } from "@/lib/routes";
import type { AuthConfig } from "@/lib/types";

export default function Login() {
  const { t } = useUI();
  const config = useRemote<AuthConfig>("/auth/config", 60000);
  const registrationEnabled =
    !config.error && config.data?.registration_enabled;
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const [invite, setInvite] = useState<string | null>(null);
  useEffect(() => {
    setInvite(new URLSearchParams(window.location.search).get("invite"));
  }, []);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    const values = new FormData(event.currentTarget);
    try {
      await mutation("/auth/login", Object.fromEntries(values));
      const invite = new URLSearchParams(window.location.search).get("invite");
      notify.success(t("common.operationSucceeded"), { persist: true });
      const returnTo = notificationReturnTo(
        new URLSearchParams(window.location.search).get("returnTo"),
      );
      const destination = invite
        ? `/join/${encodeURIComponent(invite)}`
        : returnTo || "/tenants";
      if (returnTo && !invite) window.location.replace(destination);
      else window.location.assign(destination);
    } catch (error) {
      setError(asError(error, "login.failed"));
    } finally {
      setBusy(false);
    }
  }
  return (
    <AuthShell
      variant="login"
      title={t("login.title")}
      description={t(
        registrationEnabled ? "register.loginDescription" : "login.description",
      )}
      notice={t(registrationEnabled ? "register.notice" : "login.notice")}
    >
      <ErrorBox message={error || config.error} />
      <form onSubmit={submit} className="form-stack prism-auth-form" aria-busy={busy}>
        <div className="prism-auth-field">
          <label htmlFor="login-username">{t("login.username")}</label>
          <div className="prism-auth-input-wrap">
            <UserRound size={18} aria-hidden="true" />
            <Input
              id="login-username"
              name="username"
              autoComplete="username"
              required
              autoFocus
              placeholder={t("login.usernamePlaceholder")}
            />
          </div>
        </div>
        <div className="prism-auth-field">
          <label htmlFor="login-password">{t("login.password")}</label>
          <PasswordInput
            id="login-password"
            name="password"
            autoComplete="current-password"
            required
            placeholder={t("login.passwordPlaceholder")}
          />
        </div>
        <Button className="prism-auth-submit mt-2 w-full" disabled={busy}>
          <span>{busy ? t("login.verifying") : t("login.submit")}</span>
          <ArrowRight aria-hidden="true" />
        </Button>
      </form>
      {registrationEnabled && (
        <p className="prism-auth-switch mt-5 text-center text-sm text-muted-foreground">
          {t("register.noAccount")}{" "}
          <Link
            className="text-primary underline underline-offset-4"
            href={
              invite
                ? `/register?invite=${encodeURIComponent(invite)}`
                : "/register"
            }
          >
            {t("register.createAccount")}<ArrowRight size={14} aria-hidden="true" />
          </Link>
        </p>
      )}
    </AuthShell>
  );
}
