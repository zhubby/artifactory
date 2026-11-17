/**
 * [INPUT]: 依赖不可变发布及镜像摘要 API。
 * [OUTPUT]: 展示原生制品和按摘要固定的镜像。
 * [POS]: 发布页面只读取发布时保存的结果。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { Activity, GitBranch, ShieldCheck } from "lucide-react";
import { useUI } from "@/components/providers";
import { useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { buildPath } from "@/lib/routes";
import {
  ArtifactTable,
  CopyButton,
  ErrorBox,
  Loading,
  PageTitle,
  SectionTitle,
} from "@/components/common";
import { Button } from "@/components/ui/button";
import type { Release } from "@/lib/types";
export function ReleaseDetail({
  id,
  projectId,
}: {
  id: string;
  projectId: string;
}) {
  const { t, date } = useUI();
  const remote = useRemote<Release>(`/releases/${id}`);
  const release = remote.data;
  if (!release)
    return (
      <>
        <ErrorBox message={remote.error} />
        <Loading />
      </>
    );
  if (release.project_id !== projectId)
    return <ErrorBox message={t("common.notFound")} />;
  return (
    <>
      <PageTitle
        eyebrow={t("releases.detailEyebrow")}
        title={release.version}
        description={t("releases.resultDescription", {
          name: release.project_name,
          date: date(release.created_at),
        })}
      >
        <Button variant="secondary" asChild>
          <Link href={buildPath(release.project_slug, release.build_number)}>
            <Activity />
            {t("releases.sourceBuild")}
          </Link>
        </Button>
      </PageTitle>
      <div className="build-meta">
        <span className="meta-item">
          <GitBranch size={14} />
          {release.commit_sha}
        </span>
        <span className="meta-item">
          <ShieldCheck size={14} />
          {t("releases.immutable")}
        </span>
      </div>
      <section className="panel">
        <SectionTitle
          title={t("builds.publish")}
          detail={t("common.artifactCount", {
            count: release.artifacts?.length || 0,
          })}
        />
        <ArtifactTable artifacts={release.artifacts || []} />
      </section>
      {!!release.images?.length && <section className="panel mt-5">
        <SectionTitle title={t("images.results")} />
        <div className="space-y-4 p-5">{release.images.map(image => {
          const ref = `${release.image_registry ? `${release.image_registry.replace(/^https?:\/\//, "").replace(/\/$/, "")}/` : ""}${image.repository}@${image.digest}`;
          return <article key={image.id} className="space-y-2">
            <strong>{image.name} · {image.target}</strong>
            <p className="break-all font-mono text-xs">{ref} <CopyButton value={ref} /></p>
            <p className="text-xs text-muted-foreground">{image.platforms.map(item => item.platform).join(" · ")}</p>
          </article>;
        })}</div>
      </section>}
      <div className="notice mt-5">{t("releases.notice")}</div>
    </>
  );
}
