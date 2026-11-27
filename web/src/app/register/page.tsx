/**
 * [INPUT]: 依赖注册 API、作用域导航、Prism 认证框架和密码显隐控件。
 * [OUTPUT]: 提供 Register 注册页面。
 * [POS]: 校验注册输入；错误桥跨配置与表单分支保持挂载。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Link } from "@/lib/tenant";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { ArrowRight, RefreshCw, UserRound } from "lucide-react";
import { AuthShell } from "@/components/auth-shell";
import { ErrorBox, Loading } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PasswordInput } from "@/components/ui/password-input";
import { mutation, useRemote } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type { AuthConfig } from "@/lib/types";

export default function Register() {
  const { t: translate } = useUI();
  const config = useRemote<AuthConfig>("/auth/config", 60000);
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState({ username: "", password: "", confirmPassword: "" });
  const submitting = useRef(false);
  const [invite, setInvite] = useState<string | null>(null);
  useEffect(() => { setInvite(new URLSearchParams(window.location.search).get("invite")); }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (submitting.current) return;
    setError(undefined);
    const values = new FormData(event.currentTarget);
    const username = String(values.get("username") || "").trim();
    const password = String(values.get("password") || "");
    const encoder = new TextEncoder();
    if (!username || encoder.encode(username).length > 100) {
      setError(new LocalizedError("register.invalidUsername"));
      return;
    }
    const passwordBytes = encoder.encode(password).length;
    if (passwordBytes < 12 || passwordBytes > 256) {
      setError(new LocalizedError("register.invalidPassword"));
      return;
    }
    if (password !== values.get("confirmPassword")) {
      setError(new LocalizedError("register.passwordMismatch"));
      return;
    }
    if (config.error || !config.data?.registration_enabled) {
      setError(new LocalizedError("register.forbidden"));
      return;
    }
    submitting.current = true;
    setBusy(true);
    try {
      await mutation("/auth/register", { username, password });
      const invite = new URLSearchParams(window.location.search).get("invite");
      notify.success(translate("common.operationSucceeded"), { persist: true });
      window.location.assign(invite ? `/join/${encodeURIComponent(invite)}` : "/tenants");
    } catch (error) {
      setError(asError(error, "register.failed"));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  }

  return (
    <AuthShell
      variant="register"
      title={translate("register.title")}
      description={translate("register.description")}
      notice={translate("register.notice")}
    >
      <ErrorBox message={config.error || (config.data?.registration_enabled ? error : undefined)} />
      {config.error ? (
        <>
          <Button variant="secondary" className="w-full" onClick={() => void config.reload()}>
            <RefreshCw />
            {translate("register.retryConfig")}
          </Button>
        </>
      ) : !config.data ? (
        <Loading variant="form" />
      ) : !config.data.registration_enabled ? (
        <p role="status" className="text-sm leading-6 text-muted-foreground">
          {translate("register.disabled")}
        </p>
      ) : (
        <>
          <form onSubmit={submit} className="form-stack prism-auth-form" aria-busy={busy}>
            <div className="prism-auth-field">
              <label htmlFor="register-username">{translate("login.username")}</label>
              <div className="prism-auth-input-wrap">
                <UserRound size={18} aria-hidden="true" />
                <Input
                  id="register-username"
                  name="username"
                  value={draft.username}
                  onChange={(event) => setDraft({ ...draft, username: event.target.value })}
                  autoComplete="username"
                  required
                  autoFocus
                  maxLength={100}
                  disabled={busy}
                  aria-describedby="register-username-hint"
                  placeholder={translate("login.usernamePlaceholder")}
                />
              </div>
              <p id="register-username-hint" className="prism-auth-field-hint">{translate("register.usernameHint")}</p>
            </div>
            <div className="prism-auth-field">
              <label htmlFor="register-password">{translate("login.password")}</label>
              <PasswordInput
                id="register-password"
                name="password"
                value={draft.password}
                onChange={(event) => setDraft({ ...draft, password: event.target.value })}
                autoComplete="new-password"
                required
                maxLength={256}
                disabled={busy}
                aria-describedby="register-password-hint"
                placeholder={translate("register.passwordPlaceholder")}
              />
              <p id="register-password-hint" className="prism-auth-field-hint">{translate("register.passwordHint")}</p>
            </div>
            <div className="prism-auth-field">
              <label htmlFor="register-confirm-password">{translate("register.confirmPassword")}</label>
              <PasswordInput
                id="register-confirm-password"
                name="confirmPassword"
                value={draft.confirmPassword}
                onChange={(event) => setDraft({ ...draft, confirmPassword: event.target.value })}
                autoComplete="new-password"
                required
                maxLength={256}
                disabled={busy}
                placeholder={translate("register.confirmPlaceholder")}
              />
            </div>
            <Button className="prism-auth-submit mt-2 w-full" disabled={busy}>
              <span>{busy ? translate("register.submitting") : translate("register.submit")}</span>
              <ArrowRight aria-hidden="true" />
            </Button>
          </form>
        </>
      )}
      <p className="prism-auth-switch mt-5 text-center text-sm">
        <Link className="text-primary underline underline-offset-4" href={invite ? `/login?invite=${encodeURIComponent(invite)}` : "/login"}>
          {translate("register.backToLogin")}
        </Link>
      </p>
    </AuthShell>
  );
}
