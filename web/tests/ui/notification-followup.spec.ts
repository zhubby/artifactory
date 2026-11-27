/**
 * [INPUT]: 依赖真实通知组件、顶栏入口、账户跳转、分页和无障碍扫描。
 * [OUTPUT]: 验证顶栏抽屉、完整范围、实时记录、登录后确认与 WCAG AA。
 * [POS]: 通知中心集成审查发现的边界回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import {
  fixtureAPI,
  tenantURL,
  project,
  tenantId,
  tenantSlug,
} from "./fixtures";
const item = {
  id: "11111111-1111-4111-8111-111111111112",
  category: "build",
  event_type: "build.status.changed",
  severity: "info",
  title: { zh: "构建进展", en: "Build update" },
  summary: { zh: "等待处理", en: "Waiting" },
  tenant_id: tenantId,
  tenant_name: "Build Team",
  tenant_slug: tenantSlug,
  project_id: project.id,
  project_name: project.name,
  project_slug: project.slug,
  resource_id: "11111111-1111-4111-8111-111111111113",
  resource_type: "build",
  url: null,
  occurred_at: "2026-11-23T10:00:00Z",
  updated_at: "2026-11-23T10:00:00Z",
  content_revision: 1,
  attention_revision: 1,
  read_revision: 0,
  needs_action: true,
};
const event = (index: number) => ({
  id: `11111111-1111-4111-8111-${String(index).padStart(12, "0")}`,
  event_type: "build.status.changed",
  title: { zh: `阶段 ${index}`, en: `Phase ${index}` },
  summary: { zh: "构建状态更新", en: "Build status changed" },
  occurred_at: `2026-11-23T10:${String(index).padStart(2, "0")}:00Z`,
});
async function setup(page: Page) {
  await fixtureAPI(page);
  await page.route("**/api/v1/notifications**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/events"))
      return route.fulfill({
        contentType: "text/event-stream",
        body: "retry: 600000\n\n",
      });
    if (url.pathname.endsWith("/summary"))
      return route.fulfill({
        json: { unread_count: 1, needs_action_count: 1, watermark: 1 },
      });
    if (url.pathname.endsWith("/scopes"))
      return route.fulfill({
        json: {
          tenants: [
            { id: tenantId, name: "Build Team", slug: tenantSlug },
            {
              id: "historical-tenant",
              name: "Historical workspace",
              slug: "historical",
            },
          ],
          projects: [
            {
              id: project.id,
              name: project.name,
              slug: project.slug,
              tenant_id: tenantId,
            },
            {
              id: "historical-project",
              name: "Historical project",
              slug: "older-project",
              tenant_id: "historical-tenant",
            },
          ],
        },
      });
    if (url.pathname.endsWith("/preferences"))
      return route.fulfill({
        json: {
          in_app: {},
          email: {},
          locale: "zh-CN",
          email_address: null,
          email_verified: false,
          email_available: true,
        },
      });
    if (url.pathname.endsWith(`/${item.id}`))
      return route.fulfill({
        json: { item, events: [event(1)], next_cursor: null },
      });
    return route.fulfill({
      json: {
        items: [item],
        next_cursor: null,
        unread_count: 1,
        needs_action_count: 1,
        watermark: 1,
      },
    });
  });
}
async function fakeStream(page: Page) {
  await page.addInitScript(() => {
    class Source extends EventTarget {
      onmessage: null = null;
      onerror: null = null;
      constructor(url: string) {
        super();
        if (url.includes("/notifications/events"))
          Object.assign(window, { inboxStream: this });
      }
      close() {}
    }
    Object.assign(window, { EventSource: Source });
  });
}
async function invalidate(page: Page) {
  await page.evaluate(() => {
    (
      window as unknown as { inboxStream: EventTarget }
    ).inboxStream.dispatchEvent(new Event("invalidate"));
  });
}

test("scope choices include accessible historical scopes outside the loaded page", async ({
  page,
}) => {
  await setup(page);
  const queries: string[] = [];
  await page.route("**/api/v1/notifications?**", (route) => {
    queries.push(new URL(route.request().url()).search);
    return route.fulfill({
      json: {
        items: [item],
        next_cursor: null,
        unread_count: 1,
        needs_action_count: 1,
        watermark: 1,
      },
    });
  });
  await page.goto("/notifications");
  await page.getByRole("combobox", { name: "工作空间", exact: true }).click();
  await page
    .getByRole("option", { name: "Historical workspace", exact: true })
    .click();
  await page.getByRole("combobox", { name: "项目", exact: true }).click();
  await page
    .getByRole("option", { name: "Historical project", exact: true })
    .click();
  await expect
    .poll(() =>
      queries.some(
        (q) =>
          q.includes("tenant_id=historical-tenant") &&
          q.includes("project_id=historical-project"),
      ),
    )
    .toBe(true);
});

test("open timeline invalidation preserves loaded history, scroll and cursor", async ({
  page,
}) => {
  await fakeStream(page);
  await setup(page);
  let updated = false;
  const cursors: string[] = [];
  await page.route(`**/api/v1/notifications/${item.id}**`, (route) => {
    const cursor = new URL(route.request().url()).searchParams.get("cursor");
    cursors.push(cursor || "first");
    if (cursor)
      return route.fulfill({
        json: {
          item: {
            ...item,
            title: updated
              ? { zh: "最新构建进展", en: "Latest build update" }
              : item.title,
          },
          events: [event(2), ...(updated ? [event(3)] : [])],
          next_cursor: null,
        },
      });
    return route.fulfill({
      json: {
        item: {
          ...item,
          title: updated
            ? { zh: "最新构建进展", en: "Latest build update" }
            : item.title,
        },
        events: [event(1)],
        next_cursor: "first-page-end",
      },
    });
  });
  await page.goto("/notifications");
  await page.getByRole("button", { name: "构建进展", exact: true }).click();
  await page.getByRole("button", { name: "加载更多", exact: true }).click();
  await expect(page.getByText("阶段 2", { exact: true })).toBeVisible();
  await page.evaluate(() => {
    document
      .querySelector(".inbox-timeline")
      ?.setAttribute("style", "min-height:2000px");
    window.scrollTo(0, 300);
  });
  const scroll = await page.evaluate(() => window.scrollY);
  updated = true;
  await invalidate(page);
  await expect(
    page.getByRole("heading", { name: "最新构建进展", exact: true }),
  ).toBeAttached();
  await expect(page.getByText("阶段 3", { exact: true })).toBeAttached();
  await expect(page.getByText("阶段 2", { exact: true })).toBeAttached();
  await expect(page.getByText("阶段 1", { exact: true })).toBeAttached();
  expect(await page.evaluate(() => window.scrollY)).toBe(scroll);
  expect(cursors.some((c) => c.includes(event(2).id))).toBe(true);
});

test("inbox invalidation preserves loaded pages while refreshing attention and pagination", async ({ page }) => {
  await fakeStream(page);
  await setup(page);
  let updated = false;
  const original = Array.from({ length: 80 }, (_, index) => ({
    ...item,
    id: `notice-${index + 1}`,
    title: { zh: `通知 ${index + 1}`, en: `Notice ${index + 1}` },
    updated_at: new Date(Date.UTC(2026, 10, 23, 12, 0, -index)).toISOString(),
  }));
  await page.route("**/api/v1/notifications?**", (route) => {
    const url = new URL(route.request().url());
    const cursor = url.searchParams.get("cursor");
    const values = updated ? [{ ...original[0], id: "new-notice", title: { zh: "新通知", en: "New notice" }, updated_at: "2026-11-23T13:00:00.000Z" },
      ...original.map((value, index) => index === 0 ? { ...value, read_revision: 1 } : value)] : original;
    const start = cursor ? values.findIndex((value) => cursor === `${value.updated_at}|${value.id}`) + 1 : 0;
    const items = values.slice(start, start + Number(url.searchParams.get("limit")));
    const last = items.at(-1)!;
    return route.fulfill({ json: { items, next_cursor: start + items.length < values.length ? `${last.updated_at}|${last.id}` : null,
      unread_count: 80, needs_action_count: values.length, watermark: updated ? 2 : 1 } });
  });
  await page.goto("/notifications");
  await page.getByRole("button", { name: "加载更多", exact: true }).click();
  await expect(page.getByRole("button", { name: "通知 60", exact: true })).toBeAttached();
  await page.evaluate(() => window.scrollTo(0, 600));
  const anchor = page.getByRole("button", { name: "通知 5", exact: true });
  const position = await anchor.evaluate((element) => element.getBoundingClientRect().top);
  updated = true;
  await invalidate(page);
  await expect(page.getByRole("button", { name: "新通知", exact: true })).toBeAttached();
  await expect(page.getByRole("button", { name: "通知 60", exact: true })).toBeAttached();
  await expect(page.getByRole("button", { name: "通知 1", exact: true }).locator("..")).not.toHaveClass(/is-unread/);
  expect(Math.abs(await anchor.evaluate((element) => element.getBoundingClientRect().top) - position)).toBeLessThan(1);
  await page.getByRole("button", { name: "加载更多", exact: true }).click();
  await expect(page.getByRole("button", { name: "通知 80", exact: true })).toBeAttached();
  await expect(page.locator(".inbox-list > li")).toHaveCount(81);
});

test("saving in-app preferences preserves disabled email subscriptions when SMTP is unavailable", async ({ page }) => {
  await setup(page);
  let saved: Record<string, unknown> | undefined;
  await page.route("**/api/v1/notifications/preferences", (route) => {
    if (route.request().method() === "PUT") saved = route.request().postDataJSON();
    return route.fulfill({ json: { in_app: {}, email: { build: true }, locale: "zh-CN",
      email_address: "alex@example.test", email_verified: true, email_available: false } });
  });
  await page.goto("/notifications");
  await page.getByRole("button", { name: "通知设置", exact: true }).click();
  const email = page.getByRole("checkbox", { name: "构建 · 邮件", exact: true });
  await expect(email).toBeChecked();
  await expect(email).toBeDisabled();
  await page.getByRole("checkbox", { name: "构建 · 站内通知", exact: true }).uncheck();
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect.poll(() => saved).toBeTruthy();
  expect(saved).not.toHaveProperty("email");
  expect(saved).toMatchObject({ in_app: { build: false }, locale: "zh-CN" });
  await expect(email).toBeChecked();
});

test("verification returns through login and still waits for confirmation", async ({
  page,
}) => {
  await fixtureAPI(page);
  let loggedIn = false;
  let consumed = 0;
  await page.route("**/api/v1/auth/login", (route) => {
    loggedIn = true;
    return route.fulfill({ json: { ok: true } });
  });
  await page.route("**/api/v1/notifications/email/verify", (route) => {
    if (!loggedIn)
      return route.fulfill({ status: 401, json: { error: "Unauthenticated" } });
    consumed++;
    return route.fulfill({ json: { verified: true } });
  });
  await page.goto("/notifications/verify?token=return-token");
  await page.getByRole("button", { name: "确认验证邮箱", exact: true }).click();
  await expect(page).toHaveURL(/\/login\?returnTo=/);
  await expect(page.getByRole("link", { name: "创建账号", exact: true })).toBeVisible();
  await page.locator('input[name="username"]').fill("alex");
  await page.locator('input[name="password"]').fill("test-password");
  await page.getByRole("button", { name: "进入工作空间", exact: true }).click();
  await expect(page).toHaveURL("/notifications/verify?token=return-token");
  expect(consumed).toBe(0);
  await page.getByRole("button", { name: "确认验证邮箱", exact: true }).click();
  await expect(page.getByText("邮箱已验证", { exact: true })).toBeVisible();
  expect(consumed).toBe(1);
});
for (const destination of [
  "https://example.invalid/",
  "//example.invalid/",
  "/\\example.invalid/",
  "/notifications/verify?token=ok&returnTo=https://example.invalid/",
  "/notifications/verify?token=ok#fragment",
]) {
  test(`login rejects unsafe return destination ${destination}`, async ({
    page,
  }) => {
    await fixtureAPI(page);
    await page.route("**/api/v1/auth/login", (route) =>
      route.fulfill({ json: { ok: true } }),
    );
    await page.goto(`/login?returnTo=${encodeURIComponent(destination)}`);
    await page.locator('input[name="username"]').fill("alex");
    await page.locator('input[name="password"]').fill("test-password");
    await page
      .getByRole("button", { name: "进入工作空间", exact: true })
      .click();
    await expect(page).toHaveURL("/tenants");
  });
}
for (const theme of ["light", "dark"]) {
  test(`${theme}: drawer, account inbox and preferences pass axe`, async ({
    page,
  }) => {
    test.setTimeout(90000);
    await setup(page);
    await page.addInitScript(
      (value) => localStorage.setItem("artifactory-theme", value),
      theme,
    );
    async function audit() {
      await page.evaluate(() => document.fonts.ready);
      const result = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
        .analyze();
      expect(
        result.violations.map((v) => ({
          id: v.id,
          nodes: v.nodes.map((n) => ({
            target: n.target,
            reason: n.failureSummary,
          })),
        })),
      ).toEqual([]);
    }
    await page.goto(tenantURL("/"));
    await page.locator(".workspace-topbar").getByRole("button", { name: /通知中心/ }).click();
    await expect(page.getByText("构建进展", { exact: true })).toBeVisible();
    await audit();
    await page.keyboard.press("Escape");
    await page.goto("/notifications");
    await expect(
      page.getByRole("button", { name: "构建进展", exact: true }),
    ).toBeVisible();
    await audit();
    await page.getByRole("button", { name: "通知设置", exact: true }).click();
    await expect(
      page.getByRole("checkbox", { name: "构建 · 邮件", exact: true }),
    ).toBeVisible();
    await audit();
  });
}
