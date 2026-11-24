/**
 * [INPUT]: 依赖账户通知夹具与浏览器交互。
 * [OUTPUT]: 验证顶栏通知入口、抽屉焦点、修订已读与跨租户请求。
 * [POS]: 持久通知的确定性浏览器验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, tenantURL, project } from "./fixtures";
const item = {
  id: "notice-1",
  category: "build",
  event_type: "build.needs_input",
  severity: "warning",
  title: { zh: "构建需要补充信息", en: "Build needs input" },
  summary: {
    zh: "Desktop Client #128 等待你的回答",
    en: "Desktop Client #128 is waiting for your answer",
  },
  tenant_id: "tenant-one",
  tenant_name: "Build Team",
  tenant_slug: "build-team",
  project_id: "project-main",
  project_name: "Desktop Client",
  project_slug: "desktop-client",
  resource_id: "build-main",
  resource_type: "build",
  url: null,
  occurred_at: "2026-11-23T10:00:00Z",
  updated_at: "2026-11-23T10:00:00Z",
  content_revision: 5,
  attention_revision: 3,
  read_revision: 2,
  needs_action: true,
};
async function setup(page: import("@playwright/test").Page) {
  await fixtureAPI(page);
  await page.route("**/api/v1/notifications**", async (route) => {
    const url = new URL(route.request().url());
    expect(route.request().headers()["x-artifactory-tenant"]).toBeUndefined();
    if (url.pathname.endsWith("/events"))
      return route.fulfill({
        contentType: "text/event-stream",
        body: "retry: 600000\n\n",
      });
    if(url.pathname.endsWith("/scopes"))return route.fulfill({json:{tenants:[{id:"tenant-one",name:"Build Team",slug:"build-team"}],projects:[{id:"project-main",name:"Desktop Client",slug:"desktop-client",tenant_id:"tenant-one"}]}});
    if (url.pathname.endsWith("/summary"))
      return route.fulfill({
        json: { unread_count: 1, needs_action_count: 1, watermark: 7 },
      });
    if (url.pathname.endsWith("/notice-1"))
      return route.fulfill({
        json: {
          item,
          events: [
            {
              id: "event-1",
              event_type: item.event_type,
              title: item.title,
              summary: item.summary,
              occurred_at: item.occurred_at,
            },
          ],
          next_cursor: null,
        },
      });
    return route.fulfill({
      json: {
        items: [item],
        next_cursor: null,
        watermark: 7,
        unread_count: 1,
        needs_action_count: 1,
      },
    });
  });
}
test("topbar opens compact inbox, restores bell focus, keeps revision and watermark", async ({
  page,
}, info) => {
  await setup(page);
  let read: unknown;
  let all: unknown;
  await page.route("**/api/v1/notifications/notice-1/read", (route) => {
    read = route.request().postDataJSON();
    return route.fulfill({
      json: { unread_count: 1, needs_action_count: 1, watermark: 8 },
    });
  });
  await page.route("**/api/v1/notifications/read-all", (route) => {
    all = route.request().postDataJSON();
    return route.fulfill({
      json: { unread_count: 1, needs_action_count: 1, watermark: 8 },
    });
  });
  await page.goto(tenantURL("/"));
  const bell = page.locator(".workspace-topbar").getByRole("button", { name: /通知中心/ });
  await expect(bell).toBeVisible();
  await expect(page.locator(".sidebar").getByRole("button", { name: /通知中心/ })).toHaveCount(0);
  await expect(bell).toContainText("1");
  await page.addStyleTag({ content: "nextjs-portal {display:none}" });
  await page.screenshot({
    path: info.outputPath("notification-topbar.png"),
    fullPage: false,
  });
  await bell.click();
  await expect(page.getByRole("dialog", { name: "通知中心" })).toBeVisible();
  await expect(page.getByText(item.title.zh, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "标为已读", exact: true }).click();
  expect(read).toEqual({ attention_revision: 3 });
  await page.getByRole("button", { name: "全部标为已读", exact: true }).click();
  expect(all).toEqual({ watermark: 7 });
  await page.screenshot({
    path: info.outputPath("notification-drawer.png"),
    fullPage: false,
  });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "通知中心", exact: true })).not.toBeVisible();
  await expect(bell).toBeFocused();
  await expect(page.getByRole("dialog", { name: "主导航", exact: true })).toHaveCount(0);
});
test("personal inbox loads without tenant and offers filters and timeline", async ({
  page,
}, info) => {
  await setup(page);
  await page.goto("/notifications");
  await expect(
    page.getByRole("heading", { name: "通知中心", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: item.title.zh, exact: true }).click();
  await expect(page.getByRole("heading", { name: "事件记录" })).toBeVisible();
  await page.addStyleTag({ content: "nextjs-portal {display:none}" });
  await page.screenshot({
    path: info.outputPath("notification-page.png"),
    fullPage: false,
  });
});
test("email verification waits for explicit confirmation", async ({ page }) => {
  await setup(page);
  let calls = 0;
  await page.route("**/api/v1/notifications/email/verify", (route) => {
    calls++;
    expect(route.request().postDataJSON()).toEqual({ token: "one-time-token" });
    return route.fulfill({ json: { verified: true } });
  });
  await page.goto("/notifications/email/verify?token=one-time-token");
  await expect(
    page.getByRole("button", { name: "确认验证邮箱", exact: true }),
  ).toBeVisible();
  expect(calls).toBe(0);
  await page.getByRole("button", { name: "确认验证邮箱", exact: true }).click();
  await expect(page.getByText("邮箱已验证", { exact: true })).toBeVisible();
  expect(calls).toBe(1);
});

test("one account stream survives navigation and invalidates without progress", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const streams: EventTarget[] = [];
    Object.assign(window, { notificationStreams: streams });
    class InboxSource extends EventTarget {
      onmessage: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor(url: string) {
        super();
        if (url.includes("/notifications/events")) streams.push(this);
      }
      close() {}
    }
    Object.assign(window, { EventSource: InboxSource });
  });
  await setup(page);
  let summaries = 0;
  await page.route("**/api/v1/notifications/summary", (route) => {
    summaries++;
    return route.fulfill({
      json: { unread_count: 1, needs_action_count: 1, watermark: summaries },
    });
  });
  await page.goto(tenantURL("/"));
  await expect.poll(() => summaries).toBeGreaterThan(0);
  await page.locator(".workspace-topbar").getByRole("button", { name: /通知中心/ }).click();
  await page.getByRole("link", { name: "查看全部通知" }).click();
  await expect(page).toHaveURL("/notifications");
  expect(
    await page.evaluate(
      () =>
        (window as unknown as { notificationStreams: EventTarget[] })
          .notificationStreams.length,
    ),
  ).toBe(1);
  const before = summaries;
  await page.evaluate(() => {
    (
      window as unknown as { notificationStreams: EventTarget[] }
    ).notificationStreams[0].dispatchEvent(new Event("invalidate"));
  });
  await expect.poll(() => summaries).toBeGreaterThan(before);
  await expect(page.locator(".top-progress")).not.toBeVisible();
});

test("pagination and scope filters preserve newer unread attention", async ({
  page,
}) => {
  await setup(page);
  const queries: string[] = [];
  const pageTwoCursor = `${item.updated_at}|${item.id}`;
  let read = false;
  await page.route("**/api/v1/notifications?**", (route) => {
    const url = new URL(route.request().url());
    queries.push(url.search);
    return route.fulfill({
      json: {
        items: url.searchParams.has("cursor")
          ? [
              {
                ...item,
                id: "notice-2",
                title: { zh: "构建已完成", en: "Build completed" },
                updated_at: "2026-11-23T09:00:00Z",
              },
            ]
          : [
              {
                ...item,
                ...(read ? { attention_revision: 4, read_revision: 3 } : {}),
              },
            ],
        next_cursor: url.searchParams.has("cursor") ? null : pageTwoCursor,
        watermark: 8,
        unread_count: 1,
        needs_action_count: 1,
      },
    });
  });
  await page.route("**/api/v1/notifications/notice-1/read", (route) => {
    expect(route.request().postDataJSON()).toEqual({ attention_revision: 3 });
    read = true;
    return route.fulfill({
      json: { unread_count: 1, needs_action_count: 1, watermark: 8 },
    });
  });
  await page.goto("/notifications");
  await page.getByRole("button", { name: "加载更多", exact: true }).click();
  await expect(page.getByText("构建已完成", { exact: true })).toBeVisible();
  expect(queries.some((v) => new URLSearchParams(v).get("cursor") === pageTwoCursor)).toBe(true);
  await page
    .getByRole("button", { name: "标为已读", exact: true })
    .first()
    .click();
  await expect(page.locator(".inbox-list .is-unread")).toHaveCount(2);
  await expect(page.getByText("构建已完成", { exact: true })).toBeAttached();
  await page.getByRole("button", { name: "需要处理", exact: true }).click();
  await expect
    .poll(() => queries.some((v) => v.includes("needs_action=true")))
    .toBe(true);
  const tenant = page.getByRole("combobox", { name: "工作区", exact: true });
  await tenant.click();
  await page.getByRole("option", { name: "Build Team", exact: true }).click();
  await expect
    .poll(() => queries.some((v) => v.includes("tenant_id=tenant-one")))
    .toBe(true);
});

test("unverified email cannot opt in and dirty preferences stay when updates arrive", async ({
  page,
}, info) => {
  await setup(page);
  await page.route("**/api/v1/notifications/preferences", (route) =>
    route.fulfill({
      json: {
        in_app: {},
        email: {},
        locale: "zh-CN",
        email_address: "alex@example.test",
        email_verified: false,
        email_available: true,
      },
    }),
  );
  await page.goto("/notifications");
  await page.getByRole("button", { name: "通知设置", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "构建 · 邮件", exact: true }),
  ).toBeDisabled();
  await expect(page.getByText("等待验证", { exact: true })).toBeVisible();
  await page
    .getByRole("checkbox", { name: "构建 · 站内通知", exact: true })
    .uncheck();
  page.once("dialog", (dialog) => dialog.dismiss());
  await page.getByRole("button", { name: "返回收件箱", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "构建 · 站内通知", exact: true }),
  ).not.toBeChecked();
  await page.addStyleTag({ content: "nextjs-portal {display:none}" });
  await page.screenshot({
    path: info.outputPath("notification-preferences.png"),
    fullPage: false,
  });
});

test("project follow uses scoped API and webhook creation requires explicit opt in", async ({
  page,
}, info) => {
  await fixtureAPI(page);
  let following = false;
  let created: unknown;
  await page.route(
    `**/api/v1/projects/${project.id}/notification-subscription`,
    (route) => {
      expect(route.request().headers()["x-artifactory-tenant"]).toBeTruthy();
      if (route.request().method() === "PUT")
        following = route.request().postDataJSON().following;
      return route.fulfill({ json: { following } });
    },
  );
  await page.goto(tenantURL("/projects/desktop-client"));
  await page.getByRole("button", { name: "关注项目", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "取消关注项目", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  let endpoints: unknown[] = [];
  await page.route(
    "**/api/v1/projects/project-main/notification-webhooks",
    (route) => {
      if (route.request().method() === "POST") {
        created = route.request().postDataJSON();
        endpoints = [
          {
            id: "endpoint-one",
            name: "Release feed",
            url_display: "https://hooks.example.test/***",
            enabled: false,
            event_types: ["build.status.changed"],
            generation: 1,
            last_status: null,
            created_at: item.occurred_at,
          },
        ];
        return route.fulfill({ json: { secret: "fixture-one-time-secret" } });
      }
      return route.fulfill({ json: endpoints });
    },
  );
  await page.goto(tenantURL("/projects/desktop-client/settings/integrations"));
  await page.locator("summary").filter({ hasText: "添加 Webhook" }).click();
  await page
    .getByRole("textbox", { name: "名称", exact: true })
    .fill("Release feed");
  await page
    .getByRole("textbox", { name: "HTTPS 地址", exact: true })
    .fill("https://hooks.example.test/notify");
  await expect(
    page.getByRole("checkbox", { name: "启用投递", exact: true }),
  ).not.toBeChecked();
  await page.getByRole("button", { name: "添加 Webhook", exact: true }).click();
  expect(created).toMatchObject({
    enabled: false,
    event_types: ["build.status.changed", "release.publication.changed"],
  });
  await expect(
    page.getByText("fixture-one-time-secret", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "发送测试", exact: true }),
  ).toBeDisabled();
  await page.addStyleTag({ content: "nextjs-portal {display:none}" });
  await page.screenshot({
    path: info.outputPath("notification-webhooks.png"),
    fullPage: false,
  });
});

test("unsubscribe link does not mutate until confirmed", async ({ page }) => {
  await fixtureAPI(page);
  let sent = 0;
  await page.route("**/api/v1/notifications/email/unsubscribe", (route) => {
    sent++;
    expect(route.request().postDataJSON()).toEqual({
      token: "unsubscribe-token",
    });
    return route.fulfill({ json: { unsubscribed: true } });
  });
  await page.goto("/notifications/unsubscribe?token=unsubscribe-token");
  await expect(
    page.getByRole("button", { name: "确认退订", exact: true }),
  ).toBeVisible();
  expect(sent).toBe(0);
  await page.getByRole("button", { name: "确认退订", exact: true }).click();
  await expect(page.getByText("已退订邮件通知", { exact: true })).toBeVisible();
  expect(sent).toBe(1);
});

test("English dark inbox preserves layout and localized event copy", async ({
  page,
  baseURL,
}, info) => {
  await setup(page);
  if (info.project.name === "mobile") await page.setViewportSize({width:320,height:740});
  await page
    .context()
    .addCookies([
      { name: "artifactory-locale", value: "en", url: baseURL! },
    ]);
  await page.addInitScript(() =>
    localStorage.setItem("artifactory-theme", "dark"),
  );
  await page.goto("/notifications");
  await expect(
    page.getByRole("heading", { name: "Notifications", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Build needs input", exact: true }),
  ).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.addStyleTag({ content: "nextjs-portal {display:none}" });
  await page.screenshot({
    path: info.outputPath("notification-english-dark.png"),
    fullPage: false,
  });
});

test("SMTP availability gates email form and verified contacts explicitly opt in", async ({
  page,
}) => {
  await fixtureAPI(page);
  let available = false;
  let saved: unknown;
  await page.route("**/api/v1/notifications/preferences", (route) => {
    if (route.request().method() === "PUT")
      saved = route.request().postDataJSON();
    return route.fulfill({
      json: {
        in_app: {},
        email: {},
        locale: "zh-CN",
        email_address: "alex@example.test",
        email_verified: true,
        email_available: available,
      },
    });
  });
  await page.goto("/notifications");
  await page.getByRole("button", { name: "通知设置", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "发送验证邮件", exact: true }),
  ).toBeDisabled();
  await expect(
    page.getByRole("checkbox", { name: "构建 · 邮件", exact: true }),
  ).toBeDisabled();
  available = true;
  await page.reload();
  await page.getByRole("button", { name: "通知设置", exact: true }).click();
  const email = page.getByRole("checkbox", {
    name: "构建 · 邮件",
    exact: true,
  });
  await expect(page.getByText("仅站内", { exact: true })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "投递 · 邮件", exact: true })).toHaveCount(0);
  await expect(email).not.toBeChecked();
  await email.check();
  await page.getByRole("button", { name: "保存", exact: true }).click();
  expect(saved).toMatchObject({
    email: { build: true, release: false },
    locale: "zh-CN",
  });
});

test("Webhook administration edits, tests, rotates and retries failed delivery", async ({
  page,
}) => {
  await fixtureAPI(page);
  const writes: string[] = [];
  const endpoint = {
    id: "endpoint-one",
    name: "Release feed",
    url_display: "https://hooks.example.test",
    enabled: true,
    event_types: ["build.status.changed"],
    generation: 1,
    last_status: "failed",
    created_at: item.occurred_at,
  };
  await page.route("**/api/v1/tenant/notification-webhooks**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (route.request().method() !== "GET") {
      writes.push(`${route.request().method()} ${path}`);
      return route.fulfill({
        json: path.endsWith("/rotate")
          ? { secret: "fixture-rotated-secret" }
          : { ok: true },
      });
    }
    return route.fulfill({
      json: path.endsWith("/deliveries")
        ? [
            {
              id: "delivery-one",
              event_type: "build.status.changed",
              status: "failed",
              attempts: 1,
              next_attempt_at: null,
              last_error: "delivery_failed",
              created_at: item.occurred_at,
            },
          ]
        : [endpoint],
    });
  });
  await page.goto(tenantURL("/settings/integrations"));
  await page.getByRole("button", { name: "发送测试", exact: true }).click();
  await page.getByRole("button", { name: "轮换签名密钥", exact: true }).click();
  await expect(
    page.getByText("fixture-rotated-secret", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "投递记录", exact: true }).click();
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await page.locator(".notification-webhook-row details summary").click();
  await page
    .getByRole("textbox", { name: "名称", exact: true })
    .fill("Changed feed");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  expect(writes.some((v) => v.endsWith("/test"))).toBe(true);
  expect(writes.some((v) => v.endsWith("/rotate"))).toBe(true);
  expect(writes.some((v) => v.endsWith("/deliveries/delivery-one/retry"))).toBe(
    true,
  );
  expect(writes.some((v) => v.startsWith("PATCH "))).toBe(true);
});
