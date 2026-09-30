import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { REPO } from "./fixture";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { isolate, launch, test } from "./fixture";

function owner(id: string, kind = "id"): string {
  const hash = crypto.createHash("sha256");
  for (const part of ["claude", kind, id]) {
    const bytes = Buffer.from(part);
    const size = Buffer.alloc(8);
    size.writeBigUInt64BE(BigInt(bytes.length));
    hash.update(size).update(bytes);
  }
  return `v1:${hash.digest("hex")}`;
}

test("native session replacement hides stale labels before the watcher publishes", async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "session-labels");
  let app: ElectronApplication | undefined;
  let watcher: ChildProcess | undefined;
  const watcherHome = fs.mkdtempSync("/tmp/labels-");
  const watcherBin = path.join(REPO, "target/debug/hide-agent-context-labels");
  const stopWatcher = async () => {
    if (!watcher || watcher.exitCode !== null || watcher.signalCode !== null) return;
    const exited = new Promise<void>((resolve) => watcher!.once("exit", () => resolve()));
    watcher.kill("SIGTERM");
    await exited;
  };
  const pane = herdr.panes[0];
  const report = (args: string[]) => execFileSync(herdr.bin, ["pane", ...args], { env: herdr.env, timeout: 30_000 });
  let publication = 0;
  const labels = (id: string, task: string) => report(["report-metadata", pane, "--source", "e2e", "--token", `label_owner=${owner(id)}`, "--token", `status_owner=${owner(id)}`, "--token", `label_generation=fixture:${++publication}`, "--token", `status_generation=fixture:${publication}`, "--token", `task=${task}`, "--token", "progress=현재 세션 진행 중", "--token", "expected_reply=현재 세션 답변 요청", "--token", "status_question_new=?"]);
  try {
    labels(`fixture-${pane}`, "세션 A의 한글 작업");
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.writeFileSync(path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "fixture-agents.json"), JSON.stringify(herdr.run(["agent", "list"])));
    const launched = await launch(run.env);
    app = launched.app;
    const page = launched.page;
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });
    let consumedSession: string | undefined;
    let consumedDemand: string | undefined;
    page.on("websocket", (socket) => socket.on("framereceived", (frame) => {
      const visit = (value: unknown): void => {
        if (Array.isArray(value)) value.forEach(visit);
        else if (value && typeof value === "object") {
          const row = value as Record<string, unknown>;
          if (row.pane_id === pane && typeof row.session_id === "string") {
            consumedSession = row.session_id;
            consumedDemand = typeof row.demand === "string" ? row.demand : undefined;
          }
          Object.values(row).forEach(visit);
        }
      };
      try { visit(JSON.parse(String(frame.payload))); } catch { /* non-JSON handshake */ }
    }));
    await page.reload();

    await expect(page.locator("[data-main-screen], [data-workspace-screen]")).toBeVisible({ timeout: 30_000 });
    await enterWorkspace(page, "fixture");
    await page.locator(`[data-pane-view="${pane}"]`).click({ position: { x: 30, y: 60 } });
    const tab = page.locator(`[data-tab="${herdr.tab}"]`);
    await expect(tab).toContainText("세션 A의 한글 작업");
    const candidate = await app.evaluate(({ BrowserWindow }) => {
      const windows = BrowserWindow.getAllWindows();
      if (windows.length !== 1) throw new Error("expected one isolated candidate window");
      return { pid: process.pid, source: windows[0]!.getMediaSourceId() };
    });
    const capture = async (name: string) => {
      const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
      if (!dir) return;
      fs.mkdirSync(dir, { recursive: true });
      await page.waitForTimeout(200);
      execFileSync("/usr/sbin/screencapture", ["-x", "-o", "-l", candidate.source.split(":")[1]!, path.join(dir, `${name}.png`)]);
      fs.writeFileSync(path.join(dir, `${name}.json`), JSON.stringify({ ...candidate, head: process.env.HIDE_QA_HEAD, build: "worktree desktop/dist", isolated: true, nativeInput: false, rendererClick: true }));
    };
    await capture("session-a");
    report(["report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", "session-b", "--seq", "2", "--session-start-source", "clear"]);
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.writeFileSync(path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "fixture-agents-b.json"), JSON.stringify(herdr.run(["agent", "list"])));
    await expect.poll(() => consumedSession).toBe("session-b");
    await expect(tab).not.toContainText("세션 A의 한글 작업");
    await expect(page.getByText("세션 A의 한글 작업", { exact: true })).toHaveCount(0);
    await expect(page.getByText("현재 세션 답변 요청", { exact: true })).toHaveCount(0);
    await expect(tab).toContainText("Claude");
    await capture("session-b-unlabelled");
    // No watcher has run: Herdr still retains the original A publication.
    // Returning to A must not revalidate that invalidated publication.
    await page.evaluate(() => {
      const probe = { count: 0, observer: new MutationObserver(() => {
        if (document.body.textContent?.includes("세션 A의 한글 작업")) probe.count++;
      }) };
      probe.observer.observe(document.body, { subtree: true, childList: true, characterData: true });
      (window as unknown as { retainedLabelProbe: typeof probe }).retainedLabelProbe = probe;
    });
    report(["report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", `fixture-${pane}`, "--seq", "3", "--session-start-source", "clear"]);
    await expect.poll(() => consumedSession).toBe(`fixture-${pane}`);
    expect(consumedDemand).toBe("none");
    const retained = (herdr.run(["agent", "list"]) as { result: { agents: { pane_id: string; tokens: Record<string, string> }[] } }).result.agents.find((agent) => agent.pane_id === pane)!;
    expect(retained.tokens.task).toBe("세션 A의 한글 작업");
    expect(retained.tokens.label_generation).toBe("fixture:1");

    await expect(tab).toContainText("Claude");
    await expect(page.getByText("세션 A의 한글 작업", { exact: true })).toHaveCount(0);
    await expect(page.getByText("현재 세션 답변 요청", { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => {
      const probe = (window as unknown as { retainedLabelProbe: { count: number; observer: MutationObserver } }).retainedLabelProbe;
      probe.observer.disconnect();
      return probe.count;
    })).toBe(0);
    await capture("retained-a-return-unlabelled");
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.writeFileSync(path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "retained-publication.json"), JSON.stringify({ ...candidate, head: process.env.HIDE_QA_HEAD, pane, observedReturn: consumedSession, demand: consumedDemand, retainedGeneration: retained.tokens.label_generation, forbiddenDomObservations: 0, watcherStarted: false, nativeInput: false }));
    labels(`fixture-${pane}`, "새 게시로 복원한 A 작업");
    await expect(tab).toContainText("새 게시로 복원한 A 작업");
    report(["report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", "session-b", "--seq", "4", "--session-start-source", "clear"]);
    // The current owner restores its own label through the actual metadata API.
    labels("session-b", "세션 B의 한글 작업");
    await expect(tab).toContainText("세션 B의 한글 작업");
    await page.locator('[data-sidebar-search]').click();
    const search = page.locator('[data-palette="Search"] [data-palette-input]');
    await expect(search).toBeFocused();
    await search.fill("세션 B의 한글 작업");
    await expect(page.locator('[data-palette="Search"]')).toContainText("세션 B의 한글 작업");
    await expect(page.locator('[data-palette="Search"]')).not.toContainText("세션 A의 한글 작업");
    await capture("session-b-current");
    await page.keyboard.press("Escape");
    // Retire the earlier synthetic demand before observing actual hook publication.
    report(["report-metadata", pane, "--source", "e2e", "--clear-token", "status_question_new"]);
    await expect.poll(() => consumedDemand).toBe("none");
    const stateRoot = path.join(watcherHome, ".local/state/hide.agent-context-labels");
    const transcriptRoot = path.join(watcherHome, ".claude/projects/fixture");
    fs.mkdirSync(stateRoot, { recursive: true });
    fs.mkdirSync(transcriptRoot, { recursive: true });
    fs.writeFileSync(path.join(transcriptRoot, "session-b.jsonl"), JSON.stringify({ type: "user", sessionId: "session-b", timestamp: "2026-09-30T10:00:00Z", origin: { kind: "human" }, message: { content: "현재 세션의 작업을 복원해줘" } }) + "\n");
    const watcherEnv = { ...herdr.env, HOME: watcherHome, HERDR_BIN_PATH: herdr.bin, HERDR_PANE_ID: pane };
    // The actual hook CLI must accept B while the durable display still owns A.
    const displayFile = path.join(stateRoot, "display-state.json");
    fs.writeFileSync(displayFile, JSON.stringify({ panes: { [pane]: { session_owner: owner(`fixture-${pane}`), state_change_seq: 1, changed_unix_ms: Date.now(), task: "이전 A hook fixture", progress: "이전 A 진행", expected_reply: "", unseen: false } } }));
    execFileSync(watcherBin, ["hook"], { env: watcherEnv, input: JSON.stringify({
      session_id: "session-b", transcript_path: path.join(transcriptRoot, "session-b.jsonl"),
      hook_event_name: "PermissionRequest", tool_name: "AskUserQuestion", tool_use_id: "b-question",
    }) });
    const earlyHook = JSON.parse(fs.readFileSync(path.join(stateRoot, "hook-state.json"), "utf8")).panes[pane];
    expect(earlyHook.session_owner).toBe(owner("session-b"));
    expect(earlyHook.attention).toBe("question");
    expect(JSON.parse(fs.readFileSync(displayFile, "utf8")).panes[pane].session_owner).toBe(owner(`fixture-${pane}`));
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.writeFileSync(path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "early-current-hook.json"), JSON.stringify({ ...candidate, head: process.env.HIDE_QA_HEAD, pane, nativeOwner: "session-b", persistedOwnerUnchanged: true, acceptedHookOwner: earlyHook.session_owner, attention: earlyHook.attention }));
    execFileSync(watcherBin, ["set-automatic-summaries", "--enabled", "false"], { env: watcherEnv });
    const startWatcher = () => { watcher = spawn(watcherBin, ["watch"], { env: watcherEnv, stdio: "ignore" }); };
    startWatcher();
    // Preserve durable A until actual watcher reconciliation confirms B.
    await expect.poll(() => JSON.parse(fs.readFileSync(displayFile, "utf8")).panes[pane].session_owner).toBe(owner("session-b"));
    await expect.poll(() => consumedDemand).toBe("question");
    const hookTokens = (herdr.run(["agent", "list"]) as { result: { agents: { pane_id: string; tokens: Record<string, string> }[] } }).result.agents.find((agent) => agent.pane_id === pane)!.tokens;
    expect(hookTokens.status_owner).toBe(owner("session-b"));
    expect(hookTokens.status_generation).not.toMatch(/^fixture:/);
    expect([hookTokens.status_question, hookTokens.status_question_new]).toContain("?");
    const reconciledHook = JSON.parse(fs.readFileSync(path.join(stateRoot, "hook-state.json"), "utf8")).panes[pane];
    expect(reconciledHook.session_owner).toBe(owner("session-b"));
    expect(reconciledHook.attention).toBe("question");
    expect(JSON.parse(fs.readFileSync(displayFile, "utf8")).panes[pane].task).toBeNull();
    await expect(tab).toContainText("Claude");
    await expect(page.getByText("이전 A hook fixture", { exact: true })).toHaveCount(0);
    await capture("early-current-hook-reconciled");
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.writeFileSync(path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "early-current-hook-reconciled.json"), JSON.stringify({ ...candidate, head: process.env.HIDE_QA_HEAD, pane, hookOwner: reconciledHook.session_owner, attention: reconciledHook.attention, demand: consumedDemand, publicationGeneration: hookTokens.status_generation, initialDemandCleared: true, actualWatcherReconciliation: true }));
    await stopWatcher();
    // Separately seed proven same-B state for restart/restoration observation.
    fs.writeFileSync(displayFile, JSON.stringify({ panes: { [pane]: { session_owner: owner("session-b"), state_change_seq: 1, changed_unix_ms: Date.now(), task: "워처가 복원한 한글 작업", progress: "검증된 세션 진행", expected_reply: "", unseen: false } } }));
    startWatcher();
    await expect(tab).toContainText("워처가 복원한 한글 작업", { timeout: 10_000 });
    await stopWatcher();
    startWatcher();
    await expect.poll(() => fs.readFileSync(path.join(stateRoot, "events.jsonl"), "utf8").split("watcher_started").length - 1).toBe(3);
    await expect(tab).toContainText("워처가 복원한 한글 작업");
    await capture("watcher-restarted-current");
    let sequence = 4;
    const publications: string[] = [];
    const currentTokens = () => (herdr.run(["agent", "list"]) as { result: { agents: { pane_id: string; tokens: Record<string, string> }[] } }).result.agents.find((agent) => agent.pane_id === pane)!.tokens;
    publications.push(currentTokens().label_generation!);
    for (const [kind, value] of [["path", path.join(transcriptRoot, "session-b.jsonl")], ["id", "session-b"], ["path", path.join(transcriptRoot, "session-b.jsonl")], ["id", "session-b"]]) {
      report(["report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude", kind === "id" ? "--agent-session-id" : "--agent-session-path", value!, "--seq", String(++sequence), "--session-start-source", "clear"]);
      await expect.poll(() => currentTokens().label_owner).toBe(owner(value!, kind));
      await expect.poll(() => currentTokens().status_owner).toBe(owner(value!, kind));
      const generation = currentTokens().label_generation!;
      expect(publications).not.toContain(generation);
      publications.push(generation);
      await expect(tab).toContainText("워처가 복원한 한글 작업");
    }
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.writeFileSync(path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "equivalent-reference-publications.json"), JSON.stringify({ ...candidate, head: process.env.HIDE_QA_HEAD, pane, sequence, distinctGenerations: publications, sameCanonicalSession: true, analysisDisabled: true }));
    await capture("equivalent-reference-current");
    report(["report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", "session-c", "--seq", String(++sequence), "--session-start-source", "clear"]);
    await expect(tab).toContainText("Claude");
    await expect(page.getByText("워처가 복원한 한글 작업", { exact: true })).toHaveCount(0);
    const log = fs.readFileSync(path.join(stateRoot, "events.jsonl"), "utf8");
    expect(log).not.toContain("analysis_recorded");
    expect(log).not.toContain("현재 세션의 작업을 복원해줘");

    // Hold an actual provider child across C -> B -> C. The provider output is
    // synthetic; the watcher, child lifetime, Herdr reports and native UI are real.
    await stopWatcher();
    const providerBin = path.join(watcherHome, "bin");
    fs.mkdirSync(providerBin);
    fs.writeFileSync(path.join(providerBin, "claude"), `#!/usr/bin/python3
import json, os, pathlib, sys, time
root = pathlib.Path(os.environ["HOME"])
if sys.argv[1:4] == ["auth", "status", "--json"]:
    print(json.dumps({"loggedIn": True}))
    sys.exit(0)
sys.stdin.read()
calls = root / "provider-calls"
with calls.open("a") as f:
    f.write(str(os.getpid()) + "\\n")
index = len(calls.read_text().splitlines())
if index == 1:
    deadline = time.monotonic() + 45
    while not (root / "release-provider").exists() and time.monotonic() < deadline:
        if os.getppid() == 1:
            sys.exit(2)
        time.sleep(0.02)
task = "폐기해야 할 지연 결과" if index == 1 else "현재 세대의 한글 작업"
print(json.dumps({"type": "result", "subtype": "success", "is_error": False,
    "structured_output": {"task": task, "task_changed": True, "progress": "현재 세대 진행",
        "expected_reply": "", "attention": "none"}}))
`, { mode: 0o755 });
    const aiSettings = path.join(watcherHome, "Library/Application Support/hide");
    fs.mkdirSync(aiSettings, { recursive: true });
    fs.writeFileSync(path.join(aiSettings, "ai.json"), JSON.stringify({ provider: "claude" }));
    fs.writeFileSync(path.join(transcriptRoot, "session-c.jsonl"), JSON.stringify({ type: "user", sessionId: "session-c", timestamp: "2026-09-30T11:00:00Z", origin: { kind: "human" }, message: { content: "현재 세대 작업을 해줘" } }) + "\n");
    execFileSync(watcherBin, ["set-automatic-summaries", "--enabled", "true"], { env: watcherEnv });
    watcher = spawn(watcherBin, ["watch"], { env: { ...watcherEnv, PATH: providerBin + ":/usr/bin:/bin" }, stdio: "ignore" });
    const callCount = () => fs.existsSync(path.join(watcherHome, "provider-calls")) ? fs.readFileSync(path.join(watcherHome, "provider-calls"), "utf8").trim().split("\n").length : 0;
    await expect.poll(callCount, { timeout: 15_000 }).toBe(1);
    const transition = (id: string, sequence: number) => report(["report-agent-session", pane, "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", id, "--seq", String(sequence), "--session-start-source", "clear"]);
    transition("session-b", ++sequence);
    await expect.poll(() => JSON.parse(fs.readFileSync(path.join(stateRoot, "display-state.json"), "utf8")).panes[pane].session_owner).toBe(owner("session-b"));
    transition("session-c", ++sequence);
    await expect.poll(() => JSON.parse(fs.readFileSync(path.join(stateRoot, "display-state.json"), "utf8")).panes[pane].session_owner).toBe(owner("session-c"));
    await expect(tab).toContainText("Claude");
    // Waiting work cannot start while the invalidated physical child is held.
    expect(callCount()).toBe(1);
    await page.evaluate(() => {
      (window as unknown as { staleLabelFrames: number }).staleLabelFrames = 0;
      new MutationObserver(() => {
        if (document.body.textContent?.includes("폐기해야 할 지연 결과"))
          (window as unknown as { staleLabelFrames: number }).staleLabelFrames++;
      }).observe(document.body, { subtree: true, childList: true, characterData: true });
    });
    fs.writeFileSync(path.join(watcherHome, "release-provider"), "");
    await expect(tab).toContainText("현재 세대의 한글 작업", { timeout: 15_000 });
    expect(callCount()).toBe(2);
    expect(await page.evaluate(() => (window as unknown as { staleLabelFrames: number }).staleLabelFrames)).toBe(0);
    const finalLog = fs.readFileSync(path.join(stateRoot, "events.jsonl"), "utf8");
    expect(finalLog).toContain("analysis_discarded_session");
    expect(finalLog).not.toContain("현재 세대 작업을 해줘");
    await capture("generation-current");
    await page.reload();
    await expect(tab).toContainText("현재 세대의 한글 작업", { timeout: 10_000 });
    await capture("reconnected-current");
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) fs.copyFileSync(path.join(stateRoot, "events.jsonl"), path.join(process.env.HIDE_E2E_SCREENSHOT_DIR, "managed-watcher.jsonl"));

  } finally {
    await stopWatcher();
    const evidenceDir = process.env.HIDE_E2E_SCREENSHOT_DIR;
    const watcherLog = path.join(watcherHome, ".local/state/hide.agent-context-labels/events.jsonl");
    if (evidenceDir && fs.existsSync(watcherLog))
      fs.copyFileSync(watcherLog, path.join(evidenceDir, "managed-watcher.jsonl"));
    fs.rmSync(watcherHome, { recursive: true, force: true });
    await app?.close();
    run.cleanup();
    herdr.stop();
  }
});
