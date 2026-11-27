/**
 * [INPUT]: 依赖账户、租户 API、共享幂等键、表单与通知。
 * [OUTPUT]: 提供 Prism 工作空间目录、创建与邀请入口。
 * [POS]: 账户入口展示真实工作空间；进入空间后直接显示项目。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { NotificationBell } from "@/components/notification-center";
import { HeaderTools } from "@/components/header-tools";
import { Link } from "@/lib/tenant";
import { useAccount } from "@/components/account";
import { BrandMark } from "@/components/brand-mark";
import { Modal } from "@/components/ui/dialog";
import { Status } from "@/components/common";
import { useRef, useState } from "react";
import {
  ArrowRight,
  Box,
  Building2,
  GitBranch,
  Layers3,
  Plus,
  Search,
  Settings2,
  X,
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
import { createIdempotencyKey } from "@/lib/idempotency";
import type { Account, Tenant } from "@/lib/types";

function DeliveryScene() {
  return (
    <div className="ph-scene" aria-hidden="true">
      <span className="ph-scene-aura" />
      <span className="ph-scene-orbit ph-orbit-back" />
      <span className="ph-scene-orbit ph-orbit-front" />
      <span className="ph-scene-floor" />
      <div className="ph-scene-pane ph-pane-back ph-glass">
        <GitBranch size={25} strokeWidth={1.5} />
        <span className="ph-code-line" />
        <span className="ph-code-line" />
        <span className="ph-code-line" />
      </div>
      <div className="ph-scene-pane ph-pane-front ph-glass">
        <BrandMark />
        <span className="ph-pane-caption">ARTIFACTORY</span>
      </div>
      <span className="ph-scene-node ph-node-box ph-glass">
        <Box size={25} strokeWidth={1.5} />
      </span>
      <span className="ph-scene-node ph-node-layers ph-glass">
        <Layers3 size={25} strokeWidth={1.5} />
      </span>
      <span className="ph-scene-caption">
        SOURCE<i />BUILD<i />DELIVER
      </span>
    </div>
  );
}

export function TenantChooser() {
  const { t } = useUI();
  const account = useAccount();
  const tenants = useRemote<Tenant[]>(account.data ? "/tenants" : null, 5000);
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const [key, setKey] = useState("");
  const search = useRef<HTMLInputElement>(null);
  const filtered = tenants.data?.filter((tenant) =>
    `${tenant.name} ${tenant.slug}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  if (!account.data) {
    return (
      <div className="authenticated-spaces prism-hall">
        <main className="hall-main ph-session-loading">
          <ErrorBox message={account.error} />
          {account.error ? (
            <Button variant="secondary" onClick={() => void account.reload()}>
              {t("common.retry")}
            </Button>
          ) : <Loading variant="detail" />}
        </main>
      </div>
    );
  }

  function beginCreate() {
    setKey(createIdempotencyKey());
    setCreating(true);
  }
  function clearSearch() {
    setQuery("");
    search.current?.focus();
  }
  return (
    <div className="tenant-hall prism-hall">
      <a className="ph-skip" href="#workspace-directory">
        {t("prismSpace.skipToSpaces")}
      </a>
      <header className="hall-header">
        <Link href="/tenants" className="brand">
          <span className="brand-mark">
            <BrandMark />
          </span>
          Artifactory
        </Link>
        <HeaderTools notification={<NotificationBell />} />
      </header>
      <main className="hall-main">
        <section className="ph-hero" aria-labelledby="workspace-hall-title">
          <div className="ph-hero-copy">
            <p className="ph-eyebrow">
              <span aria-hidden="true" />
              {t("workspace.welcome", { name: account.data.display_name })}
            </p>
            <h1 id="workspace-hall-title">
              {t("prismSpace.heroLineOne")}<br />
              <span>{t("prismSpace.heroLineTwo")}</span>
            </h1>
            <p className="ph-hero-description">{t("prismSpace.description")}</p>
            <Button className="ph-create-button" onClick={beginCreate}>
              <Plus aria-hidden="true" />
              {t("tenants.create")}
            </Button>
            <p className="ph-hero-note">{t("prismSpace.hierarchy")}</p>
          </div>
          <DeliveryScene />
        </section>
        <section
          id="workspace-directory"
          className="ph-directory"
          aria-labelledby="tenant-list-title"
          tabIndex={-1}
        >
          <div className="ph-directory-heading">
            <div className="ph-directory-title">
              <h2 id="tenant-list-title">{t("tenants.choose")}</h2>
              <span className="ph-directory-count">{tenants.data?.length ?? "—"}</span>
            </div>
            <div className="ph-search ph-glass">
              <Search size={18} aria-hidden="true" />
              <input
                ref={search}
                type="search"
                aria-label={t("workspace.searchTenant")}
                placeholder={t("workspace.searchTenant")}
                autoComplete="off"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
              {query && (
                <button
                  type="button"
                  className="ph-clear"
                  onClick={clearSearch}
                  aria-label={t("prismSpace.clearSearch")}
                >
                  <X size={17} aria-hidden="true" />
                </button>
              )}
            </div>
          </div>
          {filtered && (
            <p className="sr-only" role="status" aria-atomic="true">
              {t("prismSpace.searchResults", {
                count: filtered.length,
                total: tenants.data?.length ?? 0,
              })}
            </p>
          )}
          <ErrorBox message={account.error || tenants.error} />
          {!filtered ? (
            tenants.error ? (
              <div className="ph-retry">
                <Button
                  variant="secondary"
                  onClick={() => void tenants.reload()}
                >
                  {t("common.retry")}
                </Button>
              </div>
            ) : <Loading variant="cards" />
          ) : !filtered.length ? (
            <div className="ph-empty">
              <span className="ph-empty-symbol" aria-hidden="true">
                {tenants.data?.length ? <Search size={27} /> : <Building2 size={27} />}
              </span>
              <h3>
                {t(
                  tenants.data?.length
                    ? "workspace.noResults"
                    : "tenants.welcome",
                )}
              </h3>
              <p>
                {t(
                  tenants.data?.length
                    ? "workspace.trySearch"
                    : "workspace.emptyHall",
                )}
              </p>
              {tenants.data?.length ? (
                <Button variant="secondary" onClick={clearSearch}>
                  {t("prismSpace.clearSearch")}
                </Button>
              ) : null}
            </div>
          ) : (
            <div className="ph-workspace-grid">
              {filtered.map((tenant) => (
                <article key={tenant.id} className="ph-workspace-card">
                  <div className="ph-card-top">
                    <span className="ph-space-avatar" aria-hidden="true">
                      {Array.from(tenant.name.trim())[0]?.toLocaleUpperCase() || "A"}
                    </span>
                    <span className="ph-space-role">{t(`tenants.${tenant.role}`)}</span>
                  </div>
                  <h3><Link className="ph-workspace-name" href={`/tenants/${tenant.slug}/projects`}>{tenant.name}</Link></h3>
                  <div className="ph-space-address">
                    <Building2 size={14} aria-hidden="true" />
                    <code>{tenant.slug}</code>
                  </div>
                  <div className="ph-card-footer">
                    <Status small value={tenant.status} />
                    <div className="ph-card-actions">
                      {["owner", "admin"].includes(tenant.role) && (
                        <Link
                          className="ph-space-settings"
                          href={`/tenants/${tenant.slug}/settings`}
                          aria-label={t("workspace.manageTenantNamed", { name: tenant.name })}
                          title={t("workspace.manage")}
                        >
                          <Settings2 size={17} aria-hidden="true" />
                        </Link>
                      )}
                      <Link
                        className="ph-enter-space"
                        href={`/tenants/${tenant.slug}/projects`}
                        aria-label={t("prismSpace.enterNamed", { name: tenant.name })}
                      >
                        {t("prismSpace.enter")}
                        <ArrowRight size={16} aria-hidden="true" />
                      </Link>
                    </div>
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
        <footer className="ph-footer">
          <span>{t("prismSpace.footer")}</span>
          <span>ARTIFACTORY</span>
        </footer>
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
            window.location.assign(`/tenants/${result.slug}/projects`);
          }}
        >
          <Field label={t("tenants.name")}>
            <Input
              name="name"
              required
              maxLength={100}
              autoComplete="organization"
              placeholder={t("prismSpace.namePlaceholder")}
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
                  window.location.assign(`/tenants/${result.tenant_slug}/projects`);
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
