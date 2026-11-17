/**
 * [INPUT]: 依赖 Radix Dialog、UI 词条及 cn 工具。
 * [OUTPUT]: 提供 Modal 弹窗与侧面板。
 * [POS]: 统一焦点约束、关闭恢复和入场布局。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useUI } from "@/components/providers";

import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { createContext, useContext, useRef, type ReactNode, type RefObject } from "react";
import { cn } from "@/lib/utils";

const modalClosers = new WeakMap<HTMLElement, () => void>();
const ModalDepth = createContext(0);

export function Modal({
  open,
  onOpenChange,
  title,
  description,
  children,
  size = "default",
  navigation,
  placement = "center",
  initialFocus,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description: string;
  children: ReactNode;
  size?: "default" | "wide";
  navigation?: ReactNode;
  placement?: "center" | "side";
  initialFocus?: RefObject<HTMLElement | null>;
}) {
  const { t } = useUI();
  const depth = useContext(ModalDepth);
  const previousFocus = useRef<HTMLElement | null>(null);
  const content = useRef<HTMLElement | null>(null);
  function change(value: boolean) {
    if (
      !value &&
      content.current?.querySelector(
        'form[data-dirty="true"], [data-unsaved="true"]',
      ) &&
      !window.confirm(t("workspace.unsaved"))
    )
      return;
    onOpenChange(value);
  }
  return (
    <ModalDepth.Provider value={depth + 1}>
      <Dialog.Root open={open} onOpenChange={change}>
        <Dialog.Portal>
          <Dialog.Overlay
            className="modal-overlay"
            style={{ zIndex: 40 + depth * 20 }}
          />
          <Dialog.Content
            aria-modal="true"
            data-app-modal=""
            style={{ zIndex: 50 + depth * 20 }}
            ref={(element) => {
              content.current = element;
              if (element) modalClosers.set(element, () => change(false));
            }}
            className={cn(
              "modal-panel fixed z-50 max-h-[90dvh] w-[calc(100%-2rem)] rounded-xl border border-border bg-card shadow-2xl",
              placement === "side" ? "drawer-panel" : "top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2",
              size === "wide"
                ? "flex h-[90dvh] max-w-[960px] flex-col overflow-hidden sm:h-[min(760px,90dvh)]"
                : "max-w-xl overflow-auto p-6",
            )}
            onEscapeKeyDown={(event) => {
              // Radix registers its top layer in an effect. A fast Escape can arrive
              // after a child is visible but before that registration has finished.
              event.preventDefault();
              event.stopImmediatePropagation();
              const top = Array.from(
                document.querySelectorAll<HTMLElement>(
                  '[data-app-modal][data-state="open"]',
                ),
              ).at(-1);
              if (top) modalClosers.get(top)?.();
            }}
            onOpenAutoFocus={(event) => {
              previousFocus.current =
                document.activeElement instanceof HTMLElement
                  ? document.activeElement
                  : null;
              if (initialFocus?.current) {
                event.preventDefault();
                initialFocus.current.focus();
              }
            }}
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              if (previousFocus.current?.isConnected)
                previousFocus.current.focus();
            }}
          >
            <div
              className={cn(
                size === "wide" && "shrink-0 px-5 pt-5 pb-4 sm:px-6",
              )}
            >
              <Dialog.Title className="pr-7 text-lg font-medium">
                {title}
              </Dialog.Title>
              <Dialog.Description className="mt-2 pr-5 text-sm leading-6 break-words text-muted-foreground">
                {description}
              </Dialog.Description>
            </div>
            <Dialog.Close
              className="absolute top-3 right-3 flex size-8 items-center justify-center rounded text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring max-md:size-11"
              aria-label={t("dialog.close")}
            >
              <X size={18} />
            </Dialog.Close>
            {navigation}
            <div
              className={cn(
                size === "wide"
                  ? "min-h-0 flex-1 overflow-y-auto overscroll-contain p-5 sm:p-6"
                  : "mt-6",
              )}
            >
              {children}
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </ModalDepth.Provider>
  );
}
