/**
 * [INPUT]: 依赖 Radix Select、原生选项声明和语义控件样式。
 * [OUTPUT]: 提供等宽对齐浮层，保留表单、异步默认值和分层关闭。
 * [POS]: 业务选项的统一入口；浮层负责交互，隐藏字段负责表单。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown, ChevronUp } from "lucide-react";
import {
  Children, Fragment, isValidElement, useEffect, useId, useLayoutEffect, useRef, useState,
  type ComponentPropsWithRef, type KeyboardEvent as ReactKeyboardEvent, type ReactNode,
} from "react";
import { cn } from "@/lib/utils";

type Option = {
  kind: "option";
  key: string;
  value: string;
  label: string;
  disabled: boolean;
  hidden: boolean;
};
type Group = { kind: "group"; key: string; label?: string; items: Entry[] };
type Entry = Option | Group;

function guardComposition(event: ReactKeyboardEvent<HTMLElement>) {
  if (!event.nativeEvent.isComposing && event.keyCode !== 229) return;
  event.preventDefault();
  event.stopPropagation();
}

function text(children: ReactNode): string {
  return Children.toArray(children).map(child => {
    if (typeof child === "string" || typeof child === "number") return String(child);
    return isValidElement<{ children?: ReactNode }>(child) ? text(child.props.children) : "";
  }).join("");
}

function readOptions(children: ReactNode) {
  const options: Option[] = [];
  function read(nodes: ReactNode, disabled = false): Entry[] {
    const entries: Entry[] = [];
    Children.forEach(nodes, child => {
      if (!isValidElement<{ children?: ReactNode; value?: string | number; label?: string; disabled?: boolean; hidden?: boolean }>(child)) return;
      if (child.type === Fragment) entries.push(...read(child.props.children, disabled));
      if (child.type === "optgroup") entries.push({
        kind: "group", key: String(entries.length), label: child.props.label,
        items: read(child.props.children, disabled || Boolean(child.props.disabled)),
      });
      if (child.type === "option") {
        const option: Option = {
          kind: "option", key: `option-${options.length}`,
          value: child.props.value === undefined ? text(child.props.children) : String(child.props.value),
          label: child.props.label ?? text(child.props.children),
          disabled: disabled || Boolean(child.props.disabled), hidden: Boolean(child.props.hidden),
        };
        options.push(option);
        entries.push(option);
      }
    });
    return entries;
  }
  return { entries: read(children), options };
}

function OptionEntries({ entries }: { entries: Entry[] }) {
  return entries.map(entry => entry.kind === "group" ? (
    <SelectPrimitive.Group key={`group-${entry.key}`}>
      {entry.label && <SelectPrimitive.Label className="select-group-label">{entry.label}</SelectPrimitive.Label>}
      <OptionEntries entries={entry.items} />
    </SelectPrimitive.Group>
  ) : !entry.hidden && (
    <SelectPrimitive.Item className="select-option" key={entry.key} value={entry.key} textValue={entry.label} disabled={entry.disabled} data-value={entry.value}>
      <SelectPrimitive.ItemText>{entry.label}</SelectPrimitive.ItemText>
      <SelectPrimitive.ItemIndicator className="select-option-indicator"><Check size={14} /></SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  ));
}

export function Select({
  children, value, defaultValue, onChange, onInvalid, onFocus, className, style, id,
  name, form, disabled, required, autoFocus, ref, "aria-label": label,
  "aria-labelledby": labelledBy, "aria-describedby": describedBy,
  "aria-invalid": ariaInvalid, ...props
}: ComponentPropsWithRef<"select">) {
  const generatedId = useId();
  const native = useRef<HTMLSelectElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const presentation = useRef<HTMLSpanElement>(null);
  const resetting = useRef(false);
  const { entries, options } = readOptions(children);
  const first = options.find(option => !option.disabled)?.value ?? "";
  const initialValue = useRef(String(defaultValue ?? first));
  const initialOptionReady = useRef(false);
  const [current, setCurrent] = useState(initialValue.current);
  const [invalid, setInvalid] = useState(false);
  const [open, setOpen] = useState(false);
  const controlled = value !== undefined;
  const requested = controlled ? String(value) : current;
  const selected = options.find(option => option.value === requested) ?? options.find(option => !option.disabled);
  const selectedValue = selected?.value ?? "";

  useLayoutEffect(() => {
    if (!open) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // 先于父弹窗的 document 捕获监听，关闭只作用于当前下拉。
      event.stopPropagation();
      event.stopImmediatePropagation();
      if (event.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      setOpen(false);
    };
    window.addEventListener("keydown", escape, true);
    return () => window.removeEventListener("keydown", escape, true);
  }, [open]);

  useLayoutEffect(() => {
    const element = presentation.current;
    const stopInternalChange = (event: Event) => {
      // 在 DOM 捕获阶段隔离无名字段，原生表单监听器也只接收真实字段。
      if (event.target instanceof HTMLSelectElement) event.stopPropagation();
    };
    element?.addEventListener("change", stopInternalChange, true);
    return () => element?.removeEventListener("change", stopInternalChange, true);
  }, []);

  useEffect(() => {
    if (options.some(option => option.value === initialValue.current)) initialOptionReady.current = true;
    const waitingForDefault = defaultValue !== undefined && current === initialValue.current && !initialOptionReady.current;
    if (!controlled && !waitingForDefault && current !== selectedValue) setCurrent(selectedValue);
  }, [controlled, current, defaultValue, selectedValue, options]);

  useLayoutEffect(() => {
    const element = trigger.current;
    const caption = element?.closest("label")?.querySelector(":scope > span");
    if (!element || label || labelledBy || !(caption instanceof HTMLElement)) return;
    if (!caption.id) caption.id = `${generatedId}-label`;
    element.setAttribute("aria-labelledby", caption.id);
  }, [generatedId, label, labelledBy]);

  useEffect(() => {
    const element = native.current;
    if (!element) return;
    for (const option of element.options) option.defaultSelected = option.value === initialValue.current;
    const parent = element.form;
    if (!parent) return;
    const reset = () => {
      resetting.current = true;
      setInvalid(false);
      const next = controlled ? selectedValue : options.find(option => option.value === initialValue.current)?.value ?? first;
      if (!controlled) {
        initialOptionReady.current = options.some(option => option.value === initialValue.current);
        setCurrent(initialValue.current);
      }
      queueMicrotask(() => {
        // 原生 reset 不产生 change；还原可见值和隐藏表单值。
        element.value = next;
        resetting.current = false;
      });
    };
    parent.addEventListener("reset", reset, true);
    return () => parent.removeEventListener("reset", reset, true);
  }, [controlled, selectedValue, first, options]);

  function change(key: string) {
    const element = native.current;
    const next = options.find(option => option.key === key);
    if (!element || !next || resetting.current || next.value === selectedValue) return;
    // 使用原生 setter，React 和表单父级均接收真实字段事件。
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set?.call(element, next.value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
  }

  return (
    <>
      <span className="select-ui" ref={presentation}>
      <SelectPrimitive.Root value={selected?.key ?? ""} onValueChange={change} disabled={disabled} open={open} onOpenChange={setOpen}>
        <SelectPrimitive.Trigger
          ref={trigger} id={id} className={cn("input select-trigger", className)} style={style}
          aria-label={label} aria-labelledby={labelledBy} aria-describedby={describedBy}
          aria-required={required || undefined} aria-invalid={ariaInvalid ?? (invalid || undefined)}
          autoFocus={autoFocus} data-value={selectedValue} data-name={name}
          onKeyDownCapture={guardComposition}
        >
          <SelectPrimitive.Value><span className="select-value">{selected?.label || "\u00a0"}</span></SelectPrimitive.Value>
          <SelectPrimitive.Icon className="select-chevron"><ChevronDown size={14} /></SelectPrimitive.Icon>
        </SelectPrimitive.Trigger>
        <SelectPrimitive.Portal>
          <SelectPrimitive.Content className="select-content" position="popper" align="start" sideOffset={5} collisionPadding={12} onKeyDownCapture={guardComposition}>
            <SelectPrimitive.ScrollUpButton className="select-scroll"><ChevronUp size={14} /></SelectPrimitive.ScrollUpButton>
            <SelectPrimitive.Viewport className="select-viewport"><OptionEntries entries={entries} /></SelectPrimitive.Viewport>
            <SelectPrimitive.ScrollDownButton className="select-scroll"><ChevronDown size={14} /></SelectPrimitive.ScrollDownButton>
          </SelectPrimitive.Content>
        </SelectPrimitive.Portal>
      </SelectPrimitive.Root>
      </span>
      <select
        {...props} ref={element => {
          native.current = element;
          if (typeof ref === "function") ref(element);
          else if (ref) ref.current = element;
        }}
        data-select-native="" className="select-native" aria-hidden="true" tabIndex={-1}
        name={name} form={form} disabled={disabled} required={required} value={selectedValue}
        onChange={event => {
          if (!controlled) setCurrent(event.target.value);
          setInvalid(false);
          onChange?.(event);
        }}
        onFocus={event => { onFocus?.(event); trigger.current?.focus({ preventScroll: true }); }}
        onInvalid={event => { onInvalid?.(event); event.preventDefault(); setInvalid(true); trigger.current?.focus({ preventScroll: true }); }}
      >{children}</select>
    </>
  );
}
