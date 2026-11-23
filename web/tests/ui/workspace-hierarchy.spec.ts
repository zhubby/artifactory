/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具和共享 Select 交互助手。
 * [OUTPUT]: 验证作用域侧栏、顶栏租户切换、设置分区与草稿保护。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { chooseOption } from "./select-helpers";
import { expect, test, type Page } from "@playwright/test";
import {
  fixtureAPI,
  project,
  release,
  repositoryRefs,
  tenantURL,
  tenantId,
  tenantSlug,
  openPreferences,
  closePreferences,
  openNavigation,
} from "./fixtures";

const other = {
  ...project,
  id: "project-other",
  slug: "server-api",
  name: "Server API",
};

const betaTenant = {
  id: "22222222-2222-4222-8222-222222222222",
  slug: "beta-team",
  name: "Beta Team",
  role: "owner",
  owner_id: "user-main",
  status: "active",
  archived_at: null,
  created_at: "2026-11-15T00:00:00Z",
};

async function otherTenantAPI(page: Page) {
  const currentTenant = { ...betaTenant, id: tenantId, slug: tenantSlug, name: "Build Team" };
  await page.route("**/api/v1/tenants", route => route.fulfill({ json: [currentTenant, betaTenant] }));
  await page.route("**/api/v1/tenants/beta-team", route => route.fulfill({ json: betaTenant }));
  await page.route("**/api/v1/tenant", route => route.fulfill({ json: {
    tenant: route.request().headers()["x-artifactory-tenant"] === betaTenant.id ? betaTenant : currentTenant,
    project_roles: { [project.id]: "admin" },
  } }));
}

test("tenant and project business sidebars expose only their own scope", async ({ page }) => {
  const events = await fixtureAPI(page);
  const sidebar = page.locator(".sidebar");
  for (const [path, labels, active] of [
    ["/", ["项目", "构建节点", "租户设置", "文档"], "项目"],
    ["/nodes", ["项目", "构建节点", "租户设置", "文档"], "构建节点"],
    ["/projects/desktop-client", ["项目概览", "Release 版本", "项目设置", "文档"], "项目概览"],
    [`/projects/desktop-client/releases/${release.id}/builds/128`, ["项目概览", "Release 版本", "项目设置", "文档"], "Release 版本"],
  ] as const) {
    await page.goto(tenantURL(path));
    await expect(page.locator(".workspace-breadcrumb > .space-selector.tenant")).toBeVisible();
    await openNavigation(page);
    await expect(sidebar.locator(".space-selector.tenant")).toHaveCount(0);
    await expect(sidebar.locator(".nav-item")).toHaveText([...labels]);
    await expect(sidebar.locator('[aria-current="page"]')).toHaveText(active);
    await expect(sidebar.locator(".space-selector.project")).toHaveCount(path.startsWith("/projects/") ? 1 : 0);
    if (path.startsWith("/projects/")) {
      await expect(sidebar.locator(".sidebar-back")).toHaveText("返回租户工作台");
      await expect(sidebar.locator(".sidebar-back")).toHaveAttribute("href", tenantURL("/"));
      const selector = sidebar.locator(".space-selector.project");
      await selector.locator("summary").click();
      await expect(selector.locator("a:not(.space-option)")).toHaveCount(0);
      await page.keyboard.press("Escape");
    }
  }
  expect(events.writes).toEqual([]);
});

test("settings reuse the sidebar with canonical groups and alias selection", async ({ page }) => {
  test.setTimeout(120000);
  const events = await fixtureAPI(page);
  const tenantSections = ["general", "members", "ai", "emissary", "integrations", "signing", "tokens", "credentials", "audit", "lifecycle"];
  const projectSections = ["general", "members", "resources", "signing", "integrations", "lifecycle"];
  for (const [scope, sections] of [["", tenantSections], ["/projects/desktop-client", projectSections]] as const) {
    const aliases = scope ? [] : [["security", "tokens"], ["resources", "ai"]];
    for (const [section, canonical] of [["", "general"], ...sections.map(value => [value, value]), ...aliases]) {
      const path = `${scope}/settings${section ? `/${section}` : ""}`;
      await page.goto(tenantURL(path));
      await openNavigation(page);
      const sidebar = page.locator(".sidebar");
      const navigation = sidebar.getByRole("navigation", { name: "管理分区", exact: true });
      await expect(navigation).toBeVisible();
      const hrefs = await navigation.getByRole("link").evaluateAll(links => links.map(link => link.getAttribute("href")));
      expect(hrefs).toEqual(sections.map(value => tenantURL(`${scope}/settings/${value}`)));
      await expect(sidebar.locator('[aria-current="page"]')).toHaveCount(1);
      await expect(navigation.locator('[aria-current="page"]')).toHaveAttribute("href", tenantURL(`${scope}/settings/${canonical}`));
      await expect(sidebar.locator(".sidebar-back")).toHaveAttribute("href", tenantURL(scope || "/"));
      await expect(page.locator(".management-tabs")).toHaveCount(0);
      await expect(page.locator(".management-content nav")).toHaveCount(0);
      await expect(page).toHaveURL(tenantURL(path));
      if (scope) {
        await expect(navigation.getByRole("link", { name: "资源与凭据", exact: true })).toBeVisible();
        await expect(navigation.getByRole("link", { name: "签名默认值", exact: true })).toBeVisible();
      } else {
        await expect(navigation.getByRole("link", { name: "签名管理", exact: true })).toBeVisible();
      }
    }
    await page.locator(".sidebar-back").click();
    await expect(page).toHaveURL(tenantURL(scope || "/"));
    await expect(page.locator(".workspace")).not.toHaveAttribute("inert", "");
  }
  expect(events.writes).toEqual([]);
});

test("ordinary members retain tokens while project settings require effective administrator access", async ({ page }) => {
  await fixtureAPI(page, "viewer");
  await page.goto(tenantURL("/"));
  await openNavigation(page);
  const sidebar = page.locator(".sidebar");
  await expect(sidebar.getByRole("link", { name: "访问令牌", exact: true })).toHaveAttribute("href", tenantURL("/settings/tokens"));
  await expect(sidebar.getByRole("link", { name: "租户设置", exact: true })).toHaveCount(0);
  await sidebar.getByRole("link", { name: "访问令牌", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/settings/tokens"));
  await openNavigation(page);
  const navigation = sidebar.getByRole("navigation", { name: "管理分区", exact: true });
  await expect(navigation.getByRole("link")).toHaveText(["访问令牌"]);
  await page.goto(tenantURL("/settings"));
  await openNavigation(page);
  await expect(navigation.locator('[aria-current="page"]')).toHaveAttribute("href", tenantURL("/settings/tokens"));
  await page.goto(tenantURL("/projects/desktop-client"));
  await openNavigation(page);
  await expect(sidebar.getByRole("link", { name: "项目设置", exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole("link", { name: "访问令牌", exact: true })).toHaveCount(0);
  await expect(sidebar.getByRole("link", { name: "构建节点", exact: true })).toHaveCount(0);
  await page.route("**/api/v1/tenant", route => route.fulfill({ json: {
    tenant: { ...betaTenant, id: tenantId, slug: tenantSlug, name: "Build Team", role: "member" },
    project_roles: { [project.id]: "admin" },
  } }));
  await page.route("**/api/v1/projects", route => route.fulfill({ json: [{ ...project, effective_role: "admin" }] }));
  await page.route("**/api/v1/projects/by-slug/desktop-client", route => route.fulfill({ json: { ...project, effective_role: "admin" } }));
  await page.reload();
  await openNavigation(page);
  await expect(sidebar.getByRole("link", { name: "项目设置", exact: true })).toHaveAttribute("href", tenantURL("/projects/desktop-client/settings"));
  await expect(sidebar.getByRole("link", { name: "租户设置", exact: true })).toHaveCount(0);
});

test("the topbar tenant selector resets every workspace scope and is keyboard accessible", async ({ page }) => {
  const events = await fixtureAPI(page);
  await otherTenantAPI(page);
  for (const path of ["/", "/nodes", "/settings/signing", "/projects/desktop-client/builds", "/projects/desktop-client/settings/resources"]) {
    await page.goto(tenantURL(path));
    const selector = page.locator(".workspace-breadcrumb > .space-selector.tenant");
    await expect(selector).toBeVisible();
    await expect(page.locator(".sidebar .space-selector.tenant")).toHaveCount(0);
    await selector.locator("summary").focus();
    await page.keyboard.press("Enter");
    const search = selector.getByRole("textbox", { name: "搜索租户", exact: true });
    await expect(search).toBeFocused();
    await search.fill("Beta");
    await expect(selector.locator(".space-option")).toHaveCount(1);
    await expect(selector.getByRole("link", { name: "查看全部租户", exact: true })).toHaveAttribute("href", "/tenants");
    await expect(selector.locator('a[href*="/settings"]')).toHaveCount(0);
    await search.press("ArrowDown");
    await expect(selector.locator(".space-option")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(tenantURL("/", betaTenant.slug));
    await expect(page.getByRole("heading", { name: "Beta Team", exact: true })).toBeVisible();
    await expect(page.locator(".space-selector.project")).toHaveCount(0);
  }
  expect(events.writes).toEqual([]);
});

for (const kind of ["tenant", "project"] as const) {
  test(`${kind} switching rejects and then accepts leaving an unsaved build draft`, async ({ page }) => {
    const events = await fixtureAPI(page);
    await otherTenantAPI(page);
    await page.route("**/api/v1/projects", route => route.fulfill({ json: [project, other] }));
    await page.route("**/api/v1/projects/by-slug/server-api", route => route.fulfill({ json: other }));
    await page.goto(tenantURL("/projects/desktop-client/releases/new"));
    await page.locator('textarea[name="notes"]').fill("Keep this build draft");
    for (const accept of [false, true]) {
      if (kind === "project") await openNavigation(page);
      const selector = page.locator(`.space-selector.${kind}`);
      const search = selector.getByRole("textbox");
      if (!await selector.evaluate(element => element.hasAttribute("open"))) {
        await selector.locator("summary").click();
        await expect(search).toBeFocused();
      }
      await search.fill(kind === "tenant" ? "Beta" : "Server");
      await expect(selector.locator(".space-option")).toHaveCount(1);
      const confirmation = page.waitForEvent("dialog").then(dialog => accept ? dialog.accept() : dialog.dismiss());
      await selector.locator(".space-option").click();
      await confirmation;
      if (!accept) {
        await expect(page).toHaveURL(tenantURL("/projects/desktop-client/releases/new"));
        await expect(page.locator('textarea[name="notes"]')).toHaveValue("Keep this build draft");
      }
    }
    await expect(page).toHaveURL(kind === "tenant" ? tenantURL("/", betaTenant.slug) : tenantURL("/projects/server-api/releases"));
    await expect(page.locator(".workspace")).not.toHaveAttribute("inert", "");
    expect(events.writes).toEqual([]);
  });
}

test("searchable project switch preserves lists and resets detail and creation context", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/projects", (route) =>
    route.fulfill({ json: [project, other] }),
  );
  await page.route("**/api/v1/projects/by-slug/server-api", (route) =>
    route.fulfill({ json: other }),
  );
  await page.route("**/api/v1/projects/project-other/refs", (route) =>
    route.fulfill({ json: repositoryRefs }),
  );
  for (const section of [
    "builds",
    "artifacts",
    "releases",
    "builds/128",
    "releases/new",
    "settings",
  ]) {
    await page.goto(tenantURL(`/projects/desktop-client/${section}`));
    if (page.viewportSize()!.width < 1024) {
      await page.getByRole("button", { name: "打开导航", exact: true }).click();
    }
    await page.locator(".space-selector.project summary").click();
    const search = page.getByRole("textbox", { name: "搜索项目", exact: true });
    await expect(search).toBeFocused();
    await search.fill("Server");
    await expect(
      page.locator(".space-selector.project .space-option"),
    ).toHaveCount(1);
    await page.keyboard.press("ArrowDown");
    await expect(
      page.locator(".space-selector.project .space-option"),
    ).toBeFocused();
    await page.keyboard.press("Enter");
    const target = section === "releases/new" ? "/releases" : ["builds", "artifacts", "releases"].includes(section)
      ? `/${section}`
      : "";
    await expect(page).toHaveURL(tenantURL(`/projects/server-api${target}`));
    await expect(page.locator(".space-selector.project summary")).toContainText(
      "Server API",
    );
  }
});

test("personal overlay preserves business drafts while profile writes are account scoped", async ({
  page,
}) => {
  await fixtureAPI(page);
  let account = { id: "user-main", username: "alex", display_name: "Alex" };
  const headers: Record<string, string>[] = [];
  await page.route("**/api/v1/auth/me", (route) => {
    headers.push(route.request().headers());
    if (route.request().method() === "PATCH")
      account = { ...account, ...route.request().postDataJSON() };
    return route.fulfill({ json: account });
  });
  await page.goto(tenantURL("/projects/desktop-client/releases/new"));
  await page.locator('textarea[name="notes"]').fill("Keep the exact build goal");
  await page.locator(".account-trigger").click();
  const menu = page.getByRole("menu", { name: "个人菜单", exact: true });
  await expect(menu.getByRole("menuitem")).toHaveText(["个人设置", "退出登录"]);
  await expect(
    menu.getByRole("menuitem", { name: "个人设置", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Enter");
  const sections = page.getByRole("navigation", {
    name: "个人设置分区",
    exact: true,
  });
  await expect(sections.getByRole("button")).toHaveText([
    "个人信息",
    "安全",
    "偏好",
    "通知",
  ]);
  await expect(
    sections.getByRole("button", { name: "个人信息", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("textbox", { name: /^用户名/ })).toHaveAttribute(
    "readonly",
    "",
  );
  await page
    .getByRole("textbox", { name: "显示名称", exact: true })
    .fill("Alex Chen");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.locator('[data-notify][data-kind="success"]')).toContainText("修改已保存");
  await page.keyboard.press("Escape");
  await expect(page.locator('textarea[name="notes"]')).toHaveValue(
    "Keep the exact build goal",
  );
  await expect(page.locator(".account-trigger")).toContainText("Alex Chen");
  await openPreferences(page);
  await chooseOption(page.getByRole("combobox", { name: "界面语言", exact: true }), "en");
  await chooseOption(page.getByRole("combobox", { name: "Theme", exact: true }), "light");
  await closePreferences(page);
  await expect(page.locator('textarea[name="notes"]')).toHaveValue(
    "Keep the exact build goal",
  );
  expect(headers.every((value) => !value["x-artifactory-tenant"])).toBe(true);
});

test("wrong current password preserves the security form and business position", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/auth/password", (route) =>
    route.fulfill({
      status: 400,
      json: { error: "Current password is incorrect" },
    }),
  );
  await page.goto(tenantURL("/projects/desktop-client/builds"));
  await openPreferences(page);
  await page.getByRole("button", { name: "安全", exact: true }).click();
  await page
    .locator('input[name="current_password"]')
    .fill("incorrect-password");
  await page.locator('input[name="new_password"]').fill("new-account-password");
  await page
    .locator('input[name="confirm_password"]')
    .fill("new-account-password");
  await page
    .getByRole("button", { name: "修改密码并退出全部会话", exact: true })
    .click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "Current password is incorrect",
  );
  await expect(page.locator('input[name="current_password"]')).toHaveValue(
    "incorrect-password",
  );
  await expect(page).toHaveURL(tenantURL("/projects/desktop-client/builds"));
});

for (const fallback of [false, true]) {
  test(`unsaved changes protect browser traversal (${fallback ? "history fallback" : "Navigation API"})`, async ({
    page,
  }) => {
    if (fallback)
      await page.addInitScript(() =>
        Object.defineProperty(window, "navigation", {
          value: undefined,
          configurable: true,
        }),
      );
    await fixtureAPI(page);
    await page.goto(tenantURL("/projects/desktop-client"));
    await page.getByRole("link", { name: "新建 Release", exact: true }).click();
    await page.locator('textarea[name="notes"]').fill("Unsaved build goal");
    const rejected = page
      .waitForEvent("dialog")
      .then((dialog) => dialog.dismiss());
    await page.evaluate(() => history.back());
    await rejected;
    await expect(page).toHaveURL(
      tenantURL("/projects/desktop-client/releases/new"),
    );
    await expect(page.locator('textarea[name="notes"]')).toHaveValue(
      "Unsaved build goal",
    );
    const accepted = page
      .waitForEvent("dialog")
      .then((dialog) => dialog.accept());
    await page.evaluate(() => history.back());
    await accepted;
    await expect(page).toHaveURL(tenantURL("/projects/desktop-client"));
  });
}

test("navigation drawer traps focus, closes on Escape and returns focus", async ({
  page,
}, info) => {
  test.skip(
    info.project.name === "desktop",
    "desktop has a persistent sidebar",
  );
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client"));
  const trigger = page.getByRole("button", { name: "打开导航", exact: true });
  await trigger.click();
  const drawer = page.getByRole("dialog", { name: "主导航", exact: true });
  await expect(drawer).toBeVisible();
  const first = drawer.getByRole("button", { name: "关闭导航", exact: true });
  await first.focus();
  await page.keyboard.press("Shift+Tab");
  await expect(drawer.getByRole("link", { name: /文档/ })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(first).toBeFocused();
  const switcher = drawer.locator(".space-selector.project summary");
  await switcher.click();
  await expect(drawer.getByRole("textbox", { name: "搜索项目", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(switcher).toBeFocused();
  await expect(drawer).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await expect(page.locator(".workspace")).not.toHaveAttribute("inert", "");
  await trigger.click();
  await drawer.getByRole("link", { name: "Release 版本", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/projects/desktop-client/releases"));
  await expect(drawer).toHaveCount(0);
  await expect(page.locator(".workspace")).not.toHaveAttribute("inert", "");
});

test("lifecycle deletion displays impact and keeps a retryable confirmation form", async ({
  page,
}) => {
  await fixtureAPI(page);
  const requests: unknown[] = [];
  await page.route("**/api/v1/projects/project-main/lifecycle", (route) => {
    if (route.request().method() === "GET")
      return route.fulfill({
        json: { projects: 1, builds: 12, artifacts: 4, devices: 0 },
      });
    requests.push(route.request().postDataJSON());
    return route.fulfill({
      status: 400,
      json: { error: "Current password is incorrect" },
    });
  });
  await page.goto(tenantURL("/projects/desktop-client/settings/lifecycle"));
  await page.getByRole("button", { name: "永久删除", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("构建: 12");
  await dialog.locator('input[name="slug"]').fill("desktop-client");
  await dialog
    .locator('input[name="current_password"]')
    .fill("incorrect-password");
  await dialog.getByRole("button", { name: "永久删除", exact: true }).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "Current password is incorrect",
  );
  await expect(dialog.locator('input[name="slug"]')).toHaveValue(
    "desktop-client",
  );
  expect(requests).toEqual([
    {
      action: "delete",
      slug: "desktop-client",
      current_password: "incorrect-password",
      idempotency_key: expect.any(String),
    },
  ]);
});
