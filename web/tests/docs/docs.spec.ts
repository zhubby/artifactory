/**
 * [INPUT]: 依赖 Playwright 与构建后的 mdBook 资产和共享 Select 交互助手。
 * [OUTPUT]: 验证公开文档的导航、主题和字体。
 * [POS]: Web 生产构建后的文档浏览器入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { chooseOption } from "../ui/select-helpers";
import { expect, test } from "@playwright/test";
import { resolve } from "node:path";
import { mkdirSync } from "node:fs";
import { fixtureAPI, tenantURL, openPreferences, closePreferences } from "../ui/fixtures";

test("source edit links point to the actual root and nested documentation files", async ({ page }) => {
  for (const [chapter, source] of [
    ["/docs/index.html", "docs/README.md"],
    ["/docs/workspaces.html", "docs/workspaces.md"],
    ["/docs/acceptance/emissary/2026-11-13.html", "docs/acceptance/emissary/2026-11-13.md"],
  ]) {
    await page.goto(chapter);
    await expect(page.locator('a[title="Suggest an edit"]')).toHaveAttribute(
      "href", `https://github.com/zhubby/artifactory/edit/main/${source}`,
    );
  }
});

test("anonymous docs routes, nested chapters, assets and 404", async ({
  request,
}) => {
  for (const path of ["/docs", "/docs/"]) {
    const response = await request.get(path);
    expect(response.status()).toBe(200);
    expect(new URL(response.url()).pathname).toBe("/docs/index.html");
    expect(await response.text()).toContain("Artifactory 文档");
  }
  const chapter = await request.get("/docs/acceptance/emissary/2026-11-13.html");
  expect(chapter.status()).toBe(200);
  expect(await chapter.text()).toContain("Rust Artifactory Emissary 验收记录");
  const workspaces = await request.get("/docs/workspaces.html");
  expect(workspaces.status()).toBe(200);
  expect(await workspaces.text()).toContain("租户与项目工作台");
  for (const [path, type] of [
    ["/docs/brand/artifactory-assembly-a.png", "image/png"],
    ["/docs/brand/artifactory-logo-kit.zip", "application/zip"],
    ["/docs/mermaid.min.js", "javascript"],
    ["/docs/docs/theme/appearance-init.js", "javascript"],
    ["/docs/fonts/inter-latin-wght-normal.woff2", "font/woff2"],
    ["/docs/fonts/ibm-plex-mono-latin-400-normal.woff2", "font/woff2"],
  ]) {
    const response = await request.get(path);
    expect(response.status()).toBe(200);
    expect(response.headers()["content-type"]).toContain(type);
    expect((await response.body()).length).toBeGreaterThan(100);
  }
  for (const path of ["/docs/missing.html", "/docs/acceptance/missing/page.html"]) {
    const response = await request.get(path);
    expect(response.status()).toBe(404);
    expect(await response.text()).toContain("页面未找到");
  }
});

test("reading, search, themes and diagrams work without the API", async ({
  page,
}, info) => {
  const failures: string[] = [];
  const apiRequests: string[] = [];
  page.on("pageerror", (error) => failures.push(error.message));
  await page.route("**/api/**", (route) => {
    apiRequests.push(route.request().url());
    return route.abort();
  });
  await page.goto("/docs");
  await expect(page.locator("main").getByRole("heading", { name: "Artifactory 文档", exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "返回控制台", exact: true })).toHaveAttribute("href", "/");
  const directory = resolve("../.local/docs-evidence", info.project.name);
  mkdirSync(directory, { recursive: true });
  await page.screenshot({ path: resolve(directory, "index.png"), fullPage: true });
  await page.locator("#search-toggle").click();
  await page.locator("#searchbar").pressSequentially("Emissary");
  await expect(page.locator("#searchresults a").first()).toBeVisible();
  await page.locator("#searchresults a").first().click();
  await expect(page).toHaveURL(/\/docs\/.*\.html/);
  await page.goto("/docs/telemetry.html");
  await expect(page.locator(".mermaid svg").first()).toBeVisible();
  for (const [name, theme] of [["Dark · 深色", "navy"], ["Light · 浅色", "light"], ["Dark · 深色", "navy"]]) {
    await page.locator("#theme-toggle").click();
    await Promise.all([
      page.waitForEvent("load"),
      page.getByRole("menuitemradio", { name, exact: true }).click(),
    ]);
    await expect(page.locator("html")).toHaveClass(new RegExp(theme));
    await expect(page.locator(".mermaid svg").first()).toBeVisible();
  }
  await page.reload();
  await expect(page.locator("html")).toHaveClass(/navy/);
  await expect(page.locator(".mermaid svg").first()).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  if (info.project.name === "mobile") {
    await page.locator("#sidebar-toggle").click();
    await expect(page.locator("#sidebar")).toBeVisible();
    await expect(page.locator("#sidebar a", { hasText: "部署与运维" }).first()).toBeVisible();
  }
  await page.screenshot({ path: resolve(directory, "telemetry.png"), fullPage: true });
  expect(apiRequests).toEqual([]);
  expect(failures).toEqual([]);
});

test("all architecture diagrams render from bundled scripts", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  for (const path of [
    "/docs/control-ha.html",
    "/docs/telemetry.html",
    "/docs/substrate-scheduling-assessment-2026-11-12.html",
    "/docs/plans/2026-11-12-001-feat-kubernetes-agent-operator-plan.html",
    "/docs/superpowers/specs/2026-11-12-kubernetes-agent-operator-design.html",
  ]) {
    await page.goto(path);
    const diagrams = page.locator(".mermaid");
    expect(await diagrams.count()).toBeGreaterThan(0);
    for (const diagram of await diagrams.all()) {
      await expect(diagram.locator("svg")).toBeVisible();
      await expect(diagram.locator(".error-icon")).toHaveCount(0);
    }
    await expect(page.locator("main")).not.toContainText("title: Kubernetes Agent Operator");
  }
  expect(errors).toEqual([]);
});

test("docs and console share explicit, system and cross-tab theme preferences", async ({ page, context }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await fixtureAPI(page);
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/docs/telemetry.html");
  await expect(page.locator("html")).toHaveClass(/navy/);
  await expect(page.locator(".mermaid svg").first()).toBeVisible();
  expect(await page.evaluate(async () => {
    await document.fonts.load('14px "Inter Variable"');
    return document.fonts.check('14px "Inter Variable"');
  })).toBe(true);
  await page.goto(tenantURL("/projects/desktop-client/builds"));
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await openPreferences(page);
  await chooseOption(page.getByRole("combobox", { name: "主题", exact: true }), "light");
  await closePreferences(page);
  await page.goto("/docs/telemetry.html");
  await expect(page.locator("html")).toHaveClass(/light/);
  await page.locator("#theme-toggle").click();
  await page.keyboard.press("Home");
  await expect(page.getByRole("menuitemradio", { name: "System · 跟随系统", exact: true })).toBeFocused();
  await Promise.all([
    page.waitForEvent("load"),
    page.keyboard.press("Enter"),
  ]);
  await expect(page.locator("html")).toHaveClass(/navy/);
  expect(await page.evaluate(() => localStorage.getItem("artifactory-theme"))).toBe("system");
  await Promise.all([
    page.waitForEvent("load"),
    page.emulateMedia({ colorScheme: "light" }),
  ]);
  await expect(page.locator("html")).toHaveClass(/light/);
  await expect(page.locator(".mermaid svg").first()).toBeVisible();
  const other = await context.newPage();
  try {
    await other.goto("/login");
    await Promise.all([
      page.waitForEvent("load"),
      other.evaluate(() => localStorage.setItem("artifactory-theme", "dark")),
    ]);
    await expect(page.locator("html")).toHaveClass(/navy/);
    await expect(page.locator(".mermaid svg").first()).toBeVisible();
  } finally {
    await other.close();
  }
  await page.goto(tenantURL("/projects/desktop-client/builds"));
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(errors).toEqual([]);
});

for (const [locale, label] of [["zh-CN", "文档"], ["en", "Documentation"]]) {
  test(`${locale}: console link opens the static book`, async ({ page, baseURL }) => {
    await fixtureAPI(page);
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: baseURL! }]);
    await page.goto(tenantURL("/"));
    if (test.info().project.name === "mobile") {
      await page.getByRole("button", { name: locale === "en" ? "Open navigation" : "打开导航" }).click();
    }
    await page.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(/\/docs\/index\.html$/);
    await expect(page.locator("main").getByRole("heading", { name: "Artifactory 文档", exact: true })).toBeVisible();
  });
}
