/**
 * [INPUT]: 依赖真实 xterm、Playwright WebSocket 路由和租户 API 夹具。
 * [OUTPUT]: 验证设备终端创建、字节交互、窗口、退出与键盘边界。
 * [POS]: 终端完整前端链回归；后端 PTY 与设备验收另行执行。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import { fixtureAPI, nodes, tenantURL } from "./fixtures";

const sessionId = "e8bd2c8f-b770-4dad-9566-63668c954d88";

async function setup(page: Page, options: { online?: boolean; enabled?: boolean; startOnAuth?: boolean; role?: "admin" | "viewer" | "operator" } = {}) {
  await fixtureAPI(page, options.role || "admin");
  await page.context().addCookies([{ name: "artifactory-locale", value: "en", url: `http://127.0.0.1:${process.env.ARTIFACTORY_UI_PORT || 3100}` }]);
  const events = { creates: [] as Record<string, unknown>[], inputs: [] as string[], sizes: [] as { columns: number; rows: number }[], acks: [] as number[], closes: 0, attaches: 0, peers: [] as WebSocketRoute[] };
  let offset = 0;
  let gate: Promise<void> | undefined;
  let releaseGate = () => {};
  const sendBytes = (bytes: Buffer, peer = events.peers.at(-1)!) => {
    peer.send(JSON.stringify({ type: "output", offset, data: bytes.toString("base64") }));
    offset += bytes.length;
  };
  const send = (text: string, peer = events.peers.at(-1)!) => sendBytes(Buffer.from(text), peer);
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: nodes.map(node => ({ ...node, capabilities: [...node.capabilities, "terminal-v1"], online: options.online ?? true })) }));
  await page.route("**/api/v1/nodes/node-main/terminal-capabilities", route => route.fulfill({ json: {
    enabled: options.enabled ?? true, online: options.online ?? true,
    account: "emissary", profiles: [{ id: "default", label: "System shell" }],
  } }));
  await page.route("**/api/v1/nodes/node-main/terminal-sessions", async route => {
    events.creates.push(route.request().postDataJSON());
    await route.fulfill({ json: { id: sessionId, node_id: "node-main", state: "pending" } });
  });
  await page.route(`**/api/v1/terminal-sessions/${sessionId}/attach`, async route => {
    events.attaches++;
    if (events.attaches > 1) await gate;
    await route.fulfill({ json: { ticket: `single-use-${events.attaches}`, stream_url: `/api/v1/terminal-sessions/${sessionId}/stream` } });
  });
  await page.route(`**/api/v1/terminal-sessions/${sessionId}/close`, route => {
    events.closes++;
    return route.fulfill({ json: {} });
  });
  await page.routeWebSocket(`**/api/v1/terminal-sessions/${sessionId}/stream`, peer => {
    events.peers.push(peer);
    peer.onMessage(raw => {
      const message = JSON.parse(raw.toString());
      if (message.type === "authenticate") {
        expect(message.renderer_id).toEqual(expect.any(String));
        expect(message.ticket).toMatch(/^single-use-/);
        peer.send(JSON.stringify({ type: "status", state: options.startOnAuth === false ? "opening" : "active", account: "emissary", cwd: "/srv/builds", shell: "/bin/sh" }));
        if (events.peers.length === 1 && options.startOnAuth !== false) send("terminal-ready\r\n", peer);
      }
      if (message.type === "input") events.inputs.push(Buffer.from(message.data, "base64").toString("utf8"));
      if (message.type === "resize") events.sizes.push({ columns: message.columns, rows: message.rows });
      if (message.type === "ack") events.acks.push(message.offset);
    });
  });
  return { events, send, sendBytes, holdAttach: () => { gate = new Promise<void>(resolve => { releaseGate = resolve; }); }, resume: () => releaseGate() };
}

async function open(page: Page) {
  await page.goto(tenantURL("/nodes/node-main/terminal"));
  await expect(page.getByRole("heading", { name: "Device terminal", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Open terminal", exact: true }).click();
  await expect(page.getByTestId("terminal-state")).toHaveText("Connected");
}

test("device entry opens an independent terminal and preserves raw keyboard input", async ({ page }) => {
  const { events, send } = await setup(page);
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "View details for Mac Studio", exact: true }).click();
  await page.getByRole("tab", { name: "Operations", exact: true }).click();
  await page.getByRole("link", { name: "Open terminal", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(events.creates).toHaveLength(0);
  await page.getByRole("button", { name: "Open terminal", exact: true }).click();
  await expect(page.getByTestId("terminal-state")).toHaveText("Connected");
  await expect(page.getByTestId("terminal-canvas")).toContainText("terminal-ready");
  await page.screenshot({ path: test.info().outputPath("terminal-desktop.png"), fullPage: true });
  await expect.poll(() => events.acks.at(-1)).toBe(Buffer.byteLength("terminal-ready\r\n"));
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await page.keyboard.type("echo test");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Control+c");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+k");
  await page.getByRole("textbox", { name: "Terminal input", exact: true }).evaluate(element => {
    const textarea = element as HTMLTextAreaElement;
    textarea.value = "";
    textarea.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true }));
    textarea.value = "中文";
    textarea.dispatchEvent(new CompositionEvent("compositionupdate", { data: "中文", bubbles: true }));
    textarea.dispatchEvent(new CompositionEvent("compositionend", { data: "中文", bubbles: true }));
  });
  await expect.poll(() => events.inputs.join("")).toContain("中文");
  const input = events.inputs.join("");
  expect(input).toContain("echo test\r");
  expect(input).toContain("\x03");
  expect(input).toContain("\x1b");
  expect(input).toContain("\x0b");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const beforeResize = events.sizes.length;
  await page.setViewportSize({ width: 960, height: 760 });
  await expect.poll(() => events.sizes.length).toBeGreaterThan(beforeResize);
  expect(events.creates[0]).toMatchObject({ profile: "default", term: "xterm-256color", renderer_id: expect.any(String), idempotency_key: expect.any(String) });
  send("last-output\r\n");
  events.peers.at(-1)!.send(JSON.stringify({ type: "exit", code: 7, signal: null, reason: "shell exited" }));
  await expect(page.getByTestId("terminal-state")).toHaveText("Session ended");
  await expect(page.getByTestId("terminal-canvas")).toContainText("last-output");
  await expect(page.getByText("Exit code 7", { exact: true })).toBeVisible();
});

test("network reconnect attaches the same session without replaying keyboard input", async ({ page }) => {
  const fixture = await setup(page);
  await open(page);
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await page.keyboard.type("before");
  await expect.poll(() => fixture.events.inputs.join("")).toBe("before");
  fixture.holdAttach();
  fixture.events.peers[0].close({ code: 1012, reason: "entry restarting" });
  await expect(page.getByTestId("terminal-state")).toHaveText("Reconnecting");
  await page.keyboard.type("discarded");
  fixture.resume();
  await expect.poll(() => fixture.events.attaches).toBe(2);
  await expect(page.getByTestId("terminal-state")).toHaveText("Connected");
  await page.keyboard.type("after");
  await expect.poll(() => fixture.events.inputs.join("")).toBe("beforeafter");
  expect(fixture.events.creates).toHaveLength(1);
  await expect(page.getByTestId("terminal-canvas")).toContainText("terminal-ready");
});

test("multiline paste requires confirmation and remote OSC52 cannot write clipboard", async ({ page }) => {
  const { events, send } = await setup(page);
  await page.addInitScript(() => {
    (window as Window & { clipboardWrites?: number }).clipboardWrites = 0;
    Object.defineProperty(navigator.clipboard, "writeText", { value: async () => {
      const target = window as Window & { clipboardWrites?: number };
      target.clipboardWrites = (target.clipboardWrites || 0) + 1;
    } });
  });
  await open(page);
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await page.getByRole("textbox", { name: "Terminal input", exact: true }).evaluate(element => {
    const data = new DataTransfer();
    data.setData("text/plain", "echo first\necho second\n");
    element.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  });
  await expect(page.getByRole("dialog", { name: "Paste multiple lines?" })).toBeVisible();
  expect(events.inputs).toHaveLength(0);
  await page.getByRole("button", { name: "Paste into terminal", exact: true }).click();
  await expect.poll(() => events.inputs.join("")).toContain("echo second");
  send("\x1b]52;c;c2VjcmV0\x07");
  await expect.poll(() => events.acks.length).toBeGreaterThan(1);
  expect(await page.evaluate(() => (window as Window & { clipboardWrites?: number }).clipboardWrites)).toBe(0);
});

test("revocation clears the screen and never reconnects or sends further input", async ({ page }) => {
  const { events } = await setup(page);
  await open(page);
  await expect(page.getByTestId("terminal-canvas")).toContainText("terminal-ready");
  events.peers[0].send(JSON.stringify({ type: "status", state: "closed", reason: "permission revoked" }));
  await expect(page.getByTestId("terminal-state")).toHaveText("Session ended");
  await expect(page.getByTestId("terminal-canvas")).not.toContainText("terminal-ready");
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await page.keyboard.type("ignored");
  expect(events.inputs).toHaveLength(0);
  expect(events.attaches).toBe(1);
});

test("320px terminal keeps focus escape and content inside the page", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 740 });
  await setup(page);
  await open(page);
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Terminal input", exact: true })).toBeFocused();
  await page.keyboard.press("Control+Shift+Space");
  await expect(page.getByRole("button", { name: "Leave terminal input", exact: true })).toBeFocused();
  const focusButton = await page.getByRole("button", { name: "Leave terminal input", exact: true }).boundingBox();
  const header = await page.locator(".workspace-topbar").boundingBox();
  expect(focusButton!.y).toBeGreaterThan(header!.y + header!.height);
  expect(focusButton!.x + focusButton!.width).toBeLessThanOrEqual(320);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: test.info().outputPath("terminal-320.png"), fullPage: true });
});

test("connecting remains truthful until the device confirms its shell", async ({ page }) => {
  const { events } = await setup(page, { startOnAuth: false });
  await page.goto(tenantURL("/nodes/node-main/terminal"));
  await page.getByRole("button", { name: "Open terminal", exact: true }).click();
  await expect.poll(() => events.peers.length).toBe(1);
  await expect(page.getByTestId("terminal-state")).toHaveText("Connecting");
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await page.keyboard.type("not-yet");
  expect(events.inputs).toHaveLength(0);
  events.peers[0].send(JSON.stringify({ type: "status", state: "active" }));
  await expect(page.getByTestId("terminal-state")).toHaveText("Connected");
});

test("split UTF-8 output is decoded continuously and acknowledged after consumption", async ({ page }) => {
  const { events, sendBytes } = await setup(page);
  await open(page);
  const text = Buffer.from("欢迎\r\n");
  sendBytes(text.subarray(0, 2));
  sendBytes(text.subarray(2));
  await expect(page.getByTestId("terminal-canvas")).toContainText("欢迎");
  await expect(page.getByTestId("terminal-canvas")).not.toContainText("�");
  await expect.poll(() => events.acks.at(-1)).toBe(Buffer.byteLength("terminal-ready\r\n") + text.length);
});

test("malformed output ends the session without forwarding more input", async ({ page }) => {
  const { events } = await setup(page);
  await open(page);
  events.peers[0].send(JSON.stringify({ type: "output", offset: 0, data: "%%%invalid%%%" }));
  await expect(page.getByTestId("terminal-state")).toHaveText("Connection failed");
  await expect.poll(() => events.closes).toBe(1);
  await page.getByRole("button", { name: "Focus terminal", exact: true }).click();
  await page.keyboard.type("ignored");
  expect(events.inputs).toHaveLength(0);
});

for (const role of ["viewer", "operator"] as const) {
  test(`${role} cannot create terminal sessions`, async ({ page }) => {
    const { events } = await setup(page, { role });
    await page.goto(tenantURL("/nodes/node-main/terminal"));
    await expect(page.getByText("A tenant owner or administrator must open this device terminal.", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Open terminal", exact: true })).toHaveCount(0);
    expect(events.creates).toHaveLength(0);
  });
}

for (const option of [{ online: false }, { enabled: false }]) {
  test(`${Object.keys(option)[0]} false prevents a queued terminal`, async ({ page }) => {
    const { events } = await setup(page, option);
    await page.goto(tenantURL("/nodes/node-main/terminal"));
    await expect(page.getByRole("button", { name: "Open terminal", exact: true })).toBeDisabled();
    expect(events.creates).toHaveLength(0);
  });
}

test("leaving the page explicitly closes the session and refresh does not restore it", async ({ page }) => {
  const { events } = await setup(page);
  await open(page);
  await page.getByRole("link", { name: "Back to devices", exact: true }).click();
  await expect.poll(() => events.closes).toBe(1);
  await page.goto(tenantURL("/nodes/node-main/terminal"));
  await expect(page.getByTestId("terminal-state")).toHaveText("Not connected");
  await page.reload();
  await expect(page.getByTestId("terminal-state")).toHaveText("Not connected");
  expect(events.creates).toHaveLength(1);
});
