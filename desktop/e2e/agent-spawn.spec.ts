// Actual spawn, lineage publication and the native delegation tree. Only the
// provider is synthetic; its session report comes from its real pane process.
import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { herdrHasFocus, startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { HIDE_CLI, isolate, launchShell, nodeOf, screenshot, test } from "./fixture";
import { captureNativeWindow } from "./native-window";
import { installSpawnProvider, nativeSpawnCommand, waitForSpawnShell, type SpawnedAgent } from "./agent-spawn-fixture";

type NativeAgent = { pane_id: string; terminal_id: string; agent_session?: { value: string }; tokens: Record<string, unknown> };

test("a spawned child appears in the native delegation tree", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "agent-spawn");
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
    execFileSync(herdr.bin, ["workspace", "report-metadata", herdr.workspace, "--source", "hide", "--token", "purpose=Spawn delegation fixture"], { env: herdr.env, timeout: 20_000 });
    // The fixture wrapper declares a controlled session after agent.start;
    // this case observes the synthetic executable's own native report instead.
    await waitForSpawnShell(herdr, parent);
    execFileSync(herdr.bin, ["agent", "start", "parent", "--kind", "claude", "--pane", parent, "--timeout", "5000"], { env: herdr.env, timeout: 20_000 });
    const agents = () => (herdr.run(["api", "snapshot"]) as { result: { snapshot: { agents: NativeAgent[] } } }).result.snapshot.agents;
    await expect.poll(() => agents().find((agent) => agent.pane_id === parent)?.agent_session?.value).toMatch(/^fixture-\d+$/);
    const native = agents().find((agent) => agent.pane_id === parent)!;
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    const command = (args: string[]) => nativeSpawnCommand(herdr, provider, parent, HIDE_CLI, args, run.env, response);
    const registered = await command(["agent", "register", "--host-scope", herdr.socket, "--session", native.agent_session!.value, "--instance", native.terminal_id, "--name", "parent", "--pane", parent]);
    expect(registered.machine).toBe(nodeOf(run.env));
    await enterWorkspace(page, "fixture");
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
    await expect.poll(() => herdrHasFocus(herdr, parent)).toBe(true);
    const keyboard = page.locator(`[data-pane-view="${parent}"] .xterm-helper-textarea`);
    await keyboard.focus();
    const children: SpawnedAgent[] = [];
    for (const [name, branch] of [["child-existing", "existing-task"], ["child", "child-task"]]) {
      const spawned = await command(["agent", "spawn", "--parent", "here", "--name", name!, "--intent", name!, "--kind", "claude", "--repo", repo, "--branch", branch!]);
      expect(spawned.parent).toBe(registered.id);
      expect(spawned.origin).toBe(registered.id);
      expect(spawned.pane.split(":")[0]).not.toBe(parent.split(":")[0]);
      await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
      await expect(keyboard).toBeFocused();
      expect(herdrHasFocus(herdr, parent)).toBe(true);
      expect(spawned.watch).toBeTruthy();
      expect(agents().find((agent) => agent.pane_id === spawned.pane)?.tokens.parent_pane).toBe(parent);
      children.push(spawned);
    }
    const child = children[1]!;
    await page.locator('[data-sidebar-mode="agents"]').click();
    const parentRow = page.locator(`[data-agent-list] [data-pane="${parent}"]`);
    await expect(parentRow).toHaveAttribute("data-delegated", "false");
    const toggle = parentRow.locator(`[data-agent-tree-toggle="${parent}"]`);
    await expect(toggle).toBeVisible();
    if (await toggle.getAttribute("aria-expanded") === "false") await toggle.click();
    const childRow = page.locator(`[data-agent-list] [data-pane="${child.pane}"]`);
    await expect(childRow).toHaveAttribute("data-delegated", "true");
    await expect(childRow).toHaveAttribute("data-depth", "1");
    await expect(childRow.locator('[data-branch-chip="child-task"]')).toBeVisible();
    await parentRow.locator(`[data-agent-open="${parent}"]`).click();
    await expect(page.locator(`[data-pane-children="${parent}"] [data-child-chip="${child.pane}"]`)).toBeVisible();
    await screenshot(page, "agent-spawn-delegation");
    await captureNativeWindow(app, "agent-spawn-delegation-native", { parent, children, state: run.env.HIDE_STATE_DIR, home: run.env.HOME, socket: herdr.socket, head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: path.resolve(__dirname, "../.."), encoding: "utf8" }).trim(), provider: "synthetic native CLI" });
  } catch (error) {
    failures.push(error);
  } finally {
    try { await app?.close(); } catch (error) { failures.push(error); }
    try { run.cleanup(); } catch (error) { failures.push(error); }
    try { herdr.stop(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "native spawn fixture or teardown failed");
});
