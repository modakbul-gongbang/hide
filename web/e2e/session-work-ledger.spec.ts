import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { continueFixtureTranscript, setFixtureLifecycle, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { FIRST_GOAL, MISSED, NEXT_GOAL, nextWork, seedFirstWork, workFixture, workRepositoryReady } from "./session-work-fixture";
import { countSent, enterWorkspace, sendEvent, showProjectRow, showTool } from "./wire";
import { chord } from "./chords";

test.describe.configure({ timeout: 180_000 });

async function connectRepository(page: import("@playwright/test").Page, repository: string) {
  const gear = page.locator('[data-project]', { hasText: "fixture" }).locator('[data-row-settings]').first();
  await (await showProjectRow(page, gear)).click();
  const sheet = page.locator('[data-project-settings]');
  await sheet.getByRole("combobox", { name: "GitHub repository", exact: true }).click();
  await page.getByRole("option", { name: "Choose repository", exact: true }).click();
  await sheet.getByRole("textbox", { name: "owner/repository" }).fill(repository);
  await sheet.getByRole("button", { name: "Apply", exact: true }).click();
  await expect(sheet).toContainText("Saved");
  await page.keyboard.press("Escape");
}

function originalRepository(stateDir: string, session: string) {
  const db = new DatabaseSync(path.join(stateDir,"links.sqlite3"),{readOnly:true});
  try { return db.prepare("SELECT repository FROM ledger_session_origins WHERE id=?").get(session)?.repository; }
  finally { db.close(); }
}

test("@platform a native session keeps its chapters, child outputs and reversible missed work through search and summaries off", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr, "session-work-ledger", undefined, github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page, "fixture");
    await showTool(page, "agent_sessions");
    const first = Date.now();
    github.clock(first, first);
    const { session, child } = seedFirstWork(herdr, first);
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL, { timeout: 45_000 });
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const record = page.locator("[data-session-record]");
    const tasks = record.locator('[data-record-work]').first().locator('[data-record-task]');
    await expect(tasks).toHaveCount(1, { timeout: 45_000 });
    await expect(tasks.first()).toContainText("Done");
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(2, { timeout: 45_000 });
    await page.locator(`[data-pane-view="${herdr.panes[0]}"] .xterm-helper-textarea`).focus();
    await page.keyboard.type(NEXT_GOAL);
    await page.keyboard.press("Enter");
    const second = Date.now();
    github.clock(first, second);
    continueFixtureTranscript(herdr, session, nextWork(second));
    await setFixtureLifecycle(herdr, herdr.panes[0], "working");
    await expect(root).toContainText(NEXT_GOAL, { timeout: 45_000 });
    await setFixtureLifecycle(herdr, herdr.panes[0], "idle");
    // Typing in the terminal chose its agent, which put the unpinned lane away (PRD right-lane B15).
    await showTool(page, "agent_sessions");
    await record.getByRole("button", { name: "Back to Sessions" }).click();
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    await expect(tasks).toHaveCount(2, { timeout: 45_000 });
    await expect(record).toContainText(FIRST_GOAL);
    await expect(record).toContainText(NEXT_GOAL);
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(3, { timeout: 45_000 });
    await expect(tasks).toContainText(["CI failed", "Done"]);
    await record.locator(`[data-record-child="${child}"] > summary`).click();
    await expect(record.locator(`[data-record-child="${child}"] [data-record-task]`)).toContainText("자식 연결 검증");
    const missed = record.locator("[data-missed-work]", { hasText: MISSED });
    await missed.getByRole("button", { name: "Dismiss", exact: true }).click();
    await record.getByText("Closed 1", { exact: true }).click();
    await missed.getByRole("button", { name: "Undo", exact: true }).click();
    await expect(missed).toHaveAttribute("data-missed-status", "open");
    await page.keyboard.press(chord("search"));
    await page.locator("[data-palette-input]").fill(NEXT_GOAL);
    const hits = page.locator('[data-palette-group="sessions"]');
    await expect(hits).toContainText(NEXT_GOAL, { timeout: 30_000 });
    await hits.locator('[data-palette-row]').first().click();
    await expect(record).toContainText(FIRST_GOAL);
    await page.keyboard.press(chord("settings"));
    await page.locator('[data-settings-tab="hideAi"]').click();
    await page.locator('[data-ai-agent-summary="true"]').click();
    await page.keyboard.press("Escape");
    await expect(record.locator('[data-record-verdict]')).toHaveCount(0);
    await expect(record.locator('[data-record-task]')).toHaveCount(0);
    await expect(record.locator('[data-record-missed]')).toHaveCount(0);
    await expect(record.locator('[data-record-request]')).toHaveCount(3);
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(3);
    await expect(record.getByRole('heading', { level: 2 })).toHaveText('Claude');
    await expect(record.locator('[data-record-child] > summary')).toContainText('Claude');
    await expect(record.locator('[data-output-source]')).toHaveText(Array(4).fill('Claude'));
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform repository edits keep old session identity and a fresh preview creates in its confirmed override", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr,"session-issue-repository",undefined,github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page,"fixture");
    await showTool(page,"agent_sessions");
    const original = seedFirstWork(herdr,Date.now()).session;
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL,{timeout:45_000});
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator('[data-session-record]').locator('[data-missed-work]',{hasText:MISSED});
    await expect(missed).toBeVisible({timeout:45_000});
    await expect.poll(()=>originalRepository(daemon!.stateDir,original),{timeout:30_000}).toBe("acme/repo");
    await connectRepository(page,"acme/override");
    await workRepositoryReady(daemon.stateDir,"acme/override");
    await missed.locator('[data-missed-issue]').click();
    await expect(missed.getByRole("status")).toContainText("Restore the session's original repository",{timeout:30_000});
    expect(originalRepository(daemon.stateDir,original)).toBe("acme/repo");
    expect(fs.existsSync(github.issue)).toBe(false);
    const fresh = seedFirstWork(herdr,Date.now(),NEXT_GOAL).session;
    await page.locator('[data-session-record]').getByRole("button",{name:"Back to Sessions"}).click();
    await expect(root).toContainText(NEXT_GOAL,{timeout:45_000});
    await expect.poll(()=>originalRepository(daemon!.stateDir,fresh),{timeout:30_000}).toBe("acme/override");
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    await missed.locator('[data-missed-issue]').click();
    const form = page.locator('[data-new-issue]');
    await expect(form).toContainText("acme/override",{timeout:30_000});
    await expect(form.locator('[data-new-issue-title]')).toHaveValue("한국어 요청 줄바꿈 확인");
    await form.locator('[data-new-issue-create]').click();
    await expect(form).toHaveCount(0,{timeout:30_000});
    expect(JSON.parse(fs.readFileSync(github.issue,"utf8")).url).toBe("https://github.com/acme/override/issues/77");
    const effects = fs.readFileSync(github.contexts,"utf8").trim().split("\n").map(line=>JSON.parse(line) as {args:string[];repository:string|null});
    expect(effects.filter(call=>call.args[0]==="issue"&&call.args[1]==="create").map(call=>call.repository)).toEqual(["acme/override"]);
    expect(originalRepository(daemon.stateDir,original)).toBe("acme/repo");
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform a recovered provider cannot authorize a typed create in another session repository", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const last = new Map<string, Record<string, unknown>>();
    countSent(page, last);
    let create: { phase: string; workspace_id: string; message: string | null } | undefined;
    page.on("websocket", socket => socket.on("framereceived", frame => {
      const answer = JSON.parse(String(frame.payload)) as { payload?: { rest?: { issue_work?: { create?: typeof create } } } };
      if (answer.payload?.rest?.issue_work?.create) create = answer.payload.rest.issue_work.create;
    }));
    const github = workFixture(herdr);
    daemon = await startHided(herdr,"session-issue-recovered-provider",undefined,github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page,"fixture");
    await showTool(page,"agent_sessions");
    const original = seedFirstWork(herdr,Date.now()).session;
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL,{timeout:45_000});
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator('[data-session-record]').locator('[data-missed-work]',{hasText:MISSED});
    await expect.poll(()=>originalRepository(daemon!.stateDir,original),{timeout:30_000}).toBe("acme/repo");
    await connectRepository(page,"acme/override");
    await workRepositoryReady(daemon.stateDir,"acme/override");
    fs.writeFileSync(github.repositoryUnavailable,"provider unavailable during preview");
    await missed.locator('[data-missed-issue]').click();
    const form = page.locator('[data-new-issue]');
    await expect(form).toContainText("Couldn't read GitHub",{timeout:30_000});
    fs.rmSync(github.repositoryUnavailable,{force:true});
    const before = fs.readFileSync(github.calls,"utf8").trim().split("\n").length;
    // A second client sends the typed create directly, bypassing only the
    // shell's Retry button; the core must enforce immutable session authority.
    await sendEvent(page,daemon,"issue_create",{
      workspace_id: await form.getAttribute("data-new-issue"),
      title: "Recovered provider must keep the original target", body: "No issue in the override",
      missed: last.get("session_issue_draft"),
    });
    await expect.poll(()=>fs.readFileSync(github.calls,"utf8").trim().split("\n").slice(before)
      .some(line=>{const args=JSON.parse(line) as string[];return args[0]==="repo"&&args[1]==="view";})).toBe(true);
    await expect.poll(()=>create?.phase,{timeout:30_000}).toBe("failed");
    expect(create?.workspace_id).toBe(await form.getAttribute("data-new-issue"));
    expect(create?.message).toBe("The issue could not be confirmed. Retry to check the previous attempt before creating it.");
    expect(fs.existsSync(github.issue)).toBe(false);
    const calls=fs.readFileSync(github.calls,"utf8").trim().split("\n").map(line=>JSON.parse(line) as string[]);
    expect(calls.filter(args=>args[0]==="issue"&&args[1]==="create")).toHaveLength(0);
    expect(originalRepository(daemon.stateDir,original)).toBe("acme/repo");
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform a delayed preview cannot accept an A-to-B-to-A repository edit as unchanged", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr,"session-issue-epoch",undefined,github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page,"fixture");
    await showTool(page,"agent_sessions");
    const {session} = seedFirstWork(herdr,Date.now());
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL,{timeout:45_000});
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator('[data-session-record]').locator('[data-missed-work]',{hasText:MISSED});
    await expect(missed).toBeVisible({timeout:45_000});
    await expect.poll(()=>originalRepository(daemon!.stateDir,session),{timeout:30_000}).toBe("acme/repo");
    const callsBefore = fs.readFileSync(github.calls,"utf8").trim().split("\n").length;
    fs.writeFileSync(github.repositoryGate,"hold the external repository read");
    await missed.locator('[data-missed-issue]').click();
    await expect.poll(()=>fs.readFileSync(github.calls,"utf8").trim().split("\n").slice(callsBefore)
      .some(line=>{const args=JSON.parse(line) as string[];return args[0]==="repo"&&args[1]==="view"&&args[3]==="nameWithOwner";})).toBe(true);
    try {
      await connectRepository(page,"acme/temporary");
      await connectRepository(page,"acme/repo");
    } finally { fs.rmSync(github.repositoryGate,{force:true}); }
    await expect(missed.getByRole("status")).toContainText("Restore the session's original repository",{timeout:30_000});
    await expect(page.locator('[data-new-issue]')).toHaveCount(0);
    expect(fs.existsSync(github.issue)).toBe(false);
    // Completion releases the old worker's slot; a fresh intent can proceed.
    await missed.locator('[data-missed-issue]').click();
    await expect(page.locator('[data-new-issue]')).toContainText("acme/repo",{timeout:30_000});
    await page.locator('[data-new-issue]').getByRole("button",{name:"Cancel",exact:true}).click();
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform migrated session work explains unknown origin and cannot create in today's repository", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr,"session-issue-legacy",undefined,github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page,"fixture");
    await showTool(page,"agent_sessions");
    const identity = seedFirstWork(herdr,Date.now()).session;
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL,{timeout:45_000});
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    await expect(page.locator('[data-session-record]').locator('[data-missed-work]',{hasText:MISSED})).toBeVisible({timeout:45_000});
    // This migration starts from completed, durable native work. Windows
    // fixture shutdown is forced; a rendered goal is not a saved label.
    const savedLabels = path.join(daemon.stateDir,"labels.json");
    const node = daemon.node;
    await expect.poll(() => {
      if (!fs.existsSync(savedLabels)) return null;
      const saved = JSON.parse(fs.readFileSync(savedLabels,"utf8")) as {
        targets?: Record<string,Record<string,{native_session_id?:string;goal?:string;end?:string}>>;
      };
      const label = saved.targets?.[node]?.[herdr.panes[0]];
      return {session:label?.native_session_id,goal:label?.goal,end:label?.end};
    }, {message:"completed native label reached disk before migration restart",timeout:30_000})
      .toEqual({session:identity,goal:FIRST_GOAL,end:"done"});
    // Emulate the actual prior schema while this owned daemon is stopped.
    daemon = await daemon.restart((stateDir)=>{
      const db = new DatabaseSync(path.join(stateDir,"links.sqlite3"));
      try { db.exec("DROP TABLE ledger_session_origins; DROP TABLE ledger_project_connections; PRAGMA user_version=2;"); }
      finally { db.close(); }
    });
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page,"fixture");
    await showTool(page,"agent_sessions");
    const back = page.getByRole("button",{name:"Back to Sessions",exact:true});
    if (await back.isVisible()) await back.click();
    await expect(root).toBeVisible();
    await expect(root).toContainText(FIRST_GOAL,{timeout:45_000});
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator('[data-session-record]').locator('[data-missed-work]',{hasText:MISSED});
    await missed.locator('[data-missed-issue]').click();
    await expect(missed.getByRole("status")).toContainText("original repository is unknown",{timeout:30_000});
    expect(originalRepository(daemon.stateDir,identity)).toBeNull();
    expect(fs.existsSync(github.issue)).toBe(false);
    await expect(page.locator('[data-new-issue]')).toHaveCount(0);
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform cancel leaves missed work open and retry reconciles an issue whose create response was lost", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr, true);
    daemon = await startHided(herdr, "session-issue-retry", undefined, github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page, "fixture");
    await showTool(page, "agent_sessions");
    const first = Date.now();
    github.clock(first, first);
    const {session} = seedFirstWork(herdr, first);
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL, { timeout: 45_000 });
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator("[data-missed-work]", { hasText: MISSED });
    await expect.poll(()=>originalRepository(daemon!.stateDir,session),{timeout:30_000}).toBe("acme/repo");
    await missed.locator('[data-missed-issue]').click({ timeout: 45_000 });
    const form = page.locator('[data-new-issue]');
    await expect(form.locator('[data-new-issue-title]')).toHaveValue("한국어 요청 줄바꿈 확인", { timeout: 30_000 });
    await expect(form.locator('[data-new-issue-body]')).toContainText("## 확인할 동작");
    await form.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(missed).toHaveAttribute("data-missed-status", "open");
    expect(fs.existsSync(github.issue)).toBe(false);
    await missed.locator('[data-missed-issue]').click();
    await form.locator('[data-new-issue-create]').click();
    await expect(form.locator('[data-new-issue-error]')).toBeVisible({ timeout: 30_000 });
    const created = JSON.parse(fs.readFileSync(github.issue, "utf8")) as { title: string };
    expect(created.title).toBe("한국어 요청 줄바꿈 확인");
    await form.locator('[data-new-issue-create]').click();
    await expect(form).toHaveCount(0, { timeout: 30_000 });
    await page.getByText("Closed 1", { exact: true }).click();
    await expect(missed).toHaveAttribute("data-missed-status", "issue");
    await expect(missed.getByRole("button", { name: "#77", exact: true })).toBeVisible();
    const calls = fs.readFileSync(github.calls, "utf8").trim().split("\n").map((line) => JSON.parse(line) as string[]);
    expect(calls.filter((args) => args[0] === "issue" && args[1] === "create")).toHaveLength(1);
    expect(calls.some((args) => args.join(" ").includes("HideSessionIssueAttempts"))).toBe(true);
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform GitHub connection retry preserves the edited preview and creates only after confirmation", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr, "session-issue-connection", undefined, github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await workRepositoryReady(daemon.stateDir);
    await enterWorkspace(page, "fixture");
    await showTool(page, "agent_sessions");
    const {session} = seedFirstWork(herdr, Date.now());
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL, { timeout: 45_000 });
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator('[data-missed-work]', { hasText: MISSED });
    await expect.poll(()=>originalRepository(daemon!.stateDir,session),{timeout:30_000}).toBe("acme/repo");
    fs.writeFileSync(github.repositoryUnavailable, "unavailable");
    await missed.locator('[data-missed-issue]').click({ timeout: 45_000 });
    const form = page.locator('[data-new-issue]');
    await expect(form.getByText("Couldn't read GitHub. Retry to check the connection; your edits stay here.")).toBeVisible({ timeout: 30_000 });
    await form.locator('[data-new-issue-title]').fill("edited title");
    await form.locator('[data-new-issue-body]').fill("edited body");
    fs.unlinkSync(github.repositoryUnavailable);
    await form.getByRole("button", { name: /Retry/ }).click();
    await expect(form.getByRole("button", { name: /Create issue/ })).toBeEnabled({ timeout: 30_000 });
    await expect(form.locator('[data-new-issue-title]')).toHaveValue("edited title");
    await expect(form.locator('[data-new-issue-body]')).toHaveValue("edited body");
    expect(fs.existsSync(github.issue)).toBe(false);
    await form.locator('[data-new-issue-create]').click();
    await expect(form).toHaveCount(0, { timeout: 30_000 });
    const calls = fs.readFileSync(github.calls, "utf8").trim().split("\n").map((line) => JSON.parse(line) as string[]);
    const creates = calls.filter((args) => args[0] === "issue" && args[1] === "create");
    expect(creates).toHaveLength(1);
    expect(creates[0]).toEqual(expect.arrayContaining(["edited title", "edited body"]));
  } finally { daemon?.stop(); herdr.stop(); }
});
