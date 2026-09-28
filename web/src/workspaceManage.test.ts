import { describe, expect, it } from "vitest";
import type { Checkout, TaskOperation, Workspace } from "./snapshot";
import { agentMenu, branchProblem, checkoutMenu, deletionConsequences, folderMenu, normalizePurpose, projectMenu, projectRemovalConsequences, purposeCountLabel, purposeIsLong, purposeScope, removalFor, scalarCount, taskFor } from "./workspaceManage";

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
    deletion_gate: { blocked_reason: null, warnings: [], button_label: "Delete worktree", can_delete_branch: true, branch_warning: null, discard_label: null },
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
  const desktop = { finder: true, newTabChord: "⌘T" };
  const browser = { finder: false, newTabChord: "⌥T" };
  /** Each item as the menu draws it: a separator line before it, its label, and its reason when disabled. */
  const drawn = (items: { label: string; separated?: boolean; unavailable: string | null; shortcut?: string }[]) =>
    items.flatMap((item) => [...(item.separated ? ["─"] : []), item.shortcut ? `${item.label} ${item.shortcut}` : item.label]);
  const primary = (patch: Partial<Checkout> = {}) => checkout({ id: "c0", label: "main", branch: "main", path: "/Users/example/hide", is_worktree: false, is_primary: true, ...patch });

  it("draws a project's menu in the board's order, Finder only on the desktop (B1)", () => {
    const project = workspace({ checkouts: [primary(), checkout()] });
    expect(drawn(projectMenu(project, desktop))).toEqual([
      "Open Overview",
      "New worktree…",
      "New tab in main ⌘T",
      "─",
      "Reveal in Finder",
      "Copy path",
      "─",
      "Pin",
      "Remove project…",
    ]);
    expect(drawn(projectMenu(project, browser))).toEqual(["Open Overview", "New worktree…", "New tab in main ⌥T", "─", "Copy path", "─", "Pin", "Remove project…"]);
    expect(projectMenu(workspace({ pinned: true, checkouts: [primary()] }), desktop).find((item) => item.id === "unpin")?.label).toBe("Unpin");
    expect(projectMenu(workspace({ is_git: false, checkouts: [primary({ is_primary: false })] }), desktop).find((item) => item.id === "new_worktree")?.unavailable).toMatch(/not a Git/);
  });

  it("offers Pin and Remove on a row Herdr shows without a registration (B3, D-14)", () => {
    const unregistered = projectMenu(workspace({ registered: false, checkouts: [primary()] }), desktop);
    expect(unregistered.filter((item) => item.id === "pin" || item.id === "remove_project").map((item) => [item.label, item.unavailable])).toEqual([
      ["Pin", null],
      ["Remove project…", null],
    ]);
  });

  it("opens the new tab in the checkout the home glyph marks", () => {
    const menu = (project: Workspace) => projectMenu(project, desktop).find((item) => item.id === "new_tab_primary")!;
    expect(menu(workspace({ checkouts: [checkout(), primary()] })).unavailable).toBeNull();
    expect(menu(workspace({ checkouts: [checkout()] })).unavailable).toMatch(/no default checkout/);
    expect(menu(workspace({ checkouts: [primary({ exists: false })] })).unavailable).toMatch(/missing/);
  });

  it("draws a checkout's menu in the board's order with the pull request after New tab (B4)", () => {
    const pr = { number: 180, title: "Sidebar readability", url: "https://example.invalid/pull/180", badge: "open" as const, review: null, is_draft: false };
    expect(drawn(checkoutMenu(workspace(), checkout({ pull_request: pr }), desktop))).toEqual([
      "Open",
      "New tab here ⌘T",
      "Open pull request #180",
      "─",
      "Set purpose…",
      "Set as default checkout",
      "Copy branch name",
      "Copy path",
      "Reveal in Finder",
      "─",
      "Delete worktree…",
    ]);
    // No pull request, no Finder, and a checkout that is not a linked worktree.
    expect(drawn(checkoutMenu(workspace(), primary({ is_primary: false }), browser))).toEqual([
      "Open",
      "New tab here ⌥T",
      "─",
      "Set purpose…",
      "Set as default checkout",
      "Copy branch name",
      "Copy path",
    ]);
    expect(checkoutMenu(workspace(), checkout(), desktop).find((item) => item.id === "delete_worktree")?.destructive).toBe(true);
    // A pull request GitHub cannot vouch for now is not offered (checkout-pr-glyph-card B10).
    const unavailable = { failure_category: "auth", available: false, loading: false, stale: false, last_success_at_unix_ms: null, unavailable_reason: "gh is not signed in" };
    expect(checkoutMenu(workspace(), checkout({ pull_request: pr, github: unavailable }), desktop).some((item) => item.id === "open_pull_request")).toBe(false);
  });

  it("says why a checkout cannot become the default (B6, D-09)", () => {
    const reason = (project: Workspace, row: Checkout) => checkoutMenu(project, row, desktop).find((item) => item.id === "set_primary")!.unavailable;
    expect(reason(workspace(), checkout())).toBeNull();
    expect(reason(workspace(), primary())).toBe("Already the default checkout.");
    expect(reason(workspace({ is_git: false }), checkout())).toMatch(/plain folder/);
    expect(reason(workspace({ device_id: "studio", remote_target_id: "studio" }), checkout())).toMatch(/another device/);
    expect(reason(workspace({ registered: false }), checkout())).toMatch(/Pin the project first/);
    expect(reason(workspace(), checkout({ exists: false }))).toMatch(/missing/);
  });

  it("greys out Finder and the default on a device's checkout and keeps the rest, deletion included (B9)", () => {
    const remote = checkoutMenu(workspace({ device_id: "studio", remote_target_id: "studio" }), checkout(), desktop);
    expect(remote.filter((item) => item.unavailable !== null).map((item) => [item.id, item.unavailable])).toEqual([
      ["set_primary", "Not available for a checkout on another device."],
      ["reveal_finder", "Only for folders on this Mac."],
    ]);
    const project = projectMenu(workspace({ device_id: "studio", remote_target_id: "studio", checkouts: [primary()] }), desktop);
    expect(project.filter((item) => item.unavailable !== null).map((item) => item.id)).toEqual(["reveal_finder"]);
  });

  it("never disables Delete worktree, dirty or unread, and keeps a detached checkout's missing branch", () => {
    const dirty = checkout();
    if (dirty.worktree) {
      dirty.worktree.dirty = true;
      dirty.worktree.deletion_gate.discard_label = "Discard 3 changed files";
    }
    const deletion = (row: Checkout) => checkoutMenu(workspace(), row, desktop).find((item) => item.id === "delete_worktree");
    expect(deletion(dirty)?.unavailable).toBeNull();
    expect(deletion(checkout({ worktree: null }))?.unavailable).toBeNull();
    expect(deletion(checkout({ is_worktree: false }))).toBeUndefined();
    expect(checkoutMenu(workspace(), checkout({ branch: null }), desktop).find((item) => item.id === "copy_branch")?.unavailable).toMatch(/Detached/);
  });

  it("names the agents a deletion stops once, by name and state, beside the gate's warnings", () => {
    const pane = (id: string, identity: string | null, status: string) => ({
      id,
      herdr_label: null,
      terminal_title: null,
      workspace_label: null,
      cwd: "/Users/example/hide.worktrees/feature",
      status_label: status,
      requires_close_confirmation: false,
      requires_close_status_check: false,
      identity_label: identity,
    });
    const row = checkout({
      tabs: [
        { id: "t1", workspace_id: "w1", checkout_id: "c1", label: "1", empty: false, delegated: false, panes: [pane("p1", "Fix the parser", "Working"), pane("p2", null, "")] },
        { id: "t2", workspace_id: "w1", checkout_id: "c1", label: "2", empty: false, delegated: false, panes: [pane("p3", "Review tests", "Idle")] },
      ],
    });
    if (row.worktree) {
      row.worktree.running_agent_count = 1;
      row.worktree.deletion_gate.warnings = ["3 changed files not committed", "ahead 2 unmerged"];
    }
    expect(deletionConsequences(row, 3)).toEqual([
      "The folder /Users/example/hide.worktrees/feature is removed from disk. This cannot be undone.",
      "3 panes in this worktree close first, stopping whatever runs there.",
      "Stops 2 agents: Fix the parser (Working), Review tests (Idle).",
      "3 changed files not committed",
      "ahead 2 unmerged",
    ]);
  });

  it("puts a folder's own checkout items after its project items, past a separator", () => {
    const folder = primary({ is_primary: false, branch: null });
    const menu = folderMenu(workspace({ is_git: false, checkouts: [folder] }), folder, desktop);
    expect(drawn(menu)).toEqual(["Open Overview", "New worktree…", "New tab in main ⌘T", "─", "Reveal in Finder", "Copy path", "─", "Pin", "Remove project…", "─", "Open", "Set purpose…"]);
    const pr = { number: 7, title: "", url: "https://example.invalid/pull/7", badge: "open" as const, review: null, is_draft: false };
    expect(drawn(folderMenu(workspace({ checkouts: [folder] }), { ...folder, pull_request: pr }, browser)).slice(-4)).toEqual(["─", "Open", "Open pull request #7", "Set purpose…"]);
  });

  it("says what removing a project closes and that an unregistered row leaves with Herdr's workspace (B3)", () => {
    const counted = { removal: { pane_count: 2, running_agent_count: 1 } };
    expect(projectRemovalConsequences(workspace(counted))).toEqual([
      "2 panes in this project close first, stopping 1 running agent.",
      "Only the registration is removed: the folder, its repository and its worktrees stay on disk.",
    ]);
    expect(projectRemovalConsequences(workspace({ ...counted, registered: false }))[1]).toMatch(/keeps no registration .* row leaves once Herdr closes the workspace/);
  });

  it("draws an agent's menu with its ⌥n and without Mark as seen or Stop agent (B7, B8)", () => {
    const agent = { id: "a", pane_id: "p1", identity_label: "배포 전 확인", agent_kind: "claude", symbol: "●", group: "working", status_label: "working", elapsed: "2m", emphasized: false, unread: false, session_id: "0b5e-session" };
    expect(drawn(agentMenu(agent, "⌥3"))).toEqual(["Show ⌥3", "─", "Copy title", "Copy session id", "─", "Close tab…"]);
    expect(drawn(agentMenu(agent, ""))[0]).toBe("Show");
    expect(agentMenu({ ...agent, session_id: null }, "").find((item) => item.id === "copy_session_id")?.unavailable).toMatch(/no session id/);
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
    // The same repository path on another device is another task.
    expect(taskFor(task({}), { ...request, deviceId: "local" })?.id).toBe(5);
    expect(taskFor(task({ device_id: "studio" }), { ...request, deviceId: "local" })).toBeNull();
    expect(taskFor(task({ device_id: "studio" }), { ...request, deviceId: "studio" })?.id).toBe(5);
  });

  it("reads only the removal of the checkout this page asked to delete", () => {
    const removal = { id: 3, repository_root: "/r", checkout_path: "/r-feature", branch: "feature", delete_branch: false, phase: "removing", message: null };
    expect(removalFor(removal, "local", "/r-feature", 2)?.id).toBe(3);
    expect(removalFor(removal, "local", "/r-other", 2)).toBeNull();
    expect(removalFor(removal, "local", "/r-feature", 3)).toBeNull();
    // The same path on another device is not this page's removal.
    expect(removalFor(removal, "studio", "/r-feature", 2)).toBeNull();
    expect(removalFor({ ...removal, device_id: "studio" }, "studio", "/r-feature", 2)?.id).toBe(3);
  });
});

describe("purposeScope", () => {
  it("names the device's Herdr as the only store for a device purpose and adds the branch description here", () => {
    expect(purposeScope("MacBook", "feature")).toBe("Kept in Herdr's workspace metadata on MacBook; its Git config is not changed.");
    expect(purposeScope(null, "feature")).toBe("Kept in Herdr's workspace metadata and as the Git description of feature on this machine.");
    expect(purposeScope(null, null)).toBe("Kept in Herdr's workspace metadata on this machine.");
  });
});
