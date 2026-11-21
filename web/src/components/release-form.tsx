/**
 * [INPUT]: 依赖 Release 版本与说明、严格 SemVer 和统一通知。
 * [OUTPUT]: 提供 Release 创建页和各发布状态的说明编辑表单。
 * [POS]: 版本创建后只读；源码、构建要求与目标由构建引导填写。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useState } from "react";
import { ArrowLeft } from "lucide-react";
import { ErrorBox, Field, PageTitle } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input, Textarea } from "@/components/ui/input";
import { mutation } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { isReleaseVersion } from "@/lib/releases";
import { projectPath, releasePath } from "@/lib/routes";
import { Link, useRouter } from "@/lib/tenant";
import type { Project, Release } from "@/lib/types";

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
  const [version, setVersion] = useState(release?.version || "");
  const [notes, setNotes] = useState(release?.notes || "");
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error>();
  return <form className="space-y-5" data-dirty={dirty ? "true" : undefined} onChange={() => setDirty(true)} onSubmit={async event => {
    event.preventDefault();
    const form = event.currentTarget;
    setError(undefined);
    if (!isReleaseVersion(version)) return setError(new LocalizedError("releaseFlow.invalidVersion"));
    setPending(true);
    try {
      const saved = await mutation<Release>(release ? `/releases/${release.id}` : `/projects/${project.id}/releases`, release ? {notes} : {version, notes}, release ? "PATCH" : "POST");
      notify.success(t("common.operationSucceeded"));
      form.removeAttribute("data-dirty"); setDirty(false); done(saved);
    } catch (error) { setError(asError(error)); } finally { setPending(false); }
  }}>
    <ErrorBox message={error} />
    <Field label={t("releaseFlow.version")}><Input aria-label={t("releaseFlow.version")} name="version" value={version} readOnly={!!release} required maxLength={100} placeholder="1.2.0-rc.1" onChange={event => setVersion(event.target.value)} /><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.versionHint")}</p></Field>
    <Field label={t("releaseFlow.notes")}><Textarea aria-label={t("releaseFlow.notes")} name="notes" value={notes} onChange={event => setNotes(event.target.value)} placeholder={t("releaseFlow.notesPlaceholder")} /><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.notesHint")}</p></Field>
    <Button type="submit" disabled={pending}>{pending ? t("common.processing") : t(release ? "releaseFlow.saveNotes" : "releaseFlow.create")}</Button>
  </form>;
}
