/**
 * [INPUT]: 依赖功能层 DOM、弹簧胶囊与独立系统辅助偏好。
 * [OUTPUT]: 提供指针折光、连续选择、局部按压与触发位置浮层动效。
 * [POS]: 根上下文的装饰增强；保留业务命中、焦点和路由节点。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect } from "react";
import { createPrismCapsule } from "./prism-capsule";

const surfaces = ".sidebar,.workspace-topbar,.account-menu,.space-selector-menu,.select-content,.modal-panel,.account-panel,.prism-auth-glass,.ph-glass,.prism-auth .header-tools,.prism-hall .header-tools,.prism-project-toolbar,.prism-device-toolbar,.terminal-controls,.inbox-drawer";
const groups = ".sidebar-navigation nav:not(.workspace-settings-navigation),.workspace-settings-group,.ui-tabs,.header-tools,.account-sections";
const controls = ".nav-item,.ui-tab,.header-docs,.command-trigger,.account-trigger,.inbox-bell,.account-sections button";
const pressControls = "[data-slot=button],.select-trigger,.command-trigger,.account-trigger,.header-docs,.inbox-bell,.space-selector summary,.ui-tab,.nav-item,.ph-enter-space";
type Capsule = ReturnType<typeof createPrismCapsule>;
type Group = { selection?: Capsule; hover?: Capsule };

export function PrismInteractions() {
  useEffect(() => {
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    const transparency = matchMedia("(prefers-reduced-transparency: reduce)");
    const contrast = matchMedia("(prefers-contrast: more)");
    const forced = matchMedia("(forced-colors: active)");
    const fine = matchMedia("(hover: hover) and (pointer: fine)");
    const root = document.documentElement;
    const prepared = new Map<HTMLElement, HTMLElement>();
    const states = new Map<HTMLElement, Group>();
    const timers = new Map<HTMLElement, ReturnType<typeof setTimeout>>();
    const waves = new Map<HTMLElement, { wave: HTMLElement; animation: Animation }>();
    let pointer: HTMLElement | undefined;
    let sample: { x: number; y: number; surface: HTMLElement } | undefined;
    let pointerFrame = 0;
    let refreshFrame = 0;
    let lastTrigger: HTMLElement | undefined;
    let pressed: HTMLElement | undefined;
    let disposed = false;

    const optical = () => !transparency.matches && !contrast.matches && !forced.matches && root.dataset.transparency !== "reduce" && root.dataset.contrast !== "more";
    const moving = () => !document.hidden && !motion.matches && root.dataset.motion !== "off";
    const resetPointer = () => {
      cancelAnimationFrame(pointerFrame);
      pointerFrame = 0;
      if (pointer) {
        pointer.removeAttribute("data-prism-pointer");
        ["--prism-pointer-x", "--prism-pointer-y", "--prism-rim-angle"].forEach(name => pointer!.style.removeProperty(name));
      }
      pointer = undefined;
      sample = undefined;
    };
    const release = () => {
      pressed?.removeAttribute("data-prism-pressed");
      pressed = undefined;
    };
    const stop = () => {
      resetPointer();
      release();
      timers.forEach((timer, shell) => { clearTimeout(timer); shell.removeAttribute("data-prism-active"); });
      timers.clear();
      waves.forEach(({ wave, animation }) => { animation.cancel(); wave.remove(); });
      waves.clear();
      states.forEach(state => { state.hover?.hide(); state.selection?.settle(); });
    };
    function syncGroups() {
      states.forEach((state, group) => {
        if (!group.isConnected) {
          resize.unobserve(group);
          state.selection?.destroy(); state.hover?.destroy(); states.delete(group);
          return;
        }
        const selected = group.querySelector<HTMLElement>(".nav-item.active,.ui-tab[aria-selected=true],button[aria-current=page]");
        if (selected && optical()) {
          state.selection ??= createPrismCapsule(group, "prism-selection-capsule");
          state.selection.move(selected, moving());
          group.dataset.prismSelection = "";
        } else {
          state.selection?.hide();
          group.removeAttribute("data-prism-selection");
        }
      });
    }
    const schedule = () => {
      if (refreshFrame || disposed) return;
      refreshFrame = requestAnimationFrame(() => { refreshFrame = 0; syncGroups(); });
    };
    const resize = new ResizeObserver(() => {
      states.forEach(state => state.hover?.hide());
      schedule();
    });
    function prepare() {
      if (pointer && !pointer.isConnected) resetPointer();
      if (pressed && !pressed.isConnected) release();
      waves.forEach(({ wave, animation }, control) => { if (!control.isConnected) { animation.cancel(); wave.remove(); waves.delete(control); } });
      prepared.forEach((sheen, shell) => { if (!shell.isConnected) { sheen.remove(); prepared.delete(shell); } });
      document.querySelectorAll<HTMLElement>(surfaces).forEach(shell => {
        if (prepared.has(shell)) return;
        const sheen = document.createElement("span");
        sheen.className = "prism-sheen";
        sheen.setAttribute("aria-hidden", "true");
        shell.prepend(sheen);
        shell.dataset.prismSurface = "";
        prepared.set(shell, sheen);
        // 居中面板保持中心几何，菜单沿触发器方向展开。
        if (shell.matches(".account-menu,.space-selector-menu")) {
          const anchor = shell.closest(".account-anchor,.space-selector")?.querySelector<HTMLElement>(".account-trigger,summary");
          const box = shell.getBoundingClientRect(), trigger = anchor?.getBoundingClientRect();
          if (trigger) shell.style.setProperty("--prism-popover-origin", `${Math.max(16, Math.min(box.width - 16, trigger.left + trigger.width / 2 - box.left))}px ${trigger.top >= box.bottom ? "100%" : "0%"}`);
        }
        if (shell.matches(".modal-panel:not(.drawer-panel),.account-panel") && lastTrigger?.isConnected) {
          const box = shell.getBoundingClientRect(), trigger = lastTrigger.getBoundingClientRect();
          shell.style.setProperty("--prism-entry-x", `${Math.max(10, Math.min(90, (trigger.left + trigger.width / 2 - box.left) / box.width * 100))}%`);
        }
      });
      document.querySelectorAll<HTMLElement>(groups).forEach(group => {
        if (states.has(group)) return;
        group.dataset.prismGroup = "";
        states.set(group, {});
        resize.observe(group);
      });
      syncGroups();
    }
    function highlight(target: Element) {
      const item = target.closest<HTMLElement>(controls);
      const group = item?.closest<HTMLElement>("[data-prism-group]");
      states.forEach((state, parent) => { if (parent !== group) state.hover?.hide(); });
      if (!item || !group || !moving() || !optical()) return;
      const state = states.get(group);
      if (!state) return;
      state.hover ??= createPrismCapsule(group, "prism-hover-capsule");
      state.hover.move(item, true);
    }
    function enter(event: PointerEvent) {
      if (!(event.target instanceof Element) || !moving() || !optical() || !fine.matches) return;
      const shell = event.target.closest<HTMLElement>(surfaces);
      if (shell && !(event.relatedTarget instanceof Node && shell.contains(event.relatedTarget))) {
        clearTimeout(timers.get(shell));
        shell.dataset.prismActive = "";
        timers.set(shell, setTimeout(() => { shell.removeAttribute("data-prism-active"); timers.delete(shell); }, 760));
      }
      highlight(event.target);
    }
    function move(event: PointerEvent) {
      if (!(event.target instanceof Element) || !moving() || !optical() || !fine.matches) return;
      const surface = event.target.closest<HTMLElement>(surfaces);
      if (!surface) { resetPointer(); return; }
      if (pointer !== surface) { resetPointer(); pointer = surface; }
      sample = { x: event.clientX, y: event.clientY, surface };
      if (pointerFrame) return;
      pointerFrame = requestAnimationFrame(() => {
        pointerFrame = 0;
        if (!sample || !sample.surface.isConnected) return;
        const { x, y, surface: shell } = sample;
        const rect = shell.getBoundingClientRect();
        shell.style.setProperty("--prism-pointer-x", `${x - rect.left}px`);
        shell.style.setProperty("--prism-pointer-y", `${y - rect.top}px`);
        shell.style.setProperty("--prism-rim-angle", `${Math.atan2(y - rect.top - rect.height / 2, x - rect.left - rect.width / 2) * 180 / Math.PI + 90}deg`);
        shell.dataset.prismPointer = "";
      });
    }
    function leave(event: PointerEvent) {
      if (!(event.relatedTarget instanceof Node) || !pointer?.contains(event.relatedTarget)) resetPointer();
      states.forEach((state, group) => {
        if (!(event.relatedTarget instanceof Node) || !group.contains(event.relatedTarget)) state.hover?.hide();
      });
    }
    function press(control: HTMLElement, x: number, y: number) {
      release();
      lastTrigger = control;
      if (!moving() || !optical() || control.matches(":disabled,[aria-disabled=true]") || control.closest("[inert]")) return;
      waves.get(control)?.animation.cancel();
      waves.get(control)?.wave.remove();
      const box = control.getBoundingClientRect();
      const wave = document.createElement("span");
      wave.className = "prism-press-wave";
      wave.setAttribute("aria-hidden", "true");
      wave.style.setProperty("--press-x", `${x - box.left}px`);
      wave.style.setProperty("--press-y", `${y - box.top}px`);
      control.append(wave);
      control.dataset.prismPressed = "";
      pressed = control;
      const animation = wave.animate([{ opacity: 0 }, { opacity: .9, offset: .16 }, { opacity: 0 }], { duration: 620, easing: "cubic-bezier(.16,1,.3,1)" });
      waves.set(control, { wave, animation });
      const clear = () => {
        wave.remove();
        if (waves.get(control)?.wave === wave) waves.delete(control);
      };
      animation.finished.then(clear, clear);
    }
    const down = (event: PointerEvent) => {
      if (event.button !== 0 || !(event.target instanceof Element)) return;
      const control = event.target.closest<HTMLElement>(pressControls);
      if (control) press(control, event.clientX, event.clientY);
      else lastTrigger = event.target.closest<HTMLElement>("button,a,summary") ?? undefined;
    };
    const key = (event: KeyboardEvent) => {
      if (event.repeat || !["Enter", " "].includes(event.key) || !(event.target instanceof Element)) return;
      const control = event.target.closest<HTMLElement>(pressControls);
      if (!control) return;
      const rect = control.getBoundingClientRect();
      press(control, rect.left + rect.width / 2, rect.top + rect.height / 2);
    };
    const focus = (event: FocusEvent) => { if (event.target instanceof Element) highlight(event.target); };
    const focusOut = (event: FocusEvent) => {
      states.forEach((state, group) => { if (!(event.relatedTarget instanceof Node) || !group.contains(event.relatedTarget)) state.hover?.hide(); });
    };
    const preference = () => { stop(); schedule(); };
    const visibility = () => { if (document.hidden) stop(); else schedule(); };
    const observer = new MutationObserver(records => {
      const selector = `${surfaces},${groups}`;
      const nodesChanged = records.some(record => record.type === "childList" && [...record.addedNodes, ...record.removedNodes].some(node => node instanceof Element && (node.matches(selector) || node.querySelector(selector))));
      if (nodesChanged) prepare();
      else if (records.some(record => record.type === "attributes" && record.target instanceof Element && record.target.matches(".nav-item,.ui-tab,.account-sections button"))) schedule();
    });
    const preferences = new MutationObserver(preference);
    prepare();
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "aria-selected", "aria-current"] });
    preferences.observe(root, { attributes: true, attributeFilter: ["data-motion", "data-transparency", "data-contrast"] });
    document.addEventListener("pointerover", enter, { passive: true });
    document.addEventListener("pointermove", move, { passive: true });
    document.addEventListener("pointerout", leave, { passive: true });
    document.addEventListener("pointerdown", down, { passive: true });
    document.addEventListener("pointerup", release, { passive: true });
    document.addEventListener("pointercancel", release, { passive: true });
    document.addEventListener("keydown", key);
    document.addEventListener("keyup", release);
    document.addEventListener("focusin", focus);
    document.addEventListener("focusout", focusOut);
    document.addEventListener("visibilitychange", visibility);
    window.addEventListener("blur", stop);
    [motion, transparency, contrast, forced, fine].forEach(query => query.addEventListener("change", preference));
    return () => {
      disposed = true;
      observer.disconnect(); preferences.disconnect(); resize.disconnect();
      cancelAnimationFrame(refreshFrame);
      stop();
      prepared.forEach((sheen, shell) => { sheen.remove(); shell.removeAttribute("data-prism-surface"); shell.style.removeProperty("--prism-popover-origin"); shell.style.removeProperty("--prism-entry-x"); });
      states.forEach((state, group) => { state.selection?.destroy(); state.hover?.destroy(); group.removeAttribute("data-prism-group"); group.removeAttribute("data-prism-selection"); });
      document.removeEventListener("pointerover", enter);
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerout", leave);
      document.removeEventListener("pointerdown", down);
      document.removeEventListener("pointerup", release);
      document.removeEventListener("pointercancel", release);
      document.removeEventListener("keydown", key);
      document.removeEventListener("keyup", release);
      document.removeEventListener("focusin", focus);
      document.removeEventListener("focusout", focusOut);
      document.removeEventListener("visibilitychange", visibility);
      window.removeEventListener("blur", stop);
      [motion, transparency, contrast, forced, fine].forEach(query => query.removeEventListener("change", preference));
    };
  }, []);
  return null;
}
