/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证peripheral-visual页面行为。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { fixtureAPI, project, tenantId, tenantURL } from "./fixtures";

for (const locale of ["zh-CN", "en"] as const)
  for (const theme of ["light", "dark"] as const) {
    test(`${locale} ${theme}: entry pages, boundary states and build stages`, async ({ page }, info) => {
      test.setTimeout(120000);
      await fixtureAPI(page);
      await page.context().addCookies([{
        name: "artifactory-locale", value: locale,
        url: info.project.use.baseURL || "http://127.0.0.1:3100",
      }]);
      await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
      const directory = resolve(process.cwd(), "../.local/workspace-evidence", info.project.name);
      mkdirSync(directory, { recursive: true });
      const capture = async (name: string, current: Page = page) => {
        await expect(current.locator("html")).toHaveAttribute("data-theme", theme);
        await current.evaluate(() => document.fonts.ready);
        await current.evaluate(() => window.scrollTo(0, 0));
        expect(await current.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
        await current.screenshot({ path: resolve(directory, `${locale}-${theme}-${name}.png`), fullPage: true, animations: "disabled" });
      };
      for (const path of ["login", "register"]) {
        await page.goto(`/${path}`);
        await expect(page.locator('input[name="username"]')).toBeVisible();
        await capture(path);
      }
      await page.route("**/api/v1/invitations/preview", route => route.fulfill({ json: {
        tenant_id: tenantId, tenant_name: "Build Team", role: "member",
        expires_at: "2027-01-01T00:00:00Z", available: true,
      } }));
      await page.goto("/join/preview");
      await expect(page.getByRole("button", { name: locale === "en" ? "Confirm and join" : "确认加入", exact: true })).toBeVisible();
      await capture("invitation");
      await page.goto("/missing-preview-page");
      await expect(page.getByRole("heading", { name: locale === "en" ? "Page not found" : "页面不存在", exact: true })).toBeVisible();
      await capture("not-found");

      await page.route("**/api/v1/projects", route => route.fulfill({ json: [] }));
      await page.goto(tenantURL("/"));
      await expect(page.locator(".empty-state")).toBeVisible();
      await capture("empty-projects");
      await page.unroute("**/api/v1/projects");

      let resume!: () => void;
      const loading = new Promise<void>(resolve => { resume = resolve; });
      await page.route("**/api/v1/projects", async route => {
        await loading;
        await route.fulfill({ json: [project] });
      });
      await page.goto(tenantURL("/"));
      try {
        await expect(page.locator(".loading-state").first()).toBeVisible();
        await capture("loading");
      } finally {
        resume();
      }
      await page.unroute("**/api/v1/projects");
      await page.route("**/api/v1/projects", route => route.fulfill({ status: 403, json: { error: "Access to this workspace is unavailable" } }));
      await page.reload();
      await expect(page.locator(".error-box[role=alert]")).toContainText("Access to this workspace is unavailable");
      await expect(page.locator(".loading-state")).toHaveCount(0);
      await capture("request-denied");

      await page.unroute("**/api/v1/projects");
      await page.route("**/api/v1/projects/by-slug/desktop-client", route => route.fulfill({ json: {
        ...project,
        name: "DesktopClientEnterpriseEdition".repeat(6),
        repository_url: "https://git.example.com/" + "a-long-repository-path/".repeat(8) + "desktop-client.git",
      } }));
      await page.goto(tenantURL("/projects/desktop-client"));
      await expect(page.getByRole("heading", { level: 1 })).toContainText("DesktopClientEnterpriseEdition");
      await capture("long-project");
      await page.unroute("**/api/v1/projects/by-slug/desktop-client");

      await fixtureAPI(page, "viewer");
      await page.goto(tenantURL("/projects/desktop-client/releases/new"));
      await expect(page.locator(".notice")).toBeVisible();
      await capture("no-release-permission");

      await fixtureAPI(page);
      await page.goto(tenantURL("/projects/desktop-client/releases/new"));
      await expect(page.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
      await capture("release-source");
      await page.locator('input[name="version"]').fill("1.2.0-rc.1");
      await page.locator('textarea[name="goal"]').fill("Build and verify native signed packages with reproducible delivery names.");
      await capture("release-contract");
    });
  }
