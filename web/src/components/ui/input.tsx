/**
 * [INPUT]: 依赖 React 原生输入属性和 cn 类名工具。
 * [OUTPUT]: 提供保留原生语义的 Input 与 Textarea。
 * [POS]: 业务表单的字段外观入口；不持有输入状态。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { cn } from "@/lib/utils";
import type { ComponentProps } from "react";

export function Input({ className, ...props }: ComponentProps<"input">) { return <input className={cn("input", className)} {...props} />; }
export function Textarea({ className, ...props }: ComponentProps<"textarea">) { return <textarea className={cn("input min-h-24 py-2 font-mono text-xs", className)} {...props} />; }
