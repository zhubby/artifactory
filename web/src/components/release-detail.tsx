/**
 * [INPUT]: 依赖构建候选摘要、共享 Select、Release、外仓分发与统一通知。
 * [OUTPUT]: 提供版本工作台、四步构建入口与说明编辑，轮询保留草稿和发布候选。
 * [POS]: 项目版本工作台；正式发布选择单个完整成功构建并冻结交付。错误桥跨加载分支保持挂载。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Select } from "@/components/ui/select";
import { useState } from "react";
import { ArrowLeft, Download, Pencil, Play, ShieldCheck } from "lucide-react";
import { useUI } from "@/components/providers";
import { downloadFile, mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { buildDeliveryLabel } from "@/lib/build-targets";
import { notify } from "@/lib/notify";
import { Link } from "@/lib/tenant";
import { buildPath, projectPath } from "@/lib/routes";
import { ArtifactTable, CopyButton, Empty, ErrorBox, Field, Loading, PageTitle, SectionTitle, Status } from "@/components/common";
import { BuildTable } from "@/components/build-table";
import { BuildWizard } from "@/components/build-wizard";
import { ReleaseForm } from "@/components/release-form";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { Tabs } from "@/components/ui/tabs";
import { SubmitForm } from "@/components/forms";
import { ImageBuilds } from "@/components/image-builds";
import type { Build, ImageCredential, Project, Release, ReleaseImage } from "@/lib/types";

export function ReleaseDetail({ id, project, readOnly = false }: { id: string; project: Project; readOnly?: boolean }) {
  const { t } = useUI();
  const remote = useRemote<Release>(`/releases/${id}`, 3000);
  const builds = useRemote<Build[]>(`/releases/${id}/builds`, 3000);
  const [tab, setTab] = useState("overview");
  const [dialog, setDialog] = useState<"edit"|"build"|"publish">();
  const [editing, setEditing] = useState<Release>();
  const [selectedBuild, setSelectedBuild] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error>();
  const release = remote.data;
  const editable = !readOnly && project.effective_role !== "viewer";
  if (!release) return <><ErrorBox key="release-feedback" message={remote.error} />{!remote.error && <Loading variant="detail" />}</>;
  if (release.project_id !== project.id) return <><ErrorBox key="release-feedback" message={t("common.notFound")} /></>;
  const draft = release.status === "draft";
  const candidates = builds.data?.filter(build => build.status === "succeeded") || [];
  const selected = builds.data?.find(build => build.id === release.selected_build_id);
  const chosen = release.status === "publish_failed" ? release.selected_build_id || "" : candidates.some(build => build.id === selectedBuild) ? selectedBuild : dialog === "publish" ? "" : candidates[0]?.id || "";
  async function publish() {
    if (!chosen) return;
    setPending(true); setError(undefined);
    try {
      const saved = await mutation<Release>(`/releases/${id}/publish`, {build_id:chosen});
      notify.success(t("common.operationSucceeded"));
      remote.setData(saved); setDialog(undefined); await Promise.all([remote.reload(), builds.reload()]);
    } catch (error) { setError(asError(error)); } finally { setPending(false); }
  }
  async function download(kind: "manifest"|"checksums") {
    try { await downloadFile(`/releases/${id}/${kind}`); notify.success(t("common.downloadStarted")); }
    catch (error) { setError(asError(error)); }
  }
  return <>
    <ErrorBox key="release-feedback" message={error || remote.error || builds.error} />
    <Link href={`${projectPath(project.slug)}/releases`} className="mb-5 inline-flex items-center gap-2 text-xs text-muted-foreground"><ArrowLeft size={14} />{t("navigation.releases")}</Link>
    <PageTitle eyebrow={project.name} title={release.version} description={t("releaseFlow.workbenchDescription")}>
      <Status value={release.status} />
      {editable && <Button variant="secondary" onClick={() => {setEditing(release);setDialog("edit");}}><Pencil />{t("releaseFlow.editNotes")}</Button>}
      {editable && draft && <Button onClick={() => setDialog("build")}><Play />{t("builds.start")}</Button>}
      {editable && (draft || release.status === "publish_failed") && <Button variant="secondary" disabled={draft ? !candidates.length : !chosen} onClick={() => {if (draft) setSelectedBuild(chosen);setDialog("publish");}}><ShieldCheck />{t(release.status === "publish_failed" ? "releaseFlow.retryPublish" : "releaseFlow.publish")}</Button>}
    </PageTitle>
    {release.publish_error && <p className="mb-5 whitespace-pre-wrap break-words text-sm text-destructive" role="status" aria-atomic="true">{release.publish_error}</p>}
    <div className="build-meta"><span className="meta-item">{t("releaseFlow.buildCount", {count:release.build_count})}</span><span className="meta-item">{t("releaseFlow.artifactCount", {count:release.artifact_count || 0})}</span></div>
    {release.status === "publishing" && <p className="notice" role="status">{t("releaseFlow.publishingHint")}</p>}
    <Tabs value={tab} onChange={setTab} label={t("releaseFlow.tabs")} items={[{value:"overview",label:t("releaseFlow.overview")},{value:"builds",label:t("releaseFlow.buildHistory")},{value:"delivery",label:t("releaseFlow.deliverables")}]}>{activeTab => <>
      <section hidden={activeTab !== "overview"} className="space-y-5">
        <section className="panel"><SectionTitle title={t("releaseFlow.notes")} />
          <div className="p-5"><p className="whitespace-pre-wrap break-words text-sm leading-6">{release.notes || t("releaseFlow.noNotes")}</p></div>
        </section>
        <section className="panel"><SectionTitle title={t("releaseFlow.deliverySelection")} /><div className="p-5 text-sm">{selected ? <Link className="break-all text-primary" href={buildPath(project.slug,selected.number,release.id)}>{buildDeliveryLabel(selected)}</Link> : <p>{t("releaseFlow.notPublished")}</p>}</div></section>
      </section>
      <section hidden={activeTab !== "builds"} className="panel"><SectionTitle title={t("releaseFlow.buildHistory")} detail={t("releaseFlow.historyHint")} />{builds.data ? <BuildTable builds={builds.data} /> : !builds.error && <Loading variant="table" />}</section>
      <div hidden={activeTab !== "delivery"} className="space-y-5">
        {release.status !== "published" ? <section className="panel"><Empty title={t("releaseFlow.noDelivery")} detail={t("releaseFlow.noDeliveryHint")} /></section> : <>
          <div className="flex flex-wrap gap-3"><Button variant="secondary" onClick={() => void download("manifest")}><Download />{t("releaseFlow.manifest")}</Button><Button variant="secondary" onClick={() => void download("checksums")}><Download />SHA256SUMS</Button></div>
          <section className="panel"><SectionTitle title={t("releaseFlow.nativeArtifacts")} /><ArtifactTable artifacts={release.artifacts || []} /></section>
          <ReleaseImages release={release} builds={builds.data || []} editable={editable} />
          <p className="notice">{t("releaseFlow.frozenHint")}</p>
        </>}
      </div>
    </>}</Tabs>
    <Modal open={dialog === "edit"} onOpenChange={open => {if (!open) setDialog(undefined);}} title={t("releaseFlow.editNotes")} description={t("releaseFlow.editHint")}>{editing && <ReleaseForm key={editing.id} project={project} release={editing} done={saved => {remote.setData(saved);setDialog(undefined);}} />}</Modal>
    {dialog === "build" && <BuildWizard project={project} release={release} canBuild={editable && project.status === "active"} onClose={() => setDialog(undefined)} />}
    <Modal open={dialog === "publish" && (draft || release.status === "publish_failed")} onOpenChange={open => {if (!open) setDialog(undefined);}} title={t("releaseFlow.publish")} description={t("releaseFlow.publishHint")}>
      {draft ? <Field label={t("releaseFlow.selectBuild")}><Select className="input" value={chosen} onChange={event => setSelectedBuild(event.target.value)}>{!chosen && <option value="" disabled hidden>{t("releaseFlow.selectBuild")}</option>}{candidates.map(build => <option key={build.id} value={build.id}>{buildDeliveryLabel(build)}</option>)}</Select></Field> : <p className="text-sm">{t("releaseFlow.retryHint")}</p>}
      <Button className="mt-5" disabled={pending || !chosen} onClick={() => void publish()}>{pending ? t("common.processing") : t("releaseFlow.confirmPublish")}</Button>
    </Modal>
  </>;
}

function ReleaseImages({ release, builds, editable }: {release:Release;builds:Build[];editable:boolean}) {
  const { t } = useUI();
  const [publication, setPublication] = useState<ReleaseImage>();
  const selectedBuild = builds.find(build => build.id === release.selected_build_id);
  const buildRemote = useRemote<Build>(selectedBuild ? `/projects/by-slug/${encodeURIComponent(release.project_slug)}/builds/${selectedBuild.number}` : null, 3000);
  const credentials = useRemote<ImageCredential[]>(publication ? `/projects/${release.project_id}/image-credentials` : null);
  if (!release.images?.length) return null;
  return <section className="panel"><SectionTitle title={t("images.results")} /><ErrorBox message={buildRemote.error} /><div className="space-y-4 p-5">{release.images.map(image => {
    const repository = `${release.image_registry ? release.image_registry.replace(/^https?:\/\//, "").replace(/\/$/, "") + "/" : ""}${image.repository}`;
    const ref = `${repository}@${image.digest}`;
    return <article key={image.id} className="space-y-3 rounded-md border border-border p-4"><strong>{image.name}</strong><p className="break-all font-mono text-xs">{repository}:{release.version} <CopyButton value={`${repository}:${release.version}`} /></p><p className="break-all font-mono text-xs">{ref} <CopyButton value={ref} /></p><p className="text-xs text-muted-foreground">{image.platforms.map(item => item.platform).join(" · ")}</p>{editable && image.execution_id && <Button variant="secondary" onClick={() => setPublication(image)}>{t("images.publishExternal")}</Button>}</article>;
  })}</div>
    {!!buildRemote.data?.image_executions?.some(execution => execution.operation === "publish") && <ImageBuilds executions={buildRemote.data.image_executions.filter(execution => execution.operation === "publish")} registry={release.image_registry || undefined} />}
    <Modal open={!!publication} onOpenChange={open => {if (!open) setPublication(undefined);}} title={t("images.publishExternal")} description={t("releaseFlow.externalHint")}><ErrorBox message={credentials.error} /><SubmitForm label={t("images.publishExternal")} onSubmit={async form => {
      await mutation(`/image-executions/${publication!.execution_id}/publish`,{credential_id:form.get("credential"),repository:form.get("repository")});setPublication(undefined);await buildRemote.reload();
    }}><Field label={t("images.credential")}><Select className="input" required name="credential"><option value="">{t("releaseFlow.chooseCredential")}</option>{credentials.data?.filter(item => item.kind === "registry" && item.enabled).map(item => <option value={item.id} key={item.id}>{item.name}</option>)}</Select></Field><Field label={t("images.repository")}><Input name="repository" required placeholder="team/application" /></Field><Field label={t("images.tags")}><Input value={release.version} readOnly /></Field></SubmitForm></Modal>
  </section>;
}
