/**
 * [INPUT]: 不依赖运行时状态。
 * [OUTPUT]: 提供语言、主题类型与持久化键。
 * [POS]: 服务端布局和客户端偏好共用的存储约定；动画没有持久化开关。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export type Locale = "zh-CN" | "en";
export type ThemeMode = "system" | "dark" | "light";
export const localeCookie = "artifactory-locale";
export const themeStorage = "artifactory-theme";
export function isLocale(value: unknown): value is Locale {
  return value === "zh-CN" || value === "en";
}
