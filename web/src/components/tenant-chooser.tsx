/**
 * [INPUT]: 依赖账户上下文、租户 API、共享表单与全局通知。
 * [OUTPUT]: 提供租户卡片选择、创建与邀请页面。
 * [POS]: 登录后的工作区入口；首次读取使用卡片骨架，失败提供重试。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Link } from "@/lib/tenant";
import { useAccount } from "@/components/account";
import { BrandMark } from "@/components/brand-mark";
import { Modal } from "@/components/ui/dialog";
import { Status } from "@/components/common";
import { useState } from "react";
import {
  ArrowRight,
  ArrowUpRight,
  Building2,
  Plus,
  Search,
  Settings2,
} from "lucide-react";
import { AuthShell } from "@/components/auth-shell";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type { Account, Tenant } from "@/lib/types";

export function TenantChooser() {
  const { t } = useUI();
  const account = useAccount();
  const tenants = useRemote<Tenant[]>(account.data ? "/tenants" : null, 5000);
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const [key, setKey] = useState("");
  const filtered = tenants.data?.filter((tenant) =>
    `${tenant.name} ${tenant.slug}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const hallError = account.error || tenants.error;
  return (
    <div className="tenant-hall">
      <header className="hall-header">
        <Link href="/tenants" className="brand">
          <span className="brand-mark">
            <BrandMark />
          </span>
          Artifactory
        </Link>
        <a href="/docs">{t("navigation.docs")} ↗</a>
      </header>
      <main className="hall-main">
        <div className="hall-heading">
          <div>
            <p className="eyebrow">{t("workspace.tenantHall")}</p>
            <h1>
              {t("workspace.welcome", {
                name: account.data?.display_name || "",
              })}
            </h1>
            <p>{t("workspace.hallDescription")}</p>
          </div>
          <Button
            onClick={() => {
              setKey(crypto.randomUUID());
              setCreating(true);
            }}
          >
            <Plus />
            {t("tenants.create")}
          </Button>
        </div>
        <section className="hall-tenants" aria-labelledby="tenant-list-title">
          <div className="workspace-section-heading">
            <div>
              <h2 id="tenant-list-title">{t("tenants.choose")}</h2>
              <span>{tenants.data?.length ?? "—"}</span>
            </div>
            <label className="workspace-filter">
              <Search size={16} />
              <input
                aria-label={t("workspace.searchTenant")}
                placeholder={t("workspace.searchTenant")}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
          </div>
          <ErrorBox message={hallError} />
          {!filtered ? (
            hallError ? (
              <div className="hall-retry">
                <Button
                  variant="secondary"
                  onClick={() => {
                    if (account.error) void account.reload();
                    else void tenants.reload();
                  }}
                >
                  {t("common.retry")}
                </Button>
              </div>
            ) : <Loading variant="cards" />
          ) : !filtered.length ? (
            <section className="hall-empty">
              <Building2 size={35} />
              <h2>
                {t(
                  tenants.data?.length
                    ? "workspace.noResults"
                    : "tenants.welcome",
                )}
              </h2>
              <p>
                {t(
                  tenants.data?.length
                    ? "workspace.trySearch"
                    : "workspace.emptyHall",
                )}
              </p>
              {!tenants.data?.length && (
                <Button
                  onClick={() => {
                    setKey(crypto.randomUUID());
                    setCreating(true);
                  }}
                >
                  <Plus />
                  {t("tenants.create")}
                </Button>
              )}
            </section>
          ) : (
            <div className="tenant-grid">
              {filtered.map((tenant) => (
                <article key={tenant.id} className="tenant-card">
                  <div className="tenant-card-top">
                    <span className="tenant-glyph" aria-hidden="true">
                      <Building2 size={22} />
                    </span>
                    <div className="tenant-card-identity">
                      <Link
                        href={`/tenants/${tenant.slug}`}
                        className="tenant-card-title"
                      >
                        {tenant.name}
                        <ArrowUpRight size={16} aria-hidden="true" />
                      </Link>
                      <p className="tenant-card-role">
                        {t(`tenants.${tenant.role}`)}
                      </p>
                    </div>
                    <Status small value={tenant.status} />
                  </div>
                  <div className="tenant-card-footer">
                    <code>{tenant.slug}</code>
                    {["owner", "admin"].includes(tenant.role) && (
                      <Link
                        href={`/tenants/${tenant.slug}/settings`}
                        aria-label={t("workspace.manageTenantNamed", {
                          name: tenant.name,
                        })}
                      >
                        <Settings2 size={14} aria-hidden="true" />
                        {t("workspace.manage")}
                      </Link>
                    )}
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
      </main>
      <Modal
        open={creating}
        onOpenChange={setCreating}
        title={t("tenants.create")}
        description={t("tenants.createDetail")}
      >
        <SubmitForm
          key={key}
          label={t("tenants.create")}
          successMessage={false}
          onSubmit={async (form, markSaved) => {
            const result = await mutation<{ id: string; slug: string }>(
              "/tenants",
              { name: form.get("name"), idempotency_key: key },
            );
            markSaved();
            notify.success(t("common.operationSucceeded"), { persist: true });
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
      </Modal>
    </div>
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
        invitation.error ? (
          <Button variant="secondary" onClick={() => void invitation.reload()}>
            {t("common.retry")}
          </Button>
        ) : <Loading variant="detail" />
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
                  const result = await mutation<{
                    tenant_id: string;
                    tenant_slug: string;
                  }>(`/invitations/${encodeURIComponent(token)}/accept`, {});
                  notify.success(t("common.operationSucceeded"), { persist: true });
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
