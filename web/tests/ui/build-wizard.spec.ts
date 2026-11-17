/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证build-wizard页面行为。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import {
  fixtureAPI,
  project,
  repositoryRefs,
  openPreferences,
  closePreferences,
  tenantURL,
} from "./fixtures";

async function next(page: Page) {
  await page.getByRole("button", { name: "继续", exact: true }).click();
}

async function reachReview(page: Page) {
  await expect(
    page.getByRole("radio", { name: "分支 main", exact: true }),
  ).toBeChecked();
  await next(page);
  await page
    .getByRole("radio", { name: "构建并收集产物", exact: true })
    .check();
  await expect(
    page.getByRole("radio", { name: "自动选择平台", exact: true }),
  ).toBeChecked();
  await next(page);
  await expect(
    page.getByRole("heading", { name: "确认后即可发起构建", exact: true }),
  ).toBeVisible();
}

test("all build entry points open the dedicated wizard without treating new as a build ID", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  const detailRequests: string[] = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname === "/api/v1/builds/new")
      detailRequests.push(request.url());
  });
  for (const path of [
    `/projects/${project.slug}`,
    `/projects/${project.slug}/builds`,
  ]) {
    await page.goto(tenantURL(path));
    await page.getByRole("link", { name: "发起构建", exact: true }).click();
    await expect(page).toHaveURL(/\/projects\/desktop-client\/builds\/new$/);
    await expect(
      page.getByRole("heading", { name: "要构建哪份源码？", exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("dialog")).toHaveCount(0);
  }
  expect(detailRequests).toEqual([]);
  expect(events.writes).toEqual([]);
});

test("only final confirmation creates a build with the existing request contract", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  const requests: Record<string, unknown>[] = [];
  await page.route("**/api/v1/projects/project-main/builds", async (route) => {
    requests.push(route.request().postDataJSON());
    await route.fulfill({
      json: { id: "build-main", project_slug: project.slug, number: 128 },
    });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await reachReview(page);
  expect(events.writes).toEqual([]);
  expect(requests).toEqual([]);
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/desktop-client\/builds\/128$/);
  expect(requests).toEqual([
    {
      ref: "refs/heads/main",
      goal: "构建本项目，并收集所选平台的可分发产物。",
      target_requirements: [],
      parameters: {},
      release_version: null,
    },
  ]);
});

test("viewer cannot operate the wizard even through a direct URL", async ({
  page,
}) => {
  await fixtureAPI(page, "viewer");
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await expect(
    page.getByText("当前账号没有发起构建的权限。", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "继续", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "发起构建", exact: true }),
  ).toHaveCount(0);
});

test("tags, manual platforms, release and parameters keep the existing payload and failed drafts", async ({
  page,
}) => {
  await fixtureAPI(page);
  const requests: Record<string, unknown>[] = [];
  await page.route("**/api/v1/projects/project-main/builds", async (route) => {
    requests.push(route.request().postDataJSON());
    await route.fulfill({
      status: 403,
      json: { error: "Release permission denied [fixture]" },
    });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await page.getByRole("button", { name: "标签", exact: true }).click();
  await page.getByRole("searchbox", { name: "搜索分支或标签" }).fill("1.2");
  await page.getByRole("radio", { name: "标签 v1.2.0", exact: true }).check();
  await next(page);
  await page
    .getByRole("radio", { name: "构建、测试并收集产物", exact: true })
    .check();
  await page
    .locator('textarea[name="goal"]')
    .fill("Build and test the desktop application");
  await page.getByRole("radio", { name: "指定目标平台", exact: true }).check();
  await next(page);
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "请至少选择一个目标平台",
  );
  await page
    .getByRole("checkbox", { name: "macOS · ARM64", exact: true })
    .check();
  await page
    .getByRole("checkbox", { name: "Linux · x64", exact: true })
    .check();
  await next(page);
  await page.locator('input[name="publish"]').check();
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "Release 版本",
  );
  expect(requests).toEqual([]);
  await page.locator('input[name="release_version"]').fill("v1.2.0-preview");
  await page.getByText("高级参数（可选）", { exact: true }).click();
  for (const value of [
    '{"unfinished":',
    "[]",
    '{"BUILD_MODE":1}',
    '{"invalid-key":"release"}',
  ]) {
    await page.locator('textarea[name="parameters"]').fill(value);
    await page.getByRole("button", { name: "发起构建", exact: true }).click();
    await expect(page.locator(".error-box[role=alert]")).toBeVisible();
    expect(requests).toEqual([]);
  }
  await page
    .locator('textarea[name="parameters"]')
    .fill('{"BUILD_MODE":"release"}');
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "Release permission denied [fixture]",
  );
  await expect(page.locator('input[name="release_version"]')).toHaveValue(
    "v1.2.0-preview",
  );
  await expect(page.locator('textarea[name="parameters"]')).toHaveValue(
    '{"BUILD_MODE":"release"}',
  );
  expect(requests).toEqual([
    {
      ref: "refs/tags/v1.2.0",
      goal: "Build and test the desktop application",
      target_requirements: [
        { os: "darwin", arch: "arm64" },
        { os: "linux", arch: "amd64" },
      ],
      parameters: { BUILD_MODE: "release" },
      release_version: "v1.2.0-preview",
    },
  ]);
});

test("source failures allow retry or explicit manual input without silently using HEAD", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  let queries = 0;
  await page.route("**/api/v1/projects/project-main/refs", async (route) => {
    queries++;
    await route.fulfill({
      status: 504,
      json: { error: "Reference query timed out [fixture]" },
    });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "Reference query timed out [fixture]",
  );
  await next(page);
  await expect(
    page.getByRole("heading", { name: "要构建哪份源码？", exact: true }),
  ).toBeVisible();
  const beforeRefresh = queries;
  await page.getByRole("button", { name: "刷新引用列表", exact: true }).click();
  await expect.poll(() => queries).toBeGreaterThan(beforeRefresh);
  await page.getByRole("button", { name: "手动输入", exact: true }).click();
  await page.locator('input[name="ref"]').fill("-invalid");
  await next(page);
  await expect(
    page.locator(".wizard-current > .error-box[role=alert]"),
  ).toContainText("请选择有效的源码引用");
  await page.locator('input[name="ref"]').fill("a".repeat(40));
  await next(page);
  await expect(
    page.getByRole("heading", { name: "这次要构建什么？", exact: true }),
  ).toBeVisible();
  expect(events.writes).toEqual([]);
});

test("empty or unknown default branches are never guessed and refresh discovers new references", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  let populated = false;
  await page.route("**/api/v1/projects/project-main/refs", async (route) => {
    await route.fulfill({
      json: populated
        ? { ...repositoryRefs, default_ref: null }
        : { default_ref: null, branches: [], tags: [] },
    });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await expect(
    page.getByText(
      "没有找到匹配的引用。可尝试其他分类，或手动输入引用与 commit。",
      { exact: true },
    ),
  ).toBeVisible();
  await next(page);
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "请选择有效的源码引用",
  );
  populated = true;
  await page.getByRole("button", { name: "刷新引用列表", exact: true }).click();
  const main = page.getByRole("radio", { name: "分支 main", exact: true });
  await expect(main).toBeVisible();
  await expect(main).not.toBeChecked();
  await main.check();
  await next(page);
  await expect(
    page.getByRole("heading", { name: "这次要构建什么？", exact: true }),
  ).toBeVisible();
  expect(events.writes).toEqual([]);
});

test("submission is guarded against repeated form submissions and never automatically retries", async ({
  page,
}) => {
  await fixtureAPI(page);
  let requests = 0;
  let finish: () => void = () => {};
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  await page.route("**/api/v1/projects/project-main/builds", async (route) => {
    requests++;
    await pending;
    await route.fulfill({
      status: 502,
      json: { error: "Build request rejected [fixture]" },
    });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await reachReview(page);
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
  await expect(page.locator('button[type="submit"]')).toBeDisabled();
  await page.locator(".wizard-current form").evaluate((form) => {
    (form as HTMLFormElement).requestSubmit();
    (form as HTMLFormElement).requestSubmit();
  });
  expect(requests).toBe(1);
  finish();
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "Build request rejected [fixture]",
  );
  await expect(
    page.getByRole("button", { name: "发起构建", exact: true }),
  ).toBeEnabled();
  expect(requests).toBe(1);
});

test("UTF-8 byte limits are validated before advancing or sending a request", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await page.getByRole("button", { name: "手动输入", exact: true }).click();
  await page.locator('input[name="ref"]').fill("中".repeat(100));
  await next(page);
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "1–255 字节",
  );
  await page.locator('input[name="ref"]').fill("refs/heads/main");
  await next(page);
  await next(page);
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "请填写 1–16000 字节的构建目标",
  );
  await page.locator('textarea[name="goal"]').fill("中".repeat(6000));
  await next(page);
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    "请填写 1–16000 字节的构建目标",
  );
  expect(events.writes).toEqual([]);
});

test("preference changes preserve answers and keyboard navigation honors reduced motion", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  const main = page.getByRole("radio", { name: "分支 main", exact: true });
  await expect(main).toBeChecked();
  await main.focus();
  await page.keyboard.press("ArrowDown");
  await expect(
    page.getByRole("radio", { name: "分支 release/v1", exact: true }),
  ).toBeChecked();
  await page.getByRole("button", { name: "继续", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("heading", { name: "这次要构建什么？", exact: true }),
  ).toBeFocused();
  await page
    .locator('textarea[name="goal"]')
    .fill("Keep this exact draft · 保留");
  await openPreferences(page);
  await page
    .getByRole("combobox", { name: "界面语言", exact: true })
    .selectOption("en");
  await page
    .getByRole("combobox", { name: "Theme", exact: true })
    .selectOption("light");
  await closePreferences(page);
  await expect(page.locator('textarea[name="goal"]')).toHaveValue(
    "Keep this exact draft · 保留",
  );
  await expect(page.locator(".wizard-summary")).toContainText(
    "refs/heads/release/v1",
  );
  await expect(page.locator("html")).toHaveAttribute("data-motion", "off");
  await expect(
    page.getByRole("radio", {
      name: "Choose platforms automatically",
      exact: true,
    }),
  ).toBeChecked();
  expect(events.writes).toEqual([]);
});

for (const locale of ["zh-CN", "en"] as const) {
  for (const theme of ["light", "dark"] as const) {
    test(`${locale} ${theme}: three-step review has no overflow and retains editable answers`, async ({
      page,
    }, info) => {
      const events = await fixtureAPI(page);
      const failures: string[] = [];
      page.on("pageerror", (error) => failures.push(error.message));
      await page.context().addCookies([
        {
          name: "artifactory-locale",
          value: locale,
          url: "http://127.0.0.1:3100",
        },
      ]);
      await page.addInitScript(
        (value) => localStorage.setItem("artifactory-theme", value),
        theme,
      );
      await page.goto(tenantURL("/projects/desktop-client/builds/new"));
      await expect(
        page.getByRole("radio", {
          name: locale === "en" ? "Branch main" : "分支 main",
          exact: true,
        }),
      ).toBeChecked();
      const continueButton = page.getByRole("button", {
        name: locale === "en" ? "Continue" : "继续",
        exact: true,
      });
      await continueButton.click();
      await page
        .getByRole("radio", {
          name:
            locale === "en" ? "Build and collect artifacts" : "构建并收集产物",
          exact: true,
        })
        .check();
      await continueButton.click();
      await expect(
        page.getByRole("heading", {
          name:
            locale === "en"
              ? "Ready to start your build?"
              : "确认后即可发起构建",
          exact: true,
        }),
      ).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth + 1,
        ),
      ).toBeTruthy();
      await page.screenshot({
        path: info.outputPath(`${locale}-${theme}-build-wizard.png`),
        fullPage: true,
        animations: "disabled",
      });
      expect(events.writes).toEqual([]);
      expect(failures).toEqual([]);
    });
  }
}

test("cancelling and refreshing discard the in-memory draft without persistence", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  page.on("dialog", (dialog) => dialog.accept());
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await expect(
    page.getByRole("radio", { name: "分支 main", exact: true }),
  ).toBeChecked();
  await next(page);
  await page.locator('textarea[name="goal"]').fill("Do not persist this draft");
  await page.getByRole("button", { name: "取消并返回", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/desktop-client\/builds$/);
  await page.getByRole("link", { name: "发起构建", exact: true }).click();
  await expect(page.locator(".wizard-summary")).not.toContainText(
    "Do not persist this draft",
  );
  await expect(
    page.getByRole("radio", { name: "分支 main", exact: true }),
  ).toBeChecked();
  await next(page);
  await page
    .locator('textarea[name="goal"]')
    .fill("Refresh should discard this too");
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "要构建哪份源码？", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".wizard-summary")).not.toContainText(
    "Refresh should discard this too",
  );
  expect(events.writes).toEqual([]);
});
