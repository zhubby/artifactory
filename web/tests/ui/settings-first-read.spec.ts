/**
 * [INPUT]: 依赖设置分区、必需选项接口和可控首读与轮询响应。
 * [OUTPUT]: 验证独立骨架、失败重试、凭据空态、表单门控和设置身份切换。
 * [POS]: 设置数据生命周期回归；草稿只随明确分区导航重置。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, openNavigation, project, tenantURL } from "./fixtures";

const reads = [
  { name: "project signing preview", path: `/projects/${project.slug}/settings/signing`, api: `/projects/${project.id}/signing-preview`, heading: "默认签名方案", result: [], empty: "项目尚未设置默认签名方案。" },
  { name: "image credentials", path: `/projects/${project.slug}/settings/resources`, api: `/projects/${project.id}/image-credentials`, heading: "镜像与构建凭据", result: [], empty: "暂无镜像与构建凭据" },
  { name: "tenant invitations", path: "/settings/members", api: "/tenant/invitations", heading: "邀请链接", result: [] },
  { name: "OTA pools", path: "/settings/emissary", api: "/pools", heading: "节点池升级策略", result: [] },
  { name: "OTA history", path: "/settings/emissary", api: "/emissary-updates", heading: "升级执行记录", result: [] },
  { name: "OTA product version", path: "/settings/emissary", api: "/emissary-version", heading: "Artifactory 统一版本", result: { product_version: "0.2.0", release_id: null, release_version: null, release_status: "missing" } },
  { name: "lifecycle impact", path: `/projects/${project.slug}/settings/lifecycle`, api: `/projects/${project.id}/lifecycle`, heading: "空间生命周期", result: { projects: 1, builds: 12, artifacts: 4, devices: 0 } },
  { name: "lifecycle operations", path: `/projects/${project.slug}/settings/lifecycle`, api: "/operations", heading: "空间生命周期", result: [] },
];

for (const scenario of reads) {
  test(`${scenario.name}: first read shows a skeleton, failure ends it and retry recovers`, async ({ page }) => {
    await fixtureAPI(page);
    if (scenario.name === "project signing preview")
      await page.route("**/api/v1/signing/profiles", route => route.fulfill({ json: [] }));
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    let fail = true;
    let calls = 0;
    await page.route(`**/api/v1${scenario.api}`, async route => {
      calls++;
      if (fail) {
        await pending;
        return route.fulfill({ status: 503, json: { error: `${scenario.name} unavailable` } });
      }
      return route.fulfill({ json: scenario.result });
    });
    try {
      await page.goto(tenantURL(scenario.path));
      const panel = page.locator("main section.panel").filter({ has: page.getByRole("heading", { name: scenario.heading, exact: true }) });
      await expect(panel.locator(".loading-state").first()).toBeVisible();
      if (scenario.empty) await expect(panel.getByText(scenario.empty, { exact: true })).toHaveCount(0);
      if (scenario.name.startsWith("lifecycle")) await expect(panel.locator(".lifecycle-actions button:enabled")).toHaveCount(0);
      release();
      await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(`${scenario.name} unavailable`);
      await expect(panel.locator(".loading-state")).toHaveCount(0);
      fail = false;
      await page.locator("main").getByRole("button", { name: "重试", exact: true }).click();
      await expect.poll(() => calls).toBeGreaterThan(1);
      await expect(panel.locator(".loading-state")).toHaveCount(0);
      await expect(page.locator('[data-notify][data-kind="error"]')).toHaveCount(0);
      if (scenario.empty) await expect(panel.getByText(scenario.empty, { exact: true })).toBeVisible();
    } finally { release(); }
  });
}

test("project resource form waits for credentials even when pools are already available", async ({ page }) => {
  const events = await fixtureAPI(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  let failed = true;
  await page.route("**/api/v1/credentials", async route => {
    if (failed) {
      await pending;
      return route.fulfill({ status: 503, json: { error: "源码凭据暂不可用" } });
    }
    return route.fulfill({ json: [{ id: "credential-main", name: "Read-only deploy", kind: "https", created_at: "2026-11-10T08:30:00Z" }] });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}/settings/resources`));
    const resources = page.locator(".management-section").filter({ has: page.getByRole("heading", { name: "共享资源", exact: true }) });
    await expect(resources.locator(".loading-state")).toBeVisible();
    await expect(resources.locator("form")).toHaveCount(0);
    release();
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("源码凭据暂不可用");
    await expect(resources.locator(".loading-state")).toHaveCount(0);
    await expect(resources.locator("form")).toHaveCount(0);
    failed = false;
    await resources.getByRole("button", { name: "重试", exact: true }).click();
    await expect(resources.locator("form")).toBeVisible();
    await expect(resources.getByRole("combobox", { name: "只读 Git 凭据", exact: true })).toContainText("Read-only deploy");
    expect(events.writes).toEqual([]);
  } finally { release(); }
});

test("OTA pool forms wait for the product version and offer no arbitrary target", async ({ page }) => {
  await fixtureAPI(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/emissary-version", async route => {
    await pending;
    return route.fulfill({ json: { product_version: "0.2.0", release_id: "release-ota", release_version: "v0.2.0", release_status: "ready" } });
  });
  try {
    await page.goto(tenantURL("/settings/emissary"));
    const pools = page.locator("section.panel").filter({ has: page.getByRole("heading", { name: "节点池升级策略", exact: true }) });
    await expect(pools.locator(".loading-state")).toBeVisible();
    await expect(pools.locator("form")).toHaveCount(0);
    release();
    await expect(pools.locator("form")).toBeVisible();
    await expect(pools).toContainText("Artifactory 统一版本");
    await expect(pools).toContainText("0.2.0");
    await expect(pools.getByRole("combobox")).toHaveCount(0);
  } finally { release(); }
});

test("settings section changes discard previous rows until their own data arrives", async ({ page }) => {
  await fixtureAPI(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/credentials", async route => {
    await pending;
    return route.fulfill({ json: [{ id: "credential-main", name: "Read-only deploy", kind: "https", created_at: "2026-11-10T08:30:00Z" }] });
  });
  try {
    await page.goto(tenantURL("/settings/tokens"));
    await expect(page.getByRole("cell", { name: /^Desktop release(?:\s|$)/ })).toBeVisible();
    await openNavigation(page);
    await page.locator(".sidebar").getByRole("link", { name: "Git 凭据", exact: true }).click();
    await expect(page).toHaveURL(tenantURL("/settings/credentials"));
    await expect(page.locator("main .loading-state")).toBeVisible();
    await expect(page.getByRole("cell", { name: /^Desktop release(?:\s|$)/ })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "暂无记录", exact: true })).toHaveCount(0);
    release();
    await expect(page.getByRole("cell", { name: "Read-only deploy", exact: true })).toBeVisible();
    await expect(page.getByRole("cell", { name: /^Desktop release(?:\s|$)/ })).toHaveCount(0);
  } finally { release(); }
});

test("OTA background history refresh retains rows and an unsaved pool policy draft", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  let reads = 0;
  let poolReads = 0;
  await page.route("**/api/v1/pools", route => route.fulfill({ json: [{ id: "pool-main", name: "default", is_default: true, emissary_update_concurrency: ++poolReads === 1 ? 1 : 3 }] }));
  await page.route("**/api/v1/emissary-releases", route => route.fulfill({ json: route.request().method() === "GET" ? [] : { id: "release-imported" } }));
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/emissary-updates", async route => {
    if (++reads > 1) await pending;
    return route.fulfill({ json: [{ id: "update-main", node_id: "node-main", node_name: "Mac Studio", previous_version: "v0.1.0", version: "v0.2.0", status: "queued", updated_at: "2026-11-24T08:30:00Z", finished_at: null }] });
  });
  try {
    await page.goto(tenantURL("/settings/emissary"));
    const ota = page.getByTestId("emissary-settings");
    await expect(ota.getByRole("cell", { name: "Mac Studio", exact: true })).toBeVisible();
    const concurrency = ota.locator('input[name="concurrency"]');
    await concurrency.fill("7");
    await page.clock.fastForward(3100);
    await expect.poll(() => reads).toBeGreaterThan(1);
    await expect(ota.getByRole("cell", { name: "Mac Studio", exact: true })).toBeVisible();
    await expect(concurrency).toHaveValue("7");
    await expect(ota.locator(".loading-state")).toHaveCount(0);
    await expect(page.getByRole("progressbar", { name: "页面加载", exact: true })).toBeHidden();
    release();
    await ota.locator('input[name="tag"]').fill("v0.2.0");
    await ota.getByRole("button", { name: "导入版本", exact: true }).click();
    await expect.poll(() => poolReads).toBeGreaterThan(1);
    await expect(concurrency).toHaveValue("7");
  } finally { release(); }
});
