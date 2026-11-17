/**
 * [INPUT]: 依赖嵌套租户页面提供的 children。
 * [OUTPUT]: 提供租户入口布局。
 * [POS]: 保持租户页面的统一外层容器。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <div className="authenticated-spaces">
      {children}
    </div>
  );
}
