/**
 * [INPUT]: 依赖完整 Release 摘要与版本路由。
 * [OUTPUT]: 提供版本、生命周期、目标修订和构建次数表。
 * [POS]: 项目概览和 Release 列表共用的版本入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useUI } from "@/components/providers";
import { Empty, Status } from "@/components/common";
import { releasePath } from "@/lib/routes";
import { Link } from "@/lib/tenant";
import type { Release } from "@/lib/types";
import { short } from "@/lib/utils";

export function ReleaseTable({ releases }: {releases:Release[]}) {
  const { t } = useUI();
  if (!releases.length) return <Empty title={t("releases.empty")} detail={t("releaseFlow.emptyHint")} />;
  return <div className="table-scroll"><table><thead><tr><th>{t("releaseFlow.version")}</th><th>{t("common.status")}</th><th>{t("releaseFlow.source")}</th><th>{t("releaseFlow.contract")}</th><th>{t("releaseFlow.buildHistory")}</th></tr></thead><tbody>{releases.map(release => <tr key={release.id}>
    <td><Link className="font-mono text-primary" href={releasePath(release.project_slug,release.id)}>{release.version}</Link></td><td><Status value={release.status} /></td><td><span className="text-xs">{release.requested_ref}</span><p className="mt-1 font-mono text-xs text-muted-foreground">{release.commit_sha ? short(release.commit_sha) : t("releaseFlow.sourceUnpinned")}</p></td><td>{t("releaseFlow.revision", {number:release.spec_revision})}</td><td>{release.build_count}</td>
  </tr>)}</tbody></table></div>;
}
