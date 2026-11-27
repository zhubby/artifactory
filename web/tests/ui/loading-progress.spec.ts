/**
 * [INPUT]: 依赖 Playwright、延迟 API 夹具和真实工作空间导航。
 * [OUTPUT]: 验证前台并发进度、静默轮询和未保存导航取消。
 * [POS]: 全局顶部加载条的浏览器回归边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, openNavigation, project, tenantId, tenantSlug, tenantURL } from "./fixtures";

const tenant = { id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", status: "active", archived_at: null, owner_id: "user-main", created_at: "2026-11-10T08:30:00Z" };

test("top progress waits for every foreground request and releases failed reads", async ({ page }) => {
  await fixtureAPI(page);
  let releaseContext!: () => void;
  let releaseProjects!: () => void;
  const context = new Promise<void>(resolve => { releaseContext = resolve; });
  const projects = new Promise<void>(resolve => { releaseProjects = resolve; });
  await page.route("**/api/v1/tenant", async route => {
    await context;
    await route.fulfill({ json: { tenant, project_roles: { [project.id]: "admin" } } });
  });
  await page.route("**/api/v1/projects", async route => {
    await projects;
    await route.fulfill({ status: 503, json: { error: "Temporarily unavailable" } });
  });
  try {
    await page.goto(tenantURL("/"));
    const progress = page.getByRole("progressbar", { name: "页面加载" });
    await expect(progress).toBeVisible();
    releaseContext();
    await expect(progress).toBeVisible();
    releaseProjects();
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Temporarily unavailable");
    await expect(progress).toBeHidden();
    await expect(page.locator(".loading-state")).toHaveCount(0);
  } finally {
    releaseContext();
    releaseProjects();
  }
});

test("tenant polling preserves cards without flashing foreground progress", async ({ page }) => {
  await fixtureAPI(page);
  let reads = 0;
  let pollStarted!: () => void;
  let releasePoll!: () => void;
  const started = new Promise<void>(resolve => { pollStarted = resolve; });
  const pending = new Promise<void>(resolve => { releasePoll = resolve; });
  await page.route("**/api/v1/tenants", async route => {
    if (++reads > 1) {
      pollStarted();
      await pending;
    }
    await route.fulfill({ json: [tenant] });
  });
  try {
    await page.goto("/tenants");
    await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
    await expect(page.getByRole("progressbar")).toBeHidden();
    await started;
    // 持续超过进度条的显示延迟，确认轮询保持静默。
    await page.waitForTimeout(350);
    await expect(page.getByRole("progressbar")).toBeHidden();
    await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
    await expect(page.locator(".loading-state")).toHaveCount(0);
  } finally {
    releasePoll();
  }
});

test("rejecting an unsaved navigation leaves no progress behind", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/releases/new"));
  await page.locator('textarea[name="notes"]').fill("Keep the current release draft");
  await expect(page.getByRole("progressbar")).toBeHidden();
  await openNavigation(page);
  page.once("dialog", dialog => dialog.dismiss());
  await page.getByRole("link", { name: "返回工作空间工作台", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/projects/desktop-client/releases/new"));
  await expect(page.locator('textarea[name="notes"]')).toHaveValue("Keep the current release draft");
  await page.waitForTimeout(350);
  await expect(page.getByRole("progressbar")).toBeHidden();
});

test("leaving a workspace releases progress while its tenant lookup is still pending", async ({ page }) => {
  await fixtureAPI(page);
  let releaseLookup!: () => void;
  const lookup = new Promise<void>(resolve => { releaseLookup = resolve; });
  await page.route(`**/api/v1/tenants/${tenantSlug}`, async route => {
    await lookup;
    await route.fulfill({ json: tenant });
  });
  try {
    await page.goto(tenantURL("/"));
    await expect(page.getByRole("progressbar")).toBeVisible();
    await openNavigation(page);
    await page.locator(".sidebar .brand").click();
    await expect(page).toHaveURL(/\/tenants$/);
    await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
    // 旧工作空间解析仍挂起，页面已离开，不得继续占用进度。
    await expect(page.getByRole("progressbar")).toBeHidden();
  } finally {
    releaseLookup();
  }
});
