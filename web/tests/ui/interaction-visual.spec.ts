/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证切换器键盘、顶栏工具焦点顺序、窄屏布局与个人设置。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, project, tenantId, tenantSlug, tenantURL, openPreferences, openNavigation } from "./fixtures";

test("space switchers enter the last result on ArrowUp from search", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client"));
  for (const kind of ["tenant", "project"]) {
    if (kind === "project") await openNavigation(page);
    const selector = page.locator(`.space-selector.${kind}`);
    const summary = selector.locator("summary");
    await summary.click();
    const search = selector.getByRole("textbox");
    const results = selector.locator(".space-selector-options a");
    await expect(search).toBeFocused();
    await search.press("ArrowUp");
    await expect(results.last()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(results.first()).toBeFocused();
    await page.keyboard.press("End");
    await expect(results.last()).toBeFocused();
    await page.keyboard.press("Home");
    await expect(results.first()).toBeFocused();
    await search.fill("no-matching-space");
    await search.press("Home");
    await expect(search).toBeFocused();
    await search.press("ArrowUp");
    if (kind === "tenant") {
      await expect(selector.getByRole("link", { name: "查看全部租户", exact: true })).toBeFocused();
    } else {
      await expect(results).toHaveCount(0);
      await expect(search).toBeFocused();
    }
    await page.keyboard.press("Escape");
    await expect(summary).toBeFocused();
  }
});

test("short viewports keep the bottom of navigation reachable by scrolling", async ({ page }, info) => {
  await page.setViewportSize({ width: info.project.name === "desktop" ? 1280 : 844, height: 390 });
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client"));
  if (page.viewportSize()!.width < 1024)
    await page.getByRole("button", { name: "打开导航", exact: true }).click();
  const sidebar = page.locator(".sidebar");
  const settings = sidebar.getByRole("link", { name: "项目设置", exact: true });
  await sidebar.getByRole("link", { name: "项目概览", exact: true }).hover();
  await page.mouse.wheel(0, 900);
  await expect(settings).toBeInViewport({ ratio: 1 });
  await settings.focus();
  await expect(settings).toBeFocused();
  await expect(sidebar.getByRole("link", { name: "文档", exact: true })).toHaveCount(0);
  if (page.viewportSize()!.width < 1024) {
    await page.keyboard.press("Escape");
    await expect(page.getByRole("button", { name: "打开导航", exact: true })).toBeFocused();
  }
});

test("personal settings keep their close control and form inside short viewports", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client"));
  for (const viewport of [{ width: 844, height: 390 }, { width: 390, height: 320 }]) {
    await page.setViewportSize(viewport);
    await openPreferences(page);
    const panel = page.locator(".account-panel");
    const close = panel.locator("header button");
    await expect(close).toBeInViewport({ ratio: 1 });
    const bounds = await panel.boundingBox();
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height);
    await panel.getByRole("button", { name: "安全", exact: true }).click();
    const submit = panel.getByRole("button", { name: "修改密码并退出全部会话", exact: true });
    await submit.scrollIntoViewIfNeeded();
    await expect(submit).toBeInViewport({ ratio: 1 });
    await close.click();
    await expect(page.locator(".account-trigger")).toBeFocused();
  }
});

test("short settings sidebars keep lifecycle links reachable and docs in topbar", async ({ page }, info) => {
  await page.setViewportSize({ width: info.project.name === "desktop" ? 1280 : 844, height: 390 });
  await fixtureAPI(page);
  for (const scope of ["", "/projects/desktop-client"]) {
    await page.goto(tenantURL(`${scope}/settings/general`));
    await openNavigation(page);
    const sidebar = page.locator(".sidebar");
    const navigation = sidebar.getByRole("navigation", { name: "管理分区", exact: true });
    const lifecycle = navigation.getByRole("link", { name: "归档与删除", exact: true });
    await lifecycle.scrollIntoViewIfNeeded();
    await expect(lifecycle).toBeInViewport({ ratio: 1 });
    await expect(sidebar.getByRole("link", { name: "文档", exact: true })).toHaveCount(0);
    if (page.viewportSize()!.width < 1024) await page.keyboard.press("Escape");
    await expect(page.locator(".workspace-topbar").getByRole("link", { name: "文档", exact: true })).toBeInViewport({ ratio: 1 });
    await expect(page.locator(".management-tabs")).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  }
});

test("long space menus keep their last result reachable in landscape", async ({ page }, info) => {
  await page.setViewportSize({ width: info.project.name === "desktop" ? 1280 : 844, height: 390 });
  await fixtureAPI(page);
  await page.route("**/api/v1/projects", route => route.fulfill({ json: [project,
    ...Array.from({ length: 12 }, (_, index) => ({ ...project, id: `extra-${index}`, slug: `extra-${index}`, name: `Extra project ${index}` })),
  ] }));
  await page.goto(tenantURL("/projects/desktop-client"));
  if (page.viewportSize()!.width < 1024)
    await page.getByRole("button", { name: "打开导航", exact: true }).click();
  const selector = page.locator(".space-selector.project");
  await selector.locator("summary").click();
  await expect(selector.getByRole("textbox")).toBeFocused();
  await selector.getByRole("textbox").press("ArrowUp");
  const lastProject = selector.locator(".space-option").last();
  await expect(lastProject).toContainText("Extra project 11");
  await expect(lastProject).toBeFocused();
  await expect(lastProject).toBeInViewport({ ratio: 1 });
  await expect(selector.getByRole("link", { name: "全部项目", exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(selector.locator("summary")).toBeFocused();
});

test("long project names leave quick navigation labels readable and clickable", async ({ page }) => {
  const events = await fixtureAPI(page);
  const longProject = { ...project, name: "Artifactory跨平台客户端与交付流水线".repeat(5) };
  await page.route("**/api/v1/projects", route => route.fulfill({ json: [longProject] }));
  await page.route("**/api/v1/projects/by-slug/desktop-client", route => route.fulfill({ json: longProject }));
  await page.goto(tenantURL("/projects/desktop-client"));
  await page.getByRole("button", { name: "快捷导航", exact: true }).click();
  await page.getByRole("combobox", { name: "搜索项目或页面", exact: true }).fill("新建 Release");
  const option = page.getByRole("option");
  await expect(option).toHaveCount(1);
  const size = await option.evaluate(element => ({
    width: element.clientWidth,
    content: element.scrollWidth,
    labelWidth: element.querySelector("span")!.getBoundingClientRect().width,
  }));
  expect(size.content).toBeLessThanOrEqual(size.width + 1);
  expect(size.labelWidth).toBeGreaterThanOrEqual(48);
  await option.click();
  await expect(page).toHaveURL(tenantURL("/projects/desktop-client/releases/new"));
  await expect(page.locator('input[name="version"]')).toBeVisible();
  expect(events.writes).toEqual([]);
});

test("320px settings topbars keep long scope names and navigation controls usable", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.setViewportSize({ width: 320, height: 640 });
  const tenant = {
    id: tenantId, slug: tenantSlug, name: "跨平台构建与交付团队".repeat(8),
    role: "owner", owner_id: "user-main", status: "active", archived_at: null,
    created_at: "2026-11-15T00:00:00Z",
  };
  const longProject = { ...project, name: "客户端构建与制品交付".repeat(8) };
  await page.route("**/api/v1/tenants", route => route.fulfill({ json: [tenant] }));
  await page.route(`**/api/v1/tenants/${tenantSlug}`, route => route.fulfill({ json: tenant }));
  await page.route("**/api/v1/tenant", route => route.fulfill({ json: {
    tenant, project_roles: { [project.id]: "admin" },
  } }));
  await page.route("**/api/v1/projects", route => route.fulfill({ json: [longProject] }));
  await page.route(`**/api/v1/projects/by-slug/${project.slug}`, route => route.fulfill({ json: longProject }));
  for (const path of ["/settings/credentials", `/projects/${project.slug}/settings/resources`]) {
    await page.goto(tenantURL(path));
    const selector = page.locator(".workspace-breadcrumb > .space-selector.tenant");
    const summary = selector.locator("summary");
    const toggle = page.getByRole("button", { name: "打开导航", exact: true });
    await expect(summary).toContainText(tenant.name);
    await expect(summary).toBeInViewport({ ratio: 1 });
    await expect(toggle).toBeInViewport({ ratio: 1 });
    const [tenantBounds, toggleBounds] = await Promise.all([summary.boundingBox(), toggle.boundingBox()]);
    expect(tenantBounds).not.toBeNull();
    expect(toggleBounds).not.toBeNull();
    const overlap = tenantBounds!.x < toggleBounds!.x + toggleBounds!.width
      && tenantBounds!.x + tenantBounds!.width > toggleBounds!.x
      && tenantBounds!.y < toggleBounds!.y + toggleBounds!.height
      && tenantBounds!.y + tenantBounds!.height > toggleBounds!.y;
    expect(overlap).toBe(false);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await summary.click();
    await expect(selector.getByRole("textbox", { name: "搜索租户", exact: true })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(summary).toBeFocused();
    await toggle.click();
    const drawer = page.getByRole("dialog", { name: "主导航", exact: true });
    await expect(drawer.getByRole("navigation", { name: "管理分区", exact: true })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(toggle).toBeFocused();
    await expect(page.locator(".workspace")).not.toHaveAttribute("inert", "");
  }
  expect(events.writes).toEqual([]);
});

for (const locale of ["zh-CN", "en"]) {
  test(`${locale}: project names report the UTF-8 limit locally and retain an editable draft`, async ({ page }, info) => {
    const events = await fixtureAPI(page);
    await page.context().addCookies([{
      name: "artifactory-locale", value: locale,
      url: String(info.project.use.baseURL),
    }]);
    await page.goto(tenantURL("/projects"));
    await page.getByRole("button", { name: locale === "en" ? "Connect project" : "接入项目", exact: true }).click();
    const dialog = page.getByRole("dialog");
    const name = dialog.locator('input[name="name"]');
    await name.fill("项".repeat(34)); // 34 characters, 102 UTF-8 bytes.
    await dialog.locator('input[name="repository_url"]').fill("https://git.example.com/team/app.git");
    await dialog.getByRole("button", { name: locale === "en" ? "Save" : "保存", exact: true }).click();
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(locale === "en"
      ? "Project name is too long. Shorten it and try again."
      : "项目名称过长，请缩短后重试。");
    expect(events.writes).toEqual([]);
    await expect(name).toHaveValue("项".repeat(34));
    await expect(name).toBeFocused();
    const requests: unknown[] = [];
    await page.route("**/api/v1/projects", route => {
      if (route.request().method() !== "POST") return route.fallback();
      const body = route.request().postDataJSON();
      requests.push(body);
      return route.fulfill({ json: { ...project, name: body.name } });
    });
    await name.fill("项".repeat(33) + "A"); // Exactly 100 UTF-8 bytes.
    await dialog.getByRole("button", { name: locale === "en" ? "Save" : "保存", exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({ name: "项".repeat(33) + "A" });
  });
}
