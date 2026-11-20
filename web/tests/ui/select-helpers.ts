/**
 * [INPUT]: 依赖 Playwright Locator 和共享 Select 的值属性。
 * [OUTPUT]: 提供通过可见选项选择值的 chooseOption。
 * [POS]: 控制台浏览器验收共用真实下拉交互。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Locator } from "@playwright/test";

export async function chooseOption(trigger: Locator, value: string) {
  await trigger.click();
  const page = trigger.page();
  await page
    .getByRole("option")
    .and(page.locator(`[data-value=${JSON.stringify(value)}]`))
    .click();
}
