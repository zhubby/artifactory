/**
 * [INPUT]: 依赖项目引用、构建目标校验、自定义选择控件与统一通知。
 * [OUTPUT]: 提供 Release 内的构建启动表单。
 * [POS]: 每次提交独立源码、要求和目标；仅版本归属由 Release 提供。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { ErrorBox, Field } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Combobox } from "@/components/ui/combobox";
import { Input, Textarea } from "@/components/ui/input";
import { mutation } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import { useRepositoryRefs } from "@/lib/repository-refs";
import { validBuildTargets } from "@/lib/build-targets";
import { notify } from "@/lib/notify";
import { buildPath } from "@/lib/routes";
import { useRouter } from "@/lib/tenant";
import type { ApplicationPlatform, Build, BuildTarget, Project, Release } from "@/lib/types";

export function BuildWizard({ release, project }: { release: Release; project: Project }) {
  const { t } = useUI();
  const router = useRouter();
  const refs = useRepositoryRefs(project.id);
  const [source, setSource] = useState("");
  const [sourceTouched, setSourceTouched] = useState(false);
  const [targets, setTargets] = useState<BuildTarget[]>([{kind:"native", key:"linux-amd64", os:"linux", arch:"amd64", application_platform:"linux", artifact_key:"app", format_suffix:".tar.gz"}]);
  useEffect(() => { if (!sourceTouched && refs.data?.default_ref) setSource(refs.data.default_ref); }, [sourceTouched, refs.data]);
  function changeTarget(index: number, target: BuildTarget) {
    setTargets(previous => previous.map((item, position) => position === index ? target : item)); setDirty(true);
  }
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
      if (!source.trim() || source !== source.trim()) throw new LocalizedError("releaseFlow.invalidSource");
      if (!validBuildTargets(targets)) throw new LocalizedError("releaseFlow.invalidTargets");
      if (!goal.trim() || goal.length > 16000) throw new LocalizedError("wizard.invalidGoal");
      const build = await mutation<Build>(`/releases/${release.id}/builds`, {ref:source, goal, target_requirements:targets, targets:targets.map(target => target.key), parameters:values});
      notify.success(t("common.operationSucceeded"));
      form.removeAttribute("data-dirty"); setDirty(false);
      router.push(buildPath(release.project_slug, build.number, release.id));
    } catch (error) { setError(asError(error)); } finally { setPending(false); }
  }}>
    <div className="notice"><strong>{release.version}</strong><p className="mt-2 text-xs">{t("releaseFlow.buildInherits")}</p></div>
    <ErrorBox message={error} />
    <ErrorBox message={refs.error} />
      <Field label={t("releaseFlow.source")}><Combobox aria-label={t("releaseFlow.source")} name="ref" value={source} required
        onValueChange={value => {setSourceTouched(true); setSource(value); setDirty(true);}}
        options={[...(refs.data?.branches || []), ...(refs.data?.tags || [])].map(ref => ({value:ref.ref,label:ref.name}))}
        loading={refs.loading} error={!!refs.error}
        emptyLabel={t("wizard.noRefs")} loadingLabel={t("common.loading")} errorLabel={t("common.requestFailed")} placeholder="refs/heads/main" />
        <p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.sourceHint")}</p>
        {refs.error && <div className="mt-2"><Button type="button" variant="ghost" onClick={refs.reload}>{t("common.retry")}</Button></div>}
      </Field>
    <Field label={t("releaseFlow.buildGoal")}><Textarea name="goal" aria-label={t("releaseFlow.buildGoal")} value={goal} maxLength={16000} required onChange={event => setGoal(event.target.value)} placeholder={t("releaseFlow.goalPlaceholder")} /></Field>
    <div className="space-y-4">
      <div><h2 className="text-sm font-medium">{t("releaseFlow.targets")}</h2><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.targetsHint")}</p></div>
      {targets.map((target, index) => <fieldset key={index} className="space-y-4 rounded-md border border-border p-4">
        <legend className="px-1 text-xs">{t("releaseFlow.targetNumber", {number:index + 1})}</legend>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t("releaseFlow.targetKey")}><Input required maxLength={63} value={target.key} onChange={event => changeTarget(index, {...target, key:event.target.value})} /></Field>
          <Field label={t("releaseFlow.targetKind")}><Select className="input" value={target.kind} onChange={event => changeTarget(index, event.target.value === "image" ? {kind:"image",key:target.key,platforms:["linux/amd64", "linux/arm64"]} : {kind:"native",key:target.key,os:"linux",arch:"amd64",application_platform:"linux"})}><option value="native">{t("releaseFlow.native")}</option><option value="image">{t("releaseFlow.image")}</option></Select></Field>
        </div>
        {target.kind === "native" ? <>
          <div className="grid gap-4 sm:grid-cols-3">
            <Field label={t("releaseFlow.hostOS")}><Select className="input" value={target.os} onChange={event => changeTarget(index, {...target,os:event.target.value as "linux"|"darwin"|"windows",application_platform:null})}><option value="linux">Linux</option><option value="darwin">macOS</option><option value="windows">Windows</option></Select></Field>
            <Field label={t("releaseFlow.arch")}><Select className="input" value={target.arch} onChange={event => changeTarget(index, {...target,arch:event.target.value as "amd64"|"arm64"})}><option value="amd64">amd64</option><option value="arm64">arm64</option></Select></Field>
            <Field label={t("releaseFlow.application")}><Select className="input" value={target.application_platform || ""} onChange={event => changeTarget(index, {...target,application_platform:(event.target.value || null) as ApplicationPlatform|null})}><option value="">{t("builds.nonApplication")}</option>{(target.os === "darwin" ? ["macos","ios","android"] : target.os === "linux" ? ["linux","android"] : ["windows"]).map(platform => <option key={platform} value={platform}>{platform}</option>)}</Select></Field>
          </div>
          <div className="grid gap-4 sm:grid-cols-2"><Field label={t("releaseFlow.artifactKey")}><Input value={target.artifact_key || ""} maxLength={63} placeholder={target.key} onChange={event => changeTarget(index,{...target,artifact_key:event.target.value || undefined})} /></Field><Field label={t("releaseFlow.suffix")}><Input value={target.format_suffix || ""} maxLength={32} placeholder=".tar.gz" onChange={event => changeTarget(index,{...target,format_suffix:event.target.value || undefined})} /></Field></div>
        </> : <div className="flex flex-wrap gap-5">{(["linux/amd64","linux/arm64"] as const).map(platform => <label key={platform} className="flex items-center gap-2 text-xs"><input type="checkbox" checked={target.platforms.includes(platform)} onChange={event => changeTarget(index,{...target,platforms:event.target.checked ? [...target.platforms,platform] : target.platforms.filter(item => item !== platform)})} />{platform}</label>)}</div>}
        <Button type="button" size="sm" variant="ghost" disabled={targets.length === 1} onClick={() => {setTargets(previous => previous.filter((_, position) => position !== index));setDirty(true);}}><Trash2 />{t("releaseFlow.removeTarget")}</Button>
      </fieldset>)}
      <Button type="button" variant="secondary" disabled={targets.length >= 32} onClick={() => {setTargets(previous => [...previous,{kind:"native",key:`target-${previous.length + 1}`,os:"linux",arch:"amd64",application_platform:"linux"}]);setDirty(true);}}><Plus />{t("releaseFlow.addTarget")}</Button>
    </div>
    <Field label={t("releaseFlow.parameters")}><Textarea name="parameters" aria-label={t("releaseFlow.parameters")} value={parameters} onChange={event => setParameters(event.target.value)} spellCheck={false} /><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.parametersHint")}</p></Field>
    <Button type="submit" disabled={pending || release.status !== "draft"}>{pending ? t("common.processing") : t("builds.start")}</Button>
  </form>;
}
