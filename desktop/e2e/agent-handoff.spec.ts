// A native caller hands off roots through the real pinned Herdr and hided.
import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { herdrHasFocus, startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { openCurrentProjectOverview } from "../../web/e2e/overview-entry";
import { HIDE_CLI, isolate, launchShell, screenshot, test } from "./fixture";
import { captureNativeWindow } from "./native-window";
import { installSpawnProvider, nativeSpawnCommand, waitForSpawnShell, type SpawnedAgent } from "./agent-spawn-fixture";

type NativeAgent = { pane_id: string; terminal_id: string; agent_session?: { value: string }; tokens?: Record<string, unknown> };

test("handoff roots have provenance without delegation or a focus change in either checkout path", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "agent-handoff");
  const response = path.join(run.root, "response.json");
  const repo = path.join(herdr.root, "fixture");
  const parent = herdr.panes[0];
  let app: ElectronApplication | undefined;
  const failures: unknown[] = [];
  try {
    execFileSync("git", ["init", "-b", "main", repo], { env: run.env, timeout: 20_000 });
    execFileSync("git", ["-C", repo, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", "commit", "--allow-empty", "-m", "Fixture"], { env: run.env, timeout: 20_000 });
    const existingCheckout = path.join(herdr.root, "existing-checkout");
    execFileSync("git", ["-C", repo, "worktree", "add", "-b", "existing-task", existingCheckout], { env: run.env, timeout: 20_000 });
    execFileSync(herdr.bin, ["worktree", "open", "--cwd", repo, "--path", existingCheckout, "--no-focus"], { env: herdr.env, timeout: 20_000 });
    const provider = installSpawnProvider(herdr, run.root);
    execFileSync(herdr.bin, ["workspace", "report-metadata", herdr.workspace, "--source", "hide", "--token", "purpose=Spawn handoff fixture"], { env: herdr.env, timeout: 20_000 });
    await waitForSpawnShell(herdr, parent);
    execFileSync(herdr.bin, ["agent", "start", "handoff-caller", "--kind", "claude", "--pane", parent, "--timeout", "5000"], { env: herdr.env, timeout: 20_000 });
    const agents = () => (herdr.run(["api", "snapshot"]) as { result: { snapshot: { agents: NativeAgent[] } } }).result.snapshot.agents;
    await expect.poll(() => agents().find((agent) => agent.pane_id === parent)?.agent_session?.value).toMatch(/^fixture-\d+$/);
    const native = agents().find((agent) => agent.pane_id === parent)!;
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    const command = (args: string[]) => nativeSpawnCommand(herdr, provider, parent, HIDE_CLI, args, run.env, response);
    const registered = await command(["agent", "register", "--host-scope", herdr.socket, "--session", native.agent_session!.value, "--instance", native.terminal_id, "--name", "handoff-caller", "--pane", parent]);
    expect(registered.origin).toBeNull();
    await enterWorkspace(page, "fixture");
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
    const keyboard = page.locator(`[data-pane-view="${parent}"] .xterm-helper-textarea`);
    await keyboard.focus();
    await expect.poll(() => herdrHasFocus(herdr, parent)).toBe(true);
    const roots: SpawnedAgent[] = [];
    for (const [name, branch] of [["handoff-existing", "existing-task"], ["handoff-new", "handoff-task"]]) {
      const root = await command(["agent", "spawn", "--name", name!, "--intent", name!, "--kind", "claude", "--repo", repo, "--branch", branch!]);
      expect(root.parent).toBeNull();
      expect(root.origin).toBe(registered.id);
      expect(root.watch).toBeNull();
      expect(root.pane).not.toBe(parent);
      expect(root.pane.split(":")[0]).not.toBe(parent.split(":")[0]);
      expect(agents().find((agent) => agent.pane_id === root.pane)?.tokens?.parent_pane).toBeUndefined();
      await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
      await expect(keyboard).toBeFocused();
      expect(herdrHasFocus(herdr, parent)).toBe(true);
      const shown = await command(["agent", "show", root.id]);
      expect(shown).toMatchObject({ parent: null, origin: registered.id, watch: null });
      const child = agents().find((agent) => agent.pane_id === root.pane)!;
      for (const check of [[], ["--check"]]) {
        const reply = await nativeSpawnCommand(herdr, provider, root.pane, HIDE_CLI, ["agent", "register", "--host-scope", herdr.socket, "--session", child.agent_session!.value, "--instance", child.terminal_id, "--name", name!, "--pane", root.pane, ...check], run.env, response);
        expect(reply).toMatchObject({ id: root.id, parent: null, origin: registered.id });
      }
      roots.push(root);
    }
    await page.locator('[data-sidebar-mode="agents"]').click();
    const parentRow = page.locator(`[data-agent-list] [data-pane="${parent}"]`);
    await expect(parentRow).toHaveAttribute("data-waiting", "false");
    await expect(parentRow.locator("[data-agent-tree-toggle], [data-agent-children]")).toHaveCount(0);
    for (const root of roots) {
      const row = page.locator(`[data-agent-list] [data-pane="${root.pane}"]`);
      await expect(row).toHaveAttribute("data-delegated", "false");
      await expect(row).toHaveAttribute("data-depth", "0");
    }
    await expect(page.locator(`[data-pane-children="${parent}"]`)).toHaveCount(0);
    const facts = { parent, roots, state: run.env.HIDE_STATE_DIR, home: run.env.HOME, socket: herdr.socket, head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: path.resolve(__dirname, "../.."), encoding: "utf8" }).trim(), provider: "synthetic native CLI" };
    await screenshot(page, "agent-handoff-roots");
    await captureNativeWindow(app, "agent-handoff-roots-native", facts).catch((error) => failures.push(error));
    await openCurrentProjectOverview(page, "fixture");
    await page.locator('[data-lens-tile-button="agents"]').click();
    for (const status of ["turn", "working", "resting"]) await page.locator(`[data-graph-chip="${status}"]`).click();
    for (const root of roots) await expect(page.locator(`[data-graph-row="${root.pane}"]`)).toHaveAttribute("data-depth", "0");
    await expect(page.locator(`[data-graph-row="${parent}"]`)).toHaveAttribute("data-depth", "0");
    await expect(page.locator("[data-graph-edge]")).toHaveCount(0);
    await screenshot(page, "agent-handoff-graph");
    await captureNativeWindow(app, "agent-handoff-graph-native", facts).catch((error) => failures.push(error));
  } catch (error) {
    failures.push(error);
  } finally {
    try { await app?.close(); } catch (error) { failures.push(error); }
    try { run.cleanup(); } catch (error) { failures.push(error); }
    try { herdr.stop(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "native handoff fixture or teardown failed");
});
