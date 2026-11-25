/**
 * [INPUT]: 依赖设备运行版本和统一 SemVer 校验。
 * [OUTPUT]: 提供历史标签与构建元数据兼容的产品版本比较。
 * [POS]: OTA 展示的纯版本边界；服务端决定目标和升级准入。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { isReleaseVersion } from "@/lib/releases";

export function emissaryProductVersion(value: string): string | undefined {
  const unprefixed = value.replace(/^(?:emissary-v|v)/, "");
  if (unprefixed.length > 100) return;
  const parts = unprefixed.split("+");
  if (parts.length > 2 || (parts.length === 2 && !/^[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*$/.test(parts[1]))) return;
  return isReleaseVersion(parts[0]) ? parts[0] : undefined;
}

export function matchesProductVersion(value: string, productVersion: string): boolean {
  const product = emissaryProductVersion(value);
  return product !== undefined && product === productVersion;
}
