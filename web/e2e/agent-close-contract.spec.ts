// Pinned Herdr behavior, observed on a repository and worktree owned by this fixture.
import { expect, test } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";

test("primary last tab and last pane close retain the linked workspace boundary", async () => {
  const herdr = await startHerdr({ agents: false });
  try {
    const repo = path.join(herdr.root, "primary");
    const linked = path.join(herdr.root, "linked");
    fs.mkdirSync(repo);
    const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { env: herdr.env, encoding: "utf8", stdio: "pipe" });
    git("init", "-b", "main");
    git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture");
    git("worktree", "add", "-b", "linked", linked);
    type Created = { result: { workspace: { workspace_id: string }; tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const primary = herdr.run(["worktree", "open", "--cwd", repo, "--path", repo, "--no-focus"]) as Created;
    const sibling = herdr.run(["worktree", "open", "--cwd", repo, "--path", linked, "--no-focus"]) as Created;
    const close = (args: string[]) => {
      const result = spawnSync(herdr.bin, args, { env: herdr.env, encoding: "utf8", timeout: 10_000 });
      return { exit: result.status, stdout: result.stdout, stderr: result.stderr };
    };
    const before = herdr.run(["api", "snapshot"]);
    const tabClose = close(["tab", "close", primary.result.tab.tab_id]);
    expect(tabClose.exit).toBe(1);
    expect(JSON.parse(tabClose.stderr).error.code).toBe("confirmation_required");
    const afterTab = herdr.run(["api", "snapshot"]);
    const secondPrimary = herdr.run(["worktree", "open", "--cwd", repo, "--path", repo, "--no-focus"]) as Created;
    const beforePane = herdr.run(["api", "snapshot"]);
    const paneClose = close(["pane", "close", secondPrimary.result.root_pane.pane_id]);
    expect(paneClose.exit).toBe(1);
    expect(JSON.parse(paneClose.stderr).error.code).toBe("confirmation_required");
    const afterPane = herdr.run(["api", "snapshot"]);
    const record = { version: execFileSync(herdr.bin, ["--version"], { encoding: "utf8" }).trim(), primary: primary.result, linked: sibling.result, before, tabClose, afterTab, beforePane, paneClose, afterPane };
    const output = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (output) fs.writeFileSync(path.join(output, "primary-close-contract.json"), JSON.stringify(record, null, 2));
    console.log(JSON.stringify({ tabClose, paneClose }));
    expect(JSON.stringify(afterTab)).toContain(sibling.result.workspace.workspace_id);
    expect(JSON.stringify(afterPane)).toContain(sibling.result.workspace.workspace_id);
  } finally { herdr.stop(); }
});
