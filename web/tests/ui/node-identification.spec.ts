/**
 * [INPUT]: 依赖 Playwright、节点夹具和工作空间路由。
 * [OUTPUT]: 验证设备身份、独立重命名、草稿与边框取整下的溢出。
 * [POS]: 节点命名的浏览器行为回归；API 拦截只替代远端服务。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, nodes, tenantURL, tenantId, tenantSlug } from "./fixtures";
import { themeStorage } from "../../src/lib/preferences";

async function setup(page: Page, role: "admin" | "viewer" | "operator" = "admin") {
  await fixtureAPI(page, role);
  await page.context().addCookies([{ name: "artifactory-locale", value: "en", url: "http://127.0.0.1" }]);
  let node = {
    ...nodes[0],
    identity: { hostname: "build-mac.local", device_model: "Mac16,9", device_vendor: "Apple", cpu_model: "Apple M4 Max", memory_total_bytes: 68719476736 },
    diagnostics: { ...nodes[0].diagnostics, network_interfaces: { sampled_at: "2026-11-20T01:02:03Z", interfaces: [{ name: "en0", addresses: ["192.168.11.10", "fd12::10"] }], truncated: true } },
  };
  const writes: unknown[] = [];
  let fail = false;
  await page.route("**/api/v1/nodes?*", route => {
    const q = new URL(route.request().url()).searchParams.get("q");
    return route.fulfill({ json: q && !node.name.includes(q) ? [] : [node, nodes[1]] });
  });
  await page.route("**/api/v1/nodes/*/name", async route => {
    expect(route.request().method()).toBe("PATCH");
    const body = route.request().postDataJSON();
    writes.push(body);
    if (fail) return route.fulfill({ status: 503, json: { error: "Rename unavailable" } });
    node = { ...node, name: body.name };
    return route.fulfill({ json: node });
  });
  return { writes, fail: () => { fail = true; }, node: () => node, updateName: (name: string) => { node = { ...node, name }; } };
}

async function open(page: Page) {
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "View details for Mac Studio", exact: true }).click();
  return page.getByRole("dialog").first();
}

test("rename trims only the name, refreshes the card and restores action focus", async ({ page }) => {
  const state = await setup(page);
  const dialog = await open(page);
  await dialog.getByRole("button", { name: "Rename node", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Node name", exact: true });
  await expect(input).toBeFocused();
  await input.fill("  Lab Mac  ");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(dialog.getByRole("heading", { name: /Lab Mac/ })).toBeVisible();
  expect(state.writes).toEqual([{ name: "Lab Mac" }]);
  await expect(dialog.getByRole("button", { name: "Rename node", exact: true })).toBeFocused();
  await expect(page.locator('[data-notify][data-kind="success"]')).toContainText("Node name updated");
  await page.keyboard.press("Escape");
  await expect(page.locator(".node-card").first()).toContainText("Lab Mac");
});

test("failed rename preserves input and polling cannot reset a draft", async ({ page }) => {
  const state = await setup(page);
  await page.clock.install();
  const dialog = await open(page);
  await dialog.getByRole("button", { name: "Rename node", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Node name", exact: true });
  await input.fill("Unfinished draft");
  state.updateName("Remote update");
  await page.clock.fastForward(10001);
  await expect(page.locator(".node-card").first()).toContainText("Remote update");
  await expect(input).toHaveValue("Unfinished draft");
  state.fail();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Rename unavailable");
  await expect(input).toHaveValue("Unfinished draft");
  await expect(input).toBeFocused();
  page.once("dialog", dialog => dialog.dismiss());
  await page.keyboard.press("Escape");
  await expect(input).toBeVisible();
  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(input).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "Rename node", exact: true })).toBeFocused();
});

test("hostname action and UTF8 validation protect the name endpoint", async ({ page }) => {
  const state = await setup(page);
  const dialog = await open(page);
  await dialog.getByRole("button", { name: "Rename node", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Node name", exact: true });
  await page.getByRole("button", { name: "Use current hostname", exact: true }).click();
  await expect(input).toHaveValue("build-mac.local");
  for (const value of [" ", "节".repeat(34), "bad\u007fname"]) {
    await input.fill(value);
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(input).toHaveAttribute("aria-invalid", "true");
    expect(state.writes).toEqual([]);
  }
  await input.fill("节".repeat(33) + "a");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(input).toHaveCount(0);
  expect(state.writes).toEqual([{ name: "节".repeat(33) + "a" }]);
});

test("Unicode whitespace normalization matches the server and preserves BOM", async ({ page }) => {
  const state = await setup(page);
  const dialog = await open(page);
  for (const value of ["\u0085Unicode node\u0085", "\uFEFFBOM node\uFEFF"]) {
    await dialog.getByRole("button", { name: "Rename node", exact: true }).click();
    await page.getByRole("textbox", { name: "Node name", exact: true }).fill(value);
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(page.getByRole("textbox", { name: "Node name", exact: true })).toHaveCount(0);
  }
  expect(state.writes).toEqual([{ name: "Unicode node" }, { name: "\uFEFFBOM node\uFEFF" }]);
});

for (const role of ["viewer", "operator"] as const) {
  test(`${role} cannot rename`, async ({ page }) => {
    const state = await setup(page, role);
    const dialog = await open(page);
    await expect(dialog.getByRole("button", { name: "Rename node", exact: true })).toHaveCount(0);
    expect(state.writes).toEqual([]);
  });
}

test("archived tenant is read only for an administrator", async ({ page }) => {
  await setup(page);
  await page.route("**/api/v1/tenant", route => route.fulfill({ json: {
    tenant: { id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", status: "archived", owner_id: "user-main" }, project_roles: {},
  } }));
  const dialog = await open(page);
  await expect(dialog.getByRole("button", { name: "Rename node", exact: true })).toHaveCount(0);
});

test("renaming out of the current search closes the selected detail", async ({ page }) => {
  await setup(page);
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("textbox", { name: "Filter current list" }).fill("Mac Studio");
  await page.getByRole("button", { name: "View details for Mac Studio", exact: true }).click();
  const dialog = page.getByRole("dialog").first();
  await dialog.getByRole("button", { name: "Rename node", exact: true }).click();
  await page.getByRole("textbox", { name: "Node name", exact: true }).fill("Linux lab");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('[data-notify][data-kind="info"]')).toContainText("no longer in the current list");
});

for (const theme of ["light", "dark"]) {
  test(`${theme}: list uses identity without per-card reads, details retain network snapshot`, async ({ page }, testInfo) => {
    await setup(page);
    await page.addInitScript(({ themeStorage, theme }) => localStorage.setItem(themeStorage, theme), { themeStorage, theme });
    const requests: string[] = [];
    page.on("request", request => { if (/\/nodes\/[^/?]+\//.test(request.url())) requests.push(request.url()); });
    await page.goto(tenantURL("/nodes"));
    const card = page.locator(".node-card").first();
    await expect(card).toContainText("build-mac.local");
    await expect(card).toContainText("node-mai");
    await expect(card).toContainText("Mac16,9");
    await expect(card).toContainText("Apple M4 Max");
    await expect(card).toContainText("64.0 GiB");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`identity-list-${theme}.png`), fullPage: true });
    expect(requests).toEqual([]);
    await expect(page.getByRole("textbox", { name: "Filter current list" })).toHaveAttribute("placeholder", /hostname.*model.*CPU.*ID/);
    await page.getByRole("button", { name: "View details for Mac Studio", exact: true }).click();
    const dialog = page.getByRole("dialog").first();
    await expect(dialog).toContainText("build-mac.local");
    await expect(dialog).toContainText("Mac16,9");
    await expect(dialog).toContainText("en0");
    await expect(dialog).toContainText("192.168.11.10");
    await expect(dialog).toContainText("IPv6");
    await expect(dialog).toContainText("fd12::10");
    await expect(dialog).toContainText("Snapshot truncated");
    await expect(dialog.locator('time[datetime="2026-11-20T01:02:03Z"]')).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath(`identity-${theme}.png`), fullPage: true });
  });
}

test("old nodes disable hostname copy; duplicate names and long hardware remain distinguishable", async ({ page }) => {
  await setup(page);
  const long = "machine-model-".repeat(40);
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [
    { ...nodes[0], id: "12345678-aaaa", name: "Shared name", identity: { device_model: long, cpu_model: long } },
    { ...nodes[1], id: "87654321-bbbb", name: "Shared name", diagnostics: { hostname: "legacy-host" } },
  ] }));
  await page.route("**/api/v1/nodes/12345678-aaaa/telemetry", route => route.fulfill({ json: { host: null, inventory: null } }));
  await page.goto(tenantURL("/nodes"));
  const cards = page.locator(".node-card");
  await expect(cards.nth(0)).toContainText("12345678");
  await expect(cards.nth(1)).toContainText("87654321");
  await expect(cards.nth(1)).toContainText("legacy-host");
  await expect(cards.nth(0).getByText(long, { exact: true })).toHaveCount(1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await cards.nth(0).getByRole("button").click();
  const dialog = page.getByRole("dialog").first();
  await expect(dialog.getByText(long, { exact: true }).first()).toBeVisible();
  await dialog.getByRole("button", { name: "Rename node", exact: true }).click();
  await expect(page.getByRole("button", { name: "Use current hostname", exact: true })).toBeDisabled();
  await expect(page.getByRole("dialog").last()).toContainText("No hostname has been reported");
  // 玻璃边框的亚像素取整产生 1px 差值；内容仍须在面板内。
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
});

test("Chinese copy and offline last network snapshot remain available", async ({ page }) => {
  const state = await setup(page);
  await page.context().addCookies([{ name: "artifactory-locale", value: "zh-CN", url: "http://127.0.0.1" }]);
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...state.node(), online: false }] }));
  await page.goto(tenantURL("/nodes"));
  await page.locator(".node-card").first().getByRole("button").click();
  const dialog = page.getByRole("dialog").first();
  await expect(dialog).toContainText("192.168.11.10");
  await expect(dialog).toContainText("fd12::10");
  await dialog.getByRole("button", { name: "重命名节点", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "节点名称", exact: true })).toBeFocused();
  await expect(page.getByRole("button", { name: "使用当前主机名", exact: true })).toBeEnabled();
});
