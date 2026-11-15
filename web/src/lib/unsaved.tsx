"use client";
import { useEffect } from "react";
import { useUI } from "@/components/providers";

let message = "Leave without saving your changes?";
let acceptedUntil = 0;
export function confirmLeave() {
  if (
    Date.now() < acceptedUntil ||
    !document.querySelector('form[data-dirty="true"], [data-unsaved="true"]')
  )
    return true;
  if (!window.confirm(message)) return false;
  acceptedUntil = Date.now() + 700;
  return true;
}
export function UnsavedChanges() {
  const { t } = useUI();
  message = t("workspace.unsaved");
  useEffect(() => {
    const unload = (event: BeforeUnloadEvent) => {
      if (
        document.querySelector('form[data-dirty="true"], [data-unsaved="true"]')
      ) {
        event.preventDefault();
        event.returnValue = "";
      }
    };
    const click = (event: MouseEvent) => {
      const link = (event.target as Element)?.closest?.("a[href]");
      if (
        !link ||
        event.defaultPrevented ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey ||
        event.altKey
      )
        return;
      const target = new URL(link.getAttribute("href")!, location.href);
      if (target.href !== location.href && !confirmLeave()) {
        event.preventDefault();
        event.stopPropagation();
      }
    };
    // The Navigation API also covers browser traversal before Next changes the page.
    const navigation = (window as Window & { navigation?: EventTarget })
      .navigation;
    const navigate = (event: Event) => {
      if (event.cancelable && !confirmLeave()) event.preventDefault();
    };
    // Older browsers only expose traversal after the URL changes. Tag our own
    // history entries so a rejected traversal can return without notifying Next.
    const push = history.pushState;
    const replace = history.replaceState;
    let index = Number(history.state?.artifactoryIndex) || 0;
    let restoring = false;
    if (!navigation) {
      replace.call(history, { ...history.state, artifactoryIndex: index }, "");
      history.pushState = function (state, title, url) {
        index += 1;
        push.call(this, { ...state, artifactoryIndex: index }, title, url);
      };
      history.replaceState = function (state, title, url) {
        replace.call(this, { ...state, artifactoryIndex: index }, title, url);
      };
    }
    const pop = (event: PopStateEvent) => {
      const destination = event.state?.artifactoryIndex;
      if (restoring) {
        restoring = false;
        event.stopImmediatePropagation();
        return;
      }
      if (typeof destination !== "number") return;
      if (destination !== index && !confirmLeave()) {
        event.stopImmediatePropagation();
        restoring = true;
        history.go(index - destination);
      } else index = destination;
    };
    window.addEventListener("beforeunload", unload);
    document.addEventListener("click", click, true);
    navigation?.addEventListener("navigate", navigate);
    if (!navigation) window.addEventListener("popstate", pop, true);
    return () => {
      window.removeEventListener("beforeunload", unload);
      document.removeEventListener("click", click, true);
      navigation?.removeEventListener("navigate", navigate);
      if (!navigation) {
        window.removeEventListener("popstate", pop, true);
        history.pushState = push;
        history.replaceState = replace;
      }
    };
  }, []);
  return null;
}
