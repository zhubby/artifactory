/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具和共享 Select 交互助手。
 * [OUTPUT]: 验证多语言概览、独立项目目录、侧栏设置与个人设置布局。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { chooseOption } from "./select-helpers";
import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import {
  fixtureAPI,
  openPreferences,
  closePreferences,
  tenantURL,
} from "./fixtures";

for (const locale of ["zh-CN", "en"] as const)
  for (const theme of ["dark", "light"] as const) {
    test(`${locale} ${theme}: workspace hierarchy, management and account`, async ({
      page,
    }, info) => {
      test.setTimeout(120000);
      const failures: string[] = [];
      page.on("pageerror", (error) => failures.push(error.message));
      const events = await fixtureAPI(page);
      await page
        .context()
        .addCookies([
          {
            name: "artifactory-locale",
            value: locale,
            url: info.project.use.baseURL || "http://127.0.0.1:38500",
          },
        ]);
      await page.addInitScript(
        (value) => localStorage.setItem("artifactory-theme", value),
        theme,
      );
      const directory = resolve(
        process.cwd(),
        "../.local/workspace-evidence",
        info.project.name,
      );
      mkdirSync(directory, { recursive: true });
      const en = locale === "en";
      const routes = [
        ["hall", "/tenants", en ? "Choose a workspace" : "选择工作空间"],
        ["tenant", tenantURL("/"), "Build Team"],
        ["projects", tenantURL("/projects"), en ? "Projects" : "项目"],
        ["project", tenantURL("/projects/desktop-client"), "Desktop Client"],
        [
          "builds",
          tenantURL("/projects/desktop-client/builds"),
          en ? "Build runs" : "构建记录",
        ],
        [
          "build-detail",
          tenantURL("/projects/desktop-client/builds/128"),
          en ? "Build #128" : "构建 #128",
        ],
        [
          "artifacts",
          tenantURL("/projects/desktop-client/artifacts"),
          en ? "Artifacts" : "制品",
        ],
        [
          "releases",
          tenantURL("/projects/desktop-client/releases"),
          en ? "Releases" : "Release 版本",
        ],
        [
          "release-detail",
          tenantURL(
            "/projects/desktop-client/releases/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          ),
          "1.2.0-internal",
        ],
        ["devices", tenantURL("/nodes"), en ? "Build nodes" : "构建节点"],
        [
          "tenant-management",
          tenantURL("/settings"),
          en ? "General" : "基本信息",
        ],
        [
          "project-management",
          tenantURL("/projects/desktop-client/settings"),
          en ? "General" : "基本信息",
        ],
      ];
      for (const [name, path, heading] of routes) {
        await page.goto(path);
        await expect(
          page.getByRole("heading", { name: heading, level: name === "hall" ? 2 : 1, exact: true }),
        ).toBeVisible();
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await expect(page.locator('[data-notify][data-kind="error"]')).toHaveCount(0);
        await expect(page.locator(".account-trigger")).toBeVisible();
        await expect(page.locator(".workspace-atmosphere")).toHaveCount(0);
        await expect(page.locator("canvas")).toHaveCount(0);
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth <= innerWidth + 1,
          ),
        ).toBe(true);
        await page.evaluate(() => document.fonts.ready);
        await page.addStyleTag({ content: "nextjs-portal { display:none !important; }" });
        if (name === "overview" || name === "project") await expect(page.locator(".dashboard-charts .recharts-wrapper")).toHaveCount(2);
        await expect(page.locator("body")).toHaveCSS("font-family", /Inter Variable/);
        await page.screenshot({
          path: resolve(directory, `${locale}-${theme}-${name}.png`),
          fullPage: true,
          animations: "disabled",
        });
        if (name === "build-detail") {
          for (const [tab, label] of [["plan", en ? "Plan" : "计划"], ["artifacts", en ? "Artifacts" : "制品"]]) {
            await page.getByRole("tab", { name: label, exact: true }).click();
            await page.screenshot({ path: resolve(directory, `${locale}-${theme}-build-${tab}.png`), fullPage: true, animations: "disabled" });
          }
        }
        if (name === "devices") {
          await page.getByRole("button", { name: en ? "View details for Mac Studio" : "查看 Mac Studio 的详情", exact: true }).click();
          await expect(page.getByRole("dialog")).toBeVisible();
          await page.screenshot({ path: resolve(directory, `${locale}-${theme}-device-drawer.png`), fullPage: false, animations: "disabled" });
          await page.keyboard.press("Escape");
        }
      }
      await openPreferences(page);
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.screenshot({
        path: resolve(directory, `${locale}-${theme}-account.png`),
        fullPage: false,
        animations: "disabled",
      });
      await closePreferences(page);
      await expect(page.locator(".account-trigger")).toBeFocused();
      expect(events.writes).toEqual([]);
      expect(failures).toEqual([]);
    });
  }

test("personal preferences preserve list filters and the SSE subscription", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/builds"));
  await page
    .getByRole("textbox", { name: "筛选当前列表", exact: true })
    .fill("main");
  await openPreferences(page);
  await chooseOption(page.getByRole("combobox", { name: "界面语言", exact: true }), "en");
  await closePreferences(page);
  await expect(
    page.getByRole("textbox", { name: "Filter current list", exact: true }),
  ).toHaveValue("main");
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  await page.getByRole("tab", { name: "Execution", exact: true }).click();
  await expect(page.getByRole("log")).toContainText(
    "native compilation complete",
  );
  const before = events.streams;
  await openPreferences(page);
  await chooseOption(page.getByRole("combobox", { name: "Theme", exact: true }), "light");
  await closePreferences(page);
  await expect(page.getByRole("log")).toContainText("原始构建日志");
  expect(events.streams).toBe(before);
});
