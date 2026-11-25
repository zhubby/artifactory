/**
 * [INPUT]: 依赖节点、统一版本、升级HTTP夹具与主题键。
 * [OUTPUT]: 验证显式发起、条件围栏、确认稳定性和失败恢复。
 * [POS]: 节点手动OTA的浏览器回归；不访问真实设备。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { fixtureAPI, nodes, tenantURL } from "./fixtures";
import { themeStorage } from "../../src/lib/preferences";

const version = { product_version: "0.2.0", release_id: "current", release_version: "0.2.0", release_status: "ready" };
const node = { ...nodes[0], capabilities: ["emissary-ota-v1"], emissary_current_version: "0.1.0" };
async function setup(page: Page, overrides = {}, release = version, role: "admin" | "viewer" = "admin") {
  const events = await fixtureAPI(page, role);
  await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: release }));
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...node, ...overrides }] }));
  return events;
}
async function operations(page: Page) {
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "查看 Mac Studio 的详情", exact: true }).click();
  await page.getByRole("tab", { name: "运维", exact: true }).click();
  return page.getByTestId("node-ota");
}

test("version differences and polling only prompt; one confirmation requests the fixed product version", async ({ page }) => {
  const events = await setup(page);
  const writes: { method: string; body: unknown; csrf: string | undefined }[] = [];
  let accepted = false;
  let reads = 0;
  await page.route("**/api/v1/emissary-updates", route => {
    reads++; return route.fulfill({ json: accepted ? [{ id: "requested", release_id: "current", node_id: node.id, version: "0.2.0", status: "queued", finished_at: null }] : [] });
  });
  await page.route(`**/api/v1/nodes/${node.id}/emissary-update`, async route => {
    writes.push({ method: route.request().method(), body: route.request().postDataJSON(), csrf: route.request().headers()["x-artifactory-csrf"] });
    accepted = true; await route.fulfill({ json: { id: "requested" } });
  });
  const panel = await operations(page);
  await expect(panel).toContainText("有可用升级，等待管理员操作");
  await expect.poll(() => reads).toBeGreaterThan(1);
  expect(events.writes).toEqual([]);
  expect(writes).toEqual([]);
  const trigger = panel.getByRole("button", { name: "升级至 0.2.0", exact: true });
  await trigger.click();
  const confirmation = page.getByRole("dialog", { name: "确认升级 Emissary", exact: true });
  await expect(confirmation).toContainText("Artifactory 0.2.0");
  await expect(confirmation).toContainText("等待当前构建、产物和日志确认完成");
  await expect(confirmation).toContainText("自动恢复原版本");
  await expect(confirmation.getByRole("combobox")).toHaveCount(0);
  await expect(confirmation.getByRole("textbox")).toHaveCount(0);
  expect(writes).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await trigger.click();
  await confirmation.getByRole("button", { name: "确认升级", exact: true }).click();
  await expect(confirmation).toHaveCount(0);
  await expect(panel).toContainText("等待执行");
  await expect(panel.getByRole("button", { name: "升级至 0.2.0", exact: true })).toHaveCount(0);
  expect(writes).toEqual([{ method: "POST", body: { product_version: "0.2.0", release_id: "current" }, csrf: "1" }]);
});

test("a failed request keeps confirmation open and a busy request cannot be duplicated", async ({ page }) => {
  await setup(page);
  let calls = 0;
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**/api/v1/nodes/${node.id}/emissary-update`, async route => {
    calls++;
    if (calls === 1) return route.fulfill({ status: 409, json: { error: "Pool concurrency limit reached" } });
    await pending; return route.fulfill({ json: { id: "requested" } });
  });
  const panel = await operations(page);
  await panel.getByRole("button", { name: "升级至 0.2.0", exact: true }).click();
  const confirmation = page.getByRole("dialog", { name: "确认升级 Emissary", exact: true });
  const submit = confirmation.getByRole("button", { name: "确认升级", exact: true });
  await submit.click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Pool concurrency limit reached");
  await expect(confirmation).toBeVisible();
  await expect(submit).toBeEnabled();
  await submit.click();
  await expect(confirmation.getByRole("button", { name: "正在处理…", exact: true })).toBeDisabled();
  await expect(confirmation.getByRole("button", { name: "取消", exact: true })).toBeDisabled();
  await expect(confirmation.getByRole("button", { name: "关闭对话框", exact: true })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(confirmation).toBeVisible();
  expect(calls).toBe(2);
  release();
  await expect(confirmation).toHaveCount(0);
  expect(calls).toBe(2);
});

for (const [name, overrides, release, reason, hidden] of [
  ["offline", { online: false }, version, "节点离线", false],
  ["paused", { emissary_pool_paused: true }, version, "后续升级已暂停", false],
  ["expired", { certificate_expires_at: "2020-01-01T00:00:00Z" }, version, "节点证书已过期", false],
  ["bootstrap", { capabilities: [] }, version, "需在节点上完成一次 OTA 初始化", true],
  ["revoked", { revoked: true }, version, "节点授权已吊销", true],
  ["aligned", { emissary_current_version: "0.2.0+build.123" }, version, "已是当前版本", true],
  ["missing", {}, { ...version, release_status: "missing", release_id: null }, "等待对应安装包", false],
  ["ambiguous", {}, { ...version, release_status: "ambiguous", release_id: null }, "存在多个对应发行", false],
] as const) {
  test(`${name}: unavailable conditions cannot request an update`, async ({ page }) => {
    const events = await setup(page, overrides, release as typeof version);
    const panel = await operations(page);
    await expect(panel).toContainText(reason);
    const button = panel.getByRole("button", { name: "升级至 0.2.0", exact: true });
    if (hidden) await expect(button).toHaveCount(0); else await expect(button).toBeDisabled();
    expect(events.writes).toEqual([]);
  });
}

test("viewers see a version difference without an upgrade action", async ({ page }) => {
  const events = await setup(page, {}, version, "viewer");
  const panel = await operations(page);
  await expect(panel).toContainText("有可用升级，等待管理员操作");
  await expect(panel.getByRole("button", { name: "升级至 0.2.0", exact: true })).toHaveCount(0);
  expect(events.writes).toEqual([]);
});

test("a target change during confirmation requires a fresh confirmation", async ({ page }) => {
  await page.clock.install();
  const events = await setup(page);
  let changed = false;
  await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: changed ? { ...version, product_version: "0.3.0", release_id: "next" } : version }));
  const panel = await operations(page);
  await panel.getByRole("button", { name: "升级至 0.2.0", exact: true }).click();
  const confirmation = page.getByRole("dialog", { name: "确认升级 Emissary", exact: true });
  changed = true;
  await page.clock.fastForward(31_000);
  await expect(confirmation).toContainText("节点状态或目标发行已变化");
  await expect(confirmation).toContainText("Artifactory 0.2.0");
  await expect(confirmation.getByRole("button", { name: "确认升级", exact: true })).toBeDisabled();
  expect(events.writes).toEqual([]);
  await confirmation.getByRole("button", { name: "取消", exact: true }).click();
  await panel.getByRole("button", { name: "升级至 0.3.0", exact: true }).click();
  await expect(confirmation).toContainText("Artifactory 0.3.0");
  await expect(confirmation.getByRole("button", { name: "确认升级", exact: true })).toBeEnabled();
});

test("a server version change rejects the frozen confirmation and cannot select a different target", async ({ page }) => {
  await setup(page);
  const writes: unknown[] = [];
  await page.route(`**/api/v1/nodes/${node.id}/emissary-update`, route => {
    writes.push(route.request().postDataJSON());
    return route.fulfill({ status: 409, json: { error: "The Artifactory version changed; review the update target" } });
  });
  const panel = await operations(page);
  await panel.getByRole("button", { name: "升级至 0.2.0", exact: true }).click();
  const confirmation = page.getByRole("dialog", { name: "确认升级 Emissary", exact: true });
  await confirmation.getByRole("button", { name: "确认升级", exact: true }).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("The Artifactory version changed");
  await expect(confirmation).toBeVisible();
  expect(writes).toEqual([{ product_version: "0.2.0", release_id: "current" }]);
});

test("an accepted update keeps cancellation and cannot be requested again", async ({ page }) => {
  await setup(page);
  let canceled = false;
  const writes: string[] = [];
  await page.route("**/api/v1/emissary-updates", route => route.fulfill({ json: [{ id: "pending", release_id: "current", node_id: node.id, version: "0.2.0", status: canceled ? "canceled" : "queued", finished_at: canceled ? "2026-11-25T00:00:00Z" : null }] }));
  await page.route("**/api/v1/emissary-updates/pending/cancel", route => {
    writes.push(route.request().method()); canceled = true; return route.fulfill({ json: {} });
  });
  const panel = await operations(page);
  await expect(panel.getByRole("button", { name: "升级至 0.2.0", exact: true })).toHaveCount(0);
  await panel.getByRole("button", { name: "取消待执行升级", exact: true }).click();
  await expect(panel.getByRole("button", { name: "升级至 0.2.0", exact: true })).toBeVisible();
  expect(writes).toEqual(["POST"]);
});

for (const locale of ["en", "zh-CN"]) for (const theme of ["light", "dark"]) {
  test(`${locale} ${theme}: update confirmation fits and meets WCAG AA`, async ({ page }) => {
    await setup(page);
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: "http://127.0.0.1:3100" }]);
    await page.addInitScript(({ themeStorage, theme }) => localStorage.setItem(themeStorage, theme), { themeStorage, theme });
    await page.goto(tenantURL("/nodes"));
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await page.getByRole("button", { name: locale === "en" ? "View details for Mac Studio" : "查看 Mac Studio 的详情", exact: true }).click();
    await page.getByRole("tab", { name: locale === "en" ? "Operations" : "运维", exact: true }).click();
    await page.getByTestId("node-ota").getByRole("button", { name: locale === "en" ? "Update to 0.2.0" : "升级至 0.2.0", exact: true }).click();
    const confirmation = page.getByRole("dialog", { name: locale === "en" ? "Confirm Emissary update" : "确认升级 Emissary", exact: true });
    await expect(confirmation).toBeVisible();
    expect(await confirmation.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
    const accessibility = await new AxeBuilder({ page }).include('[data-app-modal]:last-of-type').withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(accessibility.violations).toEqual([]);
    await confirmation.screenshot({ path: test.info().outputPath(`manual-update-${locale}-${theme}.png`) });
  });
}
