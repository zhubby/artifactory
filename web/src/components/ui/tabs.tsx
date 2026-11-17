/**
 * [INPUT]: 依赖 React 焦点引用和受控选中值。
 * [OUTPUT]: 提供 Tabs 键盘可访问标签。
 * [POS]: 保留内容节点并统一标签键盘导航。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useId, useRef, type ReactNode } from "react";

export function Tabs({ value, onChange, label, items, children }: {
  value: string;
  onChange: (value: string) => void;
  label: string;
  items: { value: string; label: string }[];
  children: (value: string) => ReactNode;
}) {
  const id = useId();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  return <>
    <div className="ui-tabs" role="tablist" aria-label={label}>
      {items.map((item, index) => <button key={item.value} ref={element => { refs.current[index] = element; }}
        type="button" role="tab" id={id + item.value} className="ui-tab"
        aria-selected={value === item.value} aria-controls={id + "panel"}
        tabIndex={value === item.value ? 0 : -1} onClick={() => onChange(item.value)}
        onKeyDown={event => {
          const next = event.key === "ArrowRight" ? (index + 1) % items.length
            : event.key === "ArrowLeft" ? (index + items.length - 1) % items.length
            : event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : undefined;
          if (next === undefined) return;
          event.preventDefault();
          onChange(items[next].value);
          refs.current[next]?.focus();
        }}>{item.label}</button>)}
    </div>
    <div role="tabpanel" id={id + "panel"} aria-labelledby={id + value} tabIndex={0}>
      {children(value)}
    </div>
  </>;
}
