/**
 * [INPUT]: 依赖 Radix Slot、样式变体和 cn 工具。
 * [OUTPUT]: 提供 Button 按钮及链接组合接口。
 * [POS]: 统一业务操作的尺寸与状态样式。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const variants = cva("inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40 [&_svg]:size-4", {
  variants: {
    variant: { default: "bg-primary text-primary-foreground hover:bg-primary/90", secondary: "border border-border bg-secondary text-foreground hover:bg-accent", ghost: "text-muted-foreground hover:bg-accent hover:text-foreground", destructive: "border border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/20" },
    size: { default: "h-8 px-3 text-xs max-md:min-h-11", sm: "h-7 px-2.5 text-xs max-md:min-h-11", icon: "size-8 max-md:size-11" },
  },
  defaultVariants: { variant: "default", size: "default" },
});

export function Button({ className, variant, size, asChild = false, ...props }: React.ComponentProps<"button"> & VariantProps<typeof variants> & { asChild?: boolean }) {
  const Component = asChild ? Slot : "button";
  return <Component data-slot="button" className={cn(variants({ variant, size, className }))} {...props} />;
}
