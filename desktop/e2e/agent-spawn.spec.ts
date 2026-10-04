// Actual spawn, lineage publication and the native delegation tree. Only the
// provider is synthetic; its session report comes from its real pane process.
import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { HIDE_CLI, isolate, launchShell, screenshot, test } from "./fixture";
import { captureNativeWindow } from "./native-window";

type NativeAgent = { pane_id: string; terminal_id: string; agent_session?: { value: string }; tokens: Record<string, unknown> };
type Reply = { ok: boolean; value: { id: string; pane: string; parent?: string; watch?: string; session?: string } };
const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;

test("a spawned child appears in the native delegation tree", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "agent-spawn");
  const script = path.join(run.root, "command.sh");
  const response = path.join(run.root, "response.json");
  const repo = path.join(herdr.root, "fixture");
  const source = path.join(run.root, "agent.c");
  const parent = herdr.panes[0];
  let app: ElectronApplication | undefined;
  try {
    execFileSync("git", ["init", "-b", "main", repo]);
    execFileSync("git", ["-C", repo, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", "commit", "--allow-empty", "-m", "Fixture"]);
    sourceCode(script, source, herdr.bin);
    execFileSync("cc", [source, "-o", path.join(herdr.root, "bin", "claude")]);
    execFileSync(herdr.bin, ["workspace", "report-metadata", herdr.workspace, "--source", "hide", "--token", "purpose=Spawn delegation fixture"], { env: herdr.env });
    // The fixture wrapper declares a controlled session after agent.start;
    // this case observes the synthetic executable's own native report instead.
    execFileSync(herdr.bin, ["agent", "start", "parent", "--kind", "claude", "--pane", parent, "--timeout", "5000"], { env: herdr.env });
    const agents = () => (herdr.run(["api", "snapshot"]) as { result: { snapshot: { agents: NativeAgent[] } } }).result.snapshot.agents;
    await expect.poll(() => agents().find((agent) => agent.pane_id === parent)?.agent_session?.value).toMatch(/^fixture-\d+$/);
    const native = agents().find((agent) => agent.pane_id === parent)!;
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    const command = async (args: string[]) => {
      fs.rmSync(response, { force: true });
      const argv = ["env", `HIDE_STATE_DIR=${run.env.HIDE_STATE_DIR}`, `HOME=${run.env.HOME}`, `HERDR_BIN_PATH=${herdr.bin}`, HIDE_CLI, ...args];
      fs.writeFileSync(script, `#!/bin/sh\n${argv.map(quote).join(" ")} > ${quote(response)}\n`, { mode: 0o700 });
      execFileSync(herdr.bin, ["pane", "send-text", parent, "!"], { env: herdr.env });
      await expect.poll(() => {
        try { return JSON.parse(fs.readFileSync(response, "utf8")) as Reply; } catch { return null; }
      }).not.toBeNull();
      const answer = JSON.parse(fs.readFileSync(response, "utf8")) as Reply;
      expect(answer.ok, JSON.stringify(answer)).toBe(true);
      return answer.value;
    };
    const registered = await command(["agent", "register", "--machine", "local", "--host-scope", herdr.socket, "--session", native.agent_session!.value, "--instance", native.terminal_id, "--name", "parent", "--pane", parent]);
    const child = await command(["agent", "spawn", "--parent", "here", "--name", "child", "--intent", "native-delegation", "--kind", "claude", "--repo", repo, "--branch", "child-task"]);
    expect(child.parent).toBe(registered.id);
    expect(child.watch).toBeTruthy();
    expect(agents().find((agent) => agent.pane_id === child.pane)?.tokens.parent_pane).toBe(parent);
    await enterWorkspace(page, "fixture");
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
    await captureNativeWindow(app, "agent-spawn-delegation-native", { parent, child, state: run.env.HIDE_STATE_DIR, home: run.env.HOME, socket: herdr.socket, provider: "synthetic native CLI" });
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});

function sourceCode(script: string, file: string, herdr: string): void {
  fs.writeFileSync(file, `#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <termios.h>
#include <sys/wait.h>
int main(int argc, char **argv) {
  if (argc > 1 && strcmp(argv[1], "--version") == 0) { puts("fixture"); return 0; }
  if (argc > 1 && strcmp(argv[1], "auth") == 0) { puts("{\\"loggedIn\\":true}"); return 0; }
  for (int i = 1; i < argc; i++) if (strcmp(argv[i], "--json-schema") == 0) return 1;
  char session[80]; snprintf(session, sizeof session, "fixture-%d", getpid());
  pid_t child = fork();
  if (child == 0) {
    execl(${JSON.stringify(herdr)}, "herdr", "pane", "report-agent-session", getenv("HERDR_PANE_ID"), "--source", "herdr:claude", "--agent", "claude", "--agent-session-id", session, "--seq", "1", NULL);
    _exit(1);
  }
  int status; waitpid(child, &status, 0);
  struct termios tio;
  if (tcgetattr(0, &tio) == 0) { tio.c_lflag &= ~(ICANON | ECHO | IEXTEN); tio.c_cc[VMIN] = 1; tio.c_cc[VTIME] = 0; tcsetattr(0, TCSANOW, &tio); }
  printf("\\nClaude Code fixture\\n❯ "); fflush(stdout);
  char b;
  while (read(0, &b, 1) > 0) {
    if (b == '!') { system(${JSON.stringify(quote(script))}); printf("\\n❯ "); fflush(stdout); }
  }
  return 0;
}
`);
}
