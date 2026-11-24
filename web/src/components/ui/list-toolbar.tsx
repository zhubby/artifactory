/**
 * [INPUT]: 依赖 UI 搜索词条、可选查询提示、图标和受控查询。
 * [OUTPUT]: 提供单行滚动的 ListToolbar 列表筛选栏。
 * [POS]: 统一业务列表的查询和筛选，结果计数独立对齐。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Search } from "lucide-react";
import { useUI } from "@/components/providers";
import type { ReactNode } from "react";

export function ListToolbar({ query, onQueryChange, count, children, placeholder }: {
  query: string;
  onQueryChange: (value: string) => void;
  count?: number;
  children?: ReactNode;
  placeholder?: string;
}) {
  const { t, number } = useUI();
  return <div className="list-toolbar">
    <div className="list-toolbar-controls filter-controls">
      <label className="workspace-filter">
        <Search size={15} aria-hidden="true" />
        <input value={query} onChange={event => onQueryChange(event.target.value)}
          aria-label={t("search.label")} placeholder={placeholder ?? t("search.placeholder")} />
      </label>
      {children}
    </div>
    {count !== undefined && <span className="list-count" aria-live="polite">{t("redesign.results", { count: number(count) })}</span>}
  </div>;
}
