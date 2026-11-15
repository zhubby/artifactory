import { expect, test } from "@playwright/test";
import {
  fixtureAPI,
  project,
  repositoryRefs,
  tenantURL,
  openPreferences,
  closePreferences,
} from "./fixtures";

const other = {
  ...project,
  id: "project-other",
  slug: "server-api",
  name: "Server API",
};

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
    "builds/new",
    "settings",
  ]) {
    await page.goto(tenantURL(`/projects/desktop-client/${section}`));
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
    const target = ["builds", "artifacts", "releases"].includes(section)
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
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.locator('textarea[name="goal"]').fill("Keep the exact build goal");
  await page.locator(".account-trigger").click();
  await page.getByRole("menuitem", { name: "个人信息", exact: true }).click();
  await expect(page.getByRole("textbox", { name: /^用户名/ })).toHaveAttribute(
    "readonly",
    "",
  );
  await page
    .getByRole("textbox", { name: "显示名称", exact: true })
    .fill("Alex Chen");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("修改已保存");
  await page.keyboard.press("Escape");
  await expect(page.locator('textarea[name="goal"]')).toHaveValue(
    "Keep the exact build goal",
  );
  await expect(page.locator(".account-trigger")).toContainText("Alex Chen");
  await openPreferences(page);
  await page
    .getByRole("combobox", { name: "界面语言", exact: true })
    .selectOption("en");
  await page
    .getByRole("combobox", { name: "Theme", exact: true })
    .selectOption("light");
  await closePreferences(page);
  await expect(page.locator('textarea[name="goal"]')).toHaveValue(
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
  await expect(page.getByRole("alert")).toContainText(
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
    await page.getByRole("link", { name: "发起构建", exact: true }).click();
    await page.getByRole("button", { name: "继续", exact: true }).click();
    await page.locator('textarea[name="goal"]').fill("Unsaved build goal");
    const rejected = page
      .waitForEvent("dialog")
      .then((dialog) => dialog.dismiss());
    await page.evaluate(() => history.back());
    await rejected;
    await expect(page).toHaveURL(
      tenantURL("/projects/desktop-client/builds/new"),
    );
    await expect(page.locator('textarea[name="goal"]')).toHaveValue(
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
  await page.goto(tenantURL("/"));
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
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
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
  await expect(dialog.getByRole("alert")).toContainText(
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
