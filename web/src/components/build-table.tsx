/**
 * [INPUT]: 依赖独立构建目标摘要与含 Release 归属的构建列表。
 * [OUTPUT]: 提供可键盘滚动的构建状态表与具名详情入口。
 * [POS]: 版本工作台与项目聚合共用的构建入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { buildTargetSummary } from "@/lib/build-targets";
import { useUI } from "@/components/providers";

import { Link } from "@/lib/tenant";
import { buildPath } from "@/lib/routes";
import { ArrowUpRight, GitBranch, GitCommitHorizontal } from "lucide-react";
import { Empty, Status } from "@/components/common";
import { elapsed, short } from "@/lib/utils";
import type { Build } from "@/lib/types";

export function BuildTable({ builds }: { builds: Build[] }) {
  const { t, date } = useUI();
  if (!builds.length)
    return <Empty title={t("builds.empty")} detail={t("builds.emptyDetail")} />;
  return (
    <div className="table-scroll delivery-build-table" tabIndex={0} role="region" aria-label={t("releaseFlow.buildHistory")}>
      <table>
        <thead>
          <tr>
            <th>{t("builds.project")}</th>
            <th>{t("builds.source")}</th>
            <th>{t("builds.buildStatus")}</th>
            <th>{t("releaseFlow.targets")}</th>
            <th>{t("builds.duration")}</th>
            <th>{t("common.createdAt")}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {builds.map((build) => (
            <tr key={build.id}>
              <td>
                <Link
                  href={buildPath(build.project_slug, build.number, build.release_id)}
                  className="font-medium hover:text-primary"
                >
                  {build.project_name}
                </Link>
                <div className="mt-1.5 font-mono text-xs text-muted-foreground">
                  #{build.number}
                  {build.release_version && (
                    <span className="ml-2 text-primary">
                      {build.release_version}
                    </span>
                  )}
                </div>
              </td>
              <td>
                <div className="flex items-center gap-1.5 text-xs">
                  <GitBranch size={12} />
                  {build.requested_ref}
                </div>
                <div className="mt-1.5 flex items-center gap-1.5 font-mono text-xs text-muted-foreground">
                  <GitCommitHorizontal size={12} />
                  {short(build.commit_sha)}
                </div>
              </td>
              <td>
                <Status value={build.status} />
              </td>
              <td>
                <span className="text-xs">{buildTargetSummary(build)}</span>
              </td>
              <td className="font-mono text-xs text-muted-foreground">
                {elapsed(build.created_at, build.finished_at)}
              </td>
              <td className="font-mono text-xs text-muted-foreground">
                {date(build.created_at)}
              </td>
              <td>
                <Link
                  className="delivery-row-link inline-flex size-11 items-center justify-center rounded-full text-muted-foreground hover:bg-secondary hover:text-primary"
                  href={buildPath(build.project_slug, build.number, build.release_id)}
                  aria-label={t("builds.view", { id: build.number })}
                >
                  <ArrowUpRight size={16} />
                </Link>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
