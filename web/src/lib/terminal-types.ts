/**
 * [INPUT]: 依赖设备终端 REST 响应与浏览器会话消息。
 * [OUTPUT]: 定义能力、会话、连接状态和字节流消息。
 * [POS]: 终端请求、传输与渲染之间的前端契约。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export type TerminalCapabilities = {
  enabled: boolean;
  online: boolean;
  profiles: { id: string; label: string }[];
  account: string;
  reason?: string | null;
};

export type TerminalSession = {
  id: string;
  node_id: string;
  state: string;
};

export type TerminalTicket = {
  ticket: string;
  stream_url: string;
  expires_at?: string | number;
};

export type TerminalPhase = "idle" | "connecting" | "running" | "reconnecting" | "closed" | "error";
export type TerminalStatus = {
  phase: TerminalPhase;
  reason?: string;
  account?: string;
  cwd?: string;
  shell?: string;
  code?: number | null;
  signal?: string | null;
};

export type TerminalServerMessage =
  | { type: "status"; state: string; reason?: string; account?: string; cwd?: string; shell?: string }
  | { type: "output"; offset: number; data: string }
  | { type: "exit"; code?: number | null; signal?: string | null; reason?: string };

export type TerminalClientMessage =
  | { type: "authenticate"; ticket: string; renderer_id: string; offset: number }
  | { type: "input"; sequence: number; data: string }
  | { type: "resize"; columns: number; rows: number }
  | { type: "ack"; offset: number }
  | { type: "close" };
