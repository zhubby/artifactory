/**
 * [INPUT]: 依赖租户请求头、SSE 游标与取消信号。
 * [OUTPUT]: 提供带重连和 complete 结束语义的构建订阅。
 * [POS]: 传递日志与快照；尾部读完后停止重连。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { assertTenant, currentTenant, tenantHeaders } from "@/lib/tenant";

/** Fetch-based SSE preserves the explicit tenant header and cancels on unmount. */
export async function subscribeBuild(
  id: string,
  signal: AbortSignal,
  receive: (kind: string, data: string) => void,
  disconnected: () => void,
) {
  const tenant = currentTenant();
  let cursor = "0";
  let retryMs = 1000;
  while (!signal.aborted && currentTenant() === tenant) {
    try {
      const headers = await tenantHeaders(tenant, signal);
      assertTenant(tenant, signal);
      const response = await fetch(`/api/v1/builds/${id}/events`, {
        signal,
        credentials: "same-origin",
        cache: "no-store",
        headers: {
          ...headers,
          Accept: "text/event-stream",
          "Last-Event-ID": cursor,
        },
      });
      assertTenant(tenant, signal);
      if (!response.ok || !response.body) {
        disconnected();
        if ([400, 401, 403, 404].includes(response.status)) return;
        throw new Error("event stream unavailable");
      }
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let pending = "";
      try {
        while (!signal.aborted && currentTenant() === tenant) {
          const chunk = await reader.read();
          if (chunk.done) break;
          pending += decoder.decode(chunk.value, { stream: true });
          let boundary: RegExpExecArray | null;
          while ((boundary = /\r?\n\r?\n/.exec(pending))) {
            const frame = pending.slice(0, boundary.index);
            pending = pending.slice(boundary.index + boundary[0].length);
            let kind = "message";
            const data: string[] = [];
            for (const line of frame.split(/\r?\n/)) {
              if (line.startsWith("event:")) kind = line.slice(6).trim();
              if (line.startsWith("data:"))
                data.push(line.slice(5).replace(/^ /, ""));
              if (line.startsWith("id:") && !line.includes("\0"))
                cursor = line.slice(3).replace(/^ /, "");
              if (line.startsWith("retry:")) {
                const value = line.slice(6).trim();
                if (/^\d+$/.test(value)) {
                  const delay = Number(value);
                  if (Number.isSafeInteger(delay) && delay <= 2147483647)
                    retryMs = delay;
                }
              }
            }
            if (data.length && !signal.aborted && currentTenant() === tenant) {
              receive(kind, data.join("\n"));
              if (kind === "complete") return;
            }
          }
        }
      } finally {
        await reader.cancel().catch(() => undefined);
      }
      if (!signal.aborted && currentTenant() === tenant) disconnected();
    } catch {
      if (!signal.aborted && currentTenant() === tenant) disconnected();
    }
    if (signal.aborted || currentTenant() !== tenant) break;
    await new Promise<void>((resolve) => {
      const done = () => {
        clearTimeout(timer);
        signal.removeEventListener("abort", done);
        resolve();
      };
      const timer = setTimeout(done, retryMs);
      signal.addEventListener("abort", done, { once: true });
    });
  }
}
