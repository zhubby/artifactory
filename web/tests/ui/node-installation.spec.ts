/**
 * [INPUT]: 依赖租户 HTTP 夹具、真实下拉与受控安装材料。
 * [OUTPUT]: 验证加入弹窗命令、过期、秘密清理和系统地址权限。
 * [POS]: 首次节点安装流程的浏览器行为回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";
import AxeBuilder from "@axe-core/playwright";

const platforms = ["linux/amd64", "linux/arm64", "darwin/amd64", "darwin/arm64", "windows/amd64", "windows/arm64"];
async function installationAPI(page: Page, endpoint = "agents.example.com:31443") {
  await fixtureAPI(page);
  await page.context().addCookies([{ name: "artifactory-locale", value: "en", url: "http://127.0.0.1:3100" }]);
  await page.route("**/api/v1/node-installation/options", route => route.fulfill({ json: {
    device_endpoint: endpoint, releases: [{ id: "release-install", version: "emissary-v0.2.0", platforms }],
  } }));
  await page.route("**/api/v1/system/node-installation", route => route.fulfill({ json: { device_endpoint: endpoint, can_edit: true } }));
}
function material(os: string, arch: string, expires = new Date(Date.now() + 900_000).toISOString()) {
  return { token: "install-once-secret", expires_at: expires, device_endpoint: "agents.example.com:31443",
    ca_pem: "-----BEGIN CERTIFICATE-----\ntrusted-ca\n-----END CERTIFICATE-----\n", ca_sha256: "c".repeat(64),
    version: "emissary-v0.2.0", os, arch, download_url: "https://build.example.com/artifacts/object?ticket=download-ticket",
    sha256: "b".repeat(64), size: 1234, public_keys: ["dGVzdC1rZXk="],
    installer_url: `https://build.example.com/api/v1/node-installation/installer.${os === "windows" ? "ps1" : "sh"}`,
    installer_sha256: "a".repeat(64) };
}

test("installation stays in one dialog with commands, locked selection and cleared secrets", async ({ page }) => {
  await installationAPI(page);
  const writes: unknown[] = [];
  await page.route("**/api/v1/node-installation", async route => {
    const input = route.request().postDataJSON(); writes.push(input);
    await route.fulfill({ json: material(input.os, input.arch) });
  });
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: "Install and join node" })).toBeVisible();
  await chooseOption(dialog.getByRole("combobox", { name: "Target system" }), "windows");
  await chooseOption(dialog.getByRole("combobox", { name: "Architecture" }), "arm64");
  await dialog.getByRole("button", { name: "Generate installation command", exact: true }).click();
  await expect(dialog.getByTestId("node-install-command")).toContainText("Get-FileHash");
  await expect(dialog.getByTestId("node-install-command")).toContainText("ARTIFACTORY_ENROLLMENT_TOKEN");
  await expect(dialog.getByTestId("node-install-command")).toContainText("finally");
  await dialog.getByText("Installation target", { exact: true }).click();
  await expect(dialog.getByRole("combobox", { name: "Target system" })).toBeDisabled();
  await expect(page.getByRole("dialog")).toHaveCount(1);
  expect(writes).toEqual([{ pool_id: "pool-main", release_id: "release-install", os: "windows", arch: "arm64" }]);
  await dialog.getByText("View registration token and connection details", { exact: true }).click();
  await expect(dialog.getByTestId("node-install-token")).toHaveText("install-once-secret");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  await expect(dialog.getByTestId("node-install-command")).toHaveCount(0);
  await expect(dialog.getByText("install-once-secret", { exact: false })).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "Generate installation command", exact: true })).toBeEnabled();
});

test("missing endpoint guides settings without issuing a token", async ({ page }) => {
  await installationAPI(page, "");
  const writes: string[] = [];
  page.on("request", request => { if (request.method() === "POST") writes.push(request.url()); });
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Configure the public device address in system settings first.");
  await expect(dialog.getByRole("link", { name: "Open node settings" })).toHaveAttribute("href", tenantURL("/settings/emissary"));
  await expect(dialog.getByRole("button", { name: "Generate installation command" })).toBeDisabled();
  expect(writes).toEqual([]);
});

test("failed generation retains choices and expired material cannot be copied", async ({ page }) => {
  await installationAPI(page);
  let failed = true;
  await page.route("**/api/v1/node-installation", route => failed
    ? route.fulfill({ status: 503, json: { error: "Installation material unavailable" } })
    : route.fulfill({ json: material("darwin", "arm64", new Date(Date.now() - 1000).toISOString()) }));
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await chooseOption(dialog.getByRole("combobox", { name: "Target system" }), "darwin");
  await chooseOption(dialog.getByRole("combobox", { name: "Architecture" }), "arm64");
  await dialog.getByRole("button", { name: "Generate installation command" }).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Installation material unavailable");
  await expect(dialog.getByRole("combobox", { name: "Target system" })).toHaveText("macOS");
  failed = false;
  await dialog.getByRole("button", { name: "Generate installation command" }).click();
  await expect(dialog).toContainText("This command has expired. Generate a new command before installing.");
  await expect(dialog.getByRole("button", { name: "Copy installation command", exact: true })).toBeDisabled();
  await expect(dialog.getByRole("button", { name: "Generate again", exact: true })).toBeEnabled();
});

test("system node address saves only for an authorized system administrator", async ({ page }) => {
  await installationAPI(page);
  const writes: unknown[] = [];
  await page.route("**/api/v1/system/node-installation", route => {
    if (route.request().method() === "PATCH") writes.push(route.request().postDataJSON());
    return route.fulfill({ json: { device_endpoint: writes.length ? "new-agents.example.com:8443" : "agents.example.com:31443", can_edit: true } });
  });
  await page.goto(tenantURL("/settings/emissary"));
  const settings = page.getByTestId("node-installation-settings");
  await settings.getByRole("textbox", { name: "Public device address" }).fill("new-agents.example.com:8443");
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  expect(writes).toEqual([{ device_endpoint: "new-agents.example.com:8443" }]);
  await page.route("**/api/v1/system/node-installation", route => route.fulfill({ json: { device_endpoint: "other.example.com:8443", can_edit: false } }));
  await page.reload();
  await expect(settings.getByRole("textbox", { name: "Public device address" })).toHaveAttribute("readonly", "");
  await expect(settings.getByRole("button", { name: "Save", exact: true })).toHaveCount(0);
  await expect(settings).toContainText("Only a system administrator can change this address.");
});

test("missing release and a failed options read offer recovery without generation", async ({ page }) => {
  await installationAPI(page);
  let failed = true;
  await page.route("**/api/v1/node-installation/options", route => failed
    ? route.fulfill({ status: 503, json: { error: "Options unavailable" } })
    : route.fulfill({ json: { device_endpoint: "agents.example.com:8443", releases: [] } }));
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Options unavailable");
  failed = false;
  await dialog.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(dialog).toContainText("Import a signed Emissary release in node settings first.");
  await expect(dialog.getByRole("button", { name: "Generate installation command" })).toBeDisabled();
});

test("a response arriving after close cannot restore a token and 320px commands stay inside the dialog", async ({ page }) => {
  await installationAPI(page);
  await page.setViewportSize({ width: 320, height: 700 });
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  let started = false;
  let delayed = true;
  await page.route("**/api/v1/node-installation", async route => {
    started = true;
    if (delayed) await pending;
    const input = route.request().postDataJSON();
    await route.fulfill({ json: material(input.os, input.arch) });
  });
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  let dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Generate installation command" }).click();
  await expect.poll(() => started).toBe(true);
  await page.keyboard.press("Escape");
  release(); delayed = false;
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  dialog = page.getByRole("dialog");
  await expect(dialog.getByTestId("node-install-command")).toHaveCount(0);
  await dialog.getByRole("button", { name: "Generate installation command" }).click();
  await expect(dialog.getByTestId("node-install-command")).toContainText("shasum -a 256");
  await expect(dialog.getByTestId("node-install-command")).toContainText("--endpoint 'agents.example.com:31443'");
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
});

test("server expiration disables command and token copies without closing the dialog", async ({ page }) => {
  await installationAPI(page);
  await page.clock.install();
  await page.route("**/api/v1/node-installation", route => route.fulfill({ json: material("linux", "amd64", new Date(Date.now() + 2000).toISOString()) }));
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Generate installation command" }).click();
  await expect(dialog.getByRole("button", { name: "Copy installation command", exact: true })).toBeEnabled();
  await page.clock.fastForward(5000);
  await expect(dialog.getByRole("button", { name: "Copy installation command", exact: true })).toBeDisabled();
  await dialog.getByText("View registration token and connection details", { exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Copy token", exact: true })).toBeDisabled();
  await expect(dialog).toBeVisible();
});

test("regeneration blocks stale copies and changing options clears the previous material", async ({ page }) => {
  await installationAPI(page);
  let complete!: () => void;
  let calls = 0;
  await page.route("**/api/v1/node-installation", async route => {
    calls += 1;
    if (calls > 1) await new Promise<void>(resolve => { complete = resolve; });
    await route.fulfill({ json: material("linux", "amd64") });
  });
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "Register new node", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Generate installation command", exact: true }).click();
  await dialog.getByRole("button", { name: "Generate again", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Copy installation command", exact: true })).toBeDisabled();
  await expect(dialog.getByRole("button", { name: "Change options", exact: true })).toBeDisabled();
  await expect.poll(() => !!complete).toBe(true);
  complete();
  await expect(dialog.getByRole("button", { name: "Change options", exact: true })).toBeEnabled();
  await dialog.getByRole("button", { name: "Change options", exact: true }).click();
  await expect(dialog.getByTestId("node-install-command")).toHaveCount(0);
  await expect(dialog.getByRole("combobox", { name: "Target system" })).toBeEnabled();
});

for (const locale of ["en", "zh-CN"] as const) for (const theme of ["light", "dark"]) {
  test(`${locale} ${theme}: generated installation remains keyboard accessible and meets WCAG AA`, async ({ page }, testInfo) => {
    await installationAPI(page);
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: "http://127.0.0.1:3100" }]);
    await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
    await page.route("**/api/v1/node-installation", route => route.fulfill({ json: material("linux", "amd64") }));
    await page.goto(tenantURL("/nodes"));
    await page.getByRole("button", { name: locale === "en" ? "Register new node" : "注册新节点", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await dialog.getByRole("button", { name: locale === "en" ? "Generate installation command" : "生成安装命令", exact: true }).click();
    await expect(dialog.getByTestId("node-install-command")).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const results = await new AxeBuilder({ page }).include('[role="dialog"]').withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(results.violations.map(violation => ({ id: violation.id, targets: violation.nodes.map(node => node.target) }))).toEqual([]);
    await dialog.screenshot({ path: testInfo.outputPath(`installation-${locale}-${theme}.png`) });
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(page.getByRole("button", { name: locale === "en" ? "Register new node" : "注册新节点", exact: true })).toBeFocused();
  });
}
