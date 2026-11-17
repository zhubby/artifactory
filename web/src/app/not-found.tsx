/**
 * [INPUT]: 依赖 Next Link、品牌和 UI 词条。
 * [OUTPUT]: 提供 NotFound 未知路由页面。
 * [POS]: 将未知地址引回已有控制台入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import Link from "next/link";
import { ArrowLeft, FileQuestion } from "lucide-react";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { BrandMark } from "@/components/brand-mark";

export default function NotFound() {
  const { t } = useUI();
  return <main className="error-page">
    <span className="brand-mark"><BrandMark /></span>
    <FileQuestion className="mt-4 text-muted-foreground" size={28} aria-hidden="true" />
    <h1>{t("redesign.notFoundTitle")}</h1>
    <p>{t("redesign.notFoundBody")}</p>
    <Button variant="secondary" asChild><Link href="/tenants"><ArrowLeft />{t("redesign.back")}</Link></Button>
  </main>;
}
