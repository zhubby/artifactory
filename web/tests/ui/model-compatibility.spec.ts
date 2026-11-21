/**
 * [INPUT]: 依赖模型设置、真实自定义下拉与模型接口夹具。
 * [OUTPUT]: 验证独立兼容字段、任意模型名和协议切换的请求合同。
 * [POS]: 模型配置的浏览器行为回归；不请求外部模型。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

test("arbitrary model names keep explicit compatibility choices after saving and reopening", async ({ page }) => {
  await fixtureAPI(page);
  let profiles: Record<string, unknown>[] = [];
  const writes: Record<string, unknown>[] = [];
  await page.route("**/api/v1/model-profiles", async route => {
    if (route.request().method() === "GET") return route.fulfill({ json: profiles });
    const body = route.request().postDataJSON();
    writes.push(body);
    profiles = [{ ...body, id: "custom-profile", has_key: true, revision: 1, api_key: "********" }];
    return route.fulfill({ json: { id: "custom-profile" } });
  });
  await page.goto(tenantURL("/settings/ai"));
  await page.getByRole("button", { name: "添加模型", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.locator('input[name="name"]').fill("自由模型");
  await dialog.locator('input[name="model"]').fill("vendor/custom-model-2026");
  await dialog.locator('input[name="api_key"]').fill("fixture-model-key");
  await dialog.getByText("接口兼容性", { exact: true }).click();
  const role = dialog.getByRole("combobox", { name: "系统消息角色", exact: true });
  const thinking = dialog.getByRole("combobox", { name: "思考参数格式", exact: true });
  const tokens = dialog.getByRole("combobox", { name: "Token 上限字段", exact: true });
  await expect(role).toHaveAttribute("data-value", "system");
  await expect(thinking).toHaveAttribute("data-value", "none");
  await expect(tokens).toHaveAttribute("data-value", "max_tokens");
  await chooseOption(role, "developer");
  await chooseOption(thinking, "enable_thinking");
  await chooseOption(tokens, "max_completion_tokens");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({
    model: "vendor/custom-model-2026", api: "openai-completions",
    parameters: { compat: { system_role: "developer", thinking_format: "enable_thinking", max_tokens_field: "max_completion_tokens" } },
  });
  await page.getByRole("button", { name: "自由模型", exact: true }).click();
  await dialog.getByText("接口兼容性", { exact: true }).click();
  await expect(dialog.locator('input[name="model"]')).toHaveValue("vendor/custom-model-2026");
  await expect(dialog.locator('input[name="api_key"]')).toHaveValue("");
  await expect(role).toHaveAttribute("data-value", "developer");
  await expect(thinking).toHaveAttribute("data-value", "enable_thinking");
  await expect(tokens).toHaveAttribute("data-value", "max_completion_tokens");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
});

test("editing compatibility preserves existing sampling and rate limits without resubmitting a key", async ({ page }) => {
  await fixtureAPI(page);
  const profile = { id: "existing-profile", name: "既有模型", api: "openai-completions", base_url: "https://model.example/v1", model: "qwen4-max", has_key: true, enabled: true, is_default: true, max_calls: 20, timeout_seconds: 600, revision: 3, parameters: { thinking_level: "medium", temperature: 0.4, requests_per_minute: 7, compat: { thinking_format: "thinking" } } };
  let written: Record<string, unknown> | undefined;
  await page.route("**/api/v1/model-profiles", route => route.fulfill({ json: [profile] }));
  await page.route("**/api/v1/model-profiles/existing-profile", async route => { written = route.request().postDataJSON(); return route.fulfill({ json: { id: profile.id } }); });
  await page.goto(tenantURL("/settings/ai"));
  await page.getByRole("button", { name: profile.name, exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByText("接口兼容性", { exact: true }).click();
  await expect(dialog.getByRole("combobox", { name: "思考参数格式", exact: true })).toHaveAttribute("data-value", "thinking");
  await chooseOption(dialog.getByRole("combobox", { name: "思考参数格式", exact: true }), "reasoning_effort");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(written).toMatchObject({ api_key: "", model: "qwen4-max", parameters: { temperature: 0.4, requests_per_minute: 7, thinking_level: "medium", compat: { system_role: "system", thinking_format: "reasoning_effort", max_tokens_field: "max_tokens" } } });
});

test("switching protocols hides compatibility and excludes it from non-completions payloads", async ({ page }) => {
  await fixtureAPI(page);
  await page.context().addCookies([{ name: "artifactory-locale", value: "en", url: "http://127.0.0.1" }]);
  const writes: Record<string, unknown>[] = [];
  await page.route("**/api/v1/model-profiles", async route => {
    if (route.request().method() === "GET") return route.fulfill({ json: [] });
    writes.push(route.request().postDataJSON()); return route.fulfill({ json: { id: "new" } });
  });
  await page.goto(tenantURL("/settings/ai"));
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.locator('input[name="name"]').fill("Protocol test");
  await dialog.locator('input[name="model"]').fill("any-model-name");
  await dialog.locator('input[name="api_key"]').fill("fixture-key");
  await dialog.getByText("API compatibility", { exact: true }).click();
  await chooseOption(dialog.getByRole("combobox", { name: "Thinking parameter format", exact: true }), "enable_thinking");
  const api = dialog.getByRole("combobox", { name: "API type", exact: true });
  await chooseOption(api, "anthropic-messages");
  await expect(dialog.getByText("API compatibility", { exact: true })).toHaveCount(0);
  await chooseOption(api, "openai-completions");
  await dialog.getByText("API compatibility", { exact: true }).click();
  await expect(dialog.getByRole("combobox", { name: "Thinking parameter format", exact: true })).toHaveAttribute("data-value", "enable_thinking");
  await chooseOption(api, "openai-responses");
  await dialog.getByRole("button", { name: "Save", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(writes).toHaveLength(1);
  expect(writes[0]?.api).toBe("openai-responses");
  expect(writes[0]?.parameters).not.toHaveProperty("compat");
});
