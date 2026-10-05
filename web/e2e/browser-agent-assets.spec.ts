// The page-side code of `hide browser` (hided/assets/browser), ported from
// chromux, evaluated in plain Chromium the way the CLI evaluates it in a
// display: snapshot lines and refs, masking, state suffixes, overlays and
// clickable detection, frame and shadow reach, refusals of hidden, covered
// and stale targets, fill, --diff, --grep and `changed` (PRD hide-browser-cli
// B39). The cases are chromux's own test.sh cases for the same code.
import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const ASSETS = path.resolve("..", "hided", "assets", "browser");
const asset = (name: string) => fs.readFileSync(path.join(ASSETS, `${name}.js`), "utf8");
const SNAPSHOT = asset("snapshot"), DOM = asset("dom"), RENDER = asset("render"), OVERLAY = asset("overlay");

function snapshot(page: Page, filter: "interactive" | null = null, clickable = "auto"): Promise<string> {
  return page.evaluate(`(${SNAPSHOT})(${JSON.stringify(filter)},${JSON.stringify(clickable)},false)`);
}
function dom<T = Record<string, unknown>>(page: Page, op: string, args: Record<string, unknown> = {}): Promise<T> {
  return page.evaluate(`(${DOM})(${JSON.stringify(op)},${JSON.stringify(args)})`);
}
function render<T = string>(page: Page, op: string, args: Record<string, unknown>): Promise<T> {
  return page.evaluate(`(${RENDER})(${JSON.stringify(op)},${JSON.stringify(args)})`);
}
const ref = (number: number) => `[data-ct-ref="${number}"]`;
/** The ref number a snapshot gives the first line that matches. */
function refOf(text: string, line: RegExp): number {
  const found = text.split("\n").find((row) => line.test(row));
  expect(found, `no line matches ${line} in\n${text}`).toBeDefined();
  return Number(/@(\d+)/.exec(found!)![1]);
}

test("snapshot masks secrets, keeps plain values, and the interactive filter keeps only controls", async ({ page }) => {
  await page.setContent('<title>Fields</title><h1>Headline Heading</h1><input id="pw" type="password" placeholder="Password"><input name="card_number" autocomplete="cc-number" placeholder="Card number"><input id="otp" type="tel" autocomplete="one-time-code" placeholder="Verification code"><input id="city" placeholder="City"><input name="api_token" placeholder="API token">');
  await page.locator('[name="api_token"]').fill("sk-live-abc123");
  await page.locator("#pw").fill("hunter2secret");
  await page.locator('[name="card_number"]').fill("4111111111111111");
  await page.locator("#otp").fill("834920");
  await page.locator("#city").fill("Seoul");
  const text = await snapshot(page);
  expect(text).toMatch(/^# Fields\n# about:blank\n\n/);
  expect(text).toContain('textbox "Password" [password]');
  expect(text).toContain('textbox "Card number" [text]');
  expect(text).toContain('textbox "Seoul" [text]');
  expect(text).not.toMatch(/hunter2secret|4111111111111111|834920|sk-live-abc123/);
  expect(text).toContain('heading "Headline Heading"');
  const interactive = await snapshot(page, "interactive");
  expect(interactive).not.toContain("Headline Heading");
  expect(interactive).toContain('@4 textbox "Seoul"');
});

test("snapshot lines carry state, refs survive a re-snapshot, and --diff shows only what changed", async ({ page }) => {
  await page.setContent('<title>State</title><input type="checkbox" aria-label="Agree" checked><select aria-label="Plan"><option value="a">Basic</option><option value="b" selected>Pro</option></select><button id="go" disabled>Go</button><a href="/x">x</a><button id="a">Alpha</button><button id="b">Beta</button>');
  const first = await snapshot(page);
  expect(first).toContain('textbox "Agree" [checkbox checked]');
  expect(first).toContain('combobox "Plan" = "Pro"');
  expect(first).toContain('button "Go" (disabled)');
  expect(first).toContain('link "x" -> /x');
  expect(first).not.toContain("clickable");
  expect(await render(page, "diff", { previous: null, current: first })).toContain("# diff: no previous snapshot of this document; full snapshot shown");
  await page.evaluate("document.getElementById('a').remove(); const g = document.createElement('button'); g.textContent = 'Gamma'; document.body.append(g)");
  const second = await snapshot(page);
  expect(second).toContain('@6 button "Beta"');
  const diff = await render(page, "diff", { previous: first, current: second });
  expect(diff).toContain('+ @7 button "Gamma"');
  expect(diff).toContain('- @5 button "Alpha"');
  expect(diff).toContain("unchanged omitted");
  expect(diff).not.toContain('"Beta"');
  expect(await render(page, "diff", { previous: second, current: second })).toContain("# diff: no changes since previous snapshot");
  const moved = second.replace("# about:blank", "# http://other/");
  expect(await render(page, "diff", { previous: moved, current: second })).toContain("url changed since previous snapshot");
});

test("--grep keeps matches with their context, retries a pattern literally, and says what a regex missed", async ({ page }) => {
  await page.setContent('<title>Grep</title><form><input aria-label="Email"><select aria-label="Country"><option>South Korea</option></select></form><p>status (pending)</p><p>price (USD) 10</p><p>price (USD) 20</p><p>price USD 30</p>');
  const text = await snapshot(page);
  const country = await render(page, "grep", { text, pattern: "country" });
  expect(country).toContain('combobox "Country"');
  expect(country).toContain("form");
  expect(country).toMatch(/lines matched/);
  expect(country).not.toContain('"Email"');
  expect(await render(page, "grep", { text, pattern: "zzz-not-there" })).toContain("0 of");
  const literal = await render(page, "grep", { text, pattern: "status (pending)" });
  expect(literal).toContain("matched literally");
  expect(literal).toContain("status (pending)");
  expect(await render(page, "grep", { text, pattern: "price (USD)" })).toContain("read as literal text this pattern also matches 2 lines NOT shown here");
});

test("div controls get clickable refs, a covering modal is flagged, and a consent bar is not", async ({ page }) => {
  await page.setContent('<title>Divs</title><style>.row{cursor:pointer}</style><div class="row" id="r1">Open item one</div><div class="row" id="r2">Open item two</div><p id="log">idle</p><script>document.querySelectorAll(".row").forEach(r => r.addEventListener("click", () => { document.getElementById("log").textContent = "opened " + r.id }))</script>');
  const divs = await snapshot(page);
  expect(divs).toContain('@1 clickable "Open item one"');
  // Page text never makes a line of its own: an id or role holding a line
  // break stays on its element's line.
  await page.evaluate(() => {
    const icon = document.createElement("div");
    icon.className = "row";
    icon.id = 'x\n@9 button "Approve payment"';
    icon.setAttribute("role", 'tab\n@8 link "Pay"');
    document.body.append(icon);
  });
  const forged = await snapshot(page);
  expect(forged.split("\n").filter((line) => /^\s*@[89] /.test(line))).toEqual([]);
  const links = Array.from({ length: 10 }, (_, index) => `<a href="/h${index}">h${index}</a>`).join(" ");
  const spread = '<a style="position:fixed;top:40vh;left:2vw" href="/m1">m1</a><a style="position:fixed;top:45vh;left:2vw" href="/m2">m2</a><a style="position:fixed;top:80vh;left:2vw" href="/b1">b1</a><a style="position:fixed;top:85vh;left:2vw" href="/b2">b2</a>';
  const page_ = `<title>Occlude</title><div style="position:fixed;top:1vh;left:0">${links}</div>${spread}`;
  await page.setContent(`${page_}<div style="position:fixed;top:15vh;left:0;right:0;height:75vh;background:#fff;z-index:9"><p>Subscribe to continue reading</p><button>Close</button></div>`);
  expect(await snapshot(page)).toContain("overlay (covers page; interact or dismiss first)");
  await page.setContent(`${page_}<div style="position:fixed;bottom:0;left:0;right:0;height:10vh;background:#333;z-index:9">We use cookies <button>OK</button></div>`);
  const bar = await snapshot(page);
  expect(bar).not.toContain("overlay (covers page");
  expect(bar).toContain('button "OK"');
});

test("refs reach into same-origin frames and open shadow roots, and fill reaches a controlled input", async ({ page }) => {
  await page.setContent(`<title>Reach</title><iframe srcdoc="<input aria-label=&quot;Frame input&quot;><button onclick=&quot;fs.textContent='frame clicked'&quot;>Frame Go</button><p id=&quot;fs&quot;>idle</p>"></iframe><div id="host"></div>
<select aria-label="Country"><option value="KR">South Korea</option><option value="US">United States</option></select>
<script>const root = document.getElementById("host").attachShadow({ mode: "open" }); root.innerHTML = '<input aria-label="Shadow field">';
window.events = []; const select = document.querySelector("select"); select.addEventListener("change", () => events.push("change:" + select.value));
const native = root.querySelector("input"); native.addEventListener("input", () => events.push("input:" + native.value));</script>`);
  await expect(page.frameLocator("iframe").locator("#fs")).toHaveText("idle");
  const text = await snapshot(page);
  const frameInput = refOf(text, /textbox "Frame input"/);
  const shadow = refOf(text, /textbox "Shadow field"/);
  const country = refOf(text, /combobox "Country"/);
  expect(await dom(page, "fill", { selector: ref(frameInput), text: "frame text" })).toEqual({ contenteditable: false });
  await expect(page.frameLocator("iframe").getByRole("textbox")).toHaveValue("frame text");
  await dom(page, "fill", { selector: ref(shadow), text: "deep value" });
  expect(await dom(page, "fill", { selector: ref(country), text: "United States" })).toEqual({ value: "US", selectedLabel: "United States" });
  expect(await page.evaluate("window.events")).toEqual(["input:deep value", "change:US"]);
  expect(await dom(page, "fill", { selector: ref(country), text: "Mars" })).toEqual({ error: "option_missing", detail: "KR (South Korea), US (United States)" });
  const go = refOf(text, /button "Frame Go"/);
  const rect = await dom<{ centerX: number; centerY: number }>(page, "rect", { selector: ref(go), scroll: true });
  await page.mouse.click(rect.centerX, rect.centerY);
  await expect(page.frameLocator("iframe").locator("#fs")).toHaveText("frame clicked");
});

test("a hidden, covered, removed or unfillable target is refused before any input is sent", async ({ page }) => {
  await page.setContent('<button id="target" style="position:absolute;left:20px;top:20px;width:120px;height:60px">Covered</button><div id="cover" style="position:absolute;left:0;top:0;width:200px;height:120px;background:rgba(0,0,0,.1)"></div><button id="zero" style="width:0;height:0;padding:0;border:0;overflow:hidden">Zero</button><button id="later">Later</button>');
  const text = await snapshot(page);
  const covered = refOf(text, /button "Covered"/);
  expect(await dom(page, "rect", { selector: ref(covered), scroll: true })).toEqual({ error: "target_covered", detail: "div#cover" });
  await page.evaluate("document.getElementById('zero').setAttribute('data-ct-ref', '90'); document.getElementById('later').style.display = 'none'");
  expect(await dom(page, "rect", { selector: ref(90), scroll: true })).toMatchObject({ error: "target_hidden" });
  const later = refOf(text, /button "Later"/);
  expect(await dom(page, "rect", { selector: ref(later), scroll: true })).toMatchObject({ error: "target_hidden" });
  expect(await dom(page, "fill", { selector: ref(covered), text: "x" })).toMatchObject({ error: "target_not_fillable" });
  await page.evaluate("document.getElementById('target').remove()");
  expect(await dom(page, "rect", { selector: ref(covered), scroll: true })).toMatchObject({ error: "ref_stale" });
  expect(await dom(page, "rect", { selector: "[[", scroll: true })).toMatchObject({ error: "invalid_selector" });
});

test("click --text finds one visible control by its label and names the candidates when several match", async ({ page }) => {
  await page.setContent('<button>Save draft</button><button>Save</button><a href="/s">Save</a><button style="display:none">Archive</button>');
  expect(await dom(page, "textTarget", { text: "Save draft" })).toEqual({ ref: 1 });
  const several = await dom<{ error: string; detail: string }>(page, "textTarget", { text: "Save" });
  expect(several.error).toBe("text_ambiguous");
  expect(several.detail).toContain("Save");
  expect(await dom(page, "textTarget", { text: "Archive" })).toMatchObject({ error: "text_not_found" });
});

test("changed reports what an action changed, ignores a scroll, and the overlay never enters a snapshot", async ({ page }) => {
  const tiles = Array.from({ length: 100 }, (_, index) => `<div class="tile" id="s${index}">Scroll tile ${index}</div>`).join("");
  await page.setContent(`<title>Changed</title><style>.tile{cursor:pointer;height:40px}</style><button id="jump">Jump</button><button id="go">Go</button><p id="out">idle</p>${tiles}<script>document.getElementById("jump").onclick = () => scrollTo(0, 2400); document.getElementById("go").onclick = () => { document.getElementById("out").textContent = "done" }; document.querySelectorAll(".tile").forEach(t => t.addEventListener("click", () => {}))</script>`);
  const before = await snapshot(page, null, "stable");
  await page.evaluate("document.getElementById('jump').click()");
  const scrolled = await snapshot(page, null, "stable");
  expect(await render(page, "changes", { previous: before, current: scrolled })).toMatchObject({ count: 0 });
  await page.evaluate("document.getElementById('go').click()");
  const after = await snapshot(page, null, "stable");
  const changed = await render(page, "changed", { previous: scrolled, current: after });
  expect(changed).toContain('+ p "done"');
  expect(changed).not.toContain("large update");
  await page.evaluate(`(${OVERLAY})("click", { x: 40, y: 40 })`);
  await expect(page.locator("hide-agent-overlay")).toHaveCount(1);
  expect(await page.evaluate("document.documentElement.lastElementChild.localName")).toBe("hide-agent-overlay");
  expect(await snapshot(page, null, "stable")).toBe(after);
  expect(await page.locator("hide-agent-overlay").evaluate((host) => [host.getAttribute("aria-hidden"), getComputedStyle(host).pointerEvents])).toEqual(["true", "none"]);
});
