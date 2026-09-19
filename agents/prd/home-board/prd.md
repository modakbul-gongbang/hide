---
topic: "project home: home-board"
status: "ready"
human_approval: "delegated to the run (operator asleep; compare both hypotheses in the morning)"
created_at: "2026-09-19"
---

# Hypothesis B: "Mission board" - one lane per checkout, agents as cards, progress as a track

Slug: `home-board`. Branch: `prd/home-board`. Worktree: this directory.

## The hypothesis

The relationships in a project are containment, not a mesh: a project owns checkouts, a checkout owns the agents working in it, an agent optionally owns delegated children. A board whose lanes are the checkouts draws that containment for free, keeps every card in a fixed slot so a status change repaints one badge and never moves anything, and lets "needs you" be a sort instead of a scan. This is where the surveyed products converged (Conductor and Sculptor list sessions per worktree, Vibe Kanban's columns are derived git state, Cursor 3's agents window is a list with a fixed inspector); the bet is that a full-page version with a per-checkout progress track and lineage-nested cards is the most legible way to see many agents at once.

## Design rules (non-negotiable)

1. **One lane per checkout, ordered by attention.** Lane order is the checkout rank the Overview already uses (Needs You first, then Done, Working, has agents, none), stable ties by path; the primary checkout is never hidden but is not pinned first. A lane is a full-width horizontal band (not a column) so long Korean identity labels have room; lanes stack vertically in a `ScrollView` with `LazyVStack`.
2. **Lane header = derived git state, read left to right as a track.** Branch name (with the checkout-kind icon), then a compact progress track of fixed stages drawn as connected segments: `changes` (changed file count, dirty), `commits` (↑ahead ↓behind vs. base), `PR` (number + Open/Draft/Merged/Closed color from `HideTheme.PullRequest`), `CI` (Passing/Failing/Running/No checks/Unknown from the github rollup mapping in `docs/status-model.md`), `merged`. A stage the data cannot fill is drawn hollow with its unavailable reason in the tooltip, never invented (no PR → hollow PR stage; lookup failed → hollow with the failure). The track is the "how far has this work got" answer; the presentation type decides each stage's state and label, the view only draws.
3. **Cards are agents; nesting is lineage.** Inside a lane, cards are laid out in a wrapping flow (`Layout` protocol or a simple wrap layout you write), sorted by the core's group order then recency. A card shows: provider badge (`AgentBadge`), status mark (`AgentStatusMark`), identity label (2 lines max), the core's `detail` second line or status word exactly as `AgentRow` chooses it, and `elapsed`. A delegated child is drawn as a smaller card attached under its parent card (indented with the lineage hairline), and a child that physically lives in another checkout appears in its own lane with a `↳ from <parent>` caption, matching the Overview decision. A stalled root shows the `stallNotice` sentence in warning color on the root card. Unread emphasis follows `emphasized`.
4. **Attention-first without leaving the board.** A slim "Needs You" strip at the top of the page lists the Needs You and Done agents across all lanes (same card, compact), so the glance answer is the first thing on the page; clicking one scrolls its lane into view and highlights the card, a second click opens the pane. When there is nothing in it, it collapses to one muted line, not a box.
5. **Hover and selection isolate relationships.** Hovering a card raises its lineage (parent and children) and dims unrelated cards in that lane to `HideTheme.Opacity.dimmed`; selecting a card (click) opens a persistent inspector column on the right (identity, status, second line, checkout, lineage chain, PR, `Open` button) without moving terminal focus; `Open` and double-click open the pane. This mirrors the Overview's inspect-versus-open rule.
6. **Header**: project name, `Needs You · Done · Working · Seen` counts in the pet badge colors, a `Start new terminal` button, and a search field that filters cards and lanes (reuse `HideSearchField`).
7. **States**: no checkouts → one empty lane with the sentence; a checkout with no agents → lane header + track only, with the uninstrumented mark when `agentLine` says hide cannot see into it; disconnected → the board dimmed with the stale notice, no counts changed; missing worktree → lane drawn hollow with `missing`.
8. **Cost**: the board is a pure function of the snapshot; memoize the presentation output by the inputs' identity so an unchanged snapshot section does not rebuild it; no timers.

## Deliverables beyond the shared brief

- `ProjectHomePresentation.swift` produces lanes (with a `TrackStage` array per lane), cards with nesting, the attention strip rows, and every label/color decision, with tests covering lane order, stage states (present, hollow, failed), nesting, cross-checkout children, stalled root, and search filtering.
- `ProjectHome.swift` draws; the wrap layout, if written, gets its own file and a layout test.
- Screenshots at 1 checkout / 1 agent, 4 checkouts / 9 agents with one delegation chain, and 12 checkouts / 30 agents (extend the fixture with reported panes), plus hover, inspector open, disconnected and narrow.
- In `RESULT.md`, answer honestly: at 12 lanes does the page still answer "where should I go" in one glance, and what did the board lose that a map would show?


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
