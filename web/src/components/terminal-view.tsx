/**
 * [INPUT]: 依赖按需加载的 xterm、fit、主题变量和会话输入回调。
 * [OUTPUT]: 提供字节渲染、完整清屏、窗口调整和受控粘贴。
 * [POS]: 只渲染终端，不创建会话或持有网络票据。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import type { Terminal } from "@xterm/xterm";

export type TerminalViewHandle = {
  write: (bytes: Uint8Array) => Promise<void>;
  focus: () => void;
  clear: () => void;
  paste: (text: string) => void;
  selection: () => string;
  size: () => { columns: number; rows: number };
};

export const TerminalView = forwardRef<TerminalViewHandle, {
  active: boolean;
  label: string;
  onReady: () => void;
  onInput: (bytes: Uint8Array) => void;
  onResize: (columns: number, rows: number) => void;
  onPaste: (text: string) => void;
  onReleaseFocus: () => void;
  onError: (error: Error) => void;
}>(function TerminalView(props, ref) {
  const container = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | undefined>(undefined);
  const callbacks = useRef(props);
  callbacks.current = props;

  useImperativeHandle(ref, () => ({
    write: bytes => new Promise<void>(resolve => {
      const target = terminal.current;
      if (target) target.write(bytes, resolve);
      else resolve();
    }),
    focus: () => terminal.current?.focus(),
    clear: () => {
      const target = terminal.current;
      target?.write("", () => {
        // reset 不清除播报缓存；通过公开选项重建无障碍视图。
        target.options.screenReaderMode = false;
        target.reset();
        target.clear();
        target.options.screenReaderMode = true;
      });
    },
    paste: text => terminal.current?.paste(text),
    selection: () => terminal.current?.getSelection() || "",
    size: () => ({ columns: terminal.current?.cols || 80, rows: terminal.current?.rows || 24 }),
  }), []);

  useEffect(() => {
    let disposed = false;
    let cleanup = () => {};
    void Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit")]).then(([{ Terminal }, { FitAddon }]) => {
      if (disposed || !container.current) return;
      const host = container.current;
      const fit = new FitAddon();
      const term = new Terminal({
        fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim(),
        fontSize: 13,
        lineHeight: 1.35,
        cursorBlink: false,
        scrollback: 2000,
        screenReaderMode: true,
        disableStdin: !callbacks.current.active,
        allowProposedApi: false,
        linkHandler: { activate: (_event, text) => {
          try {
            const url = new URL(text);
            if (["http:", "https:"].includes(url.protocol)) window.open(url.href, "_blank", "noopener,noreferrer");
          } catch { /* 非 URL 终端输出不产生浏览器动作。 */ }
        } },
      });
      term.loadAddon(fit);
      term.open(host);
      terminal.current = term;
      term.parser.registerOscHandler(52, () => true);
      term.attachCustomKeyEventHandler(event => {
        event.stopPropagation();
        if (event.type === "keydown" && event.ctrlKey && event.shiftKey && event.code === "Space") {
          event.preventDefault();
          callbacks.current.onReleaseFocus();
          return false;
        }
        return true;
      });
      const input = term.onData(data => callbacks.current.onInput(new TextEncoder().encode(data)));
      const binary = term.onBinary(data => callbacks.current.onInput(Uint8Array.from(data, character => character.charCodeAt(0))));
      const resize = term.onResize(({ cols, rows }) => callbacks.current.onResize(cols, rows));
      const paste = (event: ClipboardEvent) => {
        if (!callbacks.current.active) return;
        const text = event.clipboardData?.getData("text/plain");
        if (text === undefined) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        callbacks.current.onPaste(text);
      };
      host.addEventListener("paste", paste, true);
      const fitTerminal = () => {
        if (host.clientWidth && host.clientHeight) fit.fit();
      };
      const observer = new ResizeObserver(fitTerminal);
      observer.observe(host);
      const theme = () => {
        const style = getComputedStyle(document.documentElement);
        term.options.theme = {
          background: style.getPropertyValue("--log-background").trim(),
          foreground: style.getPropertyValue("--log-foreground").trim(),
          cursor: style.getPropertyValue("--foreground").trim(),
          selectionBackground: style.getPropertyValue("--selection").trim(),
        };
      };
      const themes = new MutationObserver(theme);
      themes.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
      theme();
      fitTerminal();
      host.querySelector("textarea")?.setAttribute("aria-label", callbacks.current.label);
      callbacks.current.onReady();
      cleanup = () => {
        observer.disconnect();
        themes.disconnect();
        host.removeEventListener("paste", paste, true);
        input.dispose(); binary.dispose(); resize.dispose();
        terminal.current = undefined;
        term.dispose();
      };
    }).catch(error => {
      if (!disposed) callbacks.current.onError(error instanceof Error ? error : new Error("Terminal renderer unavailable"));
    });
    return () => { disposed = true; cleanup(); };
  }, []);

  useEffect(() => {
    if (terminal.current) terminal.current.options.disableStdin = !props.active;
  }, [props.active]);

  useEffect(() => {
    container.current?.querySelector("textarea")?.setAttribute("aria-label", props.label);
  }, [props.label]);

  return <div ref={container} className="terminal-canvas" data-terminal-input="" data-testid="terminal-canvas" />;
});
