/**
 * [INPUT]: 依赖服务端语言、系统动效偏好、主题和账户 Provider。
 * [OUTPUT]: 提供本地化、主题及默认开启的动画状态。
 * [POS]: 根布局的客户端上下文边界；动效仅适配系统设置，不读取浏览器动效开关。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { NextIntlClientProvider, useLocale, useTranslations } from "next-intl";
import { ThemeProvider } from "next-themes";
import {
  createContext,
  useContext,
  useEffect,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import zh from "@/messages/zh-CN.json";
import en from "@/messages/en.json";
import {
  localeCookie,
  themeStorage,
  type Locale,
} from "@/lib/preferences";
import { date as formatDate } from "@/lib/utils";
import { AccountProvider } from "@/components/account";

const PreferencesContext = createContext<{
  locale: Locale;
  setLocale: (value: Locale) => void;
  motion: boolean;
  ready: boolean;
} | null>(null);

function subscribeReduced(callback: () => void) {
  const query = window.matchMedia("(prefers-reduced-motion: reduce)");
  query.addEventListener("change", callback);
  return () => query.removeEventListener("change", callback);
}

export function Providers({
  initialLocale,
  children,
}: {
  initialLocale: Locale;
  children: ReactNode;
}) {
  const [locale, updateLocale] = useState(initialLocale);
  const [ready, setReady] = useState(false);
  const reduced = useSyncExternalStore(
    subscribeReduced,
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    () => true,
  );
  useEffect(() => {
    setReady(true);
  }, []);
  useEffect(() => {
    document.documentElement.lang = locale;
    document.title =
      locale === "en"
        ? "Artifactory · Build console"
        : "Artifactory · 构建控制台";
  }, [locale]);
  const motion = ready && !reduced;
  useEffect(() => {
    document.documentElement.dataset.motion = motion ? "on" : "off";
  }, [motion]);
  function setLocale(value: Locale) {
    updateLocale(value);
    try {
      document.cookie = `${localeCookie}=${value}; Path=/; Max-Age=31536000; SameSite=Lax${location.protocol === "https:" ? "; Secure" : ""}`;
    } catch {
      /* Keep the in-memory locale if cookie access is restricted. */
    }
  }
  return (
    <ThemeProvider
      attribute="data-theme"
      storageKey={themeStorage}
      defaultTheme="system"
      enableSystem
      disableTransitionOnChange
    >
      <PreferencesContext.Provider
        value={{
          locale,
          setLocale,
          motion,
          ready,
        }}
      >
        <NextIntlClientProvider
          locale={locale}
          messages={locale === "en" ? en : zh}
          timeZone="UTC"
        >
          <AccountProvider>{children}</AccountProvider>
        </NextIntlClientProvider>
      </PreferencesContext.Provider>
    </ThemeProvider>
  );
}

export function usePreferences() {
  const context = useContext(PreferencesContext);
  if (!context) throw new Error("Preferences require Providers");
  return context;
}

export function useUI() {
  const t = useTranslations();
  const locale = useLocale();
  return {
    t,
    locale,
    number: (value: number) => new Intl.NumberFormat(locale).format(value),
    date: (value?: string | null) => formatDate(value, locale),
  };
}
