/**
 * [INPUT]: 依赖普通 HTTP 可用的 Crypto.getRandomValues。
 * [OUTPUT]: 提供 UUID v4 格式的 createIdempotencyKey。
 * [POS]: 创建租户和生命周期共用请求键；表单重试复用同一键。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export function createIdempotencyKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, value => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
