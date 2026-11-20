/**
 * [INPUT]: 依赖受控输入、异步建议、React 生命周期与既有输入样式。
 * [OUTPUT]: 提供允许任意文本的 Combobox 与建议选项类型。
 * [POS]: 建议浮层与完整输入框等宽对齐，视口不足时收缩；保留表单合同。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useId, useLayoutEffect, useRef, useState, type ComponentProps } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import styles from "./combobox.module.css";

export type ComboboxOption = { value: string; label: string };
type ComboboxProps = Omit<ComponentProps<"input">, "value" | "defaultValue" | "onChange" | "list" | "children"> & {
  value: string;
  onValueChange: (value: string) => void;
  options: ComboboxOption[];
  loading?: boolean;
  error?: boolean;
  emptyLabel: string;
  loadingLabel: string;
  errorLabel: string;
};

export function Combobox({
  value, onValueChange, options, loading = false, error = false,
  emptyLabel, loadingLabel, errorLabel, className, disabled, readOnly,
  onFocus, onBlur, onClick, onKeyDown, ...inputProps
}: ComboboxProps) {
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState<string>();
  const [position, setPosition] = useState<{ top: number; left: number; width: number; maxHeight: number; transform: string }>();
  const query = value.trim().toLowerCase();
  const filtered = loading || error ? [] : options.filter(option => `${option.label} ${option.value}`.toLowerCase().includes(query));
  const activeIndex = filtered.findIndex(option => option.value === active);
  const expanded = open && !disabled && !readOnly;
  const listID = `${id}-list`;
  const activeID = activeIndex >= 0 ? `${id}-option-${activeIndex}` : undefined;

  function close() { setOpen(false); setActive(undefined); }
  function choose(option: ComboboxOption) {
    onValueChange(option.value);
    close();
    input.current?.focus({ preventScroll: true });
  }

  useLayoutEffect(() => {
    if (!expanded) return;
    function place() {
      const anchor = input.current?.getBoundingClientRect();
      if (!anchor) return;
      const viewport = window.visualViewport;
      const leftEdge = viewport?.offsetLeft || 0;
      const topEdge = viewport?.offsetTop || 0;
      const width = viewport?.width || innerWidth;
      const height = viewport?.height || innerHeight;
      const below = topEdge + height - anchor.bottom - 8;
      const above = anchor.top - topEdge - 8;
      const downward = below >= 160 || below >= above;
      const maxHeight = Math.max(48, Math.min(280, downward ? below : above));
      const popupWidth = Math.min(anchor.width, width - 16);
      setPosition({
        left: Math.max(leftEdge + 8, Math.min(anchor.left, leftEdge + width - popupWidth - 8)),
        top: downward ? anchor.bottom + 4 : anchor.top - 4,
        width: popupWidth, maxHeight, transform: downward ? "none" : "translateY(-100%)",
      });
    }
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    window.visualViewport?.addEventListener("resize", place);
    window.visualViewport?.addEventListener("scroll", place);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
      window.visualViewport?.removeEventListener("resize", place);
      window.visualViewport?.removeEventListener("scroll", place);
    };
  }, [expanded]);

  useEffect(() => {
    if (!expanded) return;
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !input.current?.contains(event.target) && !popup.current?.contains(event.target)) close();
    };
    // 在 Radix 的 document 捕获监听前消费 Escape。
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.isComposing || event.target !== input.current) return;
      event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation(); close();
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      window.removeEventListener("keydown", escape, true);
    };
  }, [expanded]);

  useEffect(() => {
    if (expanded && activeID) document.getElementById(activeID)?.scrollIntoView({ block: "nearest" });
  }, [expanded, activeID]);

  return <div className={styles.root}>
    <input {...inputProps} ref={input} value={value} disabled={disabled} readOnly={readOnly}
      className={cn("input", styles.input, className)} autoComplete="off"
      role="combobox" aria-autocomplete="list" aria-expanded={expanded}
      aria-controls={expanded ? listID : undefined} aria-activedescendant={expanded ? activeID : undefined}
      onChange={event => { onValueChange(event.target.value); setActive(undefined); setOpen(true); }}
      onFocus={event => { onFocus?.(event); if (!event.defaultPrevented) setOpen(true); }}
      onBlur={event => { onBlur?.(event); close(); }}
      onClick={event => { onClick?.(event); if (!event.defaultPrevented) setOpen(true); }}
      onKeyDown={event => {
        onKeyDown?.(event);
        if (event.defaultPrevented || disabled || readOnly || event.nativeEvent.isComposing || event.keyCode === 229) return;
        if (event.key === "Escape" && expanded) {
          event.preventDefault(); event.stopPropagation(); event.nativeEvent.stopImmediatePropagation(); close();
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault(); setOpen(true);
          if (!filtered.length) return;
          const current = expanded ? activeIndex : -1;
          const next = event.key === "ArrowDown"
            ? (current + 1) % filtered.length
            : current < 0 ? filtered.length - 1 : (current - 1 + filtered.length) % filtered.length;
          setActive(filtered[next].value);
        } else if (expanded && activeIndex >= 0 && (event.key === "Home" || event.key === "End")) {
          event.preventDefault(); setActive(filtered[event.key === "Home" ? 0 : filtered.length - 1].value);
        } else if (event.key === "Enter" && expanded) {
          event.preventDefault();
          if (activeIndex >= 0) choose(filtered[activeIndex]);
          else close();
        } else if (event.key === "Tab") close();
      }} />
    {!readOnly && <ChevronDown className={styles.chevron} size={15} aria-hidden="true" />}
    {expanded && position && createPortal(<div ref={popup} className={styles.popup} style={position}
      onPointerDown={event => event.preventDefault()}>
      <div id={listID} role="listbox" aria-label={inputProps["aria-label"]} aria-busy={loading}>
        {!loading && !error && filtered.map((option, index) => <button type="button" role="option" tabIndex={-1}
          id={`${id}-option-${index}`} key={option.value} aria-selected={option.value === value}
          className={styles.option} data-value={option.value} data-active={active === option.value || undefined}
          onPointerMove={() => setActive(option.value)} onClick={() => choose(option)}>
          <span className={styles.copy}><span>{option.label}</span><code>{option.value}</code></span>
          {option.value === value && <Check size={14} aria-hidden="true" />}
        </button>)}
      </div>
      {loading ? <div role="status" className={styles.loading}>
        <span className="sr-only">{loadingLabel}</span>
        <span className={styles.skeleton} aria-hidden="true" /><span className={styles.skeleton} aria-hidden="true" />
      </div> : (error || !filtered.length) && <p role="status" className={styles.message}>{error ? errorLabel : emptyLabel}</p>}
    </div>, document.body)}
  </div>;
}
