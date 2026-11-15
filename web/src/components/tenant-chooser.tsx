"use client";

import Link from "next/link";
import { useState } from "react";
import { ArrowRight, Building2, LogOut, Plus, RefreshCw } from "lucide-react";
import { AuthShell } from "@/components/auth-shell";
import { useUI } from "@/components/providers";
import { Empty, ErrorBox, Field, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { Account, Tenant } from "@/lib/types";

export function TenantChooser() {
  const { t } = useUI();
  const account = useRemote<Account>("/auth/me", 60000);
  const tenants = useRemote<Tenant[]>(account.data ? "/tenants" : null, 30000);
  const [creating, setCreating] = useState(false);
  const [key] = useState(() => crypto.randomUUID());
  return (
    <AuthShell
      title={t("tenants.choose")}
      description={t("tenants.chooseDetail")}
      notice={t("tenants.isolation")}
    >
      <ErrorBox message={account.error || tenants.error} />
      {(account.error || tenants.error) && (
        <Button
          variant="secondary"
          onClick={() =>
            void (account.error ? account.reload() : tenants.reload())
          }
        >
          <RefreshCw />
          {t("common.retry")}
        </Button>
      )}
      {!account.data || !tenants.data ? (
        !account.error && !tenants.error && <Loading />
      ) : (
        <>
          <p className="mb-5 text-sm text-muted-foreground">
            {account.data.username}
          </p>
          <div className="grid gap-3">
            {tenants.data.map((tenant) => (
              <Link
                key={tenant.id}
                href={`/tenants/${tenant.slug}`}
                className="flex min-w-0 items-center gap-4 rounded-xl border border-border bg-background/70 p-4 transition-colors hover:border-primary focus-visible:outline-2 focus-visible:outline-primary"
              >
                <Building2 className="shrink-0 text-primary" />
                <span className="min-w-0 flex-1">
                  <span className="block break-words font-medium">
                    {tenant.name}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {t(`tenants.${tenant.role}`)}
                  </span>
                </span>
                <ArrowRight className="shrink-0" size={18} />
              </Link>
            ))}
          </div>
          {!tenants.data.length && (
            <Empty
              title={t("tenants.welcome")}
              detail={t("tenants.createDetail")}
            />
          )}
          {creating || !tenants.data.length ? (
            <div className="mt-6 border-t border-border pt-5">
              <SubmitForm
                label={t("tenants.create")}
                onSubmit={async (form) => {
                  const result = await mutation<{ id: string; slug: string }>("/tenants", {
                    name: form.get("name"),
                    idempotency_key: key,
                  });
                  window.location.assign(`/tenants/${result.slug}`);
                }}
              >
                <Field label={t("tenants.name")}>
                  <Input
                    name="name"
                    required
                    maxLength={100}
                    autoComplete="organization"
                  />
                </Field>
              </SubmitForm>
            </div>
          ) : (
            <Button
              className="mt-5 w-full"
              variant="secondary"
              onClick={() => setCreating(true)}
            >
              <Plus />
              {t("tenants.create")}
            </Button>
          )}
          <Button
            variant="ghost"
            className="mt-4 w-full"
            onClick={() =>
              void mutation("/auth/logout", {}).then(() =>
                window.location.assign("/login"),
              )
            }
          >
            <LogOut />
            {t("navigation.logout")}
          </Button>
        </>
      )}
    </AuthShell>
  );
}

interface Preview {
  tenant_id: string;
  tenant_name: string;
  role: "admin" | "member";
  expires_at: string;
  available: boolean;
}
export function InvitationJoin({ token }: { token: string }) {
  const { t, date } = useUI();
  const invitation = useRemote<Preview>(
    `/invitations/${encodeURIComponent(token)}`,
    30000,
  );
  const account = useRemote<Account>("/auth/me", 60000);
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  return (
    <AuthShell
      title={t("tenants.join")}
      description={t("tenants.joinDetail")}
      notice={t("tenants.isolation")}
    >
      <ErrorBox message={invitation.error || error} />
      {!invitation.data ? (
        <>
          <Loading />
          <Button variant="secondary" onClick={() => void invitation.reload()}>
            {t("common.retry")}
          </Button>
        </>
      ) : (
        <>
          <div className="mb-6 rounded-xl border border-border p-5">
            <Building2 className="mb-3 text-primary" />
            <p className="break-words text-xl">{invitation.data.tenant_name}</p>
            <p className="mt-2 text-sm text-muted-foreground">
              {t(`tenants.${invitation.data.role}`)} ·{" "}
              {date(invitation.data.expires_at)}
            </p>
          </div>
          {!invitation.data.available ? (
            <p role="status" className="notice">
              {t("tenants.inviteUnavailable")}
            </p>
          ) : !account.data ? (
            <div className="grid gap-3">
              <Button asChild>
                <Link href={`/login?invite=${encodeURIComponent(token)}`}>
                  {t("login.submit")}
                </Link>
              </Button>
              <Button variant="secondary" asChild>
                <Link href={`/register?invite=${encodeURIComponent(token)}`}>
                  {t("register.createAccount")}
                </Link>
              </Button>
            </div>
          ) : (
            <Button
              className="w-full"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError(undefined);
                try {
                  const result = await mutation<{ tenant_id: string; tenant_slug: string }>(
                    `/invitations/${encodeURIComponent(token)}/accept`,
                    {},
                  );
                  window.location.assign(`/tenants/${result.tenant_slug}`);
                } catch (error) {
                  setError(asError(error));
                  setBusy(false);
                }
              }}
            >
              {busy ? t("common.processing") : t("tenants.confirmJoin")}
              <ArrowRight />
            </Button>
          )}
        </>
      )}
    </AuthShell>
  );
}
