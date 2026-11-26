/**
 * [INPUT]: 依赖 Playwright、租户入口和确定性 API 夹具。
 * [OUTPUT]: 验证会话检查、租户卡片、HTTP 创建入口和失败重试。
 * [POS]: 租户大厅的交互与响应式回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, tenantId, tenantSlug, tenantURL } from "./fixtures";
import type { Tenant } from "../../src/lib/types";

const tenants: Tenant[] = [
  { id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", status: "active", archived_at: null, owner_id: "user-main", created_at: "2026-11-10T08:30:00Z" },
  { id: "tenant-beta", slug: "beta-library", name: "Beta 团队工作空间".repeat(5), role: "member", status: "archived", archived_at: "2026-11-19T08:30:00Z", owner_id: "other", created_at: "2026-11-10T08:30:00Z" },
];

for (const path of ["/", "/tenants"]) {
  test(`${path}: unconfirmed sessions never show the tenant hall`, async ({ page }) => {
    const responseGate = Promise.withResolvers<void>();
    await page.route("**/api/v1/auth/config", route => route.fulfill({ json: { registration_enabled: false } }));
    await page.route("**/api/v1/auth/me", async route => {
      await responseGate.promise;
      await route.fulfill({ status: 401, json: { error: "Unauthenticated" } });
    });
    const checking = page.waitForRequest("**/api/v1/auth/me");
    await page.goto(path);
    try {
      await checking;
      await expect(page).toHaveURL("/tenants");
      await expect(page.locator(".loading-state")).toBeVisible();
      await expect(page.locator(".tenant-hall")).toHaveCount(0);
    } finally {
      responseGate.resolve();
    }
    await expect(page).toHaveURL("/login");
    await expect(page.locator('input[name="username"]')).toBeVisible();
  });
}

test("the home entry offers retry when the session check fails", async ({ page }) => {
  await fixtureAPI(page);
  let accountAvailable = false;
  await page.route("**/api/v1/auth/me", route => accountAvailable
    ? route.fallback()
    : route.fulfill({ status: 503, json: { error: "Account unavailable" } }));
  await page.goto("/");
  const retry = page.getByRole("button", { name: "重试", exact: true });
  await expect(retry).toBeVisible();
  await expect(page.locator(".tenant-hall")).toHaveCount(0);
  accountAvailable = true;
  await retry.click();
  await expect(page.getByRole("heading", { name: "你好，Alex", exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
});

for (const entry of ["hall-heading", "hall-empty"]) {
  test(`${entry}: tenant creation works on an ordinary HTTP origin and keeps retries idempotent`, async ({ page, context, baseURL }) => {
    const origin = "http://artifactory-http.test";
    await context.route(`${origin}/**`, async route => {
      const url = new URL(route.request().url());
      await route.fulfill({ response: await route.fetch({ url: `${baseURL}${url.pathname}${url.search}` }) });
    });
    await fixtureAPI(page);
    const keys: string[] = [];
    await page.route("**/api/v1/tenants", async route => {
      if (route.request().method() === "GET") return route.fulfill({ json: [] });
      keys.push(route.request().postDataJSON().idempotency_key);
      if (keys.length === 1) return route.abort("failed");
      return route.fulfill({ json: { id: tenantId, slug: tenantSlug } });
    });
    await page.goto(`${origin}/tenants`);
    expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
    expect(await page.evaluate(() => typeof crypto.randomUUID)).toBe("undefined");
    await page.locator(`.${entry}`).getByRole("button", { name: "创建租户", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "创建租户", exact: true });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("textbox", { name: "租户名称", exact: true }).fill("HTTP Team");
    await dialog.getByRole("button", { name: "创建租户", exact: true }).click();
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("无法连接服务");
    await expect(dialog.getByRole("textbox")).toHaveValue("HTTP Team");
    await dialog.getByRole("button", { name: "创建租户", exact: true }).click();
    await expect(page).toHaveURL(`${origin}/tenants/${tenantSlug}`);
    expect(keys).toHaveLength(2);
    expect(keys[0]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(keys[1]).toBe(keys[0]);
  });
}

test("tenant hall uses compact responsive cards and keeps management independent", async ({ page }, info) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/tenants", route => route.fulfill({ json: tenants }));
  await page.route("**/api/v1/operations", route => route.fulfill({ json: [{
    id: "old-operation", tenant_id: tenantId, project_id: null, slug: "removed-workspace",
    action: "delete", state: "succeeded", phase: "done", blockers: [], counts: {},
    error: null, created_at: "2026-11-20T00:00:00Z", updated_at: "2026-11-20T00:00:00Z",
  }] }));
  await page.goto("/tenants");
  const cards = page.locator(".tenant-card");
  await expect(cards).toHaveCount(2);
  await expect(page.getByRole("heading", { name: "操作进度", exact: true })).toHaveCount(0);
  await expect(page.getByText("removed-workspace", { exact: true })).toHaveCount(0);
  const first = (await cards.nth(0).boundingBox())!;
  const second = (await cards.nth(1).boundingBox())!;
  if ((page.viewportSize()?.width || 0) >= 768) {
    expect(Math.abs(first.y - second.y)).toBeLessThan(2);
    expect(second.x).toBeGreaterThan(first.x + first.width);
  } else {
    expect(Math.abs(first.x - second.x)).toBeLessThan(2);
    expect(second.y).toBeGreaterThanOrEqual(first.y + first.height);
  }
  await expect(page.getByRole("link", { name: "管理租户 Build Team", exact: true })).toHaveAttribute("href", `/tenants/${tenantSlug}/settings`);
  await expect(cards.nth(1).getByRole("link")).toHaveCount(1);
  const search = page.getByRole("textbox", { name: "搜索租户", exact: true });
  await search.fill("BETA-LIBRARY");
  await expect(cards).toHaveCount(1);
  await expect(page.getByRole("link", { name: tenants[1].name, exact: true })).toBeVisible();
  await search.fill("unknown-tenant");
  await expect(page.getByRole("heading", { name: "没有匹配的结果", exact: true })).toBeVisible();
  await search.clear();
  await expect(cards).toHaveCount(2);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await page.getByRole("heading", { name: "选择租户", exact: true }).click();
  await page.screenshot({ path: info.outputPath("tenant-hall-cards.png"), fullPage: true });
  await cards.first().click({ position: { x: 10, y: 10 } });
  await expect(page).toHaveURL(tenantURL("/"));
});

test("tenant hall ends the initial skeleton on failure and offers retry", async ({ page }) => {
  await fixtureAPI(page);
  let fail = true;
  await page.route("**/api/v1/tenants", route => fail
    ? route.fulfill({ status: 503, json: { error: "Tenant list unavailable" } })
    : route.fulfill({ json: tenants }));
  await page.goto("/tenants");
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Tenant list unavailable");
  await expect(page.locator(".hall-main .loading-state")).toHaveCount(0);
  fail = false;
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
});
