/**
 * [INPUT]: 依赖租户 HTTP wrapper、一次性附着票据和浏览器 WebSocket。
 * [OUTPUT]: 提供单写者终端连接、消费后 ACK、原渲染器重连与显式清理。
 * [POS]: 终端数据传输边界；秘密仅在首帧发送，不持久化或记录。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { api } from "@/lib/api";
import { LocalizedError } from "@/lib/errors";
import { currentTenant, tenantHeaders } from "@/lib/tenant";
import type { TerminalClientMessage, TerminalServerMessage, TerminalStatus, TerminalTicket } from "@/lib/terminal-types";

const FRAME_BYTES = 16 * 1024;
const OUTPUT_BUDGET = 1024 * 1024;
const INPUT_BUDGET = 256 * 1024;
const RECONNECT_MS = 30_000;

export class TerminalConnectionError extends Error {
  constructor(readonly code: "protocol" | "outputGap" | "connection" | "inputLimit") {
    super(code);
  }
}

export function encodeTerminalBytes(bytes: Uint8Array): string {
  let value = "";
  for (const byte of bytes) value += String.fromCharCode(byte);
  return btoa(value);
}

function decodeBytes(value: string): Uint8Array {
  if (value.length > Math.ceil(FRAME_BYTES / 3) * 4 || !/^[A-Za-z0-9+/]*={0,2}$/.test(value))
    throw new TerminalConnectionError("protocol");
  const raw = atob(value);
  if (raw.length > FRAME_BYTES) throw new TerminalConnectionError("protocol");
  return Uint8Array.from(raw, character => character.charCodeAt(0));
}

function streamURL(path: string, session: string): string {
  const url = new URL(path, location.href);
  if (url.protocol === "https:") url.protocol = "wss:";
  if (url.protocol === "http:") url.protocol = "ws:";
  if (url.protocol !== (location.protocol === "https:" ? "wss:" : "ws:") ||
      url.host !== location.host || url.username || url.password || url.search || url.hash ||
      url.pathname !== `/api/v1/terminal-sessions/${encodeURIComponent(session)}/stream`)
    throw new TerminalConnectionError("protocol");
  return url.href;
}

function readMessage(raw: unknown): TerminalServerMessage {
  if (typeof raw !== "string" || raw.length > 64 * 1024) throw new TerminalConnectionError("protocol");
  const value = JSON.parse(raw) as Record<string, unknown>;
  if (!value || typeof value !== "object") throw new TerminalConnectionError("protocol");
  if (value.type === "output" && Number.isSafeInteger(value.offset) && Number(value.offset) >= 0 && typeof value.data === "string")
    return value as TerminalServerMessage;
  if (value.type === "status" && typeof value.state === "string" &&
      ["reason", "account", "cwd", "shell"].every(key => value[key] == null || typeof value[key] === "string")) {
    return { type: "status", state: value.state, ...Object.fromEntries(
      ["reason", "account", "cwd", "shell"].filter(key => typeof value[key] === "string").map(key => [key, value[key]]),
    ) };
  }
  if (value.type === "exit" && (value.code == null || Number.isSafeInteger(value.code)) &&
      (value.signal == null || typeof value.signal === "string") && (value.reason == null || typeof value.reason === "string"))
    return { type: "exit", code: value.code as number | null | undefined, signal: value.signal as string | null | undefined,
      reason: typeof value.reason === "string" ? value.reason : undefined };
  throw new TerminalConnectionError("protocol");
}

export class TerminalConnection {
  private socket?: WebSocket;
  private controller = new AbortController();
  private timer?: ReturnType<typeof setTimeout>;
  private stopped = false;
  private generation = 0;
  private sequence = 0;
  private offset = 0;
  private queued = 0;
  private writes = Promise.resolve();
  private deadline = 0;
  private retries = 0;
  private ready = false;
  private finishing = false;
  private scope = currentTenant();
  private headers?: Record<string, string>;
  private readonly onPageHide = () => this.close();
  private readonly onLogout = () => { this.handlers.clear(); this.close(); };

  constructor(
    private readonly session: string,
    private readonly renderer: string,
    private readonly handlers: {
      status: (status: TerminalStatus) => void;
      output: (data: Uint8Array) => Promise<void>;
      failure: (error: Error) => void;
      clear: () => void;
    },
  ) {}

  start(): void {
    window.addEventListener("pagehide", this.onPageHide);
    window.addEventListener("artifactory:session-ended", this.onLogout);
    this.handlers.status({ phase: "connecting" });
    void this.connect();
  }

  input(bytes: Uint8Array): void {
    if (!this.ready || !this.current()) return;
    const wireBytes = Math.ceil(bytes.length / 3) * 4 + Math.ceil(bytes.length / FRAME_BYTES) * 128;
    if (wireBytes + this.socket!.bufferedAmount > INPUT_BUDGET) {
      this.fail(new TerminalConnectionError("inputLimit"));
      return;
    }
    for (let at = 0; at < bytes.length; at += FRAME_BYTES)
      this.send({ type: "input", sequence: ++this.sequence, data: encodeTerminalBytes(bytes.subarray(at, at + FRAME_BYTES)) });
  }

  resize(columns: number, rows: number): void {
    if (!this.ready) return;
    this.send({ type: "resize", columns: Math.max(2, Math.min(500, columns)), rows: Math.max(2, Math.min(500, rows)) });
  }

  close(): void {
    if (this.stopped) return;
    this.send({ type: "close" });
    this.stop();
    this.handlers.status({ phase: "closed" });
    void api(`/terminal-sessions/${encodeURIComponent(this.session)}/close`, {
      method: "POST", body: "{}", headers: this.headers, keepalive: true, progress: "background",
    }).catch(() => { /* 服务端租约在清理请求失败后回收会话。 */ });
  }

  private current(): boolean {
    return !this.stopped && this.scope === currentTenant() && this.socket?.readyState === WebSocket.OPEN;
  }

  private send(message: TerminalClientMessage): void {
    if (this.current()) this.socket!.send(JSON.stringify(message));
  }

  private stop(): void {
    this.stopped = true;
    this.ready = false;
    this.generation++;
    this.controller.abort();
    clearTimeout(this.timer);
    this.socket?.close(1000);
    window.removeEventListener("pagehide", this.onPageHide);
    window.removeEventListener("artifactory:session-ended", this.onLogout);
  }

  private fail(error: unknown): void {
    if (this.stopped) return;
    this.close();
    this.handlers.clear();
    this.handlers.status({ phase: "error" });
    this.handlers.failure(error instanceof Error ? error : new TerminalConnectionError("protocol"));
  }

  private retry(): void {
    if (this.stopped || this.finishing) return;
    this.ready = false;
    if (this.scope !== currentTenant()) { this.handlers.clear(); this.close(); return; }
    this.deadline ||= Date.now() + RECONNECT_MS;
    if (Date.now() >= this.deadline) { this.fail(new TerminalConnectionError("connection")); return; }
    this.handlers.status({ phase: "reconnecting" });
    this.timer = setTimeout(() => void this.connect(), Math.min(250 * 2 ** this.retries++, 2500, this.deadline - Date.now()));
  }

  private async connect(): Promise<void> {
    const generation = ++this.generation;
    const valid = () => !this.stopped && generation === this.generation;
    try {
      await this.writes;
      this.headers ||= await tenantHeaders(this.scope, this.controller.signal);
      const grant = await api<TerminalTicket>(`/terminal-sessions/${encodeURIComponent(this.session)}/attach`, {
        method: "POST", body: JSON.stringify({ renderer_id: this.renderer, offset: this.offset }),
        signal: this.controller.signal, progress: "background",
      });
      if (!valid()) return;
      const socket = new WebSocket(streamURL(grant.stream_url, this.session));
      this.socket = socket;
      socket.onopen = () => {
        if (!valid()) { socket.close(); return; }
        this.send({ type: "authenticate", ticket: grant.ticket, renderer_id: this.renderer, offset: this.offset });
        grant.ticket = "";
      };
      socket.onmessage = event => {
        if (!valid() || this.finishing) return;
        try { this.message(readMessage(event.data), socket); }
        catch (error) { this.fail(error); }
      };
      socket.onerror = () => { /* close 包含重连所需的连接结果，不记录消息或票据。 */ };
      socket.onclose = event => {
        if (!valid() || this.finishing) return;
        this.ready = false;
        if ([1000, 1002, 1003, 1008, 1009].includes(event.code)) {
          if (event.code === 1008) {
            this.stop();
            this.handlers.clear();
            this.handlers.status({ phase: "closed", reason: event.reason || undefined });
          } else this.finish({ phase: "closed", reason: event.reason || undefined });
        } else this.retry();
      };
    } catch (error) {
      if (!valid()) return;
      if (error instanceof LocalizedError && error.key === "common.networkFailed") this.retry();
      else this.fail(error);
    }
  }

  private message(message: TerminalServerMessage, socket: WebSocket): void {
    if (message.type === "status") {
      if (["closed", "failed", "closing"].includes(message.state)) {
        this.stop();
        this.handlers.clear();
        this.handlers.status({ phase: message.state === "failed" ? "error" : "closed", ...message });
        return;
      }
      this.ready = message.state === "active";
      if (this.ready) { this.deadline = 0; this.retries = 0; }
      this.handlers.status({ ...message, phase: this.ready ? "running" : this.deadline ? "reconnecting" : "connecting" });
      return;
    }
    if (message.type === "exit") {
      if (/auth|permission|revok|session.expir|tenant/i.test(message.reason || "")) {
        this.stop();
        this.handlers.clear();
        this.handlers.status({ ...message, phase: "closed" });
      } else this.finish({ ...message, phase: "closed" });
      return;
    }
    const bytes = decodeBytes(message.data);
    this.queued += bytes.length;
    if (this.queued > OUTPUT_BUDGET) throw new TerminalConnectionError("protocol");
    this.writes = this.writes.then(async () => {
      try {
        if (this.stopped) return;
        if (message.offset > this.offset) throw new TerminalConnectionError("outputGap");
        const consumed = this.offset - message.offset;
        if (consumed < bytes.length) {
          await this.handlers.output(bytes.subarray(consumed));
          this.offset = message.offset + bytes.length;
        }
        if (this.socket === socket) this.send({ type: "ack", offset: this.offset });
      } finally { this.queued -= bytes.length; }
    }).catch(error => this.fail(error));
  }

  private finish(status: TerminalStatus): void {
    this.finishing = true;
    this.ready = false;
    clearTimeout(this.timer);
    // 已接收的最后输出先交给渲染器消费，再展示进程退出结果。
    void this.writes.then(() => {
      if (this.stopped) return;
      this.stop();
      this.handlers.status(status);
    });
  }
}
