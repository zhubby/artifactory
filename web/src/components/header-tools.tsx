/**
 * [INPUT]: 依赖账户插槽、文档地址和调用方通知入口。
 * [OUTPUT]: 提供文档、账户和最右通知的共享顶栏工具。
 * [POS]: 工作区、租户大厅与通知页共用；账户位置由单实例菜单选择。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import type { ReactNode } from "react";
import { BookOpen } from "lucide-react";
import { AccountSlot } from "@/components/account";
import { useUI } from "@/components/providers";
import { cn } from "@/lib/utils";

export function HeaderTools({ children, notification, workspace = false }: {
  children?: ReactNode;
  notification?: ReactNode;
  workspace?: boolean;
}) {
  const { t } = useUI();
  return (
    <div className={cn("header-tools", workspace && "workspace-tools")}>
      {children}
      <a className="header-docs" href="/docs" aria-label={t("navigation.docs")} title={t("navigation.docs")}>
        <BookOpen size={18} aria-hidden="true" />
        <span>{t("navigation.docs")}</span>
      </a>
      <AccountSlot placement="header" />
      {notification}
    </div>
  );
}
