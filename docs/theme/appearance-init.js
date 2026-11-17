/**
 * [INPUT]: 依赖主题偏好存储和系统配色媒体查询。
 * [OUTPUT]: 设置 mdBook 首帧主题偏好。
 * [POS]: 在 mdBook 主题选择前衔接控制台偏好。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
// Runs before mdBook selects its theme, so both surfaces share one preference.
(() => {
  if (window.artifactoryDocsPreference) return;
  let preference = "system";
  try {
    const saved = localStorage.getItem("artifactory-theme");
    if (["light", "dark", "system"].includes(saved)) preference = saved;
  } catch { /* System mode remains available without storage. */ }
  const dark = preference === "dark" || (preference === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  window.artifactoryDocsPreference = preference;
  try { localStorage.setItem("mdbook-theme", dark ? "navy" : "light"); } catch { /* Apply the resolved theme in memory too. */ }
  window.default_theme = dark ? "navy" : "light";
})();
