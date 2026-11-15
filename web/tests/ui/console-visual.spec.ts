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
        ["hall", "/tenants", en ? "Welcome, Alex" : "你好，Alex"],
        ["tenant", tenantURL("/"), "Build Team"],
        ["project", tenantURL("/projects/desktop-client"), "Desktop Client"],
        [
          "builds",
          tenantURL("/projects/desktop-client/builds"),
          en ? "Build runs" : "构建记录",
        ],
        [
          "build-detail",
          tenantURL("/projects/desktop-client/builds/128"),
          "Desktop Client",
        ],
        [
          "artifacts",
          tenantURL("/projects/desktop-client/artifacts"),
          en ? "Artifacts" : "制品仓库",
        ],
        [
          "releases",
          tenantURL("/projects/desktop-client/releases"),
          en ? "Internal releases" : "内部发布",
        ],
        [
          "release-detail",
          tenantURL(
            "/projects/desktop-client/releases/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
          ),
          "v1.2.0-internal",
        ],
        ["devices", tenantURL("/nodes"), en ? "Shared devices" : "共享设备"],
        [
          "tenant-management",
          tenantURL("/settings"),
          en ? "Tenant settings" : "租户管理",
        ],
        [
          "project-management",
          tenantURL("/projects/desktop-client/settings"),
          en ? "Project settings" : "项目管理",
        ],
      ];
      for (const [name, path, heading] of routes) {
        await page.goto(path);
        await expect(
          page.getByRole("heading", { name: heading, exact: true }),
        ).toBeVisible();
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await expect(page.locator(".error-box[role=alert]")).toHaveCount(0);
        await expect(page.locator(".account-trigger")).toBeVisible();
        await expect(page.locator(".workspace-atmosphere")).toHaveCount(1);
        await expect(page.locator("canvas")).toHaveCount(0);
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth <= innerWidth + 1,
          ),
        ).toBe(true);
        await page.evaluate(() => document.fonts.ready);
        if (
          [
            "hall",
            "tenant",
            "project",
            "tenant-management",
            "project-management",
          ].includes(name)
        ) {
          await page.screenshot({
            path: resolve(directory, `${locale}-${theme}-${name}.png`),
            fullPage: true,
            animations: "disabled",
          });
        }
      }
      await openPreferences(page);
      await expect(page.getByRole("dialog")).toBeVisible();
      await page.screenshot({
        path: resolve(directory, `${locale}-${theme}-account.png`),
        fullPage: true,
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
  await page
    .getByRole("combobox", { name: "界面语言", exact: true })
    .selectOption("en");
  await closePreferences(page);
  await expect(
    page.getByRole("textbox", { name: "Filter current list", exact: true }),
  ).toHaveValue("main");
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  await expect(page.getByRole("log")).toContainText(
    "native compilation complete",
  );
  const before = events.streams;
  await openPreferences(page);
  await page
    .getByRole("combobox", { name: "Theme", exact: true })
    .selectOption("light");
  await closePreferences(page);
  await expect(page.getByRole("log")).toContainText("原始构建日志");
  expect(events.streams).toBe(before);
});
