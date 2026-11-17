/**
 * [INPUT]: 依赖认证 API、AuthShell 和共享表单。
 * [OUTPUT]: 提供 Login 登录页面。
 * [POS]: 保留登录草稿并呈现本地化请求错误。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import Link from "next/link";
import { useEffect, useState, type FormEvent } from "react";
import { ArrowRight } from "lucide-react";
import { AuthShell } from "@/components/auth-shell";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ErrorBox, Field } from "@/components/common";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { AuthConfig } from "@/lib/types";

export default function Login() {
  const { t } = useUI();
  const config = useRemote<AuthConfig>("/auth/config", 60000);
  const registrationEnabled = !config.error && config.data?.registration_enabled;
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const [invite, setInvite] = useState<string | null>(null);
  useEffect(() => { setInvite(new URLSearchParams(window.location.search).get("invite")); }, []);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    const values = new FormData(event.currentTarget);
    try {
      await mutation("/auth/login", Object.fromEntries(values));
      const invite = new URLSearchParams(window.location.search).get("invite");
      window.location.assign(invite ? `/join/${encodeURIComponent(invite)}` : "/tenants");
    } catch (error) {
      setError(asError(error, "login.failed"));
    } finally {
      setBusy(false);
    }
  }
  return (
    <AuthShell
      title={t("login.title")}
      description={t(registrationEnabled ? "register.loginDescription" : "login.description")}
      notice={t(registrationEnabled ? "register.notice" : "login.notice")}
    >
      <ErrorBox message={error} />
      <form onSubmit={submit} className="form-stack">
        <Field label={t("login.username")}>
          <Input
            name="username"
            autoComplete="username"
            required
            autoFocus
            placeholder={t("login.usernamePlaceholder")}
          />
        </Field>
        <Field label={t("login.password")}>
          <Input
            name="password"
            type="password"
            autoComplete="current-password"
            required
            placeholder={t("login.passwordPlaceholder")}
          />
        </Field>
        <Button className="mt-2 w-full" disabled={busy}>
          {busy ? t("login.verifying") : t("login.submit")}
          <ArrowRight />
        </Button>
      </form>
      {registrationEnabled && (
        <p className="mt-5 text-center text-sm text-muted-foreground">
          {t("register.noAccount")} {" "}
          <Link className="text-primary underline underline-offset-4" href={invite ? `/register?invite=${encodeURIComponent(invite)}` : "/register"}>
            {t("register.createAccount")}
          </Link>
        </p>
      )}
    </AuthShell>
  );
}
