/**
 * [INPUT]: 依赖规划实时摘要、独立审计标签夹具和真实构建页面。
 * [OUTPUT]: 验证实时统计、持久终态、旧审计身份和镜像确认文案。
 * [POS]: 规划反馈的三视口回归；外部模型不参与测试。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { build, fixtureAPI, project, release, tenantURL } from "./fixtures";

const run = {id:"run-feedback",state:"queued",purpose:"initial",calls:0,usage:{totalTokens:0},error:null};
async function openPlan(page:Page,status="planning") {
  const value={...build,status,goal:"Build pinned source",jobs:[],source_manifest_id:"manifest-fixed"};
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`,route=>route.fulfill({json:value}));
  await page.route(`**/api/v1/builds/${build.id}/events`,route=>route.fulfill({contentType:"text/event-stream",body:`retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(value)}\n\n`}));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  await page.getByRole("tab",{name:/^(计划|Plan)$/,exact:true}).click();
  return page.locator("section.panel").filter({has:page.getByRole("heading",{name:/^(AI 构建计划|AI build plan)$/,exact:true})});
}

test("active planning shows current runtime calls before a proposal and updates through the existing poll",async({page},info)=>{
  await fixtureAPI(page);let reads=0;
  await page.route(`**/api/v1/builds/${build.id}/plans`,route=>{
    const calls=++reads===1?3:4;
    return route.fulfill({json:{plans:[],events:[],runs:[{...run,id:"old-run",runtime:{status:"running",generation:1,calls:90,usage:{totalTokens:900}}},{...run,runtime:{status:"running",generation:2,calls,usage:{totalTokens:calls*100}}}]}});
  });
  const panel=await openPlan(page);
  await expect(panel.getByText("3 次模型调用",{exact:true})).toBeVisible();
  await expect(panel.locator(".status-running")).toBeVisible();
  await expect(panel.getByText("300 Tokens",{exact:true})).toBeVisible();
  await expect(panel.getByText("4 次模型调用",{exact:true})).toBeVisible();
  await expect(panel.getByText("400 Tokens",{exact:true})).toBeVisible();
  await page.screenshot({path:info.outputPath("planning-runtime.png"),fullPage:true});
});

test("unavailable runtime labels the confirmed statistics instead of implying live zero calls",async({page})=>{
  await fixtureAPI(page);
  await page.route(`**/api/v1/builds/${build.id}/plans`,route=>route.fulfill({json:{plans:[],events:[],runs:[{...run,runtime_unavailable:true}]}}));
  const panel=await openPlan(page);
  await expect(panel.getByText("实时状态暂不可用，以下为已确认统计。",{exact:true})).toBeVisible();
  await expect(panel.getByText("已确认 0 次模型调用",{exact:true})).toBeVisible();
  await expect(panel.getByText("0 次模型调用",{exact:true})).toHaveCount(0);
});

for(const status of ["canceled","needs_input","canceling"]) test(`${status} retains persisted planning statistics over a late runtime snapshot`,async({page})=>{
  await fixtureAPI(page);
  await page.route(`**/api/v1/builds/${build.id}/plans`,route=>route.fulfill({json:{plans:[],events:[],runs:[{...run,state:status==="canceling"?"queued":status,calls:2,usage:{totalTokens:200},runtime:{status:"running",generation:2,calls:99,usage:{totalTokens:999}}}]}}));
  const panel=await openPlan(page,status);
  await expect(panel.getByText("2 次模型调用",{exact:true})).toBeVisible();
  await expect(panel.getByText("200 Tokens",{exact:true})).toBeVisible();
  await expect(panel.locator(".status-running")).toHaveCount(0);
});

for(const locale of ["zh-CN","en"]) test(`${locale} legacy audit keeps unmatched results separate and preserves chunk loading`,async({page})=>{
  await fixtureAPI(page);
  await page.context().addCookies([{name:"artifactory-locale",value:locale,url:"http://127.0.0.1"}]);
  await page.route(`**/api/v1/builds/${build.id}/plans`,route=>route.fulfill({json:{plans:[],events:[],runs:[]}}));
  await page.route(`**/api/v1/builds/${build.id}/planning/conversations`,route=>route.fulfill({json:{conversations:[{run_id:run.id,generation:1,pod_uid:"pod-feedback",status:"succeeded",model:"custom-model",source_digest:null,started_at:build.created_at,ended_at:build.created_at}]}}));
  const entries=[
    {sequence:1,kind:"system",payload:{text:"Inspect the pinned source."}},
    {sequence:2,kind:"message",payload:{role:"assistant",content:[{type:"toolCall",name:"read_file",arguments:{path:"package.json"}}]}},
    {sequence:3,kind:"message",payload:{role:"toolResult",toolName:"read_file",isError:true,content:[{type:"text",text:"file not found"}]}},
    {sequence:4,kind:"message_chunk",payload:{message_id:"chunked",kind:"message",index:0,total:2,text:'{"role":"assistant","content":"Merged'}},
  ].map(message=>({...message,created_at:build.created_at}));
  await page.route(`**/api/v1/builds/${build.id}/planning/conversations/${run.id}/1/messages?*`,route=>{
    const after=new URL(route.request().url()).searchParams.get("after");
    return route.fulfill({json:after==="0"?{messages:entries,next_cursor:4}:{messages:[{sequence:5,kind:"message_chunk",payload:{message_id:"chunked",kind:"message",index:1,total:2,text:' audit message"}'},created_at:build.created_at}],next_cursor:null}});
  });
  await openPlan(page,"succeeded");
  await page.getByRole("tab",{name:locale==="en"?"AI audit":"AI 审计",exact:true}).click();
  const audit=page.locator(".planning-audit");
  await expect(audit.getByRole("button",{name:/package.json/})).toBeVisible();
  await expect(audit.locator(".audit-action")).toHaveCount(4);
  await audit.getByRole("button",{name:/package.json/}).click();
  await expect(audit.getByText(locale==="en"?"No recorded result":"未记录结果",{exact:true})).toBeVisible();
  await audit.getByRole("button",{name:locale==="en"?"Load more messages":"加载更多消息",exact:true}).click();
  await expect(audit.getByRole("button",{name:/Merged audit message/})).toBeVisible();
});

test("image-only confirmation describes validated planning without requiring native devices",async({page})=>{
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/${project.id}/build-platforms`,route=>route.fulfill({json:[]}));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}`));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await dialog.getByRole("radio",{name:"构建并收集产物",exact:true}).check();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("radio",{name:/^OCI 镜像/})).toBeChecked();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByText(/计划通过校验后开始构建/)).toBeVisible();
  await expect(dialog.getByText(/分配给授权节点执行/)).toHaveCount(0);
});
