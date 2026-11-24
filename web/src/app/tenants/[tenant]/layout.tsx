/**
 * [INPUT]: 依赖持续挂载的 Console 与子路由的校验结果。
 * [OUTPUT]: 提供租户工作区布局。
 * [POS]: 将工作区放在 catch-all 页面边界之外，保留同范围导航与请求。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { ReactNode } from "react";
import { Console } from "@/components/console";

export default function Layout({ children }: { children: ReactNode }) {
  return <><Console />{children}</>;
}
