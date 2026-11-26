/**
 * [INPUT]: 依赖 Next.js 路由跳转。
 * [OUTPUT]: 将站点首页转到租户入口。
 * [POS]: 控制台的根路径，共用租户入口的会话检查。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { redirect } from "next/navigation";

export default function Page() {
  redirect("/tenants");
}
