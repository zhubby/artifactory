/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证侧栏草稿保护、全局错误通知与退出重试。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, project, tenantURL, openNavigation } from "./fixtures";

test("cancelling project configuration restores the saved fields and clears the leave warning", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL(`/projects/${project.slug}/settings/general`));
  const name = page.getByRole("textbox", { name: "项目名称", exact: true });
  const repository = page.locator('input[name="repository_url"]');
  const context = page.locator('textarea[name="build_context"]');
  await expect(name).toHaveValue(project.name);
  await name.fill("Unsaved project name");
  await repository.fill("https://git.example.com/unsaved.git");
  await context.fill("Unsaved build instructions");
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(name).toHaveValue(project.name);
  await expect(repository).toHaveValue(project.repository_url);
  await expect(context).toHaveValue("");
  let warnings = 0;
  page.on("dialog", async dialog => {
    warnings += 1;
    await dialog.dismiss();
  });
  await openNavigation(page);
  await page.getByRole("link", { name: "成员与邀请", exact: true }).click();
  await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/settings/members`));
  expect(warnings).toBe(0);
});

test("submodule credential additions and removals protect unsaved edits during navigation", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}`, route => route.fulfill({
    json: { ...project, source_credentials: [{ repository_url: "https://git.example.com/team/private.git", credential_id: "credential-main" }] },
  }));
  const resources = tenantURL(`/projects/${project.slug}/settings/resources`);
  await page.goto(resources);
  await page.getByRole("button", { name: "移除", exact: true }).click();
  let warnings = 0;
  const reject = async (dialog: import("@playwright/test").Dialog) => {
    warnings += 1;
    await dialog.dismiss();
  };
  page.on("dialog", reject);
  await openNavigation(page);
  await page.getByRole("link", { name: "基本信息", exact: true }).click();
  await expect.poll(() => warnings).toBe(1);
  await expect(page).toHaveURL(resources);
  if (await page.getByRole("dialog", { name: "主导航", exact: true }).isVisible())
    await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "移除", exact: true })).toHaveCount(0);
  page.off("dialog", reject);
  page.once("dialog", dialog => dialog.accept());
  await openNavigation(page);
  await page.getByRole("link", { name: "基本信息", exact: true }).click();
  await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/settings/general`));
  await openNavigation(page);
  await page.getByRole("link", { name: "资源与凭据", exact: true }).click();
  await expect(page.getByRole("button", { name: "移除", exact: true })).toHaveCount(1);
  await page.getByRole("button", { name: "添加关联", exact: true }).click();
  page.on("dialog", reject);
  await openNavigation(page);
  await page.getByRole("link", { name: "基本信息", exact: true }).click();
  await expect.poll(() => warnings).toBe(2);
  await expect(page).toHaveURL(resources);
  if (await page.getByRole("dialog", { name: "主导航", exact: true }).isVisible())
    await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "移除", exact: true })).toHaveCount(2);
});

test("failed logout stays in the menu with a visible error and can be retried", async ({ page }) => {
  await fixtureAPI(page);
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  let attempts = 0;
  await page.route("**/api/v1/auth/logout", async route => {
    attempts += 1;
    if (attempts === 1) return route.abort("failed");
    return route.fulfill({ json: {} });
  });
  await page.goto(tenantURL("/"));
  await page.getByRole("button", { name: "个人菜单", exact: true }).click();
  const logout = page.getByRole("menuitem", { name: "退出登录", exact: true });
  await logout.click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("网络");
  await expect(logout).toBeEnabled();
  await expect(page).toHaveURL(tenantURL("/"));
  expect(errors).toEqual([]);
  await logout.click();
  await expect(page).toHaveURL("/login");
  expect(attempts).toBe(2);
  expect(errors).toEqual([]);
});
