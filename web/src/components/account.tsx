/**
 * [INPUT]: 依赖账户 API、顶栏与侧栏插槽、Radix 弹窗和未保存保护。
 * [OUTPUT]: 提供 AccountProvider、useAccount 与 AccountSlot。
 * [POS]: 单实例菜单在插槽间移动；记录宿主焦点以恢复断点隐藏造成的失焦。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import {
  createContext,
  useCallback,
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { usePathname } from "next/navigation";
import * as Dialog from "@radix-ui/react-dialog";
import {
  LogOut,
  Settings2,
  UserRound,
  ShieldCheck,
  SlidersHorizontal,
  X,
  Bell,
} from "lucide-react";
import { useRemote, mutation } from "@/lib/api";
import type { Account } from "@/lib/types";
import { useUI } from "@/components/providers";
import { PreferenceFields } from "@/components/preferences-controls";
import { ErrorBox, Field } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { UnsavedChanges, confirmLeave } from "@/lib/unsaved";
import { tenantFromPath } from "@/lib/routes";
import { asError } from "@/lib/errors";
import { isNotificationTarget, notify } from "@/lib/notify";
import { NotificationProvider } from "@/components/notification-provider";
import { NotificationSettings } from "@/components/notification-settings";
import { NotificationFocusBranch } from "@/components/notifications";

const Context = createContext<ReturnType<typeof useRemote<Account>> | null>(
  null,
);
type AccountPlacement = "header" | "sidebar";
type AccountSlots = Record<AccountPlacement, HTMLDivElement | null>;
const PlacementContext = createContext<((placement: AccountPlacement, element: HTMLDivElement | null) => void) | null>(null);

export function AccountSlot({ placement }: { placement: AccountPlacement }) {
  const register = useContext(PlacementContext);
  const ref = useCallback((element: HTMLDivElement | null) => register?.(placement, element), [placement, register]);
  return <div ref={ref} className={`account-slot account-slot-${placement}`} />;
}
export function useAccount() {
  const value = useContext(Context);
  if (!value) throw new Error("Account provider missing");
  return value;
}
export function AccountProvider({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [slots, setSlots] = useState<AccountSlots>({ header: null, sidebar: null });
  const register = useCallback((placement: AccountPlacement, element: HTMLDivElement | null) => {
    setSlots((current) => current[placement] === element ? current : { ...current, [placement]: element });
  }, []);
  const account = useRemote<Account>(
    /^\/(tenants|join|notifications)(\/|$)/.test(pathname) &&
    !/^\/notifications\/(verify|unsubscribe|email)(\/|$)/.test(pathname)
      ? "/auth/me"
      : null,
    60000,
  );
  return (
    <Context.Provider value={account}>
      <PlacementContext.Provider value={register}>
        <UnsavedChanges />
        <NotificationProvider>{children}</NotificationProvider>
        {account.data && (
          <AccountMenu account={account.data} update={account.setData} slots={slots} />
        )}
      </PlacementContext.Provider>
    </Context.Provider>
  );
}
function AccountMenu({
  account,
  update,
  slots,
}: {
  account: Account;
  update: (account: Account) => void;
  slots: AccountSlots;
}) {
  const { t } = useUI();
  const pathname = usePathname();
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  const [compact, setCompact] = useState(false);
  const lastFocused = useRef<HTMLElement | null>(null);
  const sidebar = !!tenantFromPath(pathname) && !compact && !!slots.sidebar;
  useLayoutEffect(() => {
    const element = document.createElement("div");
    setHost(element);
    const media = window.matchMedia("(max-width: 1023px)");
    const sync = () => setCompact(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => { media.removeEventListener("change", sync); element.remove(); };
  }, []);
  useLayoutEffect(() => {
    if (!host) return;
    const target = sidebar ? slots.sidebar : slots.header;
    host.className = target ? "account-host" : "account-host account-fallback";
    const active = document.activeElement;
    const focused = host.contains(active) ? active as HTMLElement
      : active === document.body && lastFocused.current && host.contains(lastFocused.current)
        ? lastFocused.current : null;
    if (host.parentElement !== (target || document.body)) (target || document.body).appendChild(host);
    focused?.focus({ preventScroll: true });
  }, [host, sidebar, slots]);
  const [menu, setMenu] = useState(false);
  const [panel, setPanel] = useState(false);
  const [section, setSection] = useState("profile");
  const [loggingOut, setLoggingOut] = useState(false);
  const [logoutError, setLogoutError] = useState<Error>();
  const trigger = useRef<HTMLButtonElement>(null);
  const sections = [
    { id: "profile", icon: UserRound },
    { id: "security", icon: ShieldCheck },
    { id: "preferences", icon: SlidersHorizontal },
    { id: "notifications", icon: Bell },
  ];
  function open() {
    setSection("profile");
    setMenu(false);
    setPanel(true);
  }
  return (
    <>
      <ErrorBox message={logoutError} />
      {host && createPortal(<div className={cn("account-anchor", sidebar ? "workspace-account" : "global-account")}
        onFocusCapture={(event) => { lastFocused.current = event.target; }}
        onBlurCapture={(event) => {
          if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) lastFocused.current = null;
        }}>
        {menu && (
          <>
            <button
              className="account-dismiss"
              aria-label={t("dialog.close")}
              onClick={() => setMenu(false)}
            />
            <div
              className="account-menu"
              role="menu"
              aria-label={t("account.menu")}
              onKeyDown={(event) => {
                const items = Array.from(
                  event.currentTarget.querySelectorAll<HTMLButtonElement>(
                    '[role="menuitem"]:not(:disabled)',
                  ),
                );
                const index = items.indexOf(
                  document.activeElement as HTMLButtonElement,
                );
                if (event.key === "Escape") {
                  setMenu(false);
                  trigger.current?.focus();
                }
                if (event.key === "Tab") setMenu(false);
                const next =
                  event.key === "ArrowDown"
                    ? (index + 1) % items.length
                    : event.key === "ArrowUp"
                      ? (index + items.length - 1) % items.length
                      : event.key === "Home"
                        ? 0
                        : event.key === "End"
                          ? items.length - 1
                          : undefined;
                if (next !== undefined) {
                  event.preventDefault();
                  items[next]?.focus();
                }
              }}
            >
              <p className="account-menu-identity">
                <strong>{account.display_name}</strong>
                <span>@{account.username}</span>
              </p>
              <button role="menuitem" autoFocus onClick={open}>
                <Settings2 />
                {t("account.settings")}
              </button>
              <button
                role="menuitem"
                disabled={loggingOut}
                onClick={async () => {
                  if (loggingOut || !confirmLeave()) return;
                  setLoggingOut(true);
                  setLogoutError(undefined);
                  try {
                    await mutation("/auth/logout", {});
                    notify.success(t("common.operationSucceeded"), { persist: true });
                    window.location.assign("/login");
                  } catch (error) {
                    setLogoutError(asError(error));
                  } finally {
                    setLoggingOut(false);
                  }
                }}
              >
                <LogOut />
                {t("navigation.logout")}
              </button>
            </div>
          </>
        )}
        <button
          ref={trigger}
          className="account-trigger"
          aria-label={t("account.menu")}
          aria-haspopup="menu"
          aria-expanded={menu}
          onClick={() => {
            setLogoutError(undefined);
            setMenu(!menu);
          }}
        >
          <span className="account-avatar" aria-hidden="true">
            {Array.from(account.display_name).slice(0, 2).join("")}
          </span>
          <span className="account-trigger-name">{account.display_name}</span>
          <Settings2 size={15} />
        </button>
      </div>, host)}
      <Dialog.Root
        open={panel}
        onOpenChange={(value) => {
          if (
            value ||
            !document.querySelector('.account-panel form[data-dirty="true"]') ||
            window.confirm(t("workspace.unsaved"))
          )
            setPanel(value);
        }}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="account-overlay" />
          <Dialog.Content
            aria-modal="true"
            className="account-panel"
            onInteractOutside={(event) => {
              if (isNotificationTarget(event.target)) event.preventDefault();
            }}
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              trigger.current?.focus();
            }}
          >
            <NotificationFocusBranch />
            <header>
              <div>
                <p className="eyebrow">ARTIFACTORY ACCOUNT</p>
                <Dialog.Title>{t("account.settings")}</Dialog.Title>
                <Dialog.Description>
                  {t("account.description")}
                </Dialog.Description>
              </div>
              <Dialog.Close asChild>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t("dialog.close")}
                >
                  <X />
                </Button>
              </Dialog.Close>
            </header>
            <nav
              className="account-sections"
              aria-label={t("account.sections")}
            >
              {sections.map((item) => (
                <button
                  key={item.id}
                  aria-current={section === item.id ? "page" : undefined}
                  className={cn(section === item.id && "active")}
                  onClick={() => {
                    if (section === item.id) return;
                    if (
                      document.querySelector(
                        '.account-panel form[data-dirty="true"]',
                      ) &&
                      !window.confirm(t("workspace.unsaved"))
                    )
                      return;
                    setSection(item.id);
                  }}
                >
                  <item.icon size={16} />
                  {t(`account.${item.id}`)}
                </button>
              ))}
            </nav>
            <div className="account-panel-content" key={section}>
              {section === "profile" ? (
                <>
                  <div className="account-profile-heading">
                    <span className="account-avatar large">
                      {Array.from(account.display_name).slice(0, 2).join("")}
                    </span>
                    <div>
                      <h3>{account.display_name}</h3>
                      <p>@{account.username}</p>
                    </div>
                  </div>
                  <SubmitForm
                    successMessage={t("account.saved")}
                    onSubmit={async (form) => {
                      const value = await mutation<Account>(
                        "/auth/me",
                        { display_name: form.get("display_name") },
                        "PATCH",
                      );
                      update(value);
                    }}
                  >
                    <Field label={t("account.displayName")}>
                      <Input
                        name="display_name"
                        required
                        maxLength={100}
                        defaultValue={account.display_name}
                        autoComplete="name"
                      />
                    </Field>
                    <Field
                      label={t("login.username")}
                      hint={t("account.usernameReadonly")}
                    >
                      <Input
                        value={account.username}
                        readOnly
                        autoComplete="username"
                      />
                    </Field>
                  </SubmitForm>
                </>
              ) : section === "security" ? (
                <SubmitForm
                  label={t("account.changePassword")}
                  successMessage={false}
                  onSubmit={async (form, markSaved) => {
                    const password = String(form.get("new_password"));
                    if (password !== form.get("confirm_password"))
                      throw new Error(t("account.passwordMismatch"));
                    await mutation("/auth/password", {
                      current_password: form.get("current_password"),
                      new_password: password,
                    });
                    markSaved();
                    notify.success(t("common.operationSucceeded"), { persist: true });
                    window.location.assign("/login");
                  }}
                >
                  <p className="notice">{t("account.passwordNotice")}</p>
                  <Field label={t("account.currentPassword")}>
                    <Input
                      name="current_password"
                      type="password"
                      required
                      maxLength={256}
                      autoComplete="current-password"
                    />
                  </Field>
                  <Field label={t("account.newPassword")}>
                    <Input
                      name="new_password"
                      type="password"
                      required
                      minLength={12}
                      maxLength={256}
                      autoComplete="new-password"
                    />
                  </Field>
                  <Field label={t("account.confirmPassword")}>
                    <Input
                      name="confirm_password"
                      type="password"
                      required
                      minLength={12}
                      maxLength={256}
                      autoComplete="new-password"
                    />
                  </Field>
                </SubmitForm>
              ) : section === "notifications" ? <NotificationSettings /> : (
                <div>
                  <h3 className="mb-3 text-lg">{t("account.preferences")}</h3>
                  <p className="mb-7 text-sm text-muted-foreground">
                    {t("preferences.description")}
                  </p>
                  <PreferenceFields expanded />
                </div>
              )}
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}
