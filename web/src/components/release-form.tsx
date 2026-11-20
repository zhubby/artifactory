/**
 * [INPUT]: 依赖项目引用、Release 合同、租户变更 API 与统一通知。
 * [OUTPUT]: 提供 Release 创建页及草稿目标合同编辑表单。
 * [POS]: 版本与源码仅在创建时填写；构建继承此处保存的合同。引用与提交错误桥固定挂载。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useId, useState } from "react";
import { ArrowLeft, Plus, Trash2 } from "lucide-react";
import { ErrorBox, Field, PageTitle } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input, Textarea } from "@/components/ui/input";
import { mutation } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { useRepositoryRefs } from "@/lib/repository-refs";
import { isReleaseVersion, validReleaseTargets } from "@/lib/releases";
import { projectPath, releasePath } from "@/lib/routes";
import { Link, useRouter } from "@/lib/tenant";
import type { ApplicationPlatform, Project, Release, ReleaseTarget } from "@/lib/types";

export function ReleaseCreate({ project }: { project: Project }) {
  const { t } = useUI();
  const router = useRouter();
  return <>
    <Link href={`${projectPath(project.slug)}/releases`} className="mb-5 inline-flex items-center gap-2 text-xs text-muted-foreground"><ArrowLeft size={14} />{t("navigation.releases")}</Link>
    <PageTitle eyebrow={project.name} title={t("releaseFlow.create")} description={t("releaseFlow.createDescription")} />
    <section className="panel p-5"><ReleaseForm project={project} done={release => router.push(releasePath(project.slug, release.id))} /></section>
  </>;
}

export function ReleaseForm({ project, release, done }: { project: Project; release?: Release; done: (release: Release) => void }) {
  const { t } = useUI();
  const refs = useRepositoryRefs(release ? "" : project.id);
  const refList = useId();
  const [version, setVersion] = useState(release?.version || "");
  const [source, setSource] = useState(release?.requested_ref || "");
  const [sourceTouched, setSourceTouched] = useState(false);
  const [goal, setGoal] = useState(release?.goal || "");
  const [targets, setTargets] = useState<ReleaseTarget[]>(release?.target_requirements || [{kind:"native", key:"linux-amd64", os:"linux", arch:"amd64", application_platform:"linux", artifact_key:"app", format_suffix:".tar.gz"}]);
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error>();
  useEffect(() => { if (!sourceTouched && refs.data?.default_ref) setSource(refs.data.default_ref); }, [sourceTouched, refs.data]);
  function changeTarget(index: number, target: ReleaseTarget) {
    setTargets(previous => previous.map((item, position) => position === index ? target : item)); setDirty(true);
  }
  return <form className="space-y-5" data-dirty={dirty ? "true" : undefined} onChange={() => setDirty(true)} onSubmit={async event => {
    event.preventDefault();
    const form = event.currentTarget;
    setError(undefined);
    if (!isReleaseVersion(version)) return setError(new LocalizedError("releaseFlow.invalidVersion"));
    if (!source.trim() || source !== source.trim()) return setError(new LocalizedError("releaseFlow.invalidSource"));
    if (!goal.trim() || goal.length > 16000) return setError(new LocalizedError("wizard.invalidGoal"));
    if (!validReleaseTargets(targets)) return setError(new LocalizedError("releaseFlow.invalidTargets"));
    setPending(true);
    try {
      const contract = { goal, target_requirements: targets, targets: targets.map(target => target.key) };
      const saved = await mutation<Release>(release ? `/releases/${release.id}` : `/projects/${project.id}/releases`, release ? {spec_revision: release.spec_revision, ...contract} : {version, ref:source, ...contract}, release ? "PATCH" : "POST");
      notify.success(t("common.operationSucceeded"));
      form.removeAttribute("data-dirty"); setDirty(false); done(saved);
    } catch (error) { setError(asError(error)); } finally { setPending(false); }
  }}>
    <ErrorBox message={error} />
    <ErrorBox message={release ? undefined : refs.error} />
    <div className="grid gap-5 md:grid-cols-2">
      <Field label={t("releaseFlow.version")}><Input aria-label={t("releaseFlow.version")} name="version" value={version} readOnly={!!release} required maxLength={100} placeholder="1.2.0-rc.1" onChange={event => setVersion(event.target.value)} /><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.versionHint")}</p></Field>
      <Field label={t("releaseFlow.source")}><Input aria-label={t("releaseFlow.source")} name="ref" list={refList} value={source} readOnly={!!release} required onChange={event => {setSourceTouched(true); setSource(event.target.value);}} placeholder="refs/heads/main" />
        <datalist id={refList}>{[...(refs.data?.branches || []), ...(refs.data?.tags || [])].map(ref => <option key={ref.ref} value={ref.ref}>{ref.name}</option>)}</datalist>
        <p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.sourceHint")}</p>
        {!release && refs.error && <div className="mt-2"><Button type="button" variant="ghost" onClick={refs.reload}>{t("common.retry")}</Button></div>}
      </Field>
    </div>
    <Field label={t("releaseFlow.goal")}><Textarea name="goal" value={goal} maxLength={16000} required onChange={event => setGoal(event.target.value)} placeholder={t("releaseFlow.goalPlaceholder")} /></Field>
    <div className="space-y-4">
      <div><h2 className="text-sm font-medium">{t("releaseFlow.targets")}</h2><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.targetsHint")}</p></div>
      {targets.map((target, index) => <fieldset key={index} className="space-y-4 rounded-md border border-border p-4">
        <legend className="px-1 text-xs">{t("releaseFlow.targetNumber", {number:index + 1})}</legend>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t("releaseFlow.targetKey")}><Input required maxLength={63} value={target.key} onChange={event => changeTarget(index, {...target, key:event.target.value})} /></Field>
          <Field label={t("releaseFlow.targetKind")}><select className="input" value={target.kind} onChange={event => changeTarget(index, event.target.value === "image" ? {kind:"image",key:target.key,platforms:["linux/amd64", "linux/arm64"]} : {kind:"native",key:target.key,os:"linux",arch:"amd64",application_platform:"linux"})}><option value="native">{t("releaseFlow.native")}</option><option value="image">{t("releaseFlow.image")}</option></select></Field>
        </div>
        {target.kind === "native" ? <>
          <div className="grid gap-4 sm:grid-cols-3">
            <Field label={t("releaseFlow.hostOS")}><select className="input" value={target.os} onChange={event => changeTarget(index, {...target,os:event.target.value as "linux"|"darwin"|"windows",application_platform:null})}><option value="linux">Linux</option><option value="darwin">macOS</option><option value="windows">Windows</option></select></Field>
            <Field label={t("releaseFlow.arch")}><select className="input" value={target.arch} onChange={event => changeTarget(index, {...target,arch:event.target.value as "amd64"|"arm64"})}><option value="amd64">amd64</option><option value="arm64">arm64</option></select></Field>
            <Field label={t("releaseFlow.application")}><select className="input" value={target.application_platform || ""} onChange={event => changeTarget(index, {...target,application_platform:(event.target.value || null) as ApplicationPlatform|null})}><option value="">{t("builds.nonApplication")}</option>{(target.os === "darwin" ? ["macos","ios","android"] : target.os === "linux" ? ["linux","android"] : ["windows"]).map(platform => <option key={platform} value={platform}>{platform}</option>)}</select></Field>
          </div>
          <div className="grid gap-4 sm:grid-cols-2"><Field label={t("releaseFlow.artifactKey")}><Input value={target.artifact_key || ""} maxLength={63} placeholder={target.key} onChange={event => changeTarget(index,{...target,artifact_key:event.target.value || undefined})} /></Field><Field label={t("releaseFlow.suffix")}><Input value={target.format_suffix || ""} maxLength={32} placeholder=".tar.gz" onChange={event => changeTarget(index,{...target,format_suffix:event.target.value || undefined})} /></Field></div>
        </> : <div className="flex flex-wrap gap-5">{(["linux/amd64","linux/arm64"] as const).map(platform => <label key={platform} className="flex items-center gap-2 text-xs"><input type="checkbox" checked={target.platforms.includes(platform)} onChange={event => changeTarget(index,{...target,platforms:event.target.checked ? [...target.platforms,platform] : target.platforms.filter(item => item !== platform)})} />{platform}</label>)}</div>}
        <Button type="button" size="sm" variant="ghost" disabled={targets.length === 1} onClick={() => {setTargets(previous => previous.filter((_, position) => position !== index));setDirty(true);}}><Trash2 />{t("releaseFlow.removeTarget")}</Button>
      </fieldset>)}
      <Button type="button" variant="secondary" disabled={targets.length >= 32} onClick={() => {setTargets(previous => [...previous,{kind:"native",key:`target-${previous.length + 1}`,os:"linux",arch:"amd64",application_platform:"linux"}]);setDirty(true);}}><Plus />{t("releaseFlow.addTarget")}</Button>
    </div>
    <p className="text-xs leading-6 text-muted-foreground">{t("releaseFlow.contractHint")}</p>
    <Button type="submit" disabled={pending}>{pending ? t("common.processing") : t(release ? "releaseFlow.saveDraft" : "releaseFlow.create")}</Button>
  </form>;
}
