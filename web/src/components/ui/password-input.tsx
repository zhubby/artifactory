/**
 * [INPUT]: 依赖 Input、语言上下文和 React 输入属性。
 * [OUTPUT]: 提供带键盘显隐按钮的 PasswordInput，保留真实字段属性。
 * [POS]: 认证表单共用密码控件；显隐只切换输入类型，不改提交值。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useId, useState, type ComponentProps } from "react";
import { Eye, EyeOff, LockKeyhole } from "lucide-react";
import { Input } from "@/components/ui/input";
import { useUI } from "@/components/providers";
import { cn } from "@/lib/utils";

export function PasswordInput({ id, className, disabled, ...props }: Omit<ComponentProps<"input">, "type">) {
  const generatedId = useId();
  const inputId = id || generatedId;
  const [visible, setVisible] = useState(false);
  const { t } = useUI();

  return (
    <div className="password-input relative">
      <LockKeyhole className="password-input-lock pointer-events-none absolute left-4 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
      <Input {...props} id={inputId} disabled={disabled} type={visible ? "text" : "password"} className={cn("pl-11 pr-14", className)} />
      <button
        type="button"
        className="password-input-toggle absolute right-1 top-1/2 grid size-11 -translate-y-1/2 place-items-center rounded-xl text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-40"
        disabled={disabled}
        aria-label={t(visible ? "prismAuth.hidePassword" : "prismAuth.showPassword")}
        aria-pressed={visible}
        aria-controls={inputId}
        onClick={() => setVisible((value) => !value)}
      >
        {visible ? <EyeOff size={18} aria-hidden="true" /> : <Eye size={18} aria-hidden="true" />}
      </button>
    </div>
  );
}
