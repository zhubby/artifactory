/**
 * [INPUT]: 依赖 Playwright、节点夹具和明暗主题偏好。
 * [OUTPUT]: 验证紧凑标题、设备摘要、渐进展开及四种宽度的阅读边界。
 * [POS]: 设备详情的信息层级及五个标签的确定性视觉验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Locator, type Page } from "@playwright/test";
import type { Node, NodeTelemetry } from "../../src/lib/types";
import { themeStorage } from "../../src/lib/preferences";
import { fixtureAPI, nodes, nodeTelemetry, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

const ipv6 = "2001:0db8:1234:5678:9abc:def0:1234:5678";

async function setup(page: Page, theme = "light", name = "Mac Studio") {
  const events = await fixtureAPI(page);
  await page.context().addCookies([
    { name: "artifactory-locale", value: "zh-CN", url: "http://127.0.0.1" },
  ]);
  await page.addInitScript(({ themeStorage, theme }) => {
    localStorage.setItem(themeStorage, theme);
  }, { themeStorage, theme });
  let node: Node = {
    ...nodes[0],
    name,
    identity: {
      hostname: "build-mac.local",
      device_model: "Mac16,9",
      device_vendor: "Apple",
      cpu_model: "Apple M4 Max",
      memory_total_bytes: 68719476736,
    },
    diagnostics: {
      ...nodes[0].diagnostics,
      network_interfaces: {
        sampled_at: "2026-11-20T01:02:03Z",
        interfaces: [
          { name: "en0", addresses: ["192.168.11.10", ipv6] },
          { name: "utun3", addresses: ["fd12::10"] },
        ],
        truncated: false,
      },
    },
  };
  let telemetry: NodeTelemetry = structuredClone(nodeTelemetry);
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [node] }));
  await page.route("**/api/v1/nodes/*/telemetry", route => route.fulfill({ json: telemetry }));
  return {
    events,
    updateName(value: string) { node = { ...node, name: value }; },
    updateTelemetry(value: NodeTelemetry) { telemetry = value; },
  };
}

async function open(page: Page) {
  await page.goto(tenantURL("/nodes"));
  await page.locator(".node-card").first().getByRole("button").click();
  const dialog = page.getByRole("dialog").first();
  await expect(dialog.locator(".node-summary-grid")).toContainText("Apple M4 Max");
  return dialog;
}

function disclosure(dialog: Locator, title: string) {
  return dialog.locator("details").filter({
    has: dialog.page().locator("summary").filter({ hasText: title }),
  }).first();
}

async function noOverflow(page: Page, dialog: Locator) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
  const rect = (await dialog.boundingBox())!;
  expect(rect.x).toBeGreaterThanOrEqual(0);
  expect(rect.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  expect(rect.height).toBeLessThanOrEqual(page.viewportSize()!.height);
}

for (const theme of ["light", "dark"]) {
  test(`${theme}: device summary and five tabs keep a clear reading hierarchy`, async ({ page }, info) => {
    const { events } = await setup(page, theme);
    const dialog = await open(page);
    const summary = dialog.locator(".node-summary-grid");
    await expect(summary).toContainText("Mac16,9");
    await expect(summary).toContainText("64.0 GiB");
    await expect(summary).toContainText("1 / 2");
    const rename = dialog.getByRole("button", { name: "重命名节点", exact: true });
    await expect(rename).toHaveAttribute("title", "重命名节点");
    const nameBox = (await dialog.locator(".node-detail-name").boundingBox())!;
    const renameBox = (await rename.boundingBox())!;
    expect(Math.abs(nameBox.y + nameBox.height / 2 - renameBox.y - renameBox.height / 2)).toBeLessThan(2);
    for (const title of ["更多硬件信息", "身份与连接"]) {
      await expect(disclosure(dialog, title)).not.toHaveAttribute("open", "");
    }
    if (info.project.name === "desktop") await expect(summary).toBeInViewport({ ratio: 1 });
    await expect(dialog.getByText("192.168.11.10", { exact: true })).toBeVisible();
    await noOverflow(page, dialog);
    await dialog.screenshot({ path: `test-results/ui/device-detail-overview-${theme}-${info.project.name}.png` });

    for (const [tab, key, content] of [
      ["资源", "resources", "CPU"],
      ["工具链", "tools", "Xcode 26.0"],
      ["元数据", "metadata", "custom.site"],
      ["运维", "operations", "Emissary"],
    ]) {
      await dialog.getByRole("tab", { name: tab, exact: true }).click();
      await expect(dialog.getByRole("tabpanel")).toContainText(content);
      if (key === "resources") {
        await expect(disclosure(dialog, "采样诊断")).not.toHaveAttribute("open", "");
        await expect(dialog.getByText("session-main", { exact: true })).toBeHidden();
      }
      if (key === "metadata") {
        await expect(disclosure(dialog, "快照信息")).not.toHaveAttribute("open", "");
        await expect(dialog.getByText("startup-main", { exact: true })).toBeHidden();
      }
      if (key === "operations") {
        await expect(disclosure(dialog, "运行诊断")).not.toHaveAttribute("open", "");
      }
      await noOverflow(page, dialog);
      await dialog.screenshot({ path: `test-results/ui/device-detail-${key}-${theme}-${info.project.name}.png` });
    }
    expect(events.writes).toEqual([]);
  });
}

test("optional fields keep zero, scalar metadata keeps false, and JSON expands on demand", async ({ page }) => {
  const state = await setup(page);
  const telemetry = structuredClone(nodeTelemetry);
  telemetry.inventory!.inventory.effective_cpu_limit = 0;
  telemetry.inventory!.inventory.kernel_version = null;
  telemetry.inventory!.inventory.virtualization = null;
  state.updateTelemetry(telemetry);
  await page.route("**/api/v1/nodes/*/metadata", route => route.fulfill({ json: {
    startup_id: "snapshot-instance",
    digest: "e".repeat(64),
    received_at: "2026-11-20T01:02:03Z",
    metadata: { "custom.zero": 0, "custom.enabled": false, "custom.settings": { retry: false, count: 0 } },
  } }));
  const dialog = await open(page);
  const hardware = disclosure(dialog, "更多硬件信息");
  await hardware.locator("summary").first().click();
  const cpu = hardware.locator(".node-facts > div").filter({ has: page.getByText("有效 CPU 配额", { exact: true }) });
  await expect(cpu.locator("dd")).toHaveText("0");
  await expect(hardware.getByText("内核版本", { exact: true })).toHaveCount(0);
  await expect(hardware.getByText("虚拟化环境", { exact: true })).toHaveCount(0);
  await dialog.getByRole("tab", { name: "元数据", exact: true }).click();
  const metadata = dialog.locator(".node-metadata-list");
  await expect(metadata.locator("dd").filter({ hasText: /^false$/ })).toBeVisible();
  await expect(metadata.locator("dd").filter({ hasText: /^0$/ })).toBeVisible();
  const complex = disclosure(dialog, "custom.settings");
  await expect(complex.locator("pre")).toBeHidden();
  await complex.locator("summary").first().click();
  await expect(complex.locator("pre")).toContainText('"retry": false');
  await expect(complex.locator("pre")).toContainText('"count": 0');
  const snapshot = disclosure(dialog, "快照信息");
  await snapshot.locator("summary").first().click();
  await expect(snapshot.getByText("snapshot-instance", { exact: true })).toBeVisible();
});

test("graphics resources present the model separately from the identifier and distinguish missing metrics from zero", async ({ page }, info) => {
  if (info.project.name === "mobile") await page.setViewportSize({ width: 320, height: 800 });
  const state = await setup(page);
  const telemetry = structuredClone(nodeTelemetry);
  telemetry.inventory!.inventory.accelerators = [{ id: "display-0", model: "Apple M4 Max" }];
  telemetry.host!.sample.accelerators = [
    { id: "display-0", utilization_percent: null, memory_used_bytes: null },
    { id: "gpu-missing-model", utilization_percent: 0, memory_used_bytes: 0 },
  ];
  state.updateTelemetry(telemetry);
  const dialog = await open(page);
  await dialog.getByRole("tab", { name: "资源", exact: true }).click();
  const graphics = dialog.locator(".node-detail-section").filter({
    has: page.getByRole("heading", { name: "图形设备", exact: true }),
  });
  await expect(graphics.getByRole("columnheader")).toHaveText(["型号", "GPU 利用率", "已用显存"]);
  const apple = graphics.getByRole("row").filter({ hasText: "display-0" });
  await expect(apple.getByText("Apple M4 Max", { exact: true })).toBeVisible();
  await expect(apple.getByText("设备 ID · display-0", { exact: true })).toBeVisible();
  await expect(apple.getByRole("cell", { name: "未上报", exact: true })).toHaveCount(2);
  const unknown = graphics.getByRole("row").filter({ hasText: "gpu-missing-model" });
  await expect(unknown.getByText("未知", { exact: true })).toBeVisible();
  await expect(unknown.getByRole("cell", { name: "0%", exact: true })).toBeVisible();
  await expect(unknown.getByRole("cell", { name: "0 B", exact: true })).toBeVisible();
  await noOverflow(page, dialog);
});

test("resource trends load on demand, preserve gaps, and switch and refresh their window", async ({ page }, info) => {
  if (info.project.name === "mobile") await page.setViewportSize({ width: 320, height: 800 });
  const theme = info.project.name === "tablet" ? "dark" : "light";
  await setup(page, theme);
  const queries: number[] = [];
  let retryShortRange = false;
  await page.route("**/api/v1/nodes/*/telemetry/history?*", route => {
    const url = new URL(route.request().url());
    expect(url.pathname).toContain(`/nodes/${nodes[0].id}/`);
    expect(url.searchParams.get("resolution")).toBe("minute");
    const start = Date.parse(url.searchParams.get("from")!);
    const end = Date.parse(url.searchParams.get("to")!);
    const minutes = (end - start) / 60_000;
    queries.push(minutes);
    if (minutes === 15 && !retryShortRange) {
      retryShortRange = true;
      return route.fulfill({ status: 503, json: { error: "临时无法读取趋势" } });
    }
    const samples = minutes === 15 ? [] : Array.from({ length: minutes }, (_, index) => ({
      bucket: new Date(start + index * 60_000).toISOString(),
      cpu_avg: index === 0 ? 0 : 12 + Math.abs(Math.sin(index / 6)) * 48 + (index % 4) * 3,
      memory_avg: (10 + Math.min(index / 4, 6) + Math.sin(index / 6)) * 1024 ** 3,
    })).filter((_, index) => index !== 18 && index !== 19);
    return route.fulfill({ json: { samples, truncated: false } });
  });

  const dialog = await open(page);
  expect(queries).toEqual([]);
  await dialog.getByRole("tab", { name: "资源", exact: true }).click();
  const trends = dialog.getByRole("region", { name: "资源趋势", exact: true });
  await expect(trends.locator(".recharts-area-curve")).toHaveCount(2);
  const curves = await trends.locator(".recharts-area-curve").evaluateAll(paths => paths.map(path => path.getAttribute("d")));
  expect(curves.map(curve => curve?.match(/M/g)?.length)).toEqual([2, 2]);
  expect(curves.every(curve => curve?.includes("C"))).toBe(true);
  expect(queries).toEqual([60]);
  const plots = await trends.locator(".node-trend-card").evaluateAll(cards => cards.map(card => card.getBoundingClientRect().toJSON()));
  expect(Math.abs(plots[0].width - plots[1].width)).toBeLessThanOrEqual(1);
  if (info.project.name === "mobile") expect(plots[1].top).toBeGreaterThan(plots[0].bottom);
  else expect(plots[0].top).toBe(plots[1].top);
  await noOverflow(page, dialog);
  await trends.screenshot({ path: info.outputPath(`node-resource-trends-${theme}.png`) });

  const range = trends.getByRole("combobox", { name: "趋势时间范围", exact: true });
  await chooseOption(range, "15");
  await expect(trends.getByText("数据暂不可用", { exact: false })).toBeVisible();
  await expect(dialog.getByText("48.0 GiB", { exact: true })).toBeVisible();
  await trends.getByRole("button", { name: "重新加载", exact: true }).click();
  await expect(trends.getByText("所选时间内暂无采样", { exact: true })).toHaveCount(2);
  await chooseOption(range, "360");
  await expect(trends.locator(".recharts-area-curve")).toHaveCount(2);
  await trends.getByRole("button", { name: "刷新趋势", exact: true }).click();
  await expect.poll(() => queries).toEqual([60, 15, 15, 360, 360]);
});

test("polling updates the selected device without closing expanded details", async ({ page }) => {
  const state = await setup(page);
  await page.clock.install();
  const dialog = await open(page);
  const hardware = disclosure(dialog, "更多硬件信息");
  await hardware.locator("summary").first().click();
  const identity = disclosure(dialog, "身份与连接");
  await identity.locator("summary").first().click();
  state.updateName("Updated build Mac");
  const telemetry = structuredClone(nodeTelemetry);
  telemetry.inventory!.inventory.os_version = "macOS 26.2";
  state.updateTelemetry(telemetry);
  await page.clock.fastForward(31000);
  await expect(dialog.getByRole("heading", { name: /Updated build Mac/ })).toBeVisible();
  await expect(hardware).toHaveAttribute("open", "");
  await expect(identity).toHaveAttribute("open", "");
  await expect(hardware.getByText("macOS 26.2", { exact: true })).toBeVisible();
});

test("320px long device names and IPv6 stay inside the panel and keep navigation reachable", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 800 });
  const name = "签名构建设备-".repeat(8);
  await setup(page, "light", name);
  const dialog = await open(page);
  await expect(dialog.getByRole("heading", { name: new RegExp(name) })).toBeVisible();
  await expect(dialog.getByText(ipv6, { exact: true })).toBeVisible();
  await noOverflow(page, dialog);
  await dialog.getByRole("tab", { name: "运维", exact: true }).click();
  await expect(dialog.getByTestId("node-ota")).toBeVisible();
  await noOverflow(page, dialog);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(page.locator(".node-card").first().getByRole("button")).toBeFocused();
});
