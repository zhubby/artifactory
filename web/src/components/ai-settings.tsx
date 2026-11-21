/**
 * [INPUT]: 依赖共享 Select、租户模型配置、统一通知和表单。
 * [OUTPUT]: 提供模型、调用预算与显式接口兼容配置。
 * [POS]: 兼容字段独立于模型名称；Worker 由 Operator 调度。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Select } from "@/components/ui/select";

import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { useUI } from "@/components/providers";
import { Empty, ErrorBox, Field, Loading, SectionTitle } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type { ModelProfile } from "@/lib/types";

export function AISettings() {
  const { t } = useUI();
  const models = useRemote<ModelProfile[]>("/model-profiles");
  const [editing, setEditing] = useState<ModelProfile | "new">();
  const [error, setError] = useState<Error>();
  async function action(path: string, body: unknown, method = "POST") {
    setError(undefined);
    try { await mutation(path, body, method); notify.success(t("common.operationSucceeded")); await models.reload(); }
    catch (error) { setError(asError(error)); }
  }
  return <div className="space-y-6">
    <ErrorBox message={error || models.error} />
    <div className="notice">{t("ai.settingsDescription")}</div>
    <section className="panel">
      <SectionTitle title={t("ai.models")}><Button size="sm" onClick={() => setEditing("new")}><Plus />{t("ai.addModel")}</Button></SectionTitle>
      {!models.data ? !models.error && <Loading variant="table" /> : models.data.length === 0 ? <Empty title={t("ai.noModels")} detail={t("ai.noModelsHint")} /> : <div className="table-scroll"><table>
        <thead><tr><th>{t("common.name")}</th><th>{t("ai.model")}</th><th>{t("ai.budget")}</th><th>{t("common.status")}</th><th /></tr></thead>
        <tbody>{models.data.map((profile) => <tr key={profile.id}>
          <td><button className="font-medium text-primary" onClick={() => setEditing(profile)}>{profile.name}</button><p className="mt-1 break-all text-xs text-muted-foreground">{profile.base_url}</p></td>
          <td><span className="font-mono text-xs">{profile.model}</span><p className="mt-1 text-xs text-muted-foreground">{profile.api}</p></td>
          <td className="text-xs">{t("ai.budgetValue", {calls: profile.max_calls, seconds: profile.timeout_seconds})}</td>
          <td><span className="tag">{profile.enabled ? t("ai.enabled") : t("ai.disabled")}</span>{profile.is_default && <span className="ml-2 tag">{t("ai.default")}</span>}</td>
          <td><Button variant="ghost" size="icon" aria-label={t("ai.deleteModel", {name: profile.name})} onClick={() => void action(`/model-profiles/${profile.id}`, {}, "DELETE")}><Trash2 /></Button></td>
        </tr>)}</tbody>
      </table></div>}
    </section>
    <Modal open={!!editing} onOpenChange={(open) => {if (!open) setEditing(undefined);}} title={editing === "new" ? t("ai.addModel") : t("ai.editModel")} description={t("ai.modelSecretHint")}>
      {editing && <ModelForm profile={editing === "new" ? undefined : editing} defaultModel={!models.data?.some((profile) => profile.is_default)} done={() => {setEditing(undefined); void models.reload();}} />}
    </Modal>
  </div>;
}

function ModelForm({profile, defaultModel, done}: {profile?: ModelProfile; defaultModel: boolean; done: () => void}) {
  const { t } = useUI();
  const [api, setApi] = useState(profile?.api || "openai-completions");
  const [endpoint, setEndpoint] = useState(profile?.base_url || "https://api.openai.com/v1");
  const [compat, setCompat] = useState<Required<NonNullable<ModelProfile["parameters"]["compat"]>>>({
    system_role: profile?.parameters.compat?.system_role ?? "system",
    thinking_format: profile?.parameters.compat?.thinking_format ?? "none",
    max_tokens_field: profile?.parameters.compat?.max_tokens_field ?? "max_tokens",
  });
  return <SubmitForm onCancel={done} onSubmit={async (form) => {
    const value = (name: string) => String(form.get(name) || "");
    await mutation(profile ? `/model-profiles/${profile.id}` : "/model-profiles", {
      name: value("name"), api, base_url: endpoint, model: value("model"), api_key: value("api_key"),
      enabled: form.has("enabled"), is_default: form.has("is_default"),
      max_calls: Number(value("max_calls")), timeout_seconds: Number(value("timeout_seconds")),
      parameters: {context_window: Number(value("context_window")), max_tokens: Number(value("max_tokens")), thinking_level: value("thinking_level"), ...(profile?.parameters.temperature === undefined ? {} : {temperature: profile.parameters.temperature}), ...(value("requests_per_minute") ? {requests_per_minute: Number(value("requests_per_minute"))} : {}), ...(api === "openai-completions" ? {compat} : {})},
    }, profile ? "PATCH" : "POST"); done();
  }}>
    <Field label={t("common.name")}><Input name="name" required maxLength={100} defaultValue={profile?.name} /></Field>
    <Field label={t("ai.api")}><Select className="input" value={api} onChange={(event) => {setApi(event.target.value); if (!profile) setEndpoint(event.target.value === "anthropic-messages" ? "https://api.anthropic.com" : "https://api.openai.com/v1");}}><option value="openai-completions">OpenAI Chat Completions</option><option value="openai-responses">OpenAI Responses</option><option value="anthropic-messages">Anthropic Messages</option></Select></Field>
    <Field label={t("ai.endpoint")}><Input value={endpoint} onChange={(event) => setEndpoint(event.target.value)} required type="url" /></Field>
    <Field label={t("ai.model")}><Input name="model" required maxLength={200} defaultValue={profile?.model} /></Field>
    <Field label="API Key" hint={t("ai.keyHint")}><Input name="api_key" type="password" autoComplete="new-password" required={!profile} /></Field>
    <div className="grid grid-cols-2 gap-3">
      <Field label={t("ai.maxCalls")}><Input name="max_calls" type="number" required min={1} max={100} defaultValue={profile?.max_calls || 20} /></Field>
      <Field label={t("ai.timeout")}><Input name="timeout_seconds" type="number" required min={30} max={3600} defaultValue={profile?.timeout_seconds || 600} /></Field>
      <Field label={t("ai.contextWindow")}><Input name="context_window" type="number" required min={1024} defaultValue={profile?.parameters.context_window || 128000} /></Field>
      <Field label={t("ai.maxTokens")}><Input name="max_tokens" type="number" required min={256} defaultValue={profile?.parameters.max_tokens || 8192} /></Field>
      <Field label={t("ai.rpm")} hint={t("ai.rpmHint")}><Input name="requests_per_minute" type="number" min={1} max={600} defaultValue={profile?.parameters.requests_per_minute} /></Field>
    </div>
    <Field label={t("ai.thinking")}><Select className="input" name="thinking_level" defaultValue={profile?.parameters.thinking_level || "off"}>{["off", "minimal", "low", "medium", "high", "xhigh", "max"].map((level) => <option key={level} value={level}>{level}</option>)}</Select></Field>
    {api === "openai-completions" && <details className="wizard-advanced">
      <summary>{t("ai.compatibility")}</summary>
      <p className="mt-3 text-xs text-muted-foreground">{t("ai.compatibilityHint")}</p>
      <div className="mt-3 grid gap-3">
        <Field label={t("ai.systemRole")}><Select name="compat_system_role" value={compat.system_role} onChange={(event) => setCompat({...compat, system_role: event.target.value as typeof compat.system_role})}><option value="system">system</option><option value="developer">developer</option></Select></Field>
        <Field label={t("ai.thinkingFormat")}><Select name="compat_thinking_format" value={compat.thinking_format} onChange={(event) => setCompat({...compat, thinking_format: event.target.value as typeof compat.thinking_format})}><option value="none">{t("ai.noThinkingParameter")}</option><option value="reasoning_effort">reasoning_effort</option><option value="enable_thinking">enable_thinking</option><option value="thinking">thinking</option></Select></Field>
        <Field label={t("ai.maxTokensField")}><Select name="compat_max_tokens_field" value={compat.max_tokens_field} onChange={(event) => setCompat({...compat, max_tokens_field: event.target.value as typeof compat.max_tokens_field})}><option value="max_tokens">max_tokens</option><option value="max_completion_tokens">max_completion_tokens</option></Select></Field>
      </div>
    </details>}
    <label className="flex gap-2 text-sm"><input type="checkbox" name="enabled" defaultChecked={profile?.enabled ?? true} />{t("ai.enabled")}</label>
    <label className="flex gap-2 text-sm"><input type="checkbox" name="is_default" defaultChecked={profile?.is_default ?? defaultModel} />{t("ai.makeDefault")}</label>
  </SubmitForm>;
}
