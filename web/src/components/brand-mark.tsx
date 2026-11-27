/**
 * [INPUT]: 依赖父元素的语义前景色。
 * [OUTPUT]: 提供固定比例的装配 A 品牌 SVG。
 * [POS]: 认证、导航和工作空间共用品牌标记。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export function BrandMark() {
  return (
    <svg
      width={24}
      height={24}
      viewBox="0 0 256 256"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M16 224 112 32H144L240 224H192L128 96 96 160H128L144 192H80L64 224Z" />
    </svg>
  );
}
