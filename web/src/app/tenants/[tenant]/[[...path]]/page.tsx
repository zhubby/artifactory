import { notFound } from "next/navigation";
import { Console } from "@/components/console";
import { isSlug, validTenantRoute } from "@/lib/routes";

export default async function Page({ params }: {
  params: Promise<{ tenant: string; path?: string[] }>;
}) {
  const { tenant, path = [] } = await params;
  if (!isSlug(tenant) || !validTenantRoute(path)) notFound();
  return <Console />;
}
