// The mobile companion on an isolated pinned Herdr and a debug hided (PRD
// mobile-companion D-11): Settings > Mobile walks a fake `tailscale` through
// its four checklist states to a QR; an iPhone-sized touch page pairs with the
// QR's code, lists the agents in four groups, opens a detail, and its reply
// and quick keys reach the pane's PTY; a loopback push endpoint decrypts
// what hided sends and proves the three modes, the viewing rule and the Seen
// clear; revoke, the phone limit, the seven-day revoke, the unreachable line
// and the empty list follow; and no code, credential or key reaches the log.
//
// Nothing here runs the operator's Tailscale: HIDE_TAILSCALE_BIN names a
// script in the test's own directory, and the push endpoint is a local server
// only a debug hided accepts.

import { devices, expect, test, type Browser, type BrowserContext, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";

test.describe.configure({ timeout: 240_000 });

const DNS = "mac.tailnet-name.ts.net";

/** A `tailscale` CLI whose answers the test writes; it records every call. */
class FakeTailscale {
  readonly dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-ts-"));
  readonly bin = path.join(this.dir, "tailscale");

  install(): void {
    const script = `#!/bin/sh
S='${this.dir}'
echo "$*" >> "$S/calls.log"
case "$1" in
  status) cat "$S/status.json" ;;
  serve)
    shift
    if [ "$1" = status ]; then cat "$S/serve.json" 2>/dev/null || echo '{}'; exit 0; fi
    last=""; for a in "$@"; do last="$a"; done
    if [ "$last" = off ]; then echo '{}' > "$S/serve.json"; exit 0; fi
    printf '{"TCP":{"443":{"HTTPS":true}},"Web":{"${DNS}:443":{"Handlers":{"/":{"Proxy":"%s"}}}}}' "$last" > "$S/serve.json"
    ;;
  *) exit 2 ;;
esac
`;
    fs.writeFileSync(this.bin, script, { mode: 0o755 });
  }

  status(value: unknown): void {
    fs.writeFileSync(path.join(this.dir, "status.json"), JSON.stringify(value));
  }

  loggedOut(): void {
    this.status({ BackendState: "NeedsLogin", Self: { DNSName: "", HostName: "mac" } });
  }

  httpsOff(): void {
    this.status({ BackendState: "Running", Self: { DNSName: `${DNS}.`, HostName: "mac" }, CurrentTailnet: { MagicDNSEnabled: true } });
  }

  ready(): void {
    this.status({ BackendState: "Running", Self: { DNSName: `${DNS}.`, HostName: "mac" }, CurrentTailnet: { MagicDNSEnabled: true }, CertDomains: [DNS] });
  }

  proxy(): string | null {
    try {
      const serve = JSON.parse(fs.readFileSync(path.join(this.dir, "serve.json"), "utf8")) as { Web?: Record<string, { Handlers?: Record<string, { Proxy?: string }> }> };
      return serve.Web?.[`${DNS}:443`]?.Handlers?.["/"]?.Proxy ?? null;
    } catch {
      return null;
    }
  }

  calls(): string {
    try {
      return fs.readFileSync(path.join(this.dir, "calls.log"), "utf8");
    } catch {
      return "";
    }
  }

  remove(): void {
    fs.rmSync(this.dir, { recursive: true, force: true });
  }
}

type Push = { authorization: string; payload: { title: string; body: string; tag: string; device_id: string; pane_id: string; clear: string[] } };

/** A push service on loopback that decrypts RFC 8291 aes128gcm with the phone's own keys. */
async function pushService() {
  const ecdh = crypto.createECDH("prime256v1");
  ecdh.generateKeys();
  const auth = crypto.randomBytes(16);
  const received: Push[] = [];
  const decrypt = (body: Buffer) => {
    const salt = body.subarray(0, 16);
    const idLength = body[20] ?? 0;
    const serverKey = body.subarray(21, 21 + idLength);
    const record = body.subarray(21 + idLength);
    const secret = ecdh.computeSecret(serverKey);
    const info = Buffer.concat([Buffer.from("WebPush: info\0"), ecdh.getPublicKey(), serverKey]);
    const ikm = Buffer.from(crypto.hkdfSync("sha256", secret, auth, info, 32));
    const cek = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: aes128gcm\0"), 16));
    const nonce = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: nonce\0"), 12));
    const decipher = crypto.createDecipheriv("aes-128-gcm", cek, nonce);
    decipher.setAuthTag(record.subarray(record.length - 16));
    const plain = Buffer.concat([decipher.update(record.subarray(0, record.length - 16)), decipher.final()]);
    return plain.subarray(0, plain.lastIndexOf(2)).toString("utf8");
  };
  const server = http.createServer((request, response) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => {
      try {
        received.push({ authorization: String(request.headers.authorization ?? ""), payload: JSON.parse(decrypt(Buffer.concat(chunks))) as Push["payload"] });
        response.writeHead(201);
      } catch {
        response.writeHead(400);
      }
      response.end();
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as { port: number }).port;
  const b64 = (bytes: Buffer) => bytes.toString("base64url");
  return {
    endpoint: `http://127.0.0.1:${port}/push/phone-one`,
    p256dh: b64(ecdh.getPublicKey()),
    auth: b64(auth),
    received,
    close: () => server.close(),
  };
}

/**
 * An iPhone-sized touch page. Chromium has no push service of its own here,
 * so the page's PushManager hands out the loopback endpoint's subscription
 * and the permission answer is "granted" once asked.
 */
async function phoneContext(browser: Browser, push?: { endpoint: string; p256dh: string; auth: string }): Promise<BrowserContext> {
  const iphone = devices["iPhone 13"];
  const context = await browser.newContext({
    viewport: iphone.viewport,
    deviceScaleFactor: iphone.deviceScaleFactor,
    userAgent: iphone.userAgent,
    isMobile: true,
    hasTouch: true,
    colorScheme: "dark",
    locale: "ko-KR",
  });
  if (push) {
    await context.addInitScript((subscription) => {
      const record = { subscribed: 0, unsubscribed: 0, permission: "default" as NotificationPermission };
      (window as unknown as { __push: typeof record }).__push = record;
      let current: unknown = null;
      const fake = {
        endpoint: subscription.endpoint,
        toJSON: () => ({ endpoint: subscription.endpoint, keys: { p256dh: subscription.p256dh, auth: subscription.auth } }),
        unsubscribe: async () => {
          record.unsubscribed += 1;
          current = null;
          return true;
        },
      };
      Object.defineProperty(Notification, "permission", { configurable: true, get: () => record.permission });
      Notification.requestPermission = async () => {
        record.permission = "granted";
        return "granted";
      };
      PushManager.prototype.subscribe = async function () {
        record.subscribed += 1;
        current = fake;
        return fake as unknown as PushSubscription;
      };
      PushManager.prototype.getSubscription = async function () {
        return current as PushSubscription | null;
      };
    }, push);
  }
  return context;
}

/** Sets and clears one pane's status tokens, the way the label plugin reports them. */
function report(herdr: HerdrFixture, pane: string, set: Record<string, string>, clear: string[] = []): void {
  const args = ["pane", "report-metadata", pane, "--source", "e2e-mobile"];
  for (const [name, value] of Object.entries(set)) args.push("--token", `${name}=${value}`);
  for (const name of clear) args.push("--clear-token", name);
  execFileSync(herdr.bin, args, { env: herdr.env, timeout: 30_000 });
}

const STATES = ["status_question_new", "status_working", "status_done", "expected_reply"];

function ask(herdr: HerdrFixture, pane: string, question: string): void {
  report(herdr, pane, { status_question_new: "?", expected_reply: question }, STATES.filter((name) => name !== "status_question_new" && name !== "expected_reply"));
}

function work(herdr: HerdrFixture, pane: string): void {
  report(herdr, pane, { status_working: "●" }, STATES.filter((name) => name !== "status_working"));
}

function finish(herdr: HerdrFixture, pane: string): void {
  report(herdr, pane, { status_done: "✓" }, STATES.filter((name) => name !== "status_done"));
}

async function openMobileSettings(page: Page, daemon: Daemon): Promise<void> {
  // A fresh document: the same address with only a new fragment would not reload.
  await page.goto("about:blank");
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await expect(page.locator("[data-sidebar-mode]").first()).toBeVisible({ timeout: 20_000 });
  await page.keyboard.press("Alt+Comma");
  await page.locator('[data-settings-tab="mobile"]').click();
  await expect(page.locator('[data-mobile-tab="true"]')).toBeVisible();
}

/** The QR's `#pair=...` fragment, opened on this loopback daemon instead of the ts.net address. */
async function pairingUrl(page: Page, daemon: Daemon): Promise<string> {
  const qr = page.locator("[data-mobile-qr]");
  await expect(qr).toBeVisible({ timeout: 20_000 });
  const url = (await qr.getAttribute("data-mobile-qr")) ?? "";
  expect(url.startsWith(`https://${DNS}/m/#pair=`)).toBe(true);
  return `${daemon.origin}/m/${url.slice(url.indexOf("#"))}`;
}

function coreLog(daemon: Daemon): string {
  const dir = path.join(daemon.stateDir, "Logs");
  if (!fs.existsSync(dir)) return "";
  return fs
    .readdirSync(dir)
    .filter((name) => name.startsWith("core"))
    .map((name) => fs.readFileSync(path.join(dir, name), "utf8"))
    .join("\n");
}

/** The group the phone was last told a pane is in, from its own `agents` frames. */
function groupOf(frames: string[], pane: string): string | null {
  for (let index = frames.length - 1; index >= 0; index -= 1) {
    const frame = JSON.parse(frames[index] ?? "{}") as { type?: string; groups?: { group: string; agents: { pane_id: string }[] }[] };
    if (frame.type !== "agents") continue;
    return frame.groups?.find((group) => group.agents.some((agent) => agent.pane_id === pane))?.group ?? null;
  }
  return null;
}

async function noPushFor(received: Push[], count: number, ms = 3_000): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
  expect(received).toHaveLength(count);
}

test("Settings > Mobile to a paired phone: list, detail, reply, quick keys, push rules and revoke", async ({ browser, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  const tailscale = new FakeTailscale();
  const push = await pushService();
  let daemon: Daemon | null = null;
  const contexts: BrowserContext[] = [];
  try {
    const [one, two] = herdr.panes;
    daemon = await startHided(herdr, "mobile", undefined, { HIDE_TAILSCALE_BIN: tailscale.bin });

    // B1: off by default, and nothing asked Tailscale anything.
    await openMobileSettings(page, daemon);
    const toggle = page.locator('[data-mobile-switch="true"]');
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await expect(page.locator('[data-mobile-tab="true"]')).toContainText("자기가 만든 항목만 지웁니다");
    await expect(page.locator("[data-push-choice]")).toHaveCount(3);
    await expect(page.locator('[role="radio"][value="off"]')).toHaveAttribute("aria-checked", "true");
    expect(tailscale.calls()).toBe("");

    // B2: the CLI is missing, so the first step carries the download link and the rest wait.
    await toggle.click();
    const step = (id: string) => page.locator(`[data-mobile-step="${id}"]`);
    await expect(step("installed")).toHaveAttribute("data-step-state", "failed", { timeout: 20_000 });
    await expect(page.locator('[data-mobile-step-link="installed"]')).toHaveAttribute("href", "https://tailscale.com/download");
    for (const id of ["logged_in", "https", "phone"]) await expect(step(id)).toHaveAttribute("data-step-state", "waiting");
    await expect(page.locator("[data-mobile-qr]")).toHaveCount(0);
    await expect(page.locator('[data-mobile-pairing="waiting"]')).toContainText("QR이 여기 나타납니다");

    // B3: installed and logged out, then HTTPS off, each seen without reopening the tab.
    tailscale.loggedOut();
    tailscale.install();
    await expect(step("installed")).toHaveAttribute("data-step-state", "ok", { timeout: 20_000 });
    await expect(step("logged_in")).toHaveAttribute("data-step-state", "failed");
    await expect(step("logged_in")).toContainText("Tailscale 앱에서 로그인");
    tailscale.httpsOff();
    await expect(step("https")).toHaveAttribute("data-step-state", "failed", { timeout: 20_000 });
    await expect(step("logged_in")).toContainText("mac");
    await expect(page.locator('[data-mobile-step-link="https"]')).toHaveAttribute("href", "https://login.tailscale.com/admin/dns");
    await screenshot(page, "mobile-settings-blocked");

    // B4: every Mac step passes, hided adds its serve entry and only then shows the QR.
    tailscale.ready();
    const firstUrl = await pairingUrl(page, daemon);
    expect(tailscale.proxy()).toBe(daemon.origin);
    await expect(step("phone")).toHaveAttribute("data-step-state", "ok");
    await expect(page.locator("[data-mobile-url]")).toHaveAttribute("data-mobile-url", `https://${DNS}`);
    await expect(page.locator("[data-mobile-countdown]")).toHaveText(/코드는 [45]:\d\d 후 만료/);

    // B10: a new code voids the one before it.
    await page.locator('[data-mobile-new-code="true"]').click();
    await expect.poll(async () => pairingUrl(page, daemon as Daemon)).not.toBe(firstUrl);
    const pairUrl = await pairingUrl(page, daemon);
    const stale = await (await phoneContext(browser)).newPage();
    contexts.push(stale.context());
    await stale.goto(firstUrl);
    await stale.locator('[data-phone-pair="true"]').tap();
    await expect(stale.locator('[data-phone-guidance="code_expired"]')).toHaveText("코드가 만료됐어요. 맥에서 QR을 다시 여세요.", { timeout: 20_000 });

    // Agent states before the phone opens: one asks, two works.
    ask(herdr, one, "배포 전에 테스트를 다시 돌릴까요?");
    work(herdr, two);

    // B11: the QR opens the pairing page; 연결 lands on the list and the Mac lists the phone.
    const phoneContextOne = await phoneContext(browser, push);
    contexts.push(phoneContextOne);
    const phone = await phoneContextOne.newPage();
    const frames: string[] = [];
    phone.on("websocket", (socket) => socket.on("framereceived", (frame) => frames.push(String(frame.payload))));
    await phone.goto(pairUrl);
    await expect(phone.getByRole("heading", { name: "mac와 연결" })).toBeVisible();
    await expect(phone.getByText("코드는 5분 안에 만료돼요. 만료되면 맥에서 QR을 다시 여세요.")).toBeVisible();
    await screenshot(phone, "mobile-phone-pair");
    await phone.locator('[data-phone-pair="true"]').tap();
    await expect(phone.locator('[data-phone-connected="true"]')).toBeVisible({ timeout: 20_000 });
    await expect(phone.locator('[data-phone-install-hint="true"]')).toBeVisible();
    await expect(page.locator("[data-mobile-phones]")).toHaveAttribute("data-mobile-phones", "1");
    await expect(page.locator("[data-mobile-phone-line]")).toHaveText("방금");
    await expect(page.getByText("연결된 폰 · 1 / 4")).toBeVisible();
    // The spent code gives way to the next one while the tab is open.
    await expect.poll(async () => pairingUrl(page, daemon as Daemon)).not.toBe(pairUrl);
    // B10: the code that paired is spent.
    const again = await (await phoneContext(browser)).newPage();
    contexts.push(again.context());
    await again.goto(pairUrl);
    await again.locator('[data-phone-pair="true"]').tap();
    await expect(again.locator('[data-phone-guidance="code_expired"]')).toBeVisible({ timeout: 20_000 });
    // The credential is in the page address for Add to Home Screen, never the code.
    expect(new URL(phone.url()).hash).toMatch(/^#k=[0-9a-f]{64}$/);

    // B19, B20: the header and the four groups in order, each row's request line.
    await expect(phone.locator("[data-phone-header]")).toHaveAttribute("data-phone-header", "mac");
    const oneRow = phone.locator(`[data-phone-agent$="|${one}"]`);
    const twoRow = phone.locator(`[data-phone-agent$="|${two}"]`);
    await expect(phone.locator('[data-phone-group="needs_you"]').locator(`[data-phone-agent$="|${one}"]`)).toBeVisible({ timeout: 20_000 });
    await expect(oneRow.locator("[data-phone-line]")).toHaveText("배포 전에 테스트를 다시 돌릴까요?");
    await expect(phone.locator('[data-phone-group="working"]').locator(`[data-phone-agent$="|${two}"]`)).toBeVisible();
    await expect(phone.locator("[data-phone-group] h2").first()).toContainText("내 확인 대기");
    // B21: live; two asks and joins one under 내 확인 대기 without a reload.
    ask(herdr, two, "어느 브랜치에 올릴까요?");
    await expect(phone.locator('[data-phone-group="needs_you"] [data-phone-group-count]')).toHaveText("2", { timeout: 20_000 });
    // B41: an idle list sends nothing; the phone hears only changes.
    const agentFrames = () => frames.filter((frame) => frame.includes('"type":"agents"')).length;
    const idleStart = agentFrames();
    await new Promise((resolve) => setTimeout(resolve, 3_000));
    expect(agentFrames() - idleStart).toBeLessThanOrEqual(1);
    await screenshot(phone, "mobile-phone-list");

    // B24: the detail shows the head and the pane's recent rows, newest at the bottom.
    const history = [...Array.from({ length: 260 }, (_, index) => `line ${String(index + 1).padStart(3, "0")}`), `wide ${"w".repeat(300)}`].join("\n");
    execFileSync(herdr.bin, ["pane", "send-text", one, `${history}\n`], { env: herdr.env, timeout: 30_000 });
    await oneRow.tap();
    const detail = phone.locator("[data-phone-detail]");
    await expect(detail).toHaveAttribute("data-phone-detail", new RegExp(`\\|${one}$`));
    await expect(detail).toContainText("Agent one");
    const rows = phone.locator('[aria-label="터미널 최근 출력"]');
    await expect(rows).toContainText("line 260", { timeout: 20_000 });
    await expect(rows).not.toContainText("line 001");
    const scrollback = phone.locator("[data-phone-scrollback]");
    expect(await scrollback.evaluate((element) => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(48);
    // B24: a row as wide as the desktop pane wraps at the phone's width; nothing scrolls sideways.
    await expect(rows).toContainText("wide www");
    expect(await scrollback.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeLessThanOrEqual(0);
    expect(await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
    // B25: pulling to the top brings the older rows, up to what the pane holds.
    await expect(phone.getByText("위로 당기면 더 불러와요")).toBeVisible();
    await scrollback.evaluate((element) => {
      element.scrollTop = 0;
    });
    await expect(rows).toContainText("line 001", { timeout: 20_000 });
    await expect(scrollback).toHaveAttribute("data-phone-scrollback", "400");
    await expect(phone.getByText("위로 당기면 더 불러와요")).toHaveCount(0);
    await expect(rows).toContainText("fixture %");
    for (const name of ["Enter", "Escape", "위 화살표", "아래 화살표", "Ctrl-C"]) await expect(phone.getByRole("button", { name, exact: true })).toBeVisible();
    // B38: fingers fit every control.
    for (const control of [phone.locator('[data-phone-key="enter"]'), phone.locator('[data-phone-send="true"]'), phone.locator('[data-phone-reply="true"]')]) {
      const box = await control.boundingBox();
      expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
    }

    // B26: an empty reply cannot be sent; a reply lands with Enter and the field clears.
    await expect(phone.locator('[data-phone-send="true"]')).toBeDisabled();
    await phone.locator('[data-phone-reply="true"]').fill("yes please");
    await phone.locator('[data-phone-send="true"]').tap();
    await expect(phone.locator('[data-phone-reply="true"]')).toHaveValue("", { timeout: 20_000 });
    await expect.poll(() => (fs.existsSync(herdr.inputLogs[0]) ? fs.readFileSync(herdr.inputLogs[0], "utf8") : "")).toMatch(/yes please[\r\n]/);
    // B27: exactly once.
    expect(fs.readFileSync(herdr.inputLogs[0], "utf8").split("yes please").length - 1).toBe(1);
    // B25: the echoed reply reaches the scrollback on its own.
    await expect(phone.locator('[aria-label="터미널 최근 출력"]')).toContainText("yes please", { timeout: 20_000 });
    // B26: one key each.
    const logLength = fs.readFileSync(herdr.inputLogs[0], "utf8").length;
    await phone.locator('[data-phone-key="escape"]').tap();
    await expect.poll(() => fs.readFileSync(herdr.inputLogs[0], "utf8").slice(logLength)).toBe("\u001b");
    await phone.locator('[data-phone-key="up"]').tap();
    // Up is CSI A, or SS3 A in the application cursor mode a TUI may set.
    await expect.poll(() => ["\u001b\u001b[A", "\u001b\u001bOA"].includes(fs.readFileSync(herdr.inputLogs[0], "utf8").slice(logLength))).toBe(true);
    // B27: a reply over the limit is named before it is sent.
    await phone.locator('[data-phone-reply="true"]').fill("x".repeat(2001));
    await expect(phone.locator('[data-phone-input-error="true"]')).toContainText("2,000자까지");
    await expect(phone.locator('[data-phone-send="true"]')).toBeDisabled();
    await phone.locator('[data-phone-reply="true"]').fill("");
    await scrollback.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await screenshot(phone, "mobile-phone-detail");

    // B30: with push on, the list offers 알림 켜기; allowing it registers the subscription.
    await page.locator('[data-push-choice="always"]').click();
    await phone.locator('[data-phone-back="true"]').tap();
    const enable = phone.locator('[data-phone-notifications="enable"]');
    await expect(enable).toBeVisible({ timeout: 20_000 });
    await enable.tap();
    await expect(enable).toHaveCount(0, { timeout: 20_000 });
    await expect(page.locator("[data-mobile-phone-line]")).toHaveText("방금 · 알림 받는 중");

    // B31, B33 (항상): two finishes; one notification for it, with no terminal content.
    finish(herdr, two);
    await expect.poll(() => push.received.length, { timeout: 20_000 }).toBe(1);
    const done = push.received[0]!;
    expect(done.payload.title).toBe("Agent two");
    expect(done.payload.body).toMatch(/^끝 · /);
    expect(done.payload.tag).toBe(`${done.payload.device_id}|${two}`);
    expect(done.payload.pane_id).toBe(two);
    expect(JSON.stringify(done.payload)).not.toContain("fixture %");
    expect(done.authorization).toMatch(/^vapid t=[\w-]+\.[\w-]+\.[\w-]+, k=[\w-]+$/);

    // B33: while the phone views one, one's request sends nothing.
    await oneRow.tap();
    await expect(detail).toBeVisible();
    work(herdr, one);
    await expect.poll(() => groupOf(frames, one)).toBe("working");
    ask(herdr, one, "정말 배포할까요?");
    await expect.poll(() => groupOf(frames, one)).toBe("needs_you");
    await noPushFor(push.received, 1);
    await expect.poll(() => coreLog(daemon as Daemon)).toContain('"reason":"viewing"');
    await phone.locator('[data-phone-back="true"]').tap();

    // B33 (끔): nothing.
    await page.locator('[data-push-choice="off"]').click();
    work(herdr, two);
    await expect.poll(() => groupOf(frames, two)).toBe("working");
    ask(herdr, two, "끔에서는 조용히");
    await expect.poll(() => groupOf(frames, two)).toBe("needs_you");
    await noPushFor(push.received, 1);

    // B33 (앱이 닫혀 있을 때만): nothing while the desktop is connected. The
    // desktop then reads two, which clears its notification (B34).
    await page.locator('[data-push-choice="app_closed"]').click();
    work(herdr, two);
    await expect.poll(() => groupOf(frames, two)).toBe("working");
    finish(herdr, two);
    await expect.poll(() => groupOf(frames, two)).toBe("done");
    await noPushFor(push.received, 1);
    await expect.poll(() => coreLog(daemon as Daemon)).toContain('"mode":"app_closed","renderers":1');
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-mobile-tab="true"]')).toHaveCount(0);
    await page.locator('[data-sidebar-mode="agents"]').click();
    await page.locator(`[data-agent-list] [data-agent-open="${two}"]`).click();
    await expect(phone.locator('[data-phone-group="seen"]').locator(`[data-phone-agent$="|${two}"]`)).toBeVisible({ timeout: 20_000 });
    await page.close();
    work(herdr, one);
    await expect.poll(() => groupOf(frames, one)).toBe("working");
    ask(herdr, one, "데스크톱이 닫혔을 때");
    await expect.poll(() => push.received.length, { timeout: 20_000 }).toBe(2);
    const closed = push.received[1]!;
    expect(closed.payload.title).toBe("Agent one");
    expect(closed.payload.body).toMatch(/^내 확인 대기 · /);
    expect(closed.payload.clear).toContain(`${closed.payload.device_id}|${two}`);

    // B28: the pane closes under an open detail; the reply bar goes inert.
    await twoRow.tap();
    execFileSync(herdr.bin, ["pane", "close", two], { env: herdr.env, timeout: 30_000 });
    await expect(phone.locator('[data-phone-rows-state="gone"]')).toHaveText("이 pane은 더 이상 열려 있지 않아요.", { timeout: 20_000 });
    await expect(phone.locator('[data-phone-reply="true"]')).toBeDisabled();
    await screenshot(phone, "mobile-phone-gone");
    await phone.locator('[data-phone-back="true"]').tap();

    // B15, B36: revoke from the Mac closes the phone at once and drops its subscription.
    const desk = await page.context().newPage();
    await openMobileSettings(desk, daemon);
    await desk.locator("[data-mobile-revoke]").click();
    await expect(desk.locator("[data-mobile-phone]")).toHaveCount(0);
    await expect(phone.locator('[data-phone-guidance="revoked"]')).toHaveText("이 폰의 연결이 해지됐어요. 맥에서 QR을 다시 여세요.", { timeout: 20_000 });
    await expect.poll(() => phone.evaluate(() => (window as unknown as { __push: { unsubscribed: number } }).__push.unsubscribed)).toBe(1);
    const phones = JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "phones.json"), "utf8")) as { phones: unknown[] };
    expect(phones.phones).toHaveLength(0);
    await screenshot(phone, "mobile-phone-revoked");

    // B7: switching off removes only hide's own serve entry.
    await desk.locator('[data-mobile-switch="true"]').click();
    await expect.poll(() => tailscale.proxy(), { timeout: 20_000 }).toBeNull();
    expect(tailscale.calls()).toContain("serve --yes --https=443 --set-path=/ off");

    // B40: each kind is its own record, and no secret reaches the log.
    const log = coreLog(daemon);
    for (const component of ["mobile_transport", "mobile_pairing", "mobile_phone", "mobile_push"]) expect(log).toContain(`"component":"${component}"`);
    const code = new URLSearchParams(pairUrl.slice(pairUrl.indexOf("#") + 1)).get("pair") ?? "";
    const credential = new URL(phone.url()).hash.slice(3);
    for (const secret of [code, JSON.parse(Buffer.from(code, "base64url").toString("utf8")).code as string, credential, push.p256dh, push.auth]) {
      if (secret) expect(log).not.toContain(secret);
    }
  } finally {
    for (const context of contexts) await context.close();
    push.close();
    daemon?.stop();
    herdr.stop();
    tailscale.remove();
  }
});

test("an empty list, the unreachable line, the phone limit and the seven-day revoke", async ({ browser, page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr({ agents: false });
  const tailscale = new FakeTailscale();
  tailscale.install();
  tailscale.ready();
  let daemon: Daemon | null = null;
  const contexts: BrowserContext[] = [];
  try {
    daemon = await startHided(herdr, "mobile-limits", undefined, { HIDE_TAILSCALE_BIN: tailscale.bin });
    await openMobileSettings(page, daemon);
    await page.locator('[data-mobile-switch="true"]').click();
    await page.locator('[data-push-choice="app_closed"]').click();
    const phoneContextA = await phoneContext(browser);
    contexts.push(phoneContextA);
    const phone = await phoneContextA.newPage();
    await phone.goto(await pairingUrl(page, daemon));
    await phone.locator('[data-phone-pair="true"]').tap();

    // B22: no agents, one line.
    await expect(phone.locator('[data-phone-empty="true"]')).toHaveText("실행 중인 에이전트가 없어요", { timeout: 20_000 });
    await screenshot(phone, "mobile-phone-empty");
    // B38: the phone's own light or dark setting decides the theme.
    await expect(phone.locator("html")).toHaveClass(/dark/);
    await phone.emulateMedia({ colorScheme: "light" });
    await expect(phone.locator("html")).not.toHaveClass(/dark/);
    await screenshot(phone, "mobile-phone-empty-light");
    await phone.emulateMedia({ colorScheme: "dark" });

    // B23, B8: the daemon goes away; the list dims under the unreachable line,
    // and the phone comes back on its own with the same credential.
    daemon = await daemon.restart(async () => {
      await expect(phone.locator('[data-phone-unreachable="true"]')).toContainText("연결 안 됨 · 맥의 hide가 꺼져 있거나 폰의 Tailscale이 꺼져 있어요. 다시 시도 중", { timeout: 20_000 });
      await screenshot(phone, "mobile-phone-unreachable");
    });
    await expect(phone.locator('[data-phone-connected="true"]')).toBeVisible({ timeout: 30_000 });
    await expect(phone.locator('[data-phone-unreachable="true"]')).toHaveCount(0);
    const restarted = daemon.origin;
    await expect.poll(() => tailscale.proxy(), { timeout: 20_000 }).toBe(restarted);

    // B23: opened with no network, the cached shell shows the same line instead of a blank page.
    await phone.reload();
    await expect(phone.locator('[data-phone-connected="true"]')).toBeVisible({ timeout: 20_000 });
    await phoneContextA.setOffline(true);
    await phone.reload();
    await expect(phone.locator('[data-phone-unreachable="true"]')).toBeVisible({ timeout: 20_000 });
    await phoneContextA.setOffline(false);
    await expect(phone.locator('[data-phone-connected="true"]')).toBeVisible({ timeout: 30_000 });

    // B29: the push mode survived the restart.
    await openMobileSettings(page, daemon);
    await expect(page.locator('[role="radio"][value="app_closed"]')).toHaveAttribute("aria-checked", "true");

    // B14: three more phones on record make four; a fifth is refused with the reason.
    const record = (id: string, lastSeen: number) => ({
      id,
      name: `폰 ${id}`,
      credential_sha256: crypto.createHash("sha256").update(id).digest("hex"),
      paired_at_ms: lastSeen,
      last_seen_ms: lastSeen,
      notifications: "unasked",
      push: null,
    });
    daemon = await daemon.restart((stateDir) => {
      const file = path.join(stateDir, "phones.json");
      const current = JSON.parse(fs.readFileSync(file, "utf8")) as { phones: unknown[] };
      current.phones.push(record("b", Date.now()), record("c", Date.now()), record("d", Date.now()));
      fs.writeFileSync(file, JSON.stringify(current));
    });
    await openMobileSettings(page, daemon);
    await expect(page.getByText("연결된 폰 · 4 / 4")).toBeVisible({ timeout: 20_000 });
    const fifth = await (await phoneContext(browser)).newPage();
    contexts.push(fifth.context());
    await fifth.goto(await pairingUrl(page, daemon));
    await fifth.locator('[data-phone-pair="true"]').tap();
    await expect(fifth.locator('[data-phone-guidance="phone_limit"]')).toHaveText("폰은 4대까지 연결할 수 있어요. 맥의 설정 > Mobile에서 하나를 해지하세요.", { timeout: 20_000 });
    await screenshot(page, "mobile-settings-ready");

    // B16: a phone away for eight days is revoked at start and told so when it opens.
    await expect(phone.locator('[data-phone-connected="true"]')).toBeVisible({ timeout: 30_000 });
    const phoneId = (JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "phones.json"), "utf8")) as { phones: { id: string; name: string }[] }).phones.find((entry) => entry.name === "iPhone")?.id;
    expect(phoneId).toBeTruthy();
    await phoneContextA.setOffline(true);
    daemon = await daemon.restart((stateDir) => {
      const file = path.join(stateDir, "phones.json");
      const current = JSON.parse(fs.readFileSync(file, "utf8")) as { phones: { id: string; last_seen_ms: number }[] };
      for (const entry of current.phones) if (entry.id === phoneId) entry.last_seen_ms = Date.now() - 8 * 24 * 60 * 60 * 1000;
      fs.writeFileSync(file, JSON.stringify(current));
    });
    await phoneContextA.setOffline(false);
    await expect(phone.locator('[data-phone-guidance="revoked"]')).toHaveText("이 폰의 연결이 해지됐어요. 맥에서 QR을 다시 여세요.", { timeout: 30_000 });
    await openMobileSettings(page, daemon);
    await expect(page.getByText("연결된 폰 · 3 / 4")).toBeVisible({ timeout: 20_000 });
  } finally {
    for (const context of contexts) await context.close();
    daemon?.stop();
    herdr.stop();
    tailscale.remove();
  }
});
