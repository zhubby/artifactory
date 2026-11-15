"use client";

import { useUI } from "@/components/providers";

import { useEffect, useRef, useState } from "react";
import { Link, useRouter } from "@/lib/tenant";
import { KeyRound, Plus, ShieldCheck, Trash2 } from "lucide-react";
import {
  CopyButton,
  Empty,
  ErrorBox,
  Field,
  Loading,
  PageTitle,
  SectionTitle,
} from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { CredentialForm, SecretDialog, SubmitForm } from "@/components/forms";
import { api, mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { Audit, Credential, Project, Token, User } from "@/lib/types";
import { cn, short } from "@/lib/utils";

export function Settings({
  user,
  projects,
  section = "tokens",
}: {
  user: User;
  projects: Project[];
  section?: string;
}) {
  const { t, date } = useUI();
  const tab = section;
  const router = useRouter();
  const setTab = (tab: string) => router.push(`/settings/${tab}`);
  const [adding, setAdding] = useState(false);
  const [secret, setSecret] = useState<string>();
  const [error, setError] = useState<Error>();
  const [tokenProjects, setTokenProjects] = useState<string[]>([]);
  const focusTab = useRef(false);
  useEffect(() => {
    setAdding(false);
    setSecret(undefined);
    setError(undefined);
    if (focusTab.current) {
      document.getElementById(`settings-tab-${tab}`)?.focus();
      focusTab.current = false;
    }
  }, [tab]);
  const remote = useRemote<unknown[]>(
    ["users", "emissary", "ai"].includes(tab)
      ? null
      : tab === "credentials"
        ? "/credentials"
        : tab === "audit"
          ? "/audit"
          : "/tokens",
  );
  const entries = remote.data;
  async function done() {
    setAdding(false);
    await remote.reload();
  }
  const tabs = [
    { id: "tokens", label: t("security.tokens") },
    ...(user.role === "admin"
      ? [
          { id: "credentials", label: t("security.credentials") },
          { id: "audit", label: t("security.audit") },
        ]
      : []),
  ];
  return (
    <>
      <div className="management-section-heading">
        <div>
          <h2>{t("navigation.security")}</h2>
          <p>{t("security.description")}</p>
        </div>
        {tab !== "audit" && (
          <Button onClick={() => setAdding(true)}>
            <Plus />
            {t(
              tab === "tokens"
                ? "security.createToken"
                : "security.addCredential",
            )}
          </Button>
        )}
      </div>
      <div className="tabs" role="tablist" aria-label={t("security.tabs")}>
        {tabs.map((entry) => (
          <button
            key={entry.id}
            role="tab"
            id={`settings-tab-${entry.id}`}
            aria-controls="settings-panel"
            tabIndex={tab === entry.id ? 0 : -1}
            aria-selected={tab === entry.id}
            className={cn("tab", tab === entry.id && "active")}
            onClick={() => {
              setTab(entry.id);
              setAdding(false);
            }}
            onKeyDown={(event) => {
              const index = tabs.findIndex((item) => item.id === entry.id);
              const next =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? tabs.length - 1
                    : event.key === "ArrowRight"
                      ? (index + 1) % tabs.length
                      : event.key === "ArrowLeft"
                        ? (index + tabs.length - 1) % tabs.length
                        : undefined;
              if (next === undefined) return;
              event.preventDefault();
              focusTab.current = true;
              setTab(tabs[next].id);
              setAdding(false);
              document.getElementById(`settings-tab-${tabs[next].id}`)?.focus();
            }}
          >
            {entry.label}
          </button>
        ))}
      </div>
      <ErrorBox message={error || remote.error} />
      <div
        id="settings-panel"
        role="tabpanel"
        aria-labelledby={`settings-tab-${tab}`}
        tabIndex={0}
      >
        {tab === "credentials" && (
          <div className="notice mb-5 flex gap-3">
            <ShieldCheck className="mt-1 size-4 shrink-0" />
            <span>{t("security.credentialNotice")}</span>
          </div>
        )}
        {
          <section className="panel">
            <SectionTitle
              title={
                tabs.find((entry) => entry.id === tab)?.label ||
                t("security.tokens")
              }
              detail={
                entries
                  ? t("common.recordCount", { count: entries.length })
                  : undefined
              }
            />
            {!entries ? (
              <div className="px-6">
                <Loading />
              </div>
            ) : entries.length === 0 ? (
              <Empty
                title={t("security.empty")}
                detail={
                  tab === "tokens"
                    ? t("security.emptyTokens")
                    : t("security.emptyDetail")
                }
              />
            ) : (
              <div className="table-scroll">
                <table>
                  <thead>
                    <tr>
                      {(tab === "tokens"
                        ? [
                            t("common.name"),
                            t("security.scopes"),
                            t("security.expires"),
                            t("common.status"),
                            "",
                          ]
                        : tab === "credentials"
                          ? [
                              t("common.name"),
                              t("common.type"),
                              t("common.createdAt"),
                              t("security.storage"),
                            ]
                          : [
                              t("common.time"),
                              t("common.action"),
                              t("common.actor"),
                              t("common.resource"),
                            ]
                      ).map((label, index) => (
                        <th key={index}>{label}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {tab === "tokens"
                      ? (entries as Token[]).map((token) => (
                          <tr key={token.id}>
                            <td>
                              {token.name}
                              <div className="mt-1 font-mono text-[10px] text-muted-foreground">
                                {t("common.projectCount", {
                                  count: token.project_ids.length,
                                })}
                              </div>
                            </td>
                            <td>
                              <div className="flex gap-1">
                                {token.scopes.map((scope) => (
                                  <span key={scope} className="tag">
                                    {scope}
                                  </span>
                                ))}
                              </div>
                            </td>
                            <td className="font-mono text-xs text-muted-foreground">
                              {date(token.expires_at)}
                            </td>
                            <td className="text-xs text-muted-foreground">
                              {token.revoked
                                ? t("status.revoked")
                                : new Date(token.expires_at) < new Date()
                                  ? t("status.expired")
                                  : t("status.valid")}
                            </td>
                            <td>
                              {!token.revoked && (
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  aria-label={t("security.revokeToken", {
                                    name: token.name,
                                  })}
                                  onClick={() => {
                                    void api(`/tokens/${token.id}`, {
                                      method: "DELETE",
                                    })
                                      .then(() => remote.reload())
                                      .catch((error) =>
                                        setError(asError(error)),
                                      );
                                  }}
                                >
                                  <Trash2 />
                                </Button>
                              )}
                            </td>
                          </tr>
                        ))
                      : tab === "credentials"
                        ? (entries as Credential[]).map((credential) => (
                            <tr key={credential.id}>
                              <td className="font-medium">{credential.name}</td>
                              <td>
                                <span className="tag">{credential.kind}</span>
                              </td>
                              <td className="font-mono text-xs text-muted-foreground">
                                {date(credential.created_at)}
                              </td>
                              <td className="text-xs text-muted-foreground">
                                {t("security.encrypted")}
                              </td>
                            </tr>
                          ))
                        : (entries as Audit[]).map((entry) => (
                            <tr key={entry.id}>
                              <td className="font-mono text-xs text-muted-foreground">
                                {date(entry.created_at)}
                              </td>
                              <td className="font-mono text-xs">
                                {entry.action}
                              </td>
                              <td className="font-mono text-xs text-muted-foreground">
                                {short(entry.actor)}
                              </td>
                              <td className="font-mono text-xs text-muted-foreground">
                                {short(entry.resource_id)}
                              </td>
                            </tr>
                          ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        }
      </div>
      <Modal
        open={adding}
        onOpenChange={setAdding}
        title={
          tab === "tokens"
            ? t("security.createProjectToken")
            : t("security.readonlyCredential")
        }
        description={
          tab === "tokens"
            ? t("security.tokenHint")
            : t("security.credentialHint")
        }
      >
        {adding &&
          (tab === "credentials" ? (
            <CredentialForm done={() => void done()} />
          ) : (
            <SubmitForm
              label={t("security.createToken")}
              onCancel={() => setAdding(false)}
              onSubmit={async (form) => {
                const result = await mutation<{ token: string }>("/tokens", {
                  name: form.get("name"),
                  project_ids: form.getAll("projects"),
                  scopes: form.getAll("scopes"),
                  days: Number(form.get("days")),
                });
                await done();
                setSecret(result.token);
              }}
            >
              <Field label={t("security.tokenName")}>
                <Input name="name" required maxLength={100} />
              </Field>
              <fieldset>
                <legend className="mb-3 text-sm font-medium">
                  {t("security.projects")}
                </legend>
                <div className="grid max-h-40 gap-3 overflow-auto">
                  {projects.map((project) => (
                    <label key={project.id} className="flex gap-2 text-xs">
                      <input
                        type="checkbox"
                        name="projects"
                        value={project.id}
                        checked={tokenProjects.includes(project.id)}
                        onChange={(event) =>
                          setTokenProjects((previous) =>
                            event.target.checked
                              ? [...previous, project.id]
                              : previous.filter((id) => id !== project.id),
                          )
                        }
                      />
                      {project.name}
                    </label>
                  ))}
                  {!projects.length && (
                    <p className="text-xs text-muted-foreground">
                      {t("security.noProjects")}
                    </p>
                  )}
                </div>
              </fieldset>
              <fieldset>
                <legend className="mb-3 text-sm font-medium">
                  {t("security.scopes")}
                </legend>
                <div className="flex gap-5">
                  {(!tokenProjects.length ||
                  tokenProjects.some(
                    (id) =>
                      projects.find((project) => project.id === id)
                        ?.effective_role === "viewer",
                  )
                    ? ["read"]
                    : ["read", "build", "release"]
                  ).map((scope) => (
                    <label key={scope} className="flex gap-2 text-xs">
                      <input
                        type="checkbox"
                        name="scopes"
                        value={scope}
                        defaultChecked={scope === "read"}
                      />
                      {scope}
                    </label>
                  ))}
                </div>
              </fieldset>
              <Field label={t("security.days")}>
                <Input
                  name="days"
                  type="number"
                  min={1}
                  max={365}
                  defaultValue={30}
                  required
                />
              </Field>
            </SubmitForm>
          ))}
      </Modal>
      <SecretDialog
        value={secret}
        title={t("security.saveToken")}
        close={() => setSecret(undefined)}
      />
      <div className="mt-6 flex items-center gap-2 text-[11px] text-muted-foreground">
        <KeyRound size={13} />
        {t("security.notice")}
      </div>
    </>
  );
}
