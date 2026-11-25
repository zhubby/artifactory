/**
 * [INPUT]: 依赖节点、Artifactory 统一版本 API 与升级夹具。
 * [OUTPUT]: 验证统一版本、同版本重试围栏与历史操作边界。
 * [POS]: 运维设置和节点升级的浏览器回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, nodes, tenantURL } from "./fixtures";

const version = { product_version: "0.2.0", release_id: "current", release_version: "0.2.0", release_status: "ready" };
async function operations(page: Page) {
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "查看 Mac Studio 的详情", exact: true }).click();
  await page.getByRole("tab", { name: "运维", exact: true }).click();
  return page.getByTestId("node-ota");
}

for (const role of ["admin", "viewer"] as const) {
  test(`${role}: unified OTA controls respect role and remove manual version targets`, async ({ page }) => {
    const events = await fixtureAPI(page, role);
    const panel = await operations(page);
    await expect(panel).toContainText("需要安装 OTA 支持");
    await expect(panel).toContainText("Artifactory 统一版本");
    await expect(panel).toContainText("0.2.0");
    await expect(page.getByRole("button", { name: "指定版本 / 回退版本" })).toHaveCount(0);
    await page.goto(tenantURL("/settings/emissary"));
    if (role === "admin") {
      const settings = page.getByTestId("emissary-settings");
      await expect(settings).toContainText("节点池升级策略");
      await expect(settings.getByTestId("product-version")).toHaveText("0.2.0");
      await expect(settings).toContainText("尚未导入 Artifactory 0.2.0 对应的签名发行");
      await expect(settings.getByRole("combobox")).toHaveCount(0);
      await expect(page.getByRole("spinbutton", { name: "同时升级节点数" })).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width + 1);
      await page.getByRole("textbox", { name: "GitHub Release 标签" }).fill("emissary-v0.2.0");
      await page.getByRole("button", { name: "导入版本", exact: true }).click();
      await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("UI fixtures do not accept writes");
      expect(events.writes).toEqual(["POST /emissary-releases"]);
    } else {
      await expect(page.getByText("此页面需要管理员权限。", { exact: true })).toBeVisible();
      await expect(page.getByTestId("emissary-settings")).toHaveCount(0);
      expect(events.writes).toEqual([]);
    }
  });
}

for (const current of ["0.2.0", "v0.2.0", "emissary-v0.2.0", "0.2.0+terminal.123"]) {
  test(`${current} matches the Artifactory product version`, async ({ page }) => {
    const events = await fixtureAPI(page);
    await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: version }));
    await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...nodes[0], capabilities: ["emissary-ota-v1"], emissary_current_version: current }] }));
    await page.route("**/api/v1/emissary-updates", route => route.fulfill({ json: [{ id: "current-failed", release_id: "current", node_id: nodes[0].id, version: "0.2.0", status: "failed", finished_at: "2026-11-25T00:00:00Z" }] }));
    const panel = await operations(page);
    await expect(panel).toContainText("已是当前版本");
    await expect(panel.getByRole("combobox")).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "重试", exact: true })).toHaveCount(0);
    expect(events.writes).toEqual([]);
  });
}

for (const status of ["ready", "missing", "ambiguous"] as const) {
  test(`${status}: a mismatched node reports the matching release readiness`, async ({ page }) => {
    await fixtureAPI(page);
    await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: { ...version, release_status: status, release_id: status === "ready" ? "current" : null } }));
    await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...nodes[0], capabilities: ["emissary-ota-v1"], emissary_current_version: "0.1.0" }] }));
    const panel = await operations(page);
    await expect(panel).toContainText(status === "ready" ? "等待升级" : "等待对应安装包");
    await expect(panel).toContainText(status === "ready" ? "签名发行已就绪" : status === "missing" ? "尚未导入 Artifactory 0.2.0" : "存在多个对应发行");
  });
}

for (const current of ["0.2.00", "0.2.0+", "0.2.0-rc.1", "0.3.0"]) {
  test(`${current} does not falsely match the product version`, async ({ page }) => {
    await fixtureAPI(page);
    await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: version }));
    await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...nodes[0], capabilities: ["emissary-ota-v1"], emissary_current_version: current }] }));
    const panel = await operations(page);
    await expect(panel).toContainText("等待升级");
    await expect(panel).not.toContainText("已是当前版本");
  });
}

test("pool policy saves only concurrency and pause while following the Artifactory version", async ({ page }) => {
  await fixtureAPI(page);
  const writes: unknown[] = [];
  await page.route("**/api/v1/pools/pool-main/emissary-update", route => {
    writes.push(route.request().postDataJSON());
    return route.fulfill({ json: { ok: true } });
  });
  await page.goto(tenantURL("/settings/emissary"));
  const policy = page.locator("section.panel").filter({ has: page.getByRole("heading", { name: "节点池升级策略", exact: true }) });
  await policy.getByRole("spinbutton", { name: "同时升级节点数" }).fill("3");
  await policy.getByRole("checkbox", { name: "暂停后续升级" }).check();
  await policy.getByRole("button", { name: "保存", exact: true }).click();
  expect(writes).toEqual([{ release_id: null, concurrency: 3, paused: true }]);
});

test("a paused mismatched node keeps the fixed product version", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: version }));
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...nodes[0], capabilities: ["emissary-ota-v1"], emissary_current_version: "0.1.0", emissary_pool_paused: true }] }));
  const panel = await operations(page);
  await expect(panel).toContainText("后续升级已暂停");
  await expect(panel.getByText("0.2.0", { exact: true })).toBeVisible();
});

test("an in-flight historical update stays frozen and only the current release can retry", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/emissary-version", route => route.fulfill({ json: version }));
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [{ ...nodes[0], capabilities: ["emissary-ota-v1"], emissary_current_version: "0.1.0", ota_draining: true }] }));
  await page.route("**/api/v1/emissary-updates", route => route.fulfill({ json: [{ id: "retry", release_id: "old", node_id: nodes[0].id, version: "0.1.5", status: "activating", finished_at: null }] }));
  const panel = await operations(page);
  await expect(panel.getByText("0.1.5", { exact: true })).toBeVisible();
  await expect(panel.getByText("0.2.0", { exact: true })).toBeVisible();
  await expect(panel).toContainText("正在验证新版本");
  await expect(panel.getByRole("button", { name: "重试", exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.route("**/api/v1/emissary-updates", route => route.fulfill({ json: [
    { id: "old-failed", release_id: "old", node_id: nodes[0].id, node_name: "Old failure", previous_version: "0.1.0", version: "0.1.5", status: "failed", finished_at: "2026-11-25T00:00:00Z", updated_at: "2026-11-25T00:00:00Z" },
    { id: "current-failed", release_id: "current", node_id: nodes[0].id, node_name: "Current failure", previous_version: "0.1.0", version: "0.2.0", status: "failed", finished_at: "2026-11-25T00:00:00Z", updated_at: "2026-11-25T00:00:00Z" },
  ] }));
  await page.goto(tenantURL("/settings/emissary"));
  await expect(page.getByRole("row").filter({ hasText: "Old failure" }).getByRole("button", { name: "重试", exact: true })).toHaveCount(0);
  await expect(page.getByRole("row").filter({ hasText: "Current failure" }).getByRole("button", { name: "重试", exact: true })).toBeVisible();
});
