/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证应用分类、具名消息展开与折叠终态补读。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, build, tenantURL } from "./fixtures";

for (const locale of ["zh-CN", "en"] as const) {
  test(`${locale}: AI planning, clarification, plan history and model settings`, async ({
    page,
  }, info) => {
    await fixtureAPI(page);
    await page
      .context()
      .addCookies([
        {
          name: "artifactory-locale",
          value: locale,
          url: "http://127.0.0.1:3100",
        },
      ]);
    const aiBuild = {
      ...build,
      goal: "Build the native desktop executable",
      status: "needs_input",
      revision_count: 1,
      source_manifest_id: "snapshot-pinned",
      clarification: { question: "Which application should be built?" },
    };
    const plan = {
      id: "plan-v1",
      revision: 1,
      created_at: build.created_at,
      contents: {
        summary: "Compile the selected desktop application",
        evidence: [{ path: "src/main.c", reason: "Native entry point" }],
        scripts: [{ name: "verify.sh", content: "test -x dist/app" }],
        recipe: {
          version: 1,
          targets: {
            native: {
              os: "darwin",
              arch: "arm64",
              application_platform: "macos",
              artifacts: ["dist/app"],
              steps: [
                {
                  name: "Compile",
                  phase: "build",
                  run: "cc src/main.c -o dist/app",
                },
              ],
            },
          },
        },
      },
    };
    await page.route(
      "**/api/v1/projects/by-slug/desktop-client/builds/128",
      (route) => route.fulfill({ json: aiBuild }),
    );
    await page.route("**/api/v1/builds/build-main/events", (route) =>
      route.fulfill({
        contentType: "text/event-stream",
        body: `retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(aiBuild)}\n\n`,
      }),
    );
    await page.route("**/api/v1/builds/build-main/plans", (route) =>
      route.fulfill({
        json: {
          plans: [plan],
          runs: [
            {
              id: "run-v1",
              state: "needs_input",
              calls: 7,
              usage: { totalTokens: 1234 },
            },
          ],
          events: [],
        },
      }),
    );
    let answer = "";
    await page.route(
      "**/api/v1/builds/build-main/clarifications",
      async (route) => {
        answer = (route.request().postDataJSON() as { answer: string }).answer;
        await route.fulfill({ json: { ok: true } });
      },
    );
    await page.goto(tenantURL("/projects/desktop-client/builds/128"));
    await page.getByRole("tab", { name: locale === "en" ? "Plan" : "计划", exact: true }).click();
    await expect(
      page.getByText("Which application should be built?", { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByText("snapshot-pinned", { exact: false }),
    ).toBeVisible();
    await page.locator('textarea[name="answer"]').fill("The desktop client");
    await page
      .getByRole("button", {
        name:
          locale === "en" ? "Submit answer and continue" : "提交回答并继续规划",
        exact: true,
      })
      .click();
    await expect.poll(() => answer).toBe("The desktop client");
    await expect(page.getByText("src/main.c", { exact: true })).toBeVisible();
    await page
      .getByText(
        locale === "en"
          ? "Execution steps and external scripts"
          : "查看执行步骤与外置脚本",
        { exact: true },
      )
      .click();
    await expect(
      page.getByText("cc src/main.c -o dist/app", { exact: true }),
    ).toBeVisible();
    await expect(page.getByText("verify.sh", { exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: `native · macOS · ${locale === "en" ? "Host" : "宿主"} darwin/arm64`, exact: true })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth + 1,
      ),
    ).toBeTruthy();
    await page.screenshot({
      path: info.outputPath(`${locale}-ai-plan.png`),
      fullPage: true,
    });

    await page.goto(tenantURL("/settings/ai"));
    await expect(
      page.getByRole("heading", {
        name: locale === "en" ? "Model profiles" : "模型配置",
        exact: true,
      }),
    ).toBeVisible();
    await page
      .getByRole("button", {
        name: locale === "en" ? "Add model" : "添加模型",
        exact: true,
      })
      .click();
    await expect(page.locator('input[name="api_key"]')).toHaveAttribute(
      "type",
      "password",
    );
    await expect(page.locator('input[name="max_calls"]')).toHaveValue("20");
    await expect(page.locator('input[name="timeout_seconds"]')).toHaveValue(
      "600",
    );
    await page.screenshot({
      path: info.outputPath(`${locale}-ai-model.png`),
      fullPage: true,
    });
    await page.keyboard.press("Escape");
    await page.goto(tenantURL(`/projects/desktop-client/releases/${build.release_id}`));
    await page.getByRole("button", { name: locale === "zh-CN" ? "发起构建" : "Start build", exact: true }).click();
    await expect(page.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
    await page.getByRole("dialog").getByRole("button", {name:locale === "zh-CN" ? "继续" : "Continue",exact:true}).click();
    await expect(page.locator('textarea[name="goal"]')).toBeVisible();
    await page
      .locator('textarea[name="goal"]')
      .fill("Build a signed desktop release");
    await expect(page.getByText(/artifactory.yaml/)).toHaveCount(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth + 1,
      ),
    ).toBeTruthy();
  });
}

test("application platform labels distinguish iOS from a CLI on the same host", async ({ page }, info) => {
  await fixtureAPI(page);
  await page.context().addCookies([{ name: "artifactory-locale", value: "en", url: "http://127.0.0.1:3100" }]);
  const targets = {
    ios: { os: "darwin", arch: "arm64", application_platform: "ios", steps: [{ name: "Build", phase: "build", run: "make simulator" }] },
    cli: { os: "darwin", arch: "arm64", application_platform: null, steps: [{ name: "Build", phase: "build", run: "make cli" }] },
  };
  const baseJob = build.jobs?.[0];
  if (!baseJob) throw new Error("build fixture must include a job");
  const item = {
    ...build, goal: "Build the iOS simulator application and CLI", status: "succeeded",
    jobs: Object.entries(targets).map(([name, target_config]) => ({ ...baseJob, id: name, target: name, target_config, attempts: [] })),
  };
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => route.fulfill({ json: item }));
  await page.route("**/api/v1/builds/build-main/plans", route => route.fulfill({ json: {
    plans: [{ id: "plan-platforms", revision: 1, created_at: build.created_at, contents: {
      summary: "Build two products", evidence: [], scripts: [], recipe: { targets },
    } }], runs: [], events: [],
  } }));
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  await expect(page.getByRole("button", { name: /^ios iOS · Host darwin\/arm64/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /^cli Non-application · Host darwin\/arm64/ })).toBeVisible();
  await page.getByRole("tab", { name: "Plan", exact: true }).click();
  await page.getByText("Execution steps and external scripts", { exact: true }).click();
  await expect(page.getByRole("heading", { name: "ios · iOS · Host darwin/arm64", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "cli · Non-application · Host darwin/arm64", exact: true })).toBeVisible();
  await page.screenshot({ path: info.outputPath("application-platforms.png"), fullPage: true });
});


test("planning conversations load on demand and paginate completed messages",async({page},info)=>{
 await fixtureAPI(page);
 await page.context().addCookies([{name:"artifactory-locale",value:"en",url:"http://127.0.0.1:3100"}]);
 const item={...build,goal:"Build source",status:"succeeded",source_manifest_id:"manifest-fixed"};
 await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128",route=>route.fulfill({json:item}));
 await page.route("**/api/v1/builds/build-main/plans",route=>route.fulfill({json:{plans:[],runs:[],events:[]}}));
 let summaries=0;let reads=0;
 await page.route("**/api/v1/builds/build-main/planning/conversations",route=>{summaries++;return route.fulfill({json:{conversations:[{run_id:"run-generation",generation:2,pod_uid:"pod-2",status:"interrupted",model:"test-model",source_digest:"a".repeat(64),started_at:build.created_at,ended_at:build.created_at}]}});});
 await page.route("**/api/v1/builds/build-main/planning/conversations/run-generation/2/messages?*",route=>{reads++;const after=new URL(route.request().url()).searchParams.get("after");return route.fulfill({json:after==="0"?{messages:[{sequence:1,kind:"message",payload:{role:"user",content:"Build pinned source"},created_at:build.created_at}],next_cursor:1}:{messages:[{sequence:2,kind:"message",payload:{role:"assistant",content:"Completed audit answer <script>"},created_at:build.created_at}],next_cursor:null}});});
 await page.goto(tenantURL("/projects/desktop-client/builds/128"));
 await page.getByRole("tab",{name:"Plan",exact:true}).click();
 expect(summaries).toBe(0);expect(reads).toBe(0);
 await page.getByText("AI conversation audit",{exact:true}).click();
 await expect.poll(()=>summaries).toBeGreaterThan(0);expect(reads).toBe(0);
 await page.locator("summary").filter({hasText:"run-gene"}).click();
 await expect.poll(()=>reads).toBe(1);
 await page.getByRole("button",{name:"Load more messages",exact:true}).click();
 await expect.poll(()=>reads).toBe(2);
 await page.getByLabel("2 · Assistant · Completed audit answer <script>",{exact:true}).click();
 await expect(page.locator("pre").filter({hasText:"Completed audit answer <script>"})).toBeVisible();
 await expect(page.locator("script").filter({hasText:"Completed audit answer"})).toHaveCount(0);
 await page.screenshot({path:info.outputPath("planning-conversation.png"),fullPage:true});
});


test("planning conversation reads final messages after its status changes",async({page},info)=>{
 await fixtureAPI(page);
 await page.context().addCookies([{name:"artifactory-locale",value:"en",url:"http://127.0.0.1:3100"}]);
 const item={...build,goal:"Build source",status:"planning",source_manifest_id:"manifest-fixed"};
 await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128",route=>route.fulfill({json:item}));
 await page.route("**/api/v1/builds/build-main/events",route=>route.fulfill({contentType:"text/event-stream",body:`retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(item)}\n\n`}));
 await page.route("**/api/v1/builds/build-main/plans",route=>route.fulfill({json:{plans:[],runs:[],events:[]}}));
 let status="running";let reads=0;
 await page.route("**/api/v1/builds/build-main/planning/conversations",route=>route.fulfill({json:{conversations:[{run_id:"run-final",generation:1,pod_uid:"pod-1",status,model:"test-model",started_at:build.created_at}]}}));
 await page.route("**/api/v1/builds/build-main/planning/conversations/run-final/1/messages?*",route=>{reads++;const after=new URL(route.request().url()).searchParams.get("after");return route.fulfill({json:{messages:after==="0"?[{sequence:1,kind:"message",payload:{role:"user",content:"Initial request"},created_at:build.created_at}]:status==="succeeded"?[{sequence:2,kind:"proposal_result",payload:{accepted:true,finalAudit:"final durable receipt"},created_at:build.created_at}]:[],next_cursor:null}});});
 await page.goto(tenantURL("/projects/desktop-client/builds/128"));await page.getByRole("tab",{name:"Plan",exact:true}).click();
 await page.getByText("AI conversation audit",{exact:true}).click();await page.locator("summary").filter({hasText:"run-fina"}).click();
 await expect.poll(()=>reads).toBe(1);status="succeeded";
 await expect(page.getByLabel("2 · Proposal result · Accepted",{exact:true})).toBeVisible({timeout:8000});
 await page.getByLabel("2 · Proposal result · Accepted",{exact:true}).click();await expect(page.getByText(/final durable receipt/)).toBeVisible();
 await page.screenshot({path:info.outputPath("planning-conversation-final.png"),fullPage:true});
});

test("collapsed planning conversation reads completion once when reopened",async({page},info)=>{
 await fixtureAPI(page);
 await page.context().addCookies([{name:"artifactory-locale",value:"en",url:"http://127.0.0.1:3100"}]);
 const item={...build,goal:"Build source",status:"planning",source_manifest_id:"manifest-fixed"};
 await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128",route=>route.fulfill({json:item}));
 await page.route("**/api/v1/builds/build-main/events",route=>route.fulfill({contentType:"text/event-stream",body:`retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(item)}\n\n`}));
 await page.route("**/api/v1/builds/build-main/plans",route=>route.fulfill({json:{plans:[],runs:[],events:[]}}));
 let status="running";const reads:string[]=[];
 await page.route("**/api/v1/builds/build-main/planning/conversations",route=>route.fulfill({json:{conversations:[{run_id:"run-collapsed",generation:1,pod_uid:"pod-1",status,model:"test-model",started_at:build.created_at}]}}));
 await page.route("**/api/v1/builds/build-main/planning/conversations/run-collapsed/1/messages?*",route=>{
  const after=new URL(route.request().url()).searchParams.get("after")!;reads.push(after);
  // 待翻页状态停止消息轮询；终态消息只能来自重开补读。
  return route.fulfill({json:after==="0"?{messages:[{sequence:1,kind:"message",payload:{role:"user",content:"Initial request"},created_at:build.created_at}],next_cursor:1}:{messages:[{sequence:2,kind:"proposal_result",payload:{accepted:true,finalAudit:"collapsed final durable receipt"},created_at:build.created_at}],next_cursor:null}});
 });
 await page.goto(tenantURL("/projects/desktop-client/builds/128"));
 await page.getByRole("tab",{name:"Plan",exact:true}).click();
 await page.getByText("AI conversation audit",{exact:true}).click();
 const session=page.locator("summary").filter({hasText:"run-coll"});
 await session.click();await expect.poll(()=>reads).toEqual(["0"]);
 await session.click();status="succeeded";
 await expect(session).toContainText("succeeded",{timeout:8000});
 expect(reads).toEqual(["0"]);
 await session.click();
 await expect(page.getByLabel("2 · Proposal result · Accepted",{exact:true})).toBeVisible();
 await page.getByLabel("2 · Proposal result · Accepted",{exact:true}).click();
 await expect(page.getByText(/collapsed final durable receipt/)).toBeVisible();
 await page.waitForTimeout(3500);expect(reads).toEqual(["0","1"]);
 await page.screenshot({path:info.outputPath("planning-conversation-reopened.png"),fullPage:true});
});
