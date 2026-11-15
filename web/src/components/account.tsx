"use client";
import {
  createContext,
  useContext,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { usePathname } from "next/navigation";
import * as Dialog from "@radix-ui/react-dialog";
import {
  LogOut,
  Settings2,
  UserRound,
  ShieldCheck,
  SlidersHorizontal,
  X,
  Check,
} from "lucide-react";
import { useRemote, mutation } from "@/lib/api";
import type { Account } from "@/lib/types";
import { useUI } from "@/components/providers";
import { PreferenceFields } from "@/components/preferences-controls";
import { ErrorBox, Field, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { UnsavedChanges, confirmLeave } from "@/lib/unsaved";

const Context = createContext<ReturnType<typeof useRemote<Account>> | null>(
  null,
);
export function useAccount() {
  const value = useContext(Context);
  if (!value) throw new Error("Account provider missing");
  return value;
}
export function AccountProvider({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const account = useRemote<Account>(
    /^\/(tenants|join)(\/|$)/.test(pathname) ? "/auth/me" : null,
    60000,
  );
  return (
    <Context.Provider value={account}>
      <UnsavedChanges />
      {children}
      {account.data && (
        <AccountMenu account={account.data} update={account.setData} />
      )}
    </Context.Provider>
  );
}
function AccountMenu({
  account,
  update,
}: {
  account: Account;
  update: (account: Account) => void;
}) {
  const { t } = useUI();
  const [menu, setMenu] = useState(false);
  const [panel, setPanel] = useState(false);
  const [section, setSection] = useState("profile");
  const [saved, setSaved] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const sections = [
    { id: "profile", icon: UserRound },
    { id: "security", icon: ShieldCheck },
    { id: "preferences", icon: SlidersHorizontal },
  ];
  function open(section: string) {
    setSection(section);
    setMenu(false);
    setSaved(false);
    setPanel(true);
  }
  return (
    <>
      <div className="account-anchor">
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
                    '[role="menuitem"]',
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
              <button role="menuitem" autoFocus onClick={() => open("profile")}>
                <UserRound />
                {t("account.profile")}
              </button>
              <button role="menuitem" onClick={() => open("preferences")}>
                <Settings2 />
                {t("account.settings")}
              </button>
              <button
                role="menuitem"
                onClick={async () => {
                  if (!confirmLeave()) return;
                  await mutation("/auth/logout", {});
                  window.location.assign("/login");
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
          onClick={() => setMenu(!menu)}
        >
          <span className="account-avatar" aria-hidden="true">
            {Array.from(account.display_name).slice(0, 2).join("")}
          </span>
          <span className="account-trigger-name">{account.display_name}</span>
          <Settings2 size={15} />
        </button>
      </div>
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
            className="account-panel"
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              trigger.current?.focus();
            }}
          >
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
                    setSaved(false);
                  }}
                >
                  <item.icon size={16} />
                  {t(`account.${item.id}`)}
                </button>
              ))}
            </nav>
            <div className="account-panel-content" key={section}>
              {saved && (
                <p
                  className="notice mb-5 flex items-center gap-2"
                  role="status"
                >
                  <Check size={16} />
                  {t("account.saved")}
                </p>
              )}
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
                    onSubmit={async (form) => {
                      const value = await mutation<Account>(
                        "/auth/me",
                        { display_name: form.get("display_name") },
                        "PATCH",
                      );
                      update(value);
                      setSaved(true);
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
                  onSubmit={async (form, markSaved) => {
                    const password = String(form.get("new_password"));
                    if (password !== form.get("confirm_password"))
                      throw new Error(t("account.passwordMismatch"));
                    await mutation("/auth/password", {
                      current_password: form.get("current_password"),
                      new_password: password,
                    });
                    markSaved();
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
              ) : (
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
