---
topic: "Hide shell UX round 4: shortcuts, explorer, pane header, links, and find"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "Two requirements act on the user's live Herdr server while they are away - forking an agent pane starts a real billable agent session, and proving the empty-pane state requires closing real panes - so an isolation boundary, not UI taste, is the dominant risk."
source_intake: "current conversation"
created_at: "2026-09-03"
updated_at: "2026-09-03"
---

# PRD: Hide shell UX round 4

## 1. Summary

Fourteen defects and gaps reported from a live session of the Hide macOS shell, fixed as one round.
Four are keyboard contracts that silently do nothing today (right-panel toggle, line-start/line-end, per-pane zoom, in-pane find).
Four are explorer defects (icons for extensionless and dotfiles, stale tree selection, row-click that only works on the disclosure triangle, no Git view).
Four are pane-surface gaps (empty state after the last pane closes, pane title priority, close/fork controls, running-server ports).
One is a terminal link router that sends schemeless URLs down the local-file path and shows a "could not find inside the selected checkout" alert.
One is a reusable project subagent that finds dead code and structural simplification, plus a closing simplification pass over this round's own work.

Approval checklist:

- scope boundary and the five explicit non-goals, including the deferred `file:line` jump (section 3).
- the structural changes: a new Git status/diff capability in `herdr-core`, per-pane zoom state, a pane-title field carried on the local projection, and a fork action that starts a real agent session (section 5).
- the keyboard rebind that removes `⌘⌥B` in favor of `⌘⇧B` with no dual binding (R1).
- the isolation boundary for live Herdr verification: a throwaway workspace this run creates and closes, never the user's own panes (section 9.2, V7).
- required-for-done verification modes: build/static, automated behavior, app runtime, and one bounded live-Herdr integration (section 9.1).
- delivery mode: local. A semantic commit on the run's branch, no push, no PR, no CI (section 4.3).
- `review_profile: high-risk` and its rationale (frontmatter).

## 2. Problem, Goal, And Users

The user is the single operator of Hide, a SwiftUI macOS shell over a Herdr terminal session, used all day to drive Claude Code and Codex agents.
They reported fourteen problems in one sitting, all found by ordinary use rather than by testing.

The problems share one shape: a control looks like it exists and does nothing.
`⌘⇧B` is not bound at all, so it reaches the terminal and is swallowed.
`⌘←`/`⌘→` are bound, but to word-back/word-forward rather than line-start/line-end.
`⌘F` reaches a vendored find bar that the shell never wires up.
There is no zoom binding at all, and the one font-size control is a settings slider that neither the terminal nor the editor reads.
A file row's name is inert unless the row is a file, so clicking a folder name does nothing while the disclosure triangle works.
A schemeless URL printed by an agent is routed to the local-file resolver and produces an alert instead of opening.

The goal is that every control the user reaches for either works or is visibly absent, and that the shell stops losing state the core already tracks.

### 2.1 User Scenarios

- SC1. Right panel toggle: the operator hides and shows the right panel from the keyboard.
  Actors: the operator.
  Primary path: with a workspace focused and the right panel visible, pressing `⌘⇧B` hides it; pressing `⌘⇧B` again shows it, and the panel returns in the same section it was showing.
  Failure state: while the core is not started, the toggle does not silently no-op; the shell surfaces the dispatch failure rather than leaving the key looking dead.
  Recovery: the previous binding `⌘⌥B` no longer toggles anything and no menu item or button label still advertises it.
  Reach: a launched shell with one workspace and one terminal pane.

- SC2. Line navigation in a terminal pane: the operator jumps to the start and end of the line they are typing into an agent prompt.
  Actors: the operator.
  Primary path: with the caret mid-line in a focused terminal pane, `⌘←` moves to the start of the line and `⌘→` to the end, matching the behavior of every other macOS text field.
  Failure state: the previous word-back/word-forward behavior is gone, so a long prompt no longer needs repeated presses.
  Recovery: with a file tab focused instead of a terminal, `⌘←`/`⌘→` keep the stock text-view line-start/line-end behavior and are not intercepted.
  Reach: a terminal pane running an interactive agent with a partially typed prompt.

- SC3. File icons for names without an extension: the operator scans the file tree of a repository root.
  Actors: the operator.
  Primary path: `.gitignore`, `.gitattributes`, `.env`, `.zshrc`, `CODEOWNERS`, `Procfile`, `Dockerfile.dev`, and `LICENSE` each show a legible icon that distinguishes them from one another or, where no specific icon exists, one deliberate generic document icon rendered at the same size and alignment as every other row.
  Failure state: when the bundled glyph font fails to register, every row falls back to its system symbol rather than rendering blank boxes.
  Recovery: adding a new dotfile to the checkout gives it the generic icon immediately, with no restart.
  Reach: the Hide repository root itself, which contains all of these names.

- SC4. Closing every pane: the operator closes the last tab and expects the window to stay.
  Actors: the operator.
  Primary path: closing panes one at a time until none remain leaves the application running with its window open, showing a centered empty state whose primary control creates a new pane; activating that control produces a working terminal pane.
  Failure state: the empty state never claims Hide is about to start something it will not start; if no checkout is selected, it says so and offers the workspace action instead.
  Recovery: after creating a pane from the empty state, closing it again returns to the same empty state, repeatably.
  Reach: a throwaway workspace this run creates for the purpose, never the operator's own workspace.

- SC5. Tree selection follows the file tabs: the operator opens two files and closes one.
  Actors: the operator.
  Primary path: opening a file highlights its row in the tree; switching file tabs moves the highlight; closing the active tab moves the highlight to the tab that becomes active.
  Failure state: closing the last file tab clears the highlight entirely rather than leaving a row highlighted with no tab behind it.
  Recovery: opening a file that lives inside a collapsed folder expands the ancestor folders and highlights the row, so the highlight is never invisible.
  Reach: a checkout with at least one nested directory and two openable files.

- SC6. Opening from the tree body: the operator clicks names rather than triangles.
  Actors: the operator.
  Primary path: clicking anywhere on a directory row's body expands or collapses it; clicking anywhere on a file row's body opens that file as a tab.
  Failure state: moving the selection with the arrow keys changes the highlighted row without opening anything, so walking the tree no longer opens every file it passes.
  Recovery: `Return` on the selected row performs the same activation as a click.
  Reach: a checkout with at least one directory and one file at the same level.

- SC7. Pane title: the operator names a pane and expects to see that name.
  Actors: the operator.
  Primary path: a pane with a Herdr label shows that label in its header; a pane with no label but a terminal title shows the terminal title; a pane with neither shows the workspace name; a pane with none of the three shows its pane id.
  Failure state: a label of only whitespace is treated as absent rather than rendering an empty header.
  Recovery: renaming the pane in Herdr updates the header without restarting Hide.
  Reach: two panes in one tab, one renamed through Herdr and one left unnamed.

- SC8. Forking an agent pane: the operator branches a Claude or Codex conversation into a sibling pane.
  Actors: the operator.
  Primary path: a pane running a detected Claude or Codex session shows a fork control in its header; activating it opens a sibling pane to the right running the same agent resumed as a fork of that session, with the parent's history present and the parent pane untouched and still running.
  Failure state: a pane with no agent, or an agent with no recorded session identity, shows no fork control at all rather than a control that fails when pressed; if the fork command fails to start, the shell reports the failure and does not leave an empty pane behind.
  Recovery: the forked pane is identifiable as a fork both in Hide's header and in Herdr's own pane state, so the operator can tell the two conversations apart after a restart.
  Reach: one pane running an interactive agent inside a throwaway workspace this run creates.

- SC9. Explorer and changes: the operator checks what changed without leaving Hide.
  Actors: the operator.
  Primary path: the right panel offers exactly two sections, the existing file explorer and a changes view; the changes view lists the checkout's modified, added, deleted, and untracked files, and selecting one shows that file's diff.
  Failure state: a checkout that is not a Git repository, or a Git command that fails, shows a stated reason in the changes view rather than an empty list that reads as "no changes".
  Recovery: making an edit and returning to the changes view shows the new state without restarting the shell.
  Reach: a checkout with at least one modified tracked file and one untracked file.

- SC10. Per-pane zoom: the operator enlarges the pane they are reading.
  Actors: the operator.
  Primary path: `⌘=` enlarges and `⌘-` shrinks the text of the focused pane only, leaving sibling panes unchanged; `⌘0` returns that pane to the default size.
  Failure state: at the smallest and largest supported sizes the shortcut stops changing the size rather than producing unreadable or clipped output.
  Recovery: the chosen size survives a restart of the shell, and a resized terminal pane reflows so no output is truncated.
  Reach: two terminal panes in one tab, both showing wrapped output.

- SC11. Running servers: the operator sees which dev servers are up.
  Actors: the operator.
  Primary path: when a process rooted at a pane's working directory is listening on a TCP port, the pane header shows that port; activating it opens `http://localhost:<port>` in Chrome.
  Failure state: when Chrome is not installed the link opens in the default browser rather than failing silently; when no process is listening the header shows no port indicator at all.
  Recovery: stopping the server removes the indicator within one refresh window, and starting another adds it.
  Reach: a pane whose working directory is a checkout, with a throwaway HTTP server this run starts and stops on an unused port.

- SC12. Clicking a link an agent printed: the operator follows a URL and a file path out of agent output.
  Actors: the operator.
  Primary path: clicking a full URL, or a schemeless host such as `docs.anthropic.com/en/docs`, opens it in Chrome; clicking a path that resolves to an existing file opens that file as a tab, whether it is inside the checkout or outside it.
  Failure state: text that is neither a resolvable file nor a plausible host produces a notice naming what could not be resolved, not a claim about the checkout.
  Recovery: after a failed click the shell stays usable and the same click on a valid target works.
  Reach: a terminal pane where this run prints a fixed set of link forms, including a schemeless host, an absolute path outside the checkout, and an unresolvable token.

- SC13. Finding text in a pane: the operator searches the visible scrollback.
  Actors: the operator.
  Primary path: `⌘F` in a focused pane reveals a search field at the top right of that pane; typing highlights every match; `Return` moves to the next match and scrolls it into view; `Shift-Return` moves to the previous one.
  Failure state: a query with no match says so in the field rather than silently doing nothing, and `Escape` closes the field and clears the highlights.
  Recovery: the same flow works with a file tab focused, searching the file's text instead of the terminal scrollback.
  Reach: a pane with enough scrollback that a match exists off-screen.

## 3. Scope And Non-Goals

In scope: the fourteen reported items, the enabling changes each needs, and one closing simplification pass over this round's own diff.

Non-goals, each a deliberate omission:

- Jumping to `file:line:column` when a terminal link carries a source location.
  The resolver already parses line and column and every layer below discards them; wiring it needs a new snapshot field, a new core event field, and a scroll-to-range API on the editor.
  Consequence: clicking `Foo.swift:120` opens the file at the top.
  Revisit when the operator asks for source-location navigation.
- An in-app browser for opened URLs.
  The operator stated an internal browser is a later step; this round opens Chrome externally.
  Consequence: following a link leaves the app.
  Revisit when the browser surface is built.
- Staging, committing, discarding, or any other write action in the changes view.
  This round makes changes visible only.
  Consequence: the operator still commits from a terminal.
  Revisit once the read view is in daily use.
- Port detection for anything other than TCP listeners owned by a process whose working directory is at or below the pane's working directory.
  Consequence: a server started elsewhere, or bound over a socket the heuristic cannot attribute, shows no indicator.
  Revisit if attribution proves too narrow in use.
- A Codex equivalent of the R8 subagent.
  Codex has no subagent-definition format that maps onto a Claude Code agent file, so cross-runtime parity is not achievable for this asset.
  Consequence: the simplification agent is available in Claude Code only.
  Revisit if Codex gains an equivalent.

Also out of scope: reworking the vendored SwiftTerm implicit-link regex, changing the left sidebar, and the remote/SSH surfaces beyond keeping them working.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
Every enabling step - the throwaway Herdr workspace for live verification, the throwaway HTTP server for the port scenario, and the dev bundle build - is agent-performable and is a task rather than pre-work.

### 4.2 Human Decisions Before PRD Approval

None required.
Delivery is local and creates no branch, push, or PR; the live-Herdr side effects are bounded to resources this run creates and closes (section 9.2, V7); and every product choice that had a defensible default was made as a recorded assumption in 4.3 rather than held for approval, per the delegating invocation.

### 4.3 Decision Traceability For Fidelity Review

Source: the operator's `/please` invocation of 2026-09-03, which listed fourteen numbered items, named the implementation runtime, asked for an Implementor rotation at roughly half context, and asked for a simplification pass at the end.

- Item 1, right-panel toggle. The operator reported `⌘⇧B` as broken. Investigation found no `⌘⇧B` binding anywhere; the panel is bound to `⌘⌥B`. Represented as R1, AC1, T1. Agent-owned assumption: `⌘⌥B` is removed rather than kept alongside `⌘⇧B`, because a second binding for one action is exactly the obsolete path engineering principle 1 forbids. Reversible.
- Item 2, line navigation. Represented as R2, AC2, T2. Agent-owned assumption: the fix lives in Hide's own `ImeTerminalView` rather than in vendored SwiftTerm, so the vendor stays a clean dependency (engineering principle 7). Reversible.
- Item 3, icons for extensionless names. The operator asked for a sensible default and asked whether a library option exists. Investigation found the icon set is a curated Seti UI glyph subset with a hand-rolled resolver and one generic fallback that dotfiles reach silently. Represented as R3, AC3, T3. Agent-owned assumption: extend the existing resolver rather than adopt a new library, because the bundled glyph set already covers these names and adding a dependency for a lookup table fails engineering principle 7. Reversible.
- Item 4, all panes closed. The operator reported the app quitting and asked for verification. Investigation found no termination path in this repository and an empty state whose copy promises a pane Hide never starts. Represented as R4, AC4, AC5, T4, and SC4. The reproduction is a task obligation, not an assumption: the cause is confirmed by observation before the fix is written.
- Item 5, tree selection versus file tabs. The operator asked for tree selection to follow the visible file tab and invited a better alternative. Investigation found the core already rewrites the selected path on open, focus, and close, and the tree only ever adds selection, never clears it, and never reveals a collapsed ancestor. Represented as R5, AC6, T5. Alternative considered and rejected: separating an "active file" marker from a "tree cursor". Rejected because it introduces a second selection concept for a case the existing single one handles correctly once the two defects are fixed (engineering principle 2).
- Item 6, clicking the row body. Represented as R6, AC7, T6. Investigation additionally found that arrow-key traversal opens a file tab for every row it passes; fixing that is inside this requirement because it is the same defect - open is bound to selection change rather than to activation.
- Item 7, pane title priority. The operator proposed label first, then project name. Represented as R7, AC8, T7. Investigation found Herdr sends both a pane label and a terminal title on the wire and the local projection drops both, so the accepted proposal is extended to a four-step ladder: pane label, terminal title, workspace name, pane id. Agent-owned assumption: the terminal title is inserted between the operator's two named steps, because it is real pane identity the shell already receives and discards. Reversible.
- Item 8, simplification subagent. Represented as R8, AC9, AC10, T8. Investigation found `.gitignore`'s `agents/` line also matches `.claude/agents/`, so the asset could not be committed; anchoring it to `/agents/` is inside this requirement, with the matching `AGENTS.md` sentence updated. Recorded non-goal: no Codex equivalent exists (section 3).
- Item 9, pane close and fork controls. The operator asked for close and fork icons, fork gated on an active agent, fork opening a sibling pane through each agent's resume command, and a Herdr-level fork marker, while stating they did not know what the Herdr API offers. Represented as R9, AC11, AC12, AC13, T9, SC8. Verified against the installed Herdr 0.8.2 and the pinned contract: `claude --resume <id> --fork-session` and `codex fork <id>` both exist; `PaneInfo.agent_session.value` carries the session id; `pane.report_metadata` writes a display-only token map that round-trips through `pane`/`agent` reads with no TTL. Agent-owned assumption: the marker is a `fork_of` token naming the parent pane id, chosen over `pane.rename` because a label is operator-owned text and overloading it would destroy R7's first priority. Reversible.
- Item 10, Explorer and Git diff. The operator asked to drop Workbench and show exactly two sections. Represented as R10, AC14, AC15, T10, SC9. Agent-owned assumption: the changes view is layered as a changed-file list first and a per-file diff second, both in this round, per engineering principle 3. Reversible.
- Item 11, per-pane zoom. Represented as R11, AC16, T11. Investigation found no zoom of any kind and a settings font slider both surfaces ignore. Agent-owned assumption: zoom is per-pane state in the core keyed by pane id, not shell-local state, because the core owns all authority in this architecture. Not casually reversible; called out in section 5.
- Item 12, running-server ports. Represented as R12, AC17, T12, SC11. Agent-owned assumptions: attribution is by the listening process's working directory being at or below the pane's working directory; the indicator refreshes on a window rather than per tick; Chrome is the target with the default browser as fallback. Reversible.
- Item 13, terminal link routing. The operator asked that web links open in Chrome and that file paths open as tabs even from outside the checkout, and asked not to be told it cannot be done. Represented as R13, AC18, AC19, T13, SC12. Root cause confirmed: anything without `://` or a scheme in a small allowlist falls through to the local-file resolver, which then reports the checkout message; schemeless hosts printed by agents are the common case. The "outside the checkout" ask is achievable - the open path has no root restriction; only the link resolver imposed one. Agent-owned assumption on the remaining case, a URL wrapped across rows inside an agent's bordered TUI box: deferred with consequence, because the join heuristic lives in vendored SwiftTerm and reworking it is a separate change (section 3 lists the vendor regex as out of scope).
- Item 14, in-pane find. Represented as R14, AC20, T14, SC13. Investigation found SwiftTerm ships a find bar the shell never wires up and the code editor never enables its find bar. Agent-owned assumption: both existing find surfaces are wired and restyled through `HideTheme` rather than a new overlay being written, per engineering principles 6 and 7. Reversible.
- Implementation runtime. The operator required Claude Opus 5 under Herdr for implementation and an Implementor rotation at roughly half context. Represented as a delivery/process fact in section 11 and the result report contract, not as product scope.
- Closing simplification. The operator asked that a simplification pass run after the work. Represented as R16, AC22, T16 - the last task, gated on every other task being complete and on the suite staying green.
- Delivery mode. `agents/config.json` declares `delivery.mode: local` and `worktree.enabled: true`. Accepted unchanged; no push, PR, or CI is in this contract.
- Principles intake. Read in full at source commit `35ab76ca23d45e714f1630054855a8c8c4568d03`: `engineering/principles.md` (trigger: writing or changing code, proposing an architecture, choosing a dependency, designing an error path) and `design/principles.md` (trigger: building or changing any screen where a user performs work). Applicable rules are translated in section 11; engineering rules 9, 11, and 12 and design rules 1 and 2 are deliberately not translated into guardrails because this round adds no new logging surface, no repeatable mutation, no data-shaped list, and no schema-driven screen - each is noted here rather than padded into section 11.
- Project rule intake. `AGENTS.md` "Performance Guide" and "Herdr API Contract" apply directly to R10, R12, and R9; both are translated in section 11. `AGENTS.md` "Evidence Belongs Outside The Repository" governs where this round's screenshots live.

## 5. Major Technical Structure Changes

- A Git status and diff capability in `herdr-core`, exposed on the snapshot for the focused checkout: the changed-file set with per-file status, and the diff text for one selected file. It replaces the existing single-file `git_diff` helper whose line-number output crosses the FFI boundary and is read by nobody. It caches by input equality plus a refresh window and never forks a subprocess while the runtime mutex is held.
- Per-pane view scale in the core's persisted UI state, keyed by pane id, replacing the global font-size value that neither the terminal nor the editor reads today.
- A pane title field carried on the local pane projection. Herdr's pane label and terminal title are already parsed off the wire and dropped before the local projection; the projection payload gains the field so the local path can use the same identity the remote path already uses.
- A fork action that starts a real agent session. It splits a sibling pane, starts the same agent kind resumed as a fork of the parent's session through the Herdr CLI, and records the lineage as a display-only Herdr pane metadata token. This is the only requirement in the round with an external, billable side effect.
- A listening-port attribution service: a cached, off-mutex reader that maps TCP listeners to pane working directories and publishes the result on the snapshot.
- No schema, storage, auth, payment, or deployment change. No new third-party dependency.

## 6. Requirements

- R1. `⌘⇧B` toggles the right panel's visibility, the toggle persists through the core's UI state, and the previous `⌘⌥B` binding and every label advertising it are removed.
- R2. In a focused terminal pane, `⌘←` moves the caret to the start of the current line and `⌘→` to its end. With a file tab focused, both keep the platform's stock line-start and line-end behavior.
- R3. Every file row resolves to a legible icon. Names with no extension and names beginning with a dot resolve to a specific icon where the bundled glyph set has one and to a single deliberate generic document icon otherwise, at the same size and alignment as every other row.
- R4. Closing the last pane leaves the application running with its window open and shows a centered empty state whose primary control creates a working pane. The empty state never claims Hide is about to do something it will not do.
- R5. The file tree's highlighted row follows the active file tab: it moves when tabs are opened, switched, or closed, it clears when no file tab is open, and it expands collapsed ancestors so the highlighted row is visible.
- R6. Clicking a directory row's body expands or collapses it; clicking a file row's body opens that file. Moving the selection with the keyboard changes the highlight without opening anything, and `Return` performs the same activation as a click.
- R7. A pane header shows, in order of availability, the pane's Herdr label, its terminal title, its workspace name, then its pane id. A value consisting only of whitespace counts as absent.
- R8. The repository carries a committed Claude Code subagent definition that reviews a change for dead code and structural simplification, and `.gitignore` is anchored so that only the top-level harness namespace is ignored, with `AGENTS.md` updated to describe the anchored rule.
- R9. A pane header carries a close control, and carries a fork control only when the pane runs a detected Claude or Codex agent with a recorded session identity. Forking opens a sibling pane to the right running that agent resumed as a fork of the parent session, leaves the parent running, records the parent's pane id as Herdr pane metadata on the new pane, and shows a fork indicator in the new pane's header. A failure to start reports the reason and leaves no empty pane behind.
- R10. The right panel presents exactly two sections, the existing file explorer and a changes view, selectable from the panel header; the Workbench name is removed from the panel and its controls. The changes view lists the focused checkout's modified, added, deleted, and untracked files and shows the diff for a selected file. A checkout that is not a Git repository, or a Git failure, states its reason rather than rendering as "no changes".
- R11. `⌘=` and `⌘-` change the text size of the focused pane only, within a bounded range, and `⌘0` restores its default. A resized terminal pane reflows so no output is truncated. The chosen size is persisted per pane.
- R12. When a process whose working directory is at or below a pane's working directory is listening on a TCP port, the pane header shows that port; activating it opens `http://localhost:<port>` in Chrome, falling back to the default browser when Chrome is absent. The indicator disappears within one refresh window after the listener stops.
- R13. Clicking a link in a terminal pane routes by resolution, not by syntax: text that resolves to an existing file opens as a file tab whether or not it is inside the checkout; text that names a plausible web host, with or without a scheme, opens in Chrome; anything else produces a notice naming what could not be resolved and does not claim it was searched for inside the checkout.
- R14. `⌘F` in a focused pane reveals a search field at that pane's top right, highlights every match as the query is typed, moves to the next match on `Return` and the previous on `Shift-Return` while scrolling it into view, reports a query with no matches, and closes and clears highlights on `Escape`. The same flow searches the file text when a file tab is focused.
- R15. The Git and port capabilities added by R10 and R12 do not fork a subprocess in a per-tick or per-event path, do not hold the runtime mutex across a subprocess or blocking read, and publish their results through the snapshot's revisioned section rather than as per-event scalars.
- R16. After every other requirement is implemented and its verification passes, a simplification pass over this round's own change removes what the round made obsolete and reduces duplicated structure it introduced or exposed, with the full suite still green afterward.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | `⌘⇧B` changes the right panel's visibility and `⌘⌥B` changes nothing; no menu item, button help text, or label in the shell still names `⌘⌥B` | machine | - |
| AC2 | in a focused terminal pane `⌘←` and `⌘→` produce the byte sequences a terminal application reads as line-start and line-end, not word-back and word-forward | machine | - |
| AC3 | `.gitignore`, `.gitattributes`, `.env`, `.zshrc`, `CODEOWNERS`, `Procfile`, `Dockerfile.dev`, and `LICENSE` each resolve to an icon, and no such name resolves to an empty or missing glyph | machine | - |
| AC4 | closing the last pane leaves the process running with its window open, showing a centered empty state whose primary control creates a working pane | judged | scripted run in a throwaway workspace: open panes, close them one at a time to zero, capture the window, activate the empty state's control, capture the resulting pane |
| AC5 | the empty state's text describes only actions the shell will actually perform from that state | judged | the captured empty state from AC4, read against what the shell does when left alone for a full refresh interval |
| AC6 | the tree highlight matches the active file tab after opening, switching, and closing tabs, is absent when no file tab is open, and is visible without manual expansion when the file lives in a collapsed folder | judged | scripted run: open two files including one nested in a collapsed folder, switch tabs, close each tab, capture the tree at each step |
| AC7 | a click on a directory row's body toggles it, a click on a file row's body opens that file, and moving the selection with the arrow keys opens nothing | judged | scripted run: click a directory name, click a file name, then traverse four rows with the arrow keys and capture the tab strip |
| AC8 | the pane header shows the pane's Herdr label when present, its terminal title when the label is absent or blank, the workspace name when both are absent, and the pane id when all three are | machine | - |
| AC9 | the subagent definition file is present in the repository's committed file list at its `.claude/agents/` path | machine | - |
| AC10 | `.gitignore` ignores the top-level harness namespace and does not ignore `.claude/agents/`, and `AGENTS.md` describes the anchored rule | machine | - |
| AC11 | the fork control is present on a pane running a detected Claude or Codex agent with a session identity and absent on every other pane, including an agent pane with no recorded session | machine | - |
| AC12 | forking a running agent pane produces a sibling pane whose agent carries the parent conversation forward as a separate session, with the parent pane still running | judged | scripted run in a throwaway workspace: start an agent, give it one distinguishing exchange, fork, capture both panes showing that exchange present in each and the parent still live |
| AC13 | the forked pane's Herdr pane state records the parent pane's id, and Hide's header marks the pane as a fork | judged | the forked pane's Herdr pane read from AC12's run, alongside the captured header |
| AC14 | the right panel offers exactly the file explorer and the changes view, and the word Workbench appears in no user-visible string in the shell | machine | - |
| AC15 | the changes view lists a checkout's modified, added, deleted, and untracked files and shows a selected file's diff, and states a reason when the checkout is not a Git repository or Git fails | judged | scripted run: prepare a checkout with one modified, one added, one deleted, and one untracked file, capture the list and one file's diff, then repeat in a non-Git directory |
| AC16 | `⌘=` and `⌘-` change only the focused pane's text size within a bounded range, `⌘0` restores its default, a resized terminal reflows without truncation, and the size survives a restart | judged | scripted run: two terminal panes with wrapped output, resize one to each bound and back to default, restart the shell, capture before and after |
| AC17 | a pane whose working directory hosts a TCP listener shows that port in its header and activating it reaches `http://localhost:<port>` in a browser, and the indicator is gone within one refresh window after the listener stops | judged | scripted run: start a throwaway HTTP server on an unused port under a pane's working directory, capture the header, activate the indicator, capture the browser, stop the server, capture the header again |
| AC18 | clicking a full URL, a schemeless host, an in-checkout path, and an absolute path outside the checkout each reach their correct destination, and an unresolvable token produces a notice that names the token without claiming it was searched for inside the checkout | machine | - |
| AC19 | a web link activated from a terminal pane opens in Chrome when Chrome is installed | judged | scripted run: print a URL in a pane, click it, capture the frontmost application and its opened address |
| AC20 | `⌘F` in a focused pane reveals a search field in that pane, highlights every match, advances and reverses through matches with the scrolled position following, reports a query with no matches, and clears on `Escape`, for both a terminal pane and a file tab | judged | scripted run: search a term with several matches including one off-screen in a terminal pane, then repeat in a file tab, capturing highlight, advance, no-match, and dismissal |
| AC21 | the Git and port capabilities perform no subprocess execution while the runtime mutex is held and no subprocess execution on a per-event or per-tick path | machine | - |
| AC22 | after the simplification pass the full build and the full automated suite pass, and the change removes rather than adds parallel implementations of what this round touched | judged | the simplification pass's own diff read against the round's diff, plus the suite result |

## 8. PRD-Level Tasks

- T1. Bind the right-panel toggle to `⌘⇧B`, remove the `⌘⌥B` binding, and remove every label that advertises the old chord. Covers R1, AC1. Depends on: none.
- T2. Route line-start and line-end from the Cmd-arrow keys in the shell's own terminal view, leaving the file editor's stock behavior intact. Covers R2, AC2. Depends on: none.
- T3. Extend file-icon resolution to cover names with no extension and names beginning with a dot, with one deliberate generic fallback. Covers R3, AC3. Depends on: none.
- T4. Reproduce the reported termination after the last pane closes and record what actually happens, then make the shell survive it and give it an empty state with a working pane-creation control and honest copy. Covers R4, AC4, AC5, SC4. Depends on: none.
- T5. Make the tree highlight follow the active file tab, clear when none is open, and reveal collapsed ancestors. Covers R5, AC6, SC5. Depends on: none.
- T6. Bind tree activation to a click on the row body and to `Return`, and unbind it from selection change. Covers R6, AC7, SC6. Depends on: none.
- T7. Carry the Herdr pane label and terminal title through the local pane projection and render the pane header from the four-step priority. Covers R7, AC8, SC7. Depends on: none.
- T8. Add the committed simplification subagent definition, anchor the harness ignore rule, and update the repository's agent notes to match. Covers R8, AC9, AC10. Depends on: none.
- T9. Add the pane header's close and fork controls, gate fork on a detected agent with a session identity, start the forked sibling through each agent's own fork command, record the parent lineage as Herdr pane metadata, and mark the forked pane in its header. Covers R9, AC11, AC12, AC13, SC8. Depends on: T7.
- T10. Add the Git status and diff capability to the core and present the right panel as an explorer section and a changes section, removing the Workbench name. Covers R10, R15, AC14, AC15, AC21, SC9. Depends on: none.
- T11. Add per-pane view scale to the core's persisted UI state and bind the zoom chords to the focused pane, applying to both the terminal and the file editor with terminal reflow. Covers R11, AC16, SC10. Depends on: none.
- T12. Add cached listening-port attribution to the core, show the ports in the pane header, and open the selected one in Chrome with a default-browser fallback. Covers R12, R15, AC17, AC21, SC11. Depends on: T9.
- T13. Re-route terminal link activation by resolution rather than syntax, allowing files outside the checkout and schemeless hosts, and open web targets in Chrome. Covers R13, AC18, AC19, SC12. Depends on: none.
- T14. Wire the existing terminal and editor find surfaces to a per-pane `⌘F`, restyled to the shell's design tokens. Covers R14, AC20, SC13. Depends on: none.
- T15. Prepare the verification fixtures this round needs: a throwaway Herdr workspace, a checkout state with one modified, one added, one deleted and one untracked file, a throwaway listening server, and a fixed set of link forms printed into a pane. Covers SC4, SC8, SC9, SC11, SC12. Depends on: none.
- T16. Run the simplification pass over this round's change and confirm the full suite still passes. Covers R16, AC22. Depends on: T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | repo health across the Rust core and the Swift shell | none |
| automated behavior | yes | resolver, policy, projection, and routing regressions | none |
| app runtime | yes | every user-visible flow, against the assembled dev bundle | final UX judgment |
| live herdr integration | yes | fork lineage and pane lifecycle against the running Herdr server | isolation boundary already approved in this PRD |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R16 | the Rust core and the Swift shell both build clean, so no requirement lands as an unbuildable change | yes | no |
| V2 | automated behavior | R1, R2, R3, R7, R13, AC1, AC2, AC3, AC8, AC11, AC18 | the pure resolvers and policies this round rewrites - shortcut claiming, key-to-byte routing, icon resolution, title priority, fork-control gating, and link routing - are covered by tests that fail if the mapping regresses, which is the class of defect every one of these items already was | yes | no |
| V3 | automated behavior | R8, R10, R15, AC9, AC10, AC14, AC21 | the committed-file, ignore-rule, user-visible-string, and no-subprocess-under-lock conditions are asserted mechanically, protecting against the asset silently becoming uncommittable again and against a cached capability regressing into a per-tick fork | yes | no |
| V4 | app runtime | R1, R2, R11, R14, AC16, AC20, SC1, SC2, SC10, SC13 | the keyboard surfaces work in the running app: panel toggle, line navigation, per-pane zoom with reflow and persistence, and per-pane find with highlight, advance, no-match, and dismissal | yes | no |
| V5 | app runtime | R3, R5, R6, R10, AC6, AC7, AC15, SC3, SC5, SC6, SC9 | the explorer and right panel behave in the running app: icons render, the highlight tracks and clears and reveals, row-body clicks activate while arrow keys do not, and the changes view lists all four change kinds, shows a diff, and states its reason on a non-Git checkout | yes | no |
| V6 | app runtime | R4, R7, R12, AC4, AC5, AC17, SC4, SC7, SC11 | the pane surface behaves in the running app: the process survives closing every pane and offers a working empty state with honest copy, headers show the right name at each priority step, and a port indicator appears, opens a browser, and disappears | yes | no |
| V7 | live herdr integration | R9, AC12, AC13, SC8 | forking a live agent pane produces a sibling carrying the parent conversation as a separate session with the parent still running, and the lineage is readable from Herdr's own pane state after the fact | yes | no |
| V8 | app runtime | R13, AC19, SC12 | clicking each link form in a live pane reaches its correct destination - Chrome for web and schemeless hosts, a file tab for in-checkout and outside-checkout paths, a naming notice for an unresolvable token | yes | no |
| V9 | build/static | R16, AC22 | after the simplification pass the build and the full automated suite still pass, so the cleanup did not trade correctness for tidiness | yes | no |

Side-effect boundary for the live mode:

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V7 | live herdr integration | R9, AC12, AC13, SC8 | forking a live agent pane produces a sibling carrying the parent conversation as a separate session with the parent still running, and the lineage is readable from Herdr's own pane state after the fact | yes | no | create and later close one throwaway Herdr workspace, its panes, and the agent sessions started inside it; never close, move, rename, or prompt any pane, tab, or workspace this run did not create | capture no agent conversation content beyond the single synthetic exchange this run itself sends; redact paths outside the checkout and any token or credential visible in captured output |

### 9.3 Human Verification

- Final visual judgment on the new surfaces: the pane header's control row, the right panel's two-section header, the search field, and the empty state, judged against `DESIGN.md` rather than against a screenshot's plausibility.
- Confirmation that removing `⌘⌥B` in favor of `⌘⇧B` matches the operator's muscle memory, since only they can say.
- Confirmation that the port-attribution rule - a listener whose process working directory is at or below the pane's - matches the servers they actually run.

## 10. Risks And Open Decisions

- Reproducing the reported termination may show a cause outside this repository, since no termination path exists in it and the window is created programmatically with its release-on-close behavior left at the AppKit default while the delegate also retains it. T4 records what is actually observed before any fix is written; if the cause is genuinely external, the empty state and survival requirements still stand and the finding is reported.
- Forking starts a real agent session and consumes the operator's account budget. Bounded to one throwaway workspace this run creates and closes, with a single synthetic exchange.
- Live verification runs against the operator's own Herdr server while they are away. The isolation boundary in V7 is the mitigation, and section 11 states it as a prohibition.
- Port attribution by working directory will miss servers started from elsewhere. Recorded as a non-goal with its consequence rather than widened speculatively.
- A URL wrapped across rows inside an agent's bordered TUI box can still resolve to only its tail, because the row-join heuristic lives in vendored SwiftTerm. Deferred with its consequence in section 3; the schemeless-host routing in R13 already fixes the common case where the tail is itself a plausible host.
- The verification screenshots are numerous and live under `agents/runs/hide-ux-round4/`, which is local-only by policy. The receipt, not the repository, is where they are proven.

## 11. Implementation Guardrails

From the operator's own instructions and this repository's notes:

- Never close, move, rename, prompt, or otherwise disturb a Herdr pane, tab, or workspace this run did not create, and never interact with the operator's running Hide instance. Verification uses its own throwaway workspace and the worktree's own bundle identity.
- Do not expand scope beyond section 6, do not change major architecture beyond section 5, and do not add a third-party dependency.
- Do not touch production data, secrets, or credentials, and do not perform any destructive or irreversible action beyond the bounded side effect declared in V7.
- Do not add hidden user flows; every new control is one of the ones section 6 names.
- Herdr integration follows `AGENTS.md` "Herdr API Contract": read the official CLI and Socket API references for the shipped version, keep the documented layer boundary (CLI wrappers for agent lifecycle and CLI-owned commands, the raw socket for custom request/response control and subscriptions), and confirm any new method, parameter, or field against `contracts/herdr-api.schema.json` through `scripts/check-herdr-contract.sh` rather than inferring it from existing call sites.
- Performance, from `AGENTS.md` "Performance Guide": never hold the runtime mutex across a subprocess, blocking I/O, or a large serialization; never fork a subprocess in a per-tick or per-event path; size the snapshot wire by what changed, putting rarely-changing sections in the revisioned section; verify any performance claim by sampling the running process rather than by reading code.
- Evidence, from `AGENTS.md` "Evidence Belongs Outside The Repository": every screenshot, trace, and run log lives under `agents/runs/hide-ux-round4/` and none of it is committed. Do not force-add a run directory to make a path linkable.
- Design, from `AGENTS.md` "Design Reference" and `DESIGN.md`: no new color, radius, or spacing value is written at a call site; additions go to `HideTheme` and are used from there. If the existing system does not cover a case, say so and propose the addition rather than settling it inline.
- engineering/principles.md rule 1: delete what this change makes obsolete in the same change - the old `⌘⌥B` binding and its labels, the global font-size value the new per-pane scale replaces, and the single-file diff helper the new Git capability replaces all go, with no compatibility path left behind.
- engineering/principles.md rule 2: choose the simplest implementation that fully meets the requirement; do not build a general mechanism where the requirement names a specific behavior.
- engineering/principles.md rule 3: grow in layers - the changes view ships its file list and its per-file diff in that order, and each item lands end to end before the next begins.
- engineering/principles.md rule 4: surface failures explicitly - a Git failure, a fork that will not start, a glyph font that will not register, and a link that resolves to nothing each state their reason; none defaults, empties, or silently skips.
- engineering/principles.md rule 5: keep concerns separated - link resolution, port attribution, Git reading, and pane projection stay distinct from the views that render them.
- engineering/principles.md rule 6 and rule 7: use what exists before writing a parallel implementation - the vendored terminal's find bar and the editor's find bar rather than a new search overlay, the bundled glyph set rather than a new icon dependency, the core's existing cache-by-input-equality pattern rather than a second caching scheme.
- engineering/principles.md rule 8: the per-pane scale, the pane title field, and the Git capability are long-term shapes, not stopgaps to be replaced next round.
- engineering/principles.md rule 10: every failure this round can produce is observable from outside the process, through the shell's existing structured stderr reporting or a visible notice.
- engineering/principles.md rule 13: fix the class - the ignore rule is anchored rather than the one file force-added, and link routing is decided by resolution rather than by patching one more scheme into an allowlist.
- design/principles.md rule 3: the most frequent action takes the fewest clicks - fork and close are one activation from the pane header, and the port opens in one.
- design/principles.md rule 4: show derived state - the port indicator, the fork marker, and the changes list each show state the operator would otherwise have to go and compute.
- design/principles.md rule 5: follow the product's existing patterns - the new controls reuse the header, panel, and row idioms already in the shell.
- design/principles.md rule 6: state the consequence before a destructive action - the pane close control follows the shell's existing close semantics and does not silently discard a running agent without the shell's established treatment.
- design/principles.md rule 7: encode state and structure visually - the fork marker and the port indicator are visual, not sentences.
- Git and PR attribution: no agent, model, vendor, or tool name appears in any branch name, commit message, trailer, or generated text.

## 12. Implementation Result Report Contract

Report:

- status: `Done`, `Partially Done`, or `Blocked`.
- user-visible changes, item by item against the fourteen reported problems.
- changed modules and the responsibility boundary each new one owns, plus the actual file structure chosen.
- whether the approved technical structure in section 5 was followed, and any deviation with its reason.
- task completion status for T1 through T16 and R/AC/V coverage.
- verification evidence by mode, with the run directory each artifact lives in.
- for T4 specifically: what was actually observed when the last pane closed, before the fix.
- for V7 specifically: which Herdr workspace, panes, and agent sessions the run created, and confirmation that every one of them was closed and that nothing else was touched.
- automated tests added or updated, and the regression each protects.
- the Implementor rotations that occurred, if any, and what each successor inherited.
- the simplification pass's own result: what it removed, and the suite result after it.
- deviations, remaining human review, not-done items, and follow-up candidates.
