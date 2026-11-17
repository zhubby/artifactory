/**
 * [INPUT]: 依赖 mdBook 主题菜单与共享偏好存储。
 * [OUTPUT]: 同步主题菜单、系统主题和跨标签变更。
 * [POS]: 维持公开文档与控制台的配色一致。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
(() => {
  const root = document.documentElement;
  const menu = document.getElementById("theme-list");
  const toggle = document.getElementById("theme-toggle");
  const media = matchMedia("(prefers-color-scheme: dark)");
  let preference = window.artifactoryDocsPreference || "system";
  const menuModes = ["system", "light", "navy"];
  function updateMenu() {
    const selected = preference === "dark" ? "navy" : preference;
    for (const id of menuModes) {
      const button = document.getElementById(id);
      if (button) {
        button.setAttribute("role", "menuitemradio");
        button.setAttribute("aria-checked", String(id === selected));
        button.classList.toggle("theme-selected", id === selected);
      }
    }
  }
  const resolve = value => value === "dark" || (value === "system" && media.matches) ? "navy" : "light";
  function apply(value, persist) {
    preference = ["light", "dark", "system"].includes(value) ? value : "system";
    const theme = resolve(preference);
    const changed = !root.classList.contains(theme);
    try {
      if (persist) localStorage.setItem("artifactory-theme", preference);
      localStorage.setItem("mdbook-theme", theme);
    } catch { /* Reading and switching remain usable without persistence. */ }
    root.classList.remove("light", "rust", "coal", "navy", "ayu");
    root.classList.add(theme);
    root.style.colorScheme = theme === "light" ? "light" : "dark";
    updateMenu();
    // mdBook Mermaid refreshes on theme changes; preserve that rendering path.
    if (changed) location.reload();
  }
  if (menu && toggle) {
    const item = document.createElement("li");
    item.setAttribute("role", "none");
    const button = document.createElement("button");
    button.type = "button";
    button.id = "system";
    button.className = "theme-system";
    button.setAttribute("role", "menuitemradio");
    button.textContent = "System · 跟随系统";
    item.append(button);
    menu.prepend(item);
    document.getElementById("light").textContent = "Light · 浅色";
    document.getElementById("navy").textContent = "Dark · 深色";
    toggle.addEventListener("click", () => {
      updateMenu();
      if (toggle.getAttribute("aria-expanded") === "true") {
        document.getElementById(preference === "dark" ? "navy" : preference).focus();
      }
    });
    menu.addEventListener("click", event => {
      const target = event.target.closest("button");
      if (!target || !menuModes.includes(target.id)) return;
      const mode = target.id === "navy" ? "dark" : target.id;
      event.stopImmediatePropagation();
      menu.style.display = "none";
      toggle.setAttribute("aria-expanded", "false");
      toggle.focus();
      apply(mode, true);
    }, true);
    menu.addEventListener("keydown", event => {
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const current = menuModes.indexOf(document.activeElement.id);
      const next = event.key === "Home" ? 0 : event.key === "End" ? 2
        : (current + (event.key === "ArrowDown" ? 1 : 2)) % 3;
      document.getElementById(menuModes[next]).focus();
    }, true);
  }
  media.addEventListener("change", () => { if (preference === "system") apply("system", false); });
  window.addEventListener("storage", event => {
    if (event.key === "artifactory-theme" || event.key === null) apply(event.newValue || "system", false);
  });
  apply(preference, false);
})();
