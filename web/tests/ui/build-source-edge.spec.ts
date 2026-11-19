/**
 * [INPUT]: 依赖源码引用查询和 Release 创建表单。
 * [OUTPUT]: 验证默认源码及迟到响应不覆盖手动输入。
 * [POS]: Release 创建时固定源码入口的回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect,test } from "@playwright/test";
import {fixtureAPI,repositoryRefs,tenantURL} from "./fixtures";
test("all repository refs remain available and the default populates the Release source",async({page})=>{
 await fixtureAPI(page);const branches=Array.from({length:125},(_,index)=>({name:`topic/${index}`,ref:`refs/heads/topic/${index}`,oid:"a".repeat(40)}));
 await page.route("**/api/v1/projects/project-main/refs",route=>route.fulfill({json:{default_ref:"refs/heads/topic/124",branches,tags:[]}}));
 await page.goto(tenantURL("/projects/desktop-client/releases/new"));
 await expect(page.locator('input[name="ref"]')).toHaveValue("refs/heads/topic/124");
 await expect(page.locator("datalist option")).toHaveCount(125);
 await page.locator('input[name="ref"]').fill("refs/tags/custom");
 await expect(page.locator('input[name="ref"]')).toHaveValue("refs/tags/custom");
});
test("a late source lookup never replaces an explicit source selection",async({page})=>{
 const events=await fixtureAPI(page);let finish!:()=>void;const pending=new Promise<void>(resolve=>{finish=resolve;});
 await page.route("**/api/v1/projects/project-main/refs",async route=>{await pending;await route.fulfill({json:repositoryRefs});});
 const requested=page.waitForRequest("**/api/v1/projects/project-main/refs");
 await page.goto(tenantURL("/projects/desktop-client/releases/new"));await requested;
 await page.locator('input[name="ref"]').fill("c".repeat(40));
 const response=page.waitForResponse("**/api/v1/projects/project-main/refs");finish();await response;
 await expect(page.locator('input[name="ref"]')).toHaveValue("c".repeat(40));expect(events.writes).toEqual([]);
});
