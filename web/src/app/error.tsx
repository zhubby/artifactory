/**
 * [INPUT]: 依赖 Next 重试回调、UI 词条和 Button。
 * [OUTPUT]: 提供 ErrorPage 路由错误界面。
 * [POS]: 显示故障反馈并允许用户重试或返回。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import Link from "next/link";
import { AlertCircle, RefreshCw } from "lucide-react";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";

export default function ErrorPage({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  const { t } = useUI();
  return <main className="error-page">
    <AlertCircle size={28} className="text-muted-foreground" aria-hidden="true" />
    <h1>{t("redesign.errorTitle")}</h1><p>{t("redesign.errorBody")}</p>
    <div className="flex gap-3"><Button onClick={reset}><RefreshCw />{t("redesign.retry")}</Button>
      <Button variant="secondary" asChild><Link href="/tenants">{t("redesign.back")}</Link></Button></div>
  </main>;
}
