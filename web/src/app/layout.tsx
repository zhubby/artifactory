/**
 * [INPUT]: 依赖语言 Cookie、Providers 与本地字体。
 * [OUTPUT]: 提供 RootLayout 与本地化页面元数据。
 * [POS]: App Router 根布局，建立主题和语言上下文。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Metadata } from "next";
import { cookies } from "next/headers";
import { Providers } from "@/components/providers";
import { isLocale, localeCookie } from "@/lib/preferences";
import "@fontsource-variable/inter";
import "@fontsource/ibm-plex-mono/400.css";
import "./globals.css";

export async function generateMetadata(): Promise<Metadata> {
  const english = (await cookies()).get(localeCookie)?.value === "en";
  return {
    title: english
      ? "Artifactory · Build console"
      : "Artifactory · 构建控制台",
    description: english
      ? "From source to delivery. Cross-platform builds and private distribution for trusted teams."
      : "从源码到交付。可信团队的跨平台应用构建与内部分发控制台。",
  };
}
export default async function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const saved = (await cookies()).get(localeCookie)?.value;
  const locale = isLocale(saved) ? saved : "zh-CN";
  return (
    <html lang={locale} suppressHydrationWarning>
      <body>
        <Providers initialLocale={locale}>{children}</Providers>
      </body>
    </html>
  );
}
