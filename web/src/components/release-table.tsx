/**
 * [INPUT]: 依赖完整 Release 摘要与版本路由。
 * [OUTPUT]: 提供可键盘滚动的版本表与可展开完整说明。
 * [POS]: 项目概览和 Release 列表共用的版本入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useUI } from "@/components/providers";
import { ChevronDown } from "lucide-react";
import { Empty, Status } from "@/components/common";
import { releasePath } from "@/lib/routes";
import { Link } from "@/lib/tenant";
import type { Release } from "@/lib/types";

export function ReleaseTable({ releases }: {releases:Release[]}) {
  const { t } = useUI();
  if (!releases.length) return <Empty title={t("releases.empty")} detail={t("releaseFlow.emptyHint")} />;
  return <div className="table-scroll delivery-release-table" tabIndex={0} role="region" aria-label={t("navigation.releases")}><table><thead><tr><th>{t("releaseFlow.version")}</th><th>{t("common.status")}</th><th>{t("releaseFlow.notes")}</th><th>{t("releaseFlow.buildHistory")}</th></tr></thead><tbody>{releases.map(release => <tr key={release.id}>
    <td><Link className="font-mono text-primary" href={releasePath(release.project_slug,release.id)}>{release.version}</Link></td><td><Status value={release.status} /></td><td>{release.notes ? <details className="release-notes-disclosure">
      <summary><span className="release-notes-preview line-clamp-2 whitespace-pre-wrap text-xs">{release.notes}</span><span className="release-notes-toggle"><span className="release-notes-expand">{t("buildDetail.expandSummary")}</span><span className="release-notes-collapse">{t("buildDetail.collapseSummary")}</span><ChevronDown size={14} aria-hidden="true" /></span></summary>
      <p className="release-notes-body whitespace-pre-wrap text-sm">{release.notes}</p>
    </details> : <p className="text-xs text-muted-foreground">{t("releaseFlow.noNotes")}</p>}</td><td>{release.build_count}</td>
  </tr>)}</tbody></table></div>;
}
