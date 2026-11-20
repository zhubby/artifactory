/**
 * [INPUT]: 依赖前台并发计数与界面翻译。
 * [OUTPUT]: 提供延迟显示、完成后淡出的 TopProgress。
 * [POS]: 根布局唯一的顶部加载条，不占内容布局。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { getProgress, getServerProgress, subscribeProgress } from "@/lib/progress";
import { useUI } from "@/components/providers";

export function TopProgress() {
  const { t } = useUI();
  const active = useSyncExternalStore(subscribeProgress, getProgress, getServerProgress) > 0;
  const [phase, setPhase] = useState<"hidden" | "loading" | "complete">("hidden");
  const current = useRef(phase);
  current.current = phase;

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (active) {
      if (current.current === "hidden") timer = setTimeout(() => setPhase("loading"), 150);
      else setPhase("loading");
    } else if (current.current !== "hidden") {
      setPhase("complete");
      timer = setTimeout(() => setPhase("hidden"), 300);
    }
    return () => clearTimeout(timer);
  }, [active]);

  return <div className="top-progress" data-state={phase} hidden={phase === "hidden"}
    role="progressbar" aria-label={t("common.pageLoading")}>
    <span aria-hidden="true" />
  </div>;
}
