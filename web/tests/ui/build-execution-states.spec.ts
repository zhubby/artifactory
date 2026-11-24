/**
 * [INPUT]: 依赖真实构建详情、原生与镜像任务夹具及规划状态。
 * [OUTPUT]: 验证镜像日志及默认计划标签的终态说明。
 * [POS]: 执行页与计划页的阶段边界回归；不请求真实构建资源。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { build, fixtureAPI, project, release, tenantURL } from "./fixtures";

const matching = "等待匹配具备能力的授权节点。";
const planningHint = "正在等待源码解析或 AI 规划，生成有效计划后自动执行。";
const digest = `sha256:${"a".repeat(64)}`;

async function snapshot(page: Page, value: unknown, tab = "执行") {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: value }));
  await page.route(`**/api/v1/builds/${build.id}/events`, route => route.fulfill({ contentType: "text/event-stream", body: `retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(value)}\n\n` }));
  await page.route(`**/api/v1/builds/${build.id}/plans`, route => route.fulfill({ json: { plans: [], runs: [], events: [] } }));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  await expect(page.getByRole("heading",{name:"构建 #128",exact:true})).toBeVisible();
  if (tab !== "过程") await page.getByRole("tab",{name:tab,exact:true}).click();
}

for (const status of ["running", "succeeded", "failed", "canceled"]) test(`image-only ${status} shows image execution and logs without a native node wait`, async ({ page }, info) => {
  const image = { ...build, status, artifacts: [], target_requirements: [{kind:"image",key:"container",platforms:["linux/amd64","linux/arm64"]}], selected_targets:["container"],
    jobs: [{id:"job-image",target:"container",status,target_config:{executor:"buildkit",os:"linux",arch:"amd64",application_platform:null,steps:[]},attempts:[]}],
    image_executions: [{id:"execution-image",job_id:"job-image",parent_id:null,operation:"build",status,spec:{name:"application",platforms:["linux/amd64","linux/arm64"]},destination_repository:"candidate/application",destination_credential:null,tags:[],root_digest:status==="succeeded"?digest:null,error:status==="failed"?"Dockerfile compilation failed":null}],
  };
  await snapshot(page,image);
  await page.route("**/api/v1/image-executions/execution-image/logs?*", route => route.fulfill({json:[{id:1,platform:"linux/arm64",text:"actual image output\n"}]}));
  const execution = page.locator(".build-layout");
  await expect(execution.getByText(matching,{exact:true})).toHaveCount(0);
  const result = execution.locator("article").filter({hasText:"application"});
  await expect(result.locator(`.status-${status}`)).toBeVisible();
  await expect(execution.getByRole("heading",{name:"执行步骤",exact:true})).toHaveCount(0);
  await result.getByText("平台构建日志",{exact:true}).click();
  await expect(result.getByRole("log")).toContainText("[linux/arm64] actual image output");
  if(status==="failed") await expect(result.getByRole("status")).toContainText("Dockerfile compilation failed");
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
  if(status==="running") await page.screenshot({path:info.outputPath("image-execution.png"),fullPage:true});
});

test("queued image jobs show their actual status before an execution record exists", async ({page}) => {
  await snapshot(page,{...build,status:"queued",jobs:[{id:"job-image",target:"container",status:"queued",target_config:{executor:"buildkit",os:"linux",arch:"amd64",application_platform:null,steps:[]},attempts:[]}],image_executions:[]});
  const execution = page.locator(".build-layout");
  await expect(execution.getByText(matching,{exact:true})).toHaveCount(0);
  await expect(execution.getByRole("heading",{name:"镜像构建",exact:true})).toBeVisible();
  await expect(execution.getByText("没有镜像执行记录。",{exact:true})).toBeVisible();
  await expect(execution.locator("section").last().locator(".status-queued")).toBeVisible();
});

test("mixed builds switch between native logs and the selected image execution", async ({page}) => {
  const native = build.jobs![0];
  const mixed = {...build, jobs:[native,{id:"job-image",target:"container",status:"running",target_config:{executor:"buildkit",os:"linux",arch:"amd64",application_platform:null,steps:[]},attempts:[]}],
    image_executions:[{id:"execution-image",job_id:"job-image",parent_id:null,operation:"build",status:"preparing",spec:{name:"container",platforms:["linux/amd64"]},destination_repository:"candidate/container",destination_credential:null,tags:[],root_digest:null,error:null}],
  };
  await snapshot(page,mixed);
  const execution = page.locator(".build-layout");
  await expect(execution.getByRole("heading",{name:"执行步骤",exact:true})).toBeVisible();
  await execution.locator(".target-button").filter({hasText:"container"}).click();
  await expect(execution.getByText(matching,{exact:true})).toHaveCount(0);
  await expect(execution.locator("article .status-preparing")).toBeVisible();
  await expect(execution.getByRole("heading",{name:"执行步骤",exact:true})).toHaveCount(0);
  await execution.locator(".target-button").filter({hasText:native.target}).click();
  await expect(execution.getByRole("heading",{name:"执行步骤",exact:true})).toBeVisible();
  await expect(execution.getByText("Mac Studio",{exact:true})).toBeVisible();
});

for (const status of ["failed", "canceled"]) test(`${status} without jobs never shows native allocation or an unfinished planning hint`, async ({page}) => {
  await snapshot(page,{...build,status,jobs:[],error:status==="failed"?"Pi planning failed":null},"过程");
  await expect(page.getByRole("tab",{name:"过程",exact:true})).toHaveAttribute("aria-selected","true");
  const execution = page.locator(".build-layout");
  await expect(execution.getByText(matching,{exact:true})).toHaveCount(0);
  await expect(execution.getByRole("heading",{name:"执行步骤",exact:true})).toHaveCount(0);
  await expect(execution.getByText("正在解析源码或生成 AI 构建计划…",{exact:true})).toHaveCount(0);
  await page.getByRole("tab",{name:"计划",exact:true}).click();
  await expect(page.getByText(planningHint,{exact:true})).not.toBeVisible();
  await expect(page.getByText(status==="failed"?"规划失败，未生成执行计划。请查看错误详情。":"构建已取消，未生成执行计划。",{exact:true})).toBeVisible();
});

for (const status of ["planning", "needs_input"]) test(`${status} without a plan retains its own next action`, async ({page}) => {
  await snapshot(page,{...build,status,jobs:[],clarification:status==="needs_input"?{question:"Which application?"}:null},"过程");
  await expect(page.getByRole("tab",{name:"过程",exact:true})).toHaveAttribute("aria-selected","true");
  await page.getByRole("tab",{name:"计划",exact:true}).click();
  if(status==="needs_input") {
    await expect(page.locator(".build-plan").getByText("Which application?",{exact:true})).toBeVisible();
    await expect(page.getByText("请回答上述问题后继续规划。",{exact:true})).toBeVisible();
    await expect(page.locator('textarea[name="answer"]')).toBeVisible();
    await expect(page.getByText(planningHint,{exact:true})).not.toBeVisible();
  } else await expect(page.getByText(planningHint,{exact:true})).toBeVisible();
});
