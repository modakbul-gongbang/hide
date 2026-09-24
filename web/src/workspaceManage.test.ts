import { describe, expect, it } from "vitest";
import type { Checkout, TaskOperation, Workspace } from "./snapshot";
import { branchProblem, checkoutMenu, normalizePurpose, projectMenu, purposeCountLabel, purposeIsLong, removalFor, scalarCount, taskFor } from "./workspaceManage";

const workspace = (patch: Partial<Workspace> = {}): Workspace => ({
  id: "w1",
  label: "hide",
  path: "/Users/example/hide",
  device_id: "local",
  remote_target_id: null,
  is_git: true,
  registered: true,
  temporary: false,
  pinned: false,
  checkouts: [],
  inactive_checkouts: { expanded: false, checkout_ids: [] },
  ...patch,
});

const checkout = (patch: Partial<Checkout> = {}): Checkout => ({
  id: "c1",
  workspace_id: "w1",
  label: "feature",
  path: "/Users/example/hide.worktrees/feature",
  branch: "feature",
  purpose: null,
  is_worktree: true,
  exists: true,
  has_panes: false,
  worktree: {
    path: "/Users/example/hide.worktrees/feature",
    branch: "feature",
    head_sha: "abc",
    is_main: false,
    missing: false,
    dirty: false,
    changed_file_count: 0,
    pane_count: 0,
    running_agent_count: 0,
    deletion_gate: { blocked_reason: null, warnings: [], button_label: "Delete worktree", can_delete_branch: true },
  },
  pull_request: null,
  tabs: [],
  active_tab_id: null,
  strip: [],
  next_tab_label: "1",
  ...patch,
});

const task = (patch: Partial<TaskOperation>): TaskOperation => ({
  id: 5,
  kind: "worktree_create",
  phase: "working",
  repository_root: "/Users/example/hide",
  branch: "feature",
  base_branch: null,
  path: null,
  pane_id: null,
  agent_kind: null,
  message: null,
  agent_phase: null,
  agent_message: null,
  ...patch,
});

describe("purpose field", () => {
  it("counts and cuts Unicode scalars the way the core does, keeping one line", () => {
    const korean = "한".repeat(90);
    expect(scalarCount(normalizePurpose(korean))).toBe(80);
    expect(normalizePurpose("a\nb\rc")).toBe("a b c");
    expect(scalarCount("🙂🙂")).toBe(2);
    expect(purposeCountLabel("한글 목적")).toBe("5 / 40");
    expect(purposeIsLong("x".repeat(41))).toBe(true);
    expect(purposeIsLong("x".repeat(40))).toBe(false);
  });
});

describe("branch names", () => {
  it("refuses what Git would refuse before a request goes out", () => {
    expect(branchProblem("feature/s5")).toBeNull();
    for (const bad of ["", "-x", "a b", "a..b", "a~1", "x.lock", "/a", "a/", "a\u0001"]) expect(branchProblem(bad), bad).not.toBeNull();
  });
});

describe("row menus", () => {
  it("offers pin and new worktree only for local projects, and says why otherwise", () => {
    expect(projectMenu(workspace()).map((item) => [item.id, item.unavailable])).toEqual([
      ["pin", null],
      ["new_worktree", null],
    ]);
    expect(projectMenu(workspace({ pinned: true }))[0]?.id).toBe("unpin");
    const remote = projectMenu(workspace({ device_id: "studio", remote_target_id: "studio" }));
    expect(remote.every((item) => item.unavailable !== null)).toBe(true);
    expect(projectMenu(workspace({ registered: false })).map((item) => item.id)).toEqual(["new_worktree"]);
    expect(projectMenu(workspace({ is_git: false }))[1]?.unavailable).toMatch(/not a Git/);
  });

  it("offers deletion only for a local linked worktree whose gate allows it", () => {
    expect(checkoutMenu(workspace(), checkout()).map((item) => item.id)).toEqual(["set_purpose", "delete_worktree"]);
    expect(checkoutMenu(workspace(), checkout({ is_worktree: false })).map((item) => item.id)).toEqual(["set_purpose"]);
    const blocked = checkout();
    if (blocked.worktree) blocked.worktree.deletion_gate.blocked_reason = "The main worktree cannot be deleted";
    expect(checkoutMenu(workspace(), blocked)[1]?.unavailable).toBe("The main worktree cannot be deleted");
    expect(checkoutMenu(workspace({ remote_target_id: "studio", device_id: "studio" }), checkout())[1]?.unavailable).not.toBeNull();
  });
});

describe("receipts", () => {
  it("reads only the task this page asked for", () => {
    const request = { kind: "worktree_create", afterId: 4, repositoryRoot: "/Users/example/hide", branch: "feature" };
    expect(taskFor(task({}), request)?.id).toBe(5);
    expect(taskFor(task({ id: 4 }), request)).toBeNull();
    expect(taskFor(task({ branch: "other" }), request)).toBeNull();
    expect(taskFor(task({ kind: "checkout_purpose" }), request)).toBeNull();
    expect(taskFor(task({}), null)).toBeNull();
  });

  it("reads only the removal of the checkout this page asked to delete", () => {
    const removal = { id: 3, repository_root: "/r", checkout_path: "/r-feature", branch: "feature", delete_branch: false, phase: "removing", message: null };
    expect(removalFor(removal, "/r-feature", 2)?.id).toBe(3);
    expect(removalFor(removal, "/r-other", 2)).toBeNull();
    expect(removalFor(removal, "/r-feature", 3)).toBeNull();
  });
});
