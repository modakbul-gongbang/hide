import { expect, test } from "@playwright/test";
import { type ChildProcess } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace } from "./wire";
import { openServerButton, openSessions, startServer, writeConversation } from "./server-session-fixture";

test.describe.configure({ timeout: 180_000 });

test("running Workspace ports and conversation content: keyboard, jump, scroll, policy and scope", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const attempts: unknown[] = [];
  const frames: { at: number; payload: Record<string, unknown> }[] = [];
  page.on("websocket", (socket) => socket.on("framereceived", ({ payload }) => { if (typeof payload === "string") { const frame = JSON.parse(payload); if (frame.payload) frames.push({ at: Date.now(), payload: frame.payload }); } }));
  let holdOld = false;
  const delayed: string[] = [];
  const dispatched: Record<string, unknown>[] = [];
  let deliver: ((message: string) => void) | undefined;
  await page.routeWebSocket("**/ws", (socket) => {
    const server = socket.connectToServer();
    deliver = (message) => socket.send(message);
    socket.onMessage((message) => { const frame = JSON.parse(String(message)); if (frame.kind === "session_search") dispatched.push(frame.payload); server.send(message); });
    server.onMessage((message) => { const frame = JSON.parse(String(message)); const search = frame.payload?.session_search; if (holdOld && search?.query === "화검" && !search.loading) delayed.push(String(message)); else socket.send(message); });
  });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  const servers: ChildProcess[] = [];
  try {
    daemon = await startHided(herdr, "server-session-search");
    const root = fs.realpathSync(path.join(herdr.root, "fixture"));
    const source = writeConversation(daemon.home, root);
    writeConversation(daemon.home, path.join(daemon.home, "projects/alpha"), "foreign-session");
    const original = fs.readFileSync(source);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const globe = openServerButton(page);
    await expect(globe).toHaveCount(1);
    await globe.click();
    await expect(page.getByText("No running servers in this Workspace.")).toBeVisible({ timeout: 15_000 });
    await page.keyboard.press("Escape");
    const first = await startServer(root); servers.push(first.child);
    await expect.poll(async () => {
      await globe.click();
      const opened = await page.getByRole("tab", { name: new RegExp(String(first.port)) }).waitFor({state:"visible",timeout:1000}).then(() => 1).catch(() => 0);
      if (!opened) await page.keyboard.press("Escape");
      return opened;
    }, { timeout: 20_000 }).toBeGreaterThan(0);
    await expect(page.locator("[data-browser-address]")).toHaveText(`127.0.0.1:${first.port}`);
    const second = await startServer(root); servers.push(second.child);
    try { await expect.poll(async () => {
      if (!await page.locator("[data-server-port]").count()) await globe.click();
      await page.locator("[data-server-port]").first().waitFor({state:"visible",timeout:1000}).catch(() => {});
      const count = await page.locator("[data-server-port]").count();
      attempts.push(await globe.evaluate((element) => { const box = element.getBoundingClientRect(); return { box: box.toJSON(), top: document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)?.tagName, state: element.getAttribute("data-state"), picker: document.querySelector('[data-server-port]')?.parentElement?.textContent }; }));
      if (count < 2) await page.keyboard.press("Escape");
      return count;
    }, { timeout: 20_000 }).toBe(2); } catch (error) {
      const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
      if (dir) {
        fs.writeFileSync(path.join(dir, "server-poll-failure.json"), JSON.stringify({errors, attempts, body: await page.locator("body").innerText(), frames: frames.filter((row) => row.payload.rest).slice(-3)}, null, 2));
        await page.screenshot({path:path.join(dir,"server-poll-failure.png")});
      }
      throw error;
    }
    await page.keyboard.press("End");
    await expect(page.locator("[data-server-port]").last()).toBeFocused();
    await page.keyboard.press("Enter");
    await openSessions(page);
    const search = page.getByRole("searchbox", { name: "Search sessions" });
    await search.fill("화검");
    const hit = page.locator("[data-content-match]");
    await expect(hit).toHaveCount(1, { timeout: 20_000 });
    await expect(hit).toContainText("Assistant");
    await expect(page.locator('[data-session="foreign-session"]')).toHaveCount(0);
    await page.locator('[data-session-row="search-session"]').click();
    const match = page.locator('[data-search-match="true"]');
    await expect(match).toBeVisible();
    const conversation = page.getByRole("list", { name: "Conversation" });
    await conversation.evaluate((element) => { element.scrollTop = 0; });
    await search.fill("foo_bar");
    await expect(hit).toContainText("foo_bar");
    await expect.poll(() => conversation.evaluate((element) => element.scrollTop)).toBe(0);
    await search.fill('OR "(literal)*"');
    await expect(hit).toHaveCount(1);
    await search.fill("no-such-content");
    await expect(page.getByText("No matching sessions", { exact: true })).toBeVisible();
    // Dispatch A to the real daemon, hold its completed network answer, then
    // deliver it only after B's newer terminal frame has reached the screen.
    holdOld = true;
    const sentBefore = dispatched.length;
    await search.fill("화검");
    await expect.poll(() => dispatched.slice(sentBefore).some((row) => row.query === "화검")).toBe(true);
    await expect.poll(() => delayed.length).toBeGreaterThan(0);
    const newerSent = dispatched.length;
    const newerFrames = frames.length;
    const oldRevision = Math.max(...delayed.map((message) => Number(JSON.parse(message).payload?.revision ?? 0)));
    await search.fill("no-such-content");
    await expect.poll(() => dispatched.slice(newerSent).some((row) => row.query === "no-such-content")).toBe(true);
    await expect.poll(() => frames.slice(newerFrames).some((row) => { const state = row.payload.session_search as {query?:string;loading?:boolean;page?:{hits:unknown[]}} | undefined; return Number(row.payload.revision) > oldRevision && state?.query === "no-such-content" && state.loading === false && state.page?.hits.length === 0; })).toBe(true);
    await expect(page.getByText("No matching sessions", {exact:true})).toBeVisible();
    holdOld = false;
    for (const message of delayed.splice(0)) deliver!(message);
    await expect(hit).toHaveCount(0);
    await expect(page.getByText("No matching sessions", {exact:true})).toBeVisible();
    await page.getByRole("combobox", { name: "Search index retention" }).click();
    await page.getByRole("option", { name: "Off", exact: true }).click();
    await expect(page.locator("[data-content-search-status]")).toContainText("Content search is off");
    await search.fill("화검");
    await expect(hit).toHaveCount(0);
    expect(fs.readFileSync(source)).toEqual(original);
    await page.getByRole("combobox", { name: "Search index retention" }).click();
    await page.getByRole("option", { name: "90 days", exact: true }).click();
    await expect(hit).toHaveCount(1, { timeout: 20_000 });
    await search.fill("rebuild-only-marker");
    await expect(page.getByText("No matching sessions", {exact:true})).toBeVisible();
    const tool = JSON.stringify({type:"response_item",payload:{type:"function_call_output",output:"x".repeat(5*1024*1024)}})+"\n";
    fs.appendFileSync(source, tool + JSON.stringify({type:"response_item",timestamp:new Date().toISOString(),payload:{type:"message",role:"assistant",content:[{type:"output_text",text:"rebuild-only-marker completed"}]}})+"\n");
    const rebuiltOriginal = fs.readFileSync(source);
    const rebuiltAfter = frames.length;
    const rebuiltDispatch = dispatched.length;
    await page.getByRole("button", {name:"Rebuild index"}).click();
    await expect.poll(() => dispatched.slice(rebuiltDispatch).some((row) => row.clear === true && row.query === "rebuild-only-marker")).toBe(true);
    await expect.poll(() => frames.slice(rebuiltAfter).some((row) => { const state = row.payload.session_search as {query?:string;indexed?:number;indexing?:boolean;loading?:boolean} | undefined; return state?.query === "rebuild-only-marker" && state.indexed === 0 && (state.indexing === true || state.loading === true); })).toBe(true);
    await expect.poll(() => frames.slice(rebuiltAfter).some((row) => { const state = row.payload.session_search as {query?:string; indexing?:boolean; loading?:boolean; page?:{hits:unknown[]}} | undefined; return state?.query === "rebuild-only-marker" && state.indexing === false && state.loading === false && state.page?.hits.length === 1; }), {timeout:20_000}).toBe(true);
    await expect(hit).toContainText("rebuild-only-marker completed");
    expect(fs.readFileSync(source)).toEqual(rebuiltOriginal);
  } finally {
    for (const child of servers) child.kill();
    daemon?.stop(); herdr.stop();
  }
});
