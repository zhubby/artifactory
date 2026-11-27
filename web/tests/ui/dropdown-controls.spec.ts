/**
 * [INPUT]: 依赖业务页面、共享下拉与成员接口夹具。
 * [OUTPUT]: 验证弹层齐宽对齐、首读门控、组合输入与表单事件。
 * [POS]: 设计规范中全站下拉要求的回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator } from "@playwright/test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fixtureAPI, openPreferences, project, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

async function expectMenuAligned(menu: Locator, control: Locator) {
  await expect.poll(async () => {
    const [popup, input] = await Promise.all([menu.boundingBox(), control.boundingBox()]);
    if (!popup || !input) return Number.POSITIVE_INFINITY;
    return Math.max(
      Math.abs(popup.width - input.width),
      Math.abs(popup.x - input.x),
      Math.abs(popup.x + popup.width - input.x - input.width),
    );
  }, { message: "下拉弹层与完整输入框应齐宽，左右边界误差不超过 1px" }).toBeLessThanOrEqual(1);
}

test("business components cannot introduce native select or datalist menus", () => {
  function scan(directory: string): string[] {
    return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) return scan(path);
      if (!/\.tsx?$/.test(path) || path.endsWith("/ui/select.tsx")) return [];
      return /<(select|datalist)\b/.test(readFileSync(path, "utf8")) ? [path] : [];
    });
  }
  expect(scan(join(process.cwd(), "src"))).toEqual([]);
});

test("preference menus use theme tokens and Escape preserves the parent dialog", async ({ page }, info) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/"));
  await openPreferences(page);
  const dialog = page.getByRole("dialog");
  const theme = dialog.getByRole("combobox", { name: "主题", exact: true });
  const themeField = page.locator(".preference-select").filter({
    has: page.locator('button[role="combobox"][aria-label="主题"]'),
  });
  expect(await theme.evaluate(element => element.tagName)).toBe("BUTTON");
  if (info.project.name === "mobile") expect((await theme.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  for (const value of ["light", "dark"]) {
    await chooseOption(theme, value);
    await expect(page.locator("html")).toHaveAttribute("data-theme", value);
    await theme.click();
    const list = page.getByRole("listbox");
    await expect(list).toBeVisible();
    await expectMenuAligned(list, themeField);
    await expect(page.getByRole("option", { selected: true })).toHaveAttribute("data-value", value);
    const audit = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(audit.violations.map(item => ({ id: item.id, nodes: item.nodes.map(node => node.target) }))).toEqual([]);
    // 全页截图会改变手机布局视口，并触发 Radix 的 resize 关闭逻辑。
    await page.screenshot({ path: info.outputPath(`dropdown-${value}.png`), fullPage: false });
    await expect(list).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(list).toHaveCount(0);
    await expect(dialog).toBeVisible();
    await expect(theme).toBeFocused();
  }
  await theme.press("ArrowDown");
  await expect(page.getByRole("option", { selected: true })).toBeFocused();
  await page.keyboard.press("Home");
  await expect(page.getByRole("option").first()).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(theme).toHaveAttribute("data-value", "system");
  await expect(theme).toBeFocused();
  await theme.press("Tab");
  await expect(theme).not.toBeFocused();
});

test("composition keys preserve the selection and Escape closes only one layer", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/"));
  await openPreferences(page);
  const dialog = page.getByRole("dialog");
  const theme = dialog.getByRole("combobox", { name: "主题", exact: true });
  await chooseOption(theme, "system");
  await theme.dispatchEvent("keydown", { key: "Enter", code: "Enter", isComposing: true, keyCode: 13, bubbles: true, cancelable: true });
  await expect(theme).toHaveAttribute("aria-expanded", "false");
  await theme.click();
  const list = page.getByRole("listbox");
  const light = page.getByRole("option").and(page.locator('[data-value="light"]'));
  const retainedDialog = page.getByRole("dialog", { includeHidden: true });
  const retainedTheme = retainedDialog.getByRole("combobox", { name: "主题", exact: true, includeHidden: true });
  await light.focus();
  for (const key of [
    { key: "Enter", code: "Enter", isComposing: true, keyCode: 13 },
    { key: " ", code: "Space", isComposing: true, keyCode: 32 },
    { key: "Enter", code: "Enter", isComposing: false, keyCode: 229 },
    { key: "Escape", code: "Escape", isComposing: true, keyCode: 27 },
  ]) {
    await light.dispatchEvent("keydown", { ...key, bubbles: true, cancelable: true });
    await expect(list).toBeVisible();
    // Radix 将父层暂时移出辅助技术树，父层 DOM 和视觉状态仍应保持。
    await expect(retainedDialog).toBeVisible();
    await expect(retainedDialog).toHaveAttribute("data-state", "open");
    await expect(retainedTheme).toHaveAttribute("data-value", "system");
  }
  await page.keyboard.press("Escape");
  await expect(list).toHaveCount(0);
  await expect(dialog).toBeVisible();
  await expect(theme).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("required asynchronous choices preserve empty values, reset and form events", async ({ page }) => {
  await fixtureAPI(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**/api/v1/projects/${project.id}/member-options`, async route => {
    await pending;
    await route.fulfill({ json: [{ id: "member-option", username: "Selectable Member", role: "member" }] });
  });
  const writes: unknown[] = [];
  await page.route(`**/api/v1/projects/${project.id}/members/*`, async route => {
    writes.push(route.request().postDataJSON());
    await route.fulfill({ json: { ok: true } });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}/settings/members`));
    const user = page.getByRole("combobox", { name: "工作空间成员", exact: true });
    const form = page.locator("form").filter({ has: user });
    await expect(user).toHaveAttribute("data-value", "");
    await form.getByRole("button", { name: "添加成员", exact: true }).click();
    await expect(user).toBeFocused();
    await expect(user).toHaveAttribute("aria-invalid", "true");
    expect(writes).toEqual([]);
    release();
    await user.click();
    await page.getByRole("option", { name: "Selectable Member", exact: true }).click();
    await expect(form).toHaveAttribute("data-dirty", "true");
    expect(await form.evaluate(element => new FormData(element as HTMLFormElement).get("user"))).toBe("member-option");
    await form.evaluate(element => (element as HTMLFormElement).reset());
    await expect(user).toHaveAttribute("data-value", "");
    expect(await form.evaluate(element => new FormData(element as HTMLFormElement).get("user"))).toBe("");
    await chooseOption(user, "member-option");
    await form.getByRole("button", { name: "添加成员", exact: true }).click();
    await expect.poll(() => writes).toEqual([{ role: "viewer" }]);
  } finally { release(); }
});

test("long menus scroll within the viewport and support End selection", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/${project.id}/member-options`, route => route.fulfill({
    json: Array.from({ length: 150 }, (_, index) => ({ id: `member-${index}`, username: `Team member ${String(index).padStart(3, "0")} ${"Long name ".repeat(10)}`, role: "member" })),
  }));
  await page.goto(tenantURL(`/projects/${project.slug}/settings/members`));
  const user = page.getByRole("combobox", { name: "工作空间成员", exact: true });
  await user.click();
  await expect(page.getByRole("option")).toHaveCount(151);
  await expectMenuAligned(page.getByRole("listbox"), page.locator('button[role="combobox"][data-name="user"]'));
  await page.keyboard.press("End");
  await expect(page.getByRole("option").last()).toBeInViewport();
  const box = (await page.getByRole("listbox").boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(page.viewportSize()!.width + 1);
  expect(box.y + box.height).toBeLessThanOrEqual(page.viewportSize()!.height + 1);
  await page.keyboard.press("Enter");
  await expect(user).toHaveAttribute("data-value", "member-149");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
});

test("late credential options preserve the saved default and emit only the real field change", async ({ page }) => {
  await fixtureAPI(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/credentials", async route => {
    await pending;
    await route.fulfill({ json: [{ id: project.credential_id, name: "Saved deploy credential", kind: "https" }] });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}/settings/resources`));
    const credential = page.locator('[role="combobox"][data-name="credential_id"]');
    const form = page.locator("form").filter({ has: credential });
    await expect(credential).toHaveCount(0);
    await expect(page.locator("#workspace-content .skeleton-form")).toBeVisible();
    await page.evaluate(() => {
      document.documentElement.dataset.credentialChanges = "[]";
      document.addEventListener("change", event => {
        if (!(event.target instanceof HTMLSelectElement) || event.target.name !== "credential_id") return;
        const changes = JSON.parse(document.documentElement.dataset.credentialChanges || "[]") as string[];
        changes.push(`${event.target.name}:${event.target.value}`);
        document.documentElement.dataset.credentialChanges = JSON.stringify(changes);
      });
    });
    release();
    await expect(credential).toHaveAttribute("data-value", project.credential_id!);
    await expect(credential).toContainText("Saved deploy credential");
    await expect(page.locator("html")).toHaveAttribute("data-credential-changes", "[]");
    expect(await form.evaluate(element => new FormData(element as HTMLFormElement).get("credential_id"))).toBe(project.credential_id);
    await chooseOption(credential, "");
    await expect(page.locator("html")).toHaveAttribute("data-credential-changes", '["credential_id:"]');
    await expect(form).toHaveAttribute("data-dirty", "true");
    expect(await form.evaluate(element => new FormData(element as HTMLFormElement).get("credential_id"))).toBe("");
    await form.evaluate(element => (element as HTMLFormElement).reset());
    await expect(credential).toHaveAttribute("data-value", project.credential_id!);
    expect(await form.evaluate(element => new FormData(element as HTMLFormElement).get("credential_id"))).toBe(project.credential_id);
    await expect(page.locator("html")).toHaveAttribute("data-credential-changes", '["credential_id:"]');
  } finally { release(); }
});
