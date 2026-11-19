/**
 * [INPUT]: 依赖固定的 Release 合同与构建创建 API。
 * [OUTPUT]: 提供 Release 内的构建启动表单。
 * [POS]: 只提交本次目标和参数；版本、源码与平台由 Release 继承。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useState } from "react";
import { ErrorBox, Field } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/input";
import { mutation } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import { buildPath } from "@/lib/routes";
import { useRouter } from "@/lib/tenant";
import type { Build, Release } from "@/lib/types";

export function BuildWizard({ release }: { release: Release }) {
  const { t } = useUI();
  const router = useRouter();
  const [goal, setGoal] = useState("");
  const [parameters, setParameters] = useState("{}");
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error>();
  return <form className="space-y-5" data-dirty={dirty ? "true" : undefined} onChange={() => setDirty(true)} onSubmit={async event => {
    event.preventDefault();
    const form = event.currentTarget;
    setError(undefined); setPending(true);
    try {
      let values: unknown;
      try { values = JSON.parse(parameters); } catch { throw new LocalizedError("common.invalidJson"); }
      if (!values || typeof values !== "object" || Array.isArray(values) || Object.entries(values).some(([key,value]) => key.startsWith("ARTIFACTORY_RELEASE_") || typeof value !== "string")) throw new LocalizedError("wizard.invalidParameters");
      if (goal.length > 16000) throw new LocalizedError("wizard.invalidGoal");
      const build = await mutation<Build>(`/releases/${release.id}/builds`, {parameters:values, ...(goal.trim() ? {goal} : {})});
      form.removeAttribute("data-dirty"); setDirty(false);
      router.push(buildPath(release.project_slug, build.number, release.id));
    } catch (error) { setError(asError(error)); } finally { setPending(false); }
  }}>
    <div className="notice"><strong>{release.version}</strong><p className="mt-2 break-all text-xs">{release.commit_sha || release.requested_ref} · {t("releaseFlow.revision", {number:release.spec_revision})}</p><p className="mt-2 text-xs">{t("releaseFlow.buildInherits")}</p></div>
    <ErrorBox message={error} />
    <Field label={t("releaseFlow.buildGoal")}><Textarea name="goal" aria-label={t("releaseFlow.buildGoal")} value={goal} maxLength={16000} onChange={event => setGoal(event.target.value)} placeholder={release.goal} /></Field>
    <Field label={t("releaseFlow.parameters")}><Textarea name="parameters" aria-label={t("releaseFlow.parameters")} value={parameters} onChange={event => setParameters(event.target.value)} spellCheck={false} /><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.parametersHint")}</p></Field>
    <Button type="submit" disabled={pending || release.status !== "draft"}>{pending ? t("common.processing") : t("builds.start")}</Button>
  </form>;
}
