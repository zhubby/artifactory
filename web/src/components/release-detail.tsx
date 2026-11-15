"use client";
import { Activity, GitBranch, ShieldCheck } from "lucide-react";
import { useUI } from "@/components/providers";
import { useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { buildPath } from "@/lib/routes";
import {
  ArtifactTable,
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
      <div className="notice mt-5">{t("releases.notice")}</div>
    </>
  );
}
