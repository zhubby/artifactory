/**
 * [INPUT]: 依赖完整 Release 摘要与版本路由。
 * [OUTPUT]: 提供版本、生命周期、说明和构建次数表。
 * [POS]: 项目概览和 Release 列表共用的版本入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useUI } from "@/components/providers";
import { Empty, Status } from "@/components/common";
import { releasePath } from "@/lib/routes";
import { Link } from "@/lib/tenant";
import type { Release } from "@/lib/types";

export function ReleaseTable({ releases }: {releases:Release[]}) {
  const { t } = useUI();
  if (!releases.length) return <Empty title={t("releases.empty")} detail={t("releaseFlow.emptyHint")} />;
  return <div className="table-scroll"><table><thead><tr><th>{t("releaseFlow.version")}</th><th>{t("common.status")}</th><th>{t("releaseFlow.notes")}</th><th>{t("releaseFlow.buildHistory")}</th></tr></thead><tbody>{releases.map(release => <tr key={release.id}>
    <td><Link className="font-mono text-primary" href={releasePath(release.project_slug,release.id)}>{release.version}</Link></td><td><Status value={release.status} /></td><td><p className="line-clamp-2 whitespace-pre-wrap text-xs">{release.notes || t("releaseFlow.noNotes")}</p></td><td>{release.build_count}</td>
  </tr>)}</tbody></table></div>;
}
