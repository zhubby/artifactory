/**
 * [INPUT]: 依赖功能层 DOM、路由和系统减少动态效果偏好。
 * [OUTPUT]: 提供无常驻帧循环的玻璃流光与连续悬停胶囊。
 * [POS]: 根上下文的装饰增强；不改变业务节点和命中区域。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect } from "react";
import { usePathname } from "next/navigation";

export function PrismInteractions() {
  const pathname = usePathname();
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const fine = window.matchMedia("(hover: hover) and (pointer: fine)");
    const shells = ".sidebar,.workspace-topbar";
    const groups = ".sidebar-navigation nav,.workspace-settings-group,.ui-tabs,.header-tools";
    const added = new Set<HTMLElement>();
    const pending = new Map<HTMLElement, ReturnType<typeof setTimeout>>();
    let observer: MutationObserver;
    const prepare = () => {
      document.querySelectorAll<HTMLElement>(shells).forEach(shell => {
        if (shell.querySelector(":scope > .prism-sheen")) return;
        const sheen = document.createElement("span");
        sheen.className = "prism-sheen";
        sheen.setAttribute("aria-hidden", "true");
        shell.prepend(sheen);
        added.add(sheen);
      });
      document.querySelectorAll<HTMLElement>(groups).forEach(group => {
        group.dataset.prismGroup = "";
      });
    };
    const enter = (event: PointerEvent) => {
      if (query.matches || !fine.matches || !(event.target instanceof Element)) return;
      const shell = event.target.closest<HTMLElement>(shells);
      if (shell && !(event.relatedTarget instanceof Node && shell.contains(event.relatedTarget))) {
        if (pending.has(shell)) clearTimeout(pending.get(shell));
        shell.removeAttribute("data-prism-active");
        requestAnimationFrame(() => { if (shell.isConnected) shell.dataset.prismActive = ""; });
        pending.set(shell, setTimeout(() => { shell.removeAttribute("data-prism-active"); pending.delete(shell); }, 700));
      }
      const item = event.target.closest<HTMLElement>(".nav-item,.ui-tab,.header-docs,.command-trigger");
      const group = item?.closest<HTMLElement>("[data-prism-group]");
      if (!item || !group || item === event.relatedTarget) return;
      let capsule = group.querySelector<HTMLElement>(":scope > .prism-hover-capsule");
      if (!capsule) {
        capsule = document.createElement("span");
        capsule.className = "prism-hover-capsule";
        capsule.setAttribute("aria-hidden", "true");
        group.prepend(capsule);
        added.add(capsule);
      }
      const bounds = group.getBoundingClientRect();
      const rect = item.getBoundingClientRect();
      capsule.style.width = `${rect.width}px`;
      capsule.style.height = `${rect.height}px`;
      capsule.style.transform = `translate(${rect.left - bounds.left + group.scrollLeft}px,${rect.top - bounds.top + group.scrollTop}px)`;
      capsule.classList.add("visible");
    };
    const leave = (event: PointerEvent) => {
      if (!(event.target instanceof Element)) return;
      const group = event.target.closest<HTMLElement>("[data-prism-group]");
      if (group && !(event.relatedTarget instanceof Node && group.contains(event.relatedTarget))) {
        group.querySelector(":scope > .prism-hover-capsule")?.classList.remove("visible");
      }
    };
    prepare();
    observer = new MutationObserver(records => {
      const selector = `${shells},${groups}`;
      if (records.some(record => Array.from(record.addedNodes).some(node =>
        node instanceof Element && (node.matches(selector) || node.querySelector(selector)),
      ))) prepare();
    });
    observer.observe(document.body, { childList: true, subtree: true });
    document.addEventListener("pointerover", enter);
    document.addEventListener("pointerout", leave);
    return () => {
      observer.disconnect();
      document.removeEventListener("pointerover", enter);
      document.removeEventListener("pointerout", leave);
      pending.forEach(clearTimeout);
      added.forEach(node => node.remove());
      document.querySelectorAll<HTMLElement>("[data-prism-group],[data-prism-active]").forEach(node => {
        node.removeAttribute("data-prism-group"); node.removeAttribute("data-prism-active");
      });
    };
  }, [pathname]);
  return null;
}
