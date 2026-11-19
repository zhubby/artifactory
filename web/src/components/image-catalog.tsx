/**
 * [INPUT]: 依赖项目镜像目录 API 和构建路由。
 * [OUTPUT]: 展示已验证镜像、平台和不可变拉取引用。
 * [POS]: 与原生制品共享项目制品仓库页面。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useUI } from "@/components/providers";
import { CopyButton, Empty, ErrorBox, Loading, SectionTitle } from "@/components/common";
import { useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { buildPath, releasePath } from "@/lib/routes";
import type { ImageArtifact } from "@/lib/types";

export function ImageCatalog({ projectId, query, order }: { projectId: string; query: string; order: string }) {
  const { t, date } = useUI();
  const remote = useRemote<ImageArtifact[]>(`/images?project_id=${projectId}`);
  const images = remote.data?.filter(image => [image.name, image.repository, image.digest, image.target].some(value => value?.toLowerCase().includes(query.toLowerCase())))
    .sort((a, b) => (Date.parse(b.created_at) - Date.parse(a.created_at)) * (order === "newest" ? 1 : -1));
  return <section className="panel mt-5">
    <SectionTitle title={t("images.results")} />
    <ErrorBox message={remote.error} />
    {!images ? <Loading /> : !images.length ? <Empty title={t("images.empty")} detail={t("images.emptyDetail")} /> : <div className="table-scroll"><table>
      <thead><tr><th>{t("images.name")}</th><th>{t("images.repository")}</th><th>{t("images.platforms")}</th><th>{t("images.sourceBuild")}</th><th>{t("images.createdAt")}</th></tr></thead>
      <tbody>{images.map(image => {
        const ref = `${image.registry ? image.registry.replace(/^https?:\/\//, "").replace(/\/$/, "") + "/" : ""}${image.repository}@${image.digest}`;
        return <tr key={image.id}>
          <td>{image.name}<p className="mt-1 text-xs"><Link href={releasePath(image.project_slug,image.release_id)}>{image.release_version}</Link></p></td><td><code className="break-all text-xs">{ref}</code> <CopyButton value={ref} /></td>
          <td>{image.platforms.map(item => item.platform).join(" · ")}</td>
          <td><Link href={buildPath(image.project_slug, image.build_number,image.release_id)}>#{image.build_number}</Link></td><td>{date(image.created_at)}</td>
        </tr>;
      })}</tbody>
    </table></div>}
  </section>;
}
