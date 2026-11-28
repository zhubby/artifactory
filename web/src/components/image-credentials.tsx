/**
 * [INPUT]: 依赖项目权限、镜像凭据 API、Select 和统一通知。
 * [OUTPUT]: 提供凭据空态、完整引用目录与分区新建表单。
 * [POS]: 输入秘密只提交一次；列表首读显示骨架，刷新保留草稿。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useState } from "react";
import { useUI } from "@/components/providers";
import { CopyButton, Empty, ErrorBox, Field, Loading, SectionTitle } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type { ImageCredential } from "@/lib/types";
export function ImageCredentials({ projectId, editable }: { projectId: string; editable: boolean }) {
  const { t } = useUI();
  const remote = useRemote<ImageCredential[]>(`/projects/${projectId}/image-credentials`);
  const [kind, setKind] = useState("registry"); const [error, setError] = useState<Error>();
  const [formVersion, setFormVersion] = useState(0);
  return <section className="panel settings-image-credentials"><SectionTitle title={t("images.credentials")} detail={remote.data ? t("common.recordCount", { count: remote.data.length }) : undefined} live /><ErrorBox message={remote.error || error} />
    <div className="settings-image-directory">{!remote.data ? remote.error ? <div className="settings-retry"><Button variant="secondary" onClick={() => void remote.reload()}>{t("common.retry")}</Button></div> : <Loading variant="list" /> : !remote.data.length ? <Empty title={t("images.noCredentials")} detail={t("images.noCredentialsHint")} /> : <ul className="settings-image-list">{remote.data.map(credential => <li key={credential.id} className="settings-image-credential">
      <div className="settings-image-identity"><h3>{credential.name}</h3><p>{credential.kind === "registry" ? t("images.registryCredential") : "BuildKit Secret"}</p></div>
      <span className="settings-state" data-state={credential.enabled ? "valid" : "disabled"}>{t(credential.enabled ? "images.enabled" : "images.disabled")}</span>
      <div className="settings-image-reference"><span>{t("images.credentialReference")}</span><code>{credential.id}</code></div>
      <div className="settings-image-actions"><CopyButton value={credential.id} />{editable && credential.enabled && <Button size="sm" variant="secondary" onClick={async () => { setError(undefined); try { await mutation(`/projects/${projectId}/image-credentials/${credential.id}`, {}, "DELETE"); notify.success(t("common.operationSucceeded")); await remote.reload(); } catch (error) { setError(asError(error)); } }}>{t("images.disable")}</Button>}</div>
    </li>)}</ul>}</div>
    {editable && <div className="settings-image-form"><div className="settings-image-form-heading"><h3>{t("images.addCredential")}</h3><p>{t("images.credentialHint")}</p></div><SubmitForm key={formVersion} label={t("images.addCredential")} onSubmit={async form => {
      const name = String(form.get("name"));
      const material = kind === "registry" ? { kind, name, login: { endpoint: form.get("endpoint"), username: form.get("username"), password: form.get("password") } } : { kind, name, value: form.get("value") };
      await mutation(`/projects/${projectId}/image-credentials`, material); setFormVersion(value => value + 1); await remote.reload();
    }}>
      <Field label={t("images.kind")}><Select value={kind} onChange={event => setKind(event.target.value)} className="input"><option value="registry">{t("images.registryCredential")}</option><option value="secret">BuildKit Secret</option></Select></Field>
      <Field label={t("images.name")}><Input name="name" required maxLength={100} autoComplete="off" /></Field>
      {kind === "registry" ? <><Field label={t("images.endpoint")}><Input name="endpoint" type="url" required placeholder="https://registry.example.com" /></Field><Field label={t("images.username")}><Input name="username" required autoComplete="off" /></Field><Field label={t("images.password")}><Input name="password" type="password" required autoComplete="new-password" /></Field></> : <Field label={t("images.secret")}><Input name="value" type="password" required autoComplete="new-password" /></Field>}
    </SubmitForm></div>}
  </section>;
}
