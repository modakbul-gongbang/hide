---
topic: "project home: home-graph"
status: "ready"
human_approval: "delegated to the run (operator asleep; compare both hypotheses in the morning)"
created_at: "2026-09-19"
---

# Hypothesis A: "Constellation" - an Obsidian-style relationship graph

Slug: `home-graph`. Branch: `prd/home-graph`. Worktree: this directory.

## The hypothesis

A project reads best as a map: the project in the middle, each checkout (worktree) a hub around it, each agent a node on its checkout, delegated children hanging off their parent, and pull requests as the checkout's outward edge. Position carries membership, color carries state, size carries recency, and hovering a node lights its relationships. The operator learns where things are and glances at the map the way they glance at Obsidian's graph.

The research that preceded this run (Obsidian's own forum on graph legibility, Conductor, Vibe Kanban, Cursor 3's agents window, Sculptor) says force graphs fail on two things: unstable positions and hairballs. Both are designable-away at hide's scale (one project: 1-15 checkouts, 1-40 agents), and the rules below are what make this hypothesis a fair test rather than a strawman. Follow them.

## Design rules (non-negotiable)

1. **Stable positions.** Layout is deterministic for a given topology: the project node is fixed at the center, checkouts are seeded on a ring at angles decided by a stable sort (primary checkout first, then branch name), agents are seeded around their checkout, children around their parent. A force simulation (repulsion between all nodes, springs on edges, a weak pull to the seed position) runs only when the *topology key* changes (set of node ids + edges), decays to rest in under one second, and then stops entirely: no `TimelineView` ticking at rest, no work per snapshot when only status changed. Node positions live in a session-local cache keyed by node id so a status change never moves a node, and a new node appears next to its parent instead of reshuffling the map. Write the simulation yourself in `ProjectHomeGraphLayout.swift` (pure, testable, O(n²) is fine at this scale); do not add a SwiftPM dependency, the shell vendors its only two packages locally and the build must stay offline.
2. **Local graph by default.** When an agent or checkout is selected (click, or the focused pane on entry), the graph shows that node's neighborhood to depth 2 in full color and dims the rest to `HideTheme.Opacity.dimmed`; a "Whole project" control returns to the full map. This is Obsidian's local graph and it is what keeps the map readable.
3. **Encode state the way the rest of hide does.** Node fill/ring uses the agent's `AgentStatusPresentation` color and the status mark glyph is drawn inside the node (`?`, `!`, `×`, `✓`, `●`, `○`, `~`, `⊘`); checkouts use the branch icon and the PR state color from `HideTheme.PullRequest` for their outward edge; delegated children are drawn smaller and their edge dashed; a `stallLevel` on a root draws a warning halo on the root and a pulsing ring is **not** used (no animation at rest). Node size: agents by recency of `lastActivity` (three sizes, decided in the presentation type), checkouts by agent count.
4. **Labels are always drawn**, never hover-only: identity label under every agent node (truncate at a token width, full text in the tooltip and accessibility label), branch under every checkout. Text must not overlap; if the layout has two labels colliding after rest, nudge along the ring (deterministic).
5. **Hover isolates relationships**: hovering a node brings its edges and neighbors to full opacity and shows a compact card (identity, status word, second line `detail`, checkout, elapsed) near the node.
6. **The glance summary sits outside the canvas**: a header row with the project name, `Needs You · Done · Working · Seen` counts in the pet badge colors, the Start new terminal button, and the Whole project / local toggle. An attention rail down the right edge lists Needs You and Done rows (click focuses that node in the graph and a second click opens the pane), because a glance must find "act now" without scanning a map.
7. **Empty and degraded states**: no checkouts → the project node alone with the empty-state sentence; no agents → checkouts alone; disconnected → the last map dimmed with the stale notice; missing worktree → a hollow checkout node with a "missing" badge.
8. **Cost**: layout recompute only on topology change; drawing is one `Canvas` pass; hit testing is a pure function over node positions; no per-frame allocation of formatted strings (cache labels in the presentation output).

## Deliverables beyond the shared brief

- `ProjectHomeGraphLayout.swift` (simulation + seeding) with tests: determinism (same input → same positions), stability (adding one node moves no existing node more than a small bound), no overlapping labels after rest for a 12-checkout/30-agent fixture, and the local-graph depth filter.
- `ProjectHomePresentation.swift` produces nodes, edges, the attention rail rows, and every label/color decision; `ProjectHome.swift` draws.
- Screenshots at 1 checkout / 1 agent, 4 checkouts / 9 agents with one delegation chain, and 12 checkouts / 30 agents (extend the fixture with reported panes), plus hover, local-graph, disconnected and narrow.
- In `RESULT.md`, answer honestly: does the map beat a list for "which agent needs me", and where did the graph need rules that a list would not?


# Project Home: shared brief (read fully before touching code)

You are the sole implementor of one of two competing hypotheses for a new **Project Home** surface in hide.
Another agent is building the other hypothesis in a sibling worktree; you never touch that worktree, and you never coordinate with it.
The operator (Hoyeon) is asleep and will compare both results in the morning. Finish the whole thing: code, tests, native screenshots, a local commit series, a pushed branch and a draft pull request.

## Ground rules (each of these has cost a run before)

- Do the work yourself in this worktree: no `sasu implement`, no `/implement`, no sub-agents, no new worktrees, no `agents/runs/.active` resumption. Ignore any `.prd-implement-active.json`.
- Read first, in this order: `AGENTS.md`, `docs/README.md`, `docs/ARCHITECTURE.md`, `docs/status-model.md`, `DESIGN.md` sections "In-Product Components" and "Projects and checkout context", `macos/AGENTS.md`, `design/agent-workflow-review.md`, `docs/PERFORMANCE_TESTING.md` sections 2-4, `docs/dev-runtime.md`. Also read `~/projects/oh-my-principle/engineering/principles.md` and `~/projects/oh-my-principle/design/principles.md`.
- Never edit `Cargo.lock` by hand. Prefer a shell-only change; touch `herdr-core` only if the shell truly lacks a fact, and then add it to the existing projection with a Rust test.
- Every visual value comes from `HideTheme` (add tokens under a new `HideTheme.Home` namespace, document them in `DESIGN.md`, and register each new constant in `scripts/pen-token-map.json` under `unmapped` with a reason, or under `mapped` if it is a plain color/spacing). Then run `node scripts/gen-pen.mjs` and `node scripts/check-pen.mjs`. Before running gen-pen and before every commit that touches `design/hide.pen`: `git diff --stat -- design/hide.pen` and confirm only the generated Foundations sheet moved; a running Pen desktop app has rewritten worktree canvases with stale boards before. If whole `Screen /` boards appear or vanish, `git checkout -- design/hide.pen` and redo only gen-pen. Do not draw new boards in the canvas; note "design canvas board pending" in the PR body instead.
- Tooltips use the command tooltip modifier (`hideTooltip`) like every other shell tooltip. No `print`, no inline colors, no numeric font sizes.
- No screenshots, logs or profiles in the commit. Run artifacts go under `agents/runs/<slug>/` (ignored).
- Commits: one per coherent unit, descriptive messages, no AI/agent/model attribution anywhere (branch, commits, PR). `python3 scripts/check-no-attribution.py` before pushing.
- Known load-flaky tests exist in the Rust suite under machine load; if `verify-cargo.sh test` fails in a deadline/timing test you did not touch, rerun once and report it, do not edit it.
- Contract scripts are zsh: `zsh scripts/check-herdr-contract.sh --schema-only`.

## What Project Home is

Today a checkout whose Herdr tab list is empty draws "No terminal open / Start new terminal" (`HideEmptyCheckoutState`, `.idle` case in `macos/Sources/HerdrMacOS/HideTerminalSurface.swift`).
Project Home replaces that empty state with a **project-scoped** page that answers, in one glance: which checkouts (worktrees) this project has, which agent is running in each, what state each agent is in, what it is working on, how far the checkout's work has got (changes, commits ahead, PR, CI), and who delegated to whom.
Its purpose is to make many parallel agents legible; the terminal is where you act, Home is where you decide where to go next.

Two entry points, both required:

1. The empty state: when the focused checkout has no tab (`checkoutStartState == .idle`), Home is what is drawn. The **Start new terminal** button stays on the page (top right of the header), because starting a terminal is still the most frequent action there.
2. A session-local toggle so the operator can open Home over a checkout that has tabs: a `HideIconButton` at the leading edge of the tab strip (`HideTabStrip` in `HideMainView.swift`, after the sidebar-restore button, SF Symbol `square.grid.2x2` or similar) plus a `ShellMenuCommand` `.projectHome` ("Project Home") in the View menu with an unbound-by-default chord if every reasonable chord is taken (check `scripts/check-shortcut-contract.sh` and the registry before choosing; `⌘⇧H` is fine if free). The toggle flips a `@Published var projectHomeVisible` on `ShellModel` (session-local, like the Agents `My Work` scope; it is not core state and must not be persisted). While visible, Home draws over the pane canvas the way `EditorViewerOverlay` sits in the `ZStack` in `HideMainView`; Escape or the same toggle closes it. Any action on Home that opens a pane or a checkout closes the overlay.

Scope of data: the focused project (`model.focusedWorkspace`, a `CoreWorkspaceSnapshot`) and its `checkouts` (`CoreCheckoutSnapshot`: branch, `worktree` with headSHA/ahead/behind/changedFileCount/dirty/lastCommitSubject, `pullRequest`, `github`, `agentSummary`, `hasPanes`, `tabs`), the canonical agents (`model.agents`, `SidebarAgent`: group/symbol/statusLabel/identityLabel/task/detail/lineageDepth/lineageChildPaneIDs/delegated/stallLevel/stallNotice/elapsed/agentKind/unread/blocked), and `model.core.snapshot?.gitWorktrees` (`CoreProjectWorktrees`: pullRequests, history, worktrees with `agentLine`, `runningAgentCount`).
Reuse `ProjectTaskForestPresentation` for lineage, `AgentStatusPresentation`/`AgentStatusMark`/`AgentBadge` for identity and status, `CheckoutCardPresentation.pullRequestState` for PR state, and the PR/CI color mapping already in `HideTheme.PullRequest`. Never derive a status a second time; the core's `group`, `symbol`, `statusLabel`, `delegated`, `stallLevel` are final.

Actions: click an agent → `model.selectAgent(paneID:)`; click a checkout → `model.selectCheckout(_:)` (disabled when `worktree == nil || !exists`); PR chip → `model.openPullRequest(_:)`; Start new terminal → `model.addTab()`; "New worktree" is out of scope.

States that must each be designed and reachable in a Swift test of the presentation type: loading (no snapshot), remote context (keep the existing remote empty state untouched), no checkouts, checkouts but no agents, agents but Herdr disconnected (`!model.agentsConnected`: keep the last projection, dim it, and show the existing `bolt.slash` stale notice), a stalled delegated child (`stallLevel` soft/hard on the root), an orphan root, and a checkout whose worktree is missing.

Non-goals: no new Git or GitHub readers, no timers, no subprocesses, no per-snapshot heavy work (layout must be recomputed only when the inputs it reads change; memoize by a value key), no pet or sidebar changes, no persistence, no right-panel Overview changes.

## Structure the code like the rest of the shell

- `macos/Sources/HerdrMacOS/ProjectHome.swift`: the view. `ProjectHomePresentation.swift`: the pure model (nodes/lanes/rows and every label, color and ordering decision) with unit tests in `macos/Tests/HerdrMacOSTests/ProjectHomePresentationTests.swift` (Swift Testing). Split further if a file passes ~400 lines.
- Tests assert what a caller observes: given fixture `SidebarAgent`/`CoreCheckoutSnapshot` values, the presentation produces these rows/nodes/edges/labels, in this order, with these states. Build fixtures with the memberwise inits that already exist for tests.
- Register any new control shape in `scripts/design-control-policy.json` only if `node scripts/check-design-contract.mjs` demands it, and prefer the existing shared controls (`HideTextButtonStyle`, `HideIconButton`, `HideInteractiveButtonStyle`, `HideEmptyState`, `HideBadge`).
- Update the owning docs in the same commits: `DESIGN.md` gets a "Project Home" subsection under "Projects and checkout context" (tokens, layout rules, states); `docs/README.md` map gets a row; `design/agent-workflow-review.md` product decisions get one line.

## Verification, in this order

1. `bash scripts/verify-swift.sh test` (builds the release core first; slow, run it once early to warm the cache, then with `--filter ProjectHome` while iterating).
2. `node scripts/check-design-contract.mjs`, `bash scripts/check-right-panel-sections.sh`, `bash scripts/check-shortcut-contract.sh`, `python3 scripts/check-core-bridge-structure.py`, `bash scripts/check-no-workstation-identity.sh`, `bash scripts/check-git-worktree-presentation.sh`. If Rust changed: `bash scripts/verify-cargo.sh lint` and `test`.
3. Native, on an isolated server. `bash macos/scripts/build_dev_app.sh` prints the bundle (`hide (<worktree>)`, its own bundle id). Then follow `docs/PERFORMANCE_TESTING.md` §3 exactly, with a run directory `agents/runs/<slug>/native/` and *your own* unique values (the other agent is doing the same on this machine at the same time, so never reuse a fixed name): `HERDR_SESSION=<slug>-$$`, `HERDR_SOCKET_PATH=/tmp/<slug>-$$.sock` (short), `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME` under the run directory, and unset `HERDR_PANE_ID HERDR_TAB_ID HERDR_WORKSPACE_ID HERDR_ENV SASU_HERDR_ROLE`. Start the server yourself with the bundle's own `herdr` binary (`<app>/Contents/Resources/...`; find it with `find <app> -name herdr -type f`) using `herdr server` under that env; the app-started server is not isolated. Prove `herdr workspace list` is empty on the private socket, then launch the bundle with `--state-path agents/runs/<slug>/native/state.json` and the same env, and confirm `pgrep -fl HerdrMacOS` shows your PID beside the operator's; never touch the operator's app, server, panes or workspaces.
4. Fixture: create a throwaway git repository under the run directory with a `main` checkout and three linked worktrees (`git worktree add`), commit something on each branch so ahead counts differ, leave one dirty. In the private server create one Herdr workspace per worktree (`herdr workspace create --cwd ...`), and in them open plain shell panes and **report** agent state through `herdr pane report-metadata` the way `scripts/agent-attention-fixture.sh` does (read that script: it sets `agent_claude`, `status_*`, `name`, `task`, `progress`, `activity` tokens without starting a real agent), so the page shows a mix: one Needs You (question), one Working, one Done, one Idle, and one delegated child (report `parent_pane=<parent pane id>` under a source so lineage appears). Register the fixture repo in the app (New Workspace on the fixture root, or `--workspace-root`; read `LaunchArguments.swift`). Then close the tabs of one checkout so the empty state shows Home, and use the toggle on a checkout with tabs.
5. Capture with `screencapture -l <windowID>` of *your* window only (`peekaboo window list --app "hide (<worktree>)" --json` gives the id; `peekaboo see --window-id ... --no-elements --path ... --json` also works). Take at minimum: Home as empty state, Home as overlay, hover/selection state, the disconnected state (SIGTERM your private server, capture, restart it), and a narrow window (900 px wide). Save under `agents/runs/<slug>/native/`. Look at each screenshot critically (Read the PNG) and fix visual defects you see before calling it done: overlapping labels, clipped text, unreadable contrast, dead space.
6. Tear down: quit your app process by PID, `kill -TERM` your server, confirm `pgrep -f <your bundle id>` is empty and the operator's workspace count is unchanged.

## Delivery

- Commit as you go. When done: `git push -u origin <branch>`, then `gh pr create --draft --base main` using `.github/pull_request_template.md` (read it; answer Risk surface / Review focus / Breaking change from the diff; delete inapplicable lines rather than writing N/A). The PR body describes the screenshots by name and location (`agents/runs/<slug>/native/*.png`, local only) and lists every check you ran with its result, and every check you did not run and why.
- Write `agents/runs/<slug>/RESULT.md`: what was built, what was verified (with the exact commands), what is unverified, the open design questions you want the operator to answer, and your own honest critique of the hypothesis after seeing it on real-shaped data (where it is legible and where it breaks: 1 checkout, 12 checkouts, 30 agents, long Korean task names).
- Stop when done. Do not merge. Do not touch `main`.
