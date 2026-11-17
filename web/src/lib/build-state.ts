/**
 * [INPUT]: 依赖公开构建状态与日志事件契约。
 * [OUTPUT]: 提供终态判断和 BuildLog 类型。
 * [POS]: 统一详情订阅与规划轮询的结束条件。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export function isBuildTerminal(status?: string) {
  return status === "succeeded" || status === "failed" || status === "canceled";
}

export interface BuildLog {
  id: number;
  attempt_id: string;
  step: string;
  text: string;
}
