/**
 * [INPUT]: 依赖前台请求及导航提供的开始与结束信号。
 * [OUTPUT]: 提供可订阅的并发计数及幂等结束函数。
 * [POS]: 请求层与顶部进度条之间的状态边界，不依赖 React。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

const pending = new Set<symbol>();
const listeners = new Set<() => void>();
function publish() {
  for (const listener of listeners) listener();
}

export function beginProgress() {
  if (typeof window === "undefined") return () => {};
  const token = Symbol();
  pending.add(token);
  publish();
  return () => {
    if (pending.delete(token)) publish();
  };
}

export function subscribeProgress(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export const getProgress = () => pending.size;
export const getServerProgress = () => 0;
