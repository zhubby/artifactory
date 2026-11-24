/**
 * [INPUT]: 依赖 Next 异步路由参数与共享租户路由校验。
 * [OUTPUT]: 合法路由返回空内容，非法路由触发 notFound。
 * [POS]: 保留服务端路由边界；工作区由上层租户布局持续渲染。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { notFound } from "next/navigation";
import { isSlug, validTenantRoute } from "@/lib/routes";

export default async function Page({ params }: {
  params: Promise<{ tenant: string; path?: string[] }>;
}) {
  const { tenant, path = [] } = await params;
  if (!isSlug(tenant) || !validTenantRoute(path)) notFound();
  return null;
}
