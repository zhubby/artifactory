/**
 * [INPUT]: 依赖异步邀请路由参数与 InvitationJoin。
 * [OUTPUT]: 提供携带邀请令牌的加入页面。
 * [POS]: 服务端路由只解析参数；客户端负责邀请查询与加入反馈。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { InvitationJoin } from "@/components/tenant-chooser";

export default async function Page({ params }: {
  params: Promise<{ token: string }>;
}) {
  const { token } = await params;
  return <InvitationJoin token={token} />;
}
