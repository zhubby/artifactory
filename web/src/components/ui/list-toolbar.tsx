/**
 * [INPUT]: 依赖 UI 搜索词条、图标和受控查询。
 * [OUTPUT]: 提供 ListToolbar 列表筛选栏。
 * [POS]: 统一业务列表的筛选、计数和辅助操作。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Search } from "lucide-react";
import { useUI } from "@/components/providers";
import type { ReactNode } from "react";

export function ListToolbar({ query, onQueryChange, count, children }: {
  query: string;
  onQueryChange: (value: string) => void;
  count?: number;
  children?: ReactNode;
}) {
  const { t, number } = useUI();
  return <div className="list-toolbar">
    <div className="list-toolbar-controls">
      <label className="workspace-filter">
        <Search size={15} aria-hidden="true" />
        <input value={query} onChange={event => onQueryChange(event.target.value)}
          aria-label={t("search.label")} placeholder={t("search.placeholder")} />
      </label>
      {children}
    </div>
    {count !== undefined && <span className="list-count" aria-live="polite">{t("redesign.results", { count: number(count) })}</span>}
  </div>;
}
