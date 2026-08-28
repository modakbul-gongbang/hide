---
topic: "herdr-ide-shell-ux"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Local-only desktop shell UX rework - it rewrites layout, input routing, terminal grid math, and adds local file editing, with no auth, network, production data, or irreversible external effect."
source_intake: "current conversation"
created_at: "2026-08-28"
updated_at: "2026-08-28"
---

# PRD: herdr-ide-shell-ux

## 1. Summary

Herdr IDE renders as a native macOS shell today, but its interaction surfaces are not usable work surfaces.
The sidebar is a single unstyled text blob with no hit testing, `cmd+D` submits a split to Herdr without ever showing a second pane in the window, the terminal grid is computed from hardcoded 9x18 pixel cells while the renderer draws a different metric, and the file tree and editor services exist in `src/files.rs` but are wired to nothing.

This PRD replaces those four surfaces with one structured frame model that render, hit testing, and accessibility all read from: a sectioned clickable sidebar, a real recursive split tree, a single terminal cell metric, and a live file tree plus text editor for the selected workspace.
It also removes the one Option-modified global shortcut the user no longer wants.

Approval checklist:

- Scope boundary: five surfaces in priority order (sidebar, split rendering, terminal grid, Option shortcut removal, file tree/editor) and the recorded non-goals - section 3.
- Structure change: a shared structured frame model plus a recursive pane tree replacing today's fixed two-column `CanvasGeometry` - section 5.
- Assumption to veto: "option + f" is read as the app's only Option-modified global shortcut (`⌥Tab`, Show workspace status) and that feature is removed - section 4.3.
- Editing depth: the editor opens, edits, and saves local text files with an explicit dirty and error state; remote workspaces show an unavailable state instead of a tree - sections 3 and 6.
- Verification modes: cargo build/static, cargo automated behavior tests, and screenshot-backed app-runtime proof of the real release bundle - section 9.
- Delivery mode: local (no branch, no PR, no push), per `agents/config.json` - section 4.3.

## 2. Problem, Goal, And Users

The user is the developer running Herdr IDE as their daily driver next to the Herdr CLI.
They drive agents, terminals, and files from one window.

Today that window fails them in five concrete ways they reported after running the release bundle:

- The Workspaces and Agents lists render as indented plain text with no visual separation, no consistent row shape, and no reaction to a click.
- `cmd+D` (split right) reports a topology change but the window keeps showing exactly one terminal, so a split looks like it opened somewhere else.
- Scrolling the terminal only moves part of the surface and the bottom of the grid is unreachable, because the PTY believes it has more rows than the renderer draws.
- An Option-modified global shortcut is registered that they do not want.
- No file tree exists for the selected workspace, and the editor pane is a permanent "No file open" placeholder.

The goal is a shell where the sidebar is a real navigable list, a split is visible where it was made, the terminal is a correct grid, and files can be browsed, read, edited, and saved.

### 2.1 User Scenarios

- SC1. Navigate from the sidebar: the user scans the sidebar, sees Workspaces and Agents as visually distinct sections with uniformly shaped rows, and clicks a workspace row to select it and an agent row to focus that agent's pane.
  Actors: the developer.
  Primary path: clicking a row selects it, the selection is visibly encoded (not only by a text marker), and clicking a disclosure control expands or collapses that node and survives an app restart.
  Failure state: a row whose target no longer exists in the Herdr snapshot reports the failure in the status area instead of silently doing nothing.
  Recovery: after the next Herdr snapshot the stale row disappears and the equivalent live row is clickable.
  Reach: a running Herdr session with at least one workspace and one agent, which is the normal launch state of this app.

- SC2. Split a pane: with a terminal focused the user presses `cmd+D` and the pane area divides, showing both panes side by side in the same tab.
  Actors: the developer.
  Primary path: the focused pane splits right, both panes are visible and separately focusable by click, `cmd+shift+D` splits down, and each split is reflected in the Herdr topology.
  Failure state: when Herdr rejects the topology change the window does not show a phantom pane and the rejection is visible in the status area.
  Recovery: the previous layout stays intact and a later successful split renders normally.
  Reach: the app's default launch state with one terminal pane focused.

- SC3. Read a long terminal session: the user scrolls a terminal holding more output than the window can show.
  Actors: the developer.
  Primary path: the whole pane area is filled by the grid, the wheel scrolls the grid by whole lines, the top of the scrollback is reachable, and the newest line is reachable again by scrolling back down.
  Failure state: a terminal that fails to scroll or snapshot reports that failure visibly rather than freezing at a stale frame.
  Recovery: the next successful snapshot restores live rendering.
  Reach: any terminal pane after enough output to exceed one screen.

- SC4. Browse and edit a file: the user picks a workspace, opens its file tree in the sidebar, clicks a file, edits it, and saves.
  Actors: the developer.
  Primary path: directories expand and collapse, clicking a file opens its content in the editor pane with the file path shown, typing modifies the buffer with a visible unsaved marker, and saving clears the marker and writes the file.
  Failure state: a file that cannot be read, is not valid UTF-8 text, is too large, or fails to save shows the reason in the editor pane and leaves the buffer intact.
  Recovery: the user can pick another file, or retry the save after fixing the cause, without restarting the app.
  Reach: a local workspace selected in the sidebar; a remote workspace shows an explicit unavailable state instead of a tree.

- SC5. Removed shortcut: the user presses the removed Option-modified global shortcut and nothing happens.
  Actors: the developer.
  Primary path: the shortcut is absent from the command registry, from settings, and from the shortcut list, and no global registration is requested for it at launch.
  Failure state: not applicable - the absence is the outcome.
  Recovery: not applicable.
  Reach: the launched app with default settings and with a previously saved settings file that still contains the removed command.

## 3. Scope And Non-Goals

In scope:

- One structured frame model that render, hit testing, and the accessibility tree all read from, replacing the text-blob `FrameModel` fields for the sidebar and editor surfaces.
- Sidebar: Workspaces, Agents, and Files as visually separated sections with a single uniform row shape (fixed row height, indent step, leading icon, label, trailing state), hover and selection encoded visually, click and disclosure hit testing, and persisted expansion state.
- Pane layout: a recursive split tree per tab with visible dividers, `cmd+D` / `cmd+shift+D` splitting the focused pane in place, click-to-focus per pane, divider drag to resize, and zoom continuing to work against the tree.
- Terminal: one cell metric derived from the actual shaped monospace font at the current backing scale, used by the PTY grid, the renderer, hit testing, and wheel scrolling; the grid fills its pane rect and scrolls by whole lines across the full scrollback.
- Removal of the Option-modified global shortcut and its command, its settings entry, its global registration, and its overlay entry point, with no compatibility shim left behind.
- File tree and editor: `LocalFileService` and `FileTreeState` wired to the selected workspace root, file open, text editing, save, dirty state, and explicit read/save failure states.
- Structured, event-named diagnostics for every new failure path (topology rejected, snapshot failed, file read/save failed) surfaced in the status area, not only in stderr.

Non-goals, each an explicit decision:

- Remote workspace file trees over SFTP. Consequence: selecting a remote workspace shows an explicit "file tree unavailable for remote workspaces" state instead of files. Rationale: `RemoteFileService` needs a live SSH transport whose failure modes are a separate product surface. Revisit when remote workspaces become the primary daily target.
- Editor features beyond plain-text viewing, editing, and saving: no syntax highlighting, no multi-file tabs, no search-and-replace, no undo history, no diff view. Consequence: the editor is a correct plain-text editor, not a code editor. Rationale: the request is "files can be viewed and edited"; each of these is a separable layer on top of a working editor. Revisit after the editor is in daily use.
- Sidebar drag-and-drop, context menus, and rename/delete/create file operations. Consequence: file mutation stays limited to editing and saving an existing file. Rationale: destructive file operations need their own consequence-stating UI. Revisit with a dedicated pass.
- Restyling the tab bar, status bar, browser pane, and pet surfaces beyond what the shared row and section tokens require. Consequence: those surfaces keep today's appearance. Rationale: keeps this pass to the four reported surfaces.
- Theme customization or a settings UI for the new visual tokens. Consequence: one built-in dark palette. Rationale: no request for theming; a token layer is enough to keep the surfaces consistent.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
Every input is local source, a local Herdr session, and the local release bundle build already used in this repository; nothing needs the user's identity, credentials, or purchase.

### 4.2 Human Decisions Before PRD Approval

None required.
Scope, structure, verification modes, and local delivery all follow from the reported defects and `agents/config.json`; the one genuinely uncertain item ("option + f") is recorded as an agent-owned reversible assumption in 4.3 rather than a blocking question, because reverting a shortcut removal is a one-commit change.

### 4.3 Decision Traceability For Fidelity Review

- User decision: the five reported problems are implemented in the stated priority order (sidebar, split rendering, terminal, shortcut removal, file tree/editor). Represented in: section 8 task order.
- User decision: the sidebar should look like the reference screenshot the user attached - visually separated Spaces and Agents sections, uniform row design, clickable rows. Represented in: R1, R2, AC1-AC3, SC1.
- User decision: splits must divide the visible pane area in the IDE window, not open somewhere that looks like a new tab, while Herdr itself keeps receiving the topology change. Represented in: R4, AC4-AC6, SC2.
- User decision: the terminal view is unacceptable today; scrolling only covers part of the space. Represented in: R6, R7, AC7-AC9, SC3.
- User decision: the unwanted Option shortcut is removed if it exists in settings. Represented in: R8, AC10, SC5.
- User decision: the selected workspace shows a file tree, and files can be viewed and edited. Represented in: R9-R12, AC11-AC15, SC4.
- Agent assumption (reversible, veto-able): the user's "option + f" refers to the app's only Option-modified shortcut, `⌥Tab` bound to "Show workspace status" (`CommandId::WorkspaceStatus`, `src/commands.rs`), including its `AGENT SWITCHER (Option+Tab)` overlay in `src/pet.rs`. No `⌥F` binding exists in the codebase. That command, its binding, its global registration, and its overlay entry point are removed. Represented in: R8, AC10, SC5.
- Agent assumption (reversible): the editor is a plain-text editor with save, because "볼 수 있는 에디팅" states viewing and editing without naming code-editor features. Represented in: R10-R12 and the editor non-goals.
- Agent assumption (reversible): the file tree roots at the selected workspace's local path, and a workspace marked remote shows an explicit unavailable state. Represented in: R9 and the remote non-goal.
- Agent assumption (reversible): divider drag-to-resize is included, because a split tree without resizing is unusable past two panes. Represented in: R5, AC6.
- Delivery decision: delivery mode is `local`, taken from `agents/config.json`; no branch, commit, push, or PR is part of this run. Represented in: context-only fact, restated in section 12.
- Rejected alternative: keeping the text-blob frame model and adding a parallel hit-test table. Rejected because two sources of geometry is exactly the class of bug behind the current terminal defect (engineering principle 13).
- Rejected alternative: fixing the terminal by tuning the hardcoded 9x18 cell constants. Rejected as instance-fixing; the metric is derived once from the shaped font instead (engineering principle 13).
- Principles intake: read `engineering/principles.md` and `design/principles.md` in full at `oh-my-principle` commit `f03e930e8c5ad8c250a24d7f70be8c4889c2d6ca`, plus `engineering/practices/test.md`. Every applicable rule is translated in section 11. Design rule 6 (state the consequence before a destructive action) is translated as a guardrail only, because this scope has no destructive user action; design rule 1's card-grid branch does not apply because every list here is compared by field, not recognized by image.
- Context-only fact: the user directed that implementation run in a Codex "luna max" implementor session; that is an execution-routing instruction for the pipeline dispatch, not a product requirement, so it shapes no requirement, task, or verification row.
- Context-only fact: the app is verified against `dist/Herdr IDE.app` built by `scripts/build-app.sh`, and exactly one instance must run during visual verification (`docs/dev-runtime.md`).

## 5. Major Technical Structure Changes

- Replace the fixed two-column `CanvasGeometry` with a recursive per-tab pane tree (horizontal/vertical split nodes with ratios, leaves bound to stable pane ids) that owns pane rects, divider rects, hit testing, and zoom. This is the layout data flow for the whole window.
- Introduce one structured frame model boundary between application state and the renderer: the model carries positioned, styled primitives (sections, rows, icons, badges, dividers, text areas, terminal grid) and the hit regions that correspond to them. Render, mouse hit testing, and the accessibility tree read the same model, so a drawn element and a clickable element cannot drift apart.
- Introduce one terminal cell metric derived from the shaped monospace font at the current backing scale factor, and make it the only source for the PTY grid size, glyph placement, background rects, mouse cell hit testing, and wheel scroll steps. The hardcoded `cell_width = 9` / `cell_height = 18` constants in `src/pty.rs` are deleted.
- Bind the existing but unwired `FileService` and `FileTreeState` modules into the application state as the file surface's domain layer, with the UI reading a projection rather than calling the filesystem directly.
- Delete the `WorkspaceStatus` command and its overlay entry point rather than hiding it behind a flag.

No API, schema, migration, infrastructure, auth, payment, or external-service boundary changes.
Persistent state stays as it is today: local JSON preference files for navigator expansion, shortcuts, and file tree expansion.

## 6. Requirements

- R1. The sidebar renders Workspaces, Agents, and Files as visually separated sections, each with a section header, and every row in every section uses one shared row shape: fixed row height, fixed indent step per depth, leading icon, label, and trailing state slot.
- R2. Every sidebar row is a hit target: clicking a row selects and activates it, clicking its disclosure control toggles expansion without activating it, hover and selection are encoded visually rather than by a text marker, and expansion state persists across restarts.
- R3. Sidebar row activation performs the row's action: a workspace row selects that workspace (setting the file tree root), a tab or pane row focuses that pane, an agent row focuses that agent's pane, and a file row opens that file in the editor. A failed action is reported in the status area with a structured event name.
- R4. `cmd+D` splits the focused pane to the right and `cmd+shift+D` splits it down inside the current tab; both new panes are rendered simultaneously in the tab's pane area, separated by a visible divider, and the same topology change is submitted to Herdr.
- R5. Panes in a split are independently focusable by click, focus is visually encoded, dividers can be dragged to change the split ratio within readable minimum sizes, and pane zoom continues to restore the exact pre-zoom tree and focus.
- R6. The terminal grid size, glyph placement, cell background rects, pointer cell hit testing, and wheel scroll steps are all derived from one cell metric measured from the shaped monospace font at the current backing scale factor.
- R7. The terminal grid fills its pane rect: the last row and last column are drawn inside the rect, wheel scrolling moves the view by whole lines, the oldest available scrollback line is reachable, and returning to the bottom shows the newest line.
- R8. The Option-modified global shortcut command is absent from the command registry, the shortcut list, the settings file schema handling, and the global registration performed at launch; a settings file that still contains it loads without error and without resurrecting the command.
- R9. Selecting a local workspace populates a Files section rooted at that workspace path, showing directories and files sorted directories-first and alphabetically, with expansion persisted; a remote workspace shows an explicit unavailable state instead of a tree.
- R10. Clicking a file opens it in the editor pane, which shows the file path and the file content; a file that is unreadable, non-UTF-8, or over the size limit shows the reason instead of content.
- R11. The editor pane accepts text input, insertion, deletion, and newline into the open document, shows an unsaved-changes marker whenever the buffer differs from the file on disk, and scrolls to keep the caret visible.
- R12. Saving the open document writes it to disk, clears the unsaved marker, and reports a failed save with its reason in the editor pane while keeping the buffer contents intact.
- R13. Every new failure path (topology rejected, terminal snapshot or scroll failure, file list, read, or save failure) emits one structured `event=<name>` diagnostic line and is visible in the app's status area, not only in stderr.

## 7. Acceptance Criteria

- AC1. The Workspaces, Agents, and Files sections are distinguishable at a glance by section header and separation, and rows in different sections share the same height, indent step, and internal alignment.
- AC2. Clicking any sidebar row changes the visible selection to that row, and clicking a disclosure control expands or collapses its subtree without changing the selection.
- AC3. Expansion and selection made in the sidebar are still in effect after quitting and relaunching the app.
- AC4. After `cmd+D` in a single-pane tab, the tab's pane area shows two panes side by side, each with content, separated by a visible divider; after `cmd+shift+D` the panes are stacked vertically.
- AC5. Clicking inside a pane moves focus to that pane, focus is visually distinguishable from unfocused panes, and pane zoom returns to the exact previous layout and focus.
- AC6. Dragging a divider changes the two adjacent panes' sizes and neither pane can be dragged below its readable minimum.
- AC7. The terminal grid's bottom row and rightmost column are fully inside the terminal pane rect at both 1x and 2x backing scale, with no clipped final row.
- AC8. In a terminal whose output exceeds one screen, scrolling up reaches the oldest available scrollback line and scrolling down returns to the newest line, moving in whole-line steps.
- AC9. The number of rows and columns the shell process is told about matches the number of rows and columns actually drawn in the pane.
- AC10. The removed Option shortcut triggers nothing, does not appear in the shortcut list, and a settings file that still contains its entry loads without error.
- AC11. Selecting a local workspace shows its top-level directories and files in the Files section, directories first and alphabetically ordered; selecting a remote workspace shows an explicit unavailable state.
- AC12. Clicking a file shows its path and content in the editor pane.
- AC13. Typing in the editor changes the visible content and raises an unsaved-changes marker.
- AC14. Saving clears the unsaved marker and the new content is present in the file on disk.
- AC15. An unreadable file, a non-UTF-8 file, an oversized file, and a failed save each show a specific reason in the app rather than an empty or unchanged pane.

## 8. PRD-Level Tasks

- T1. Introduce the structured frame model and its hit-region contract, and move the existing surfaces onto it so render, hit testing, and the accessibility tree read one source. Covers R1, R13.
- T2. Build the sectioned sidebar row model, its shared visual tokens, and its rendering. Covers R1, AC1. Depends on: T1.
- T3. Implement sidebar hit testing, hover, selection, disclosure toggling, persisted expansion, and row activation actions. Covers R2, R3, AC2, AC3. Depends on: T2.
- T4. Replace the fixed canvas geometry with the recursive pane tree, and route `cmd+D` / `cmd+shift+D`, click-to-focus, zoom, and the Herdr topology submission through it. Covers R4, R5, AC4, AC5. Depends on: T1.
- T5. Render split dividers and implement divider drag resizing with minimum pane sizes. Covers R5, AC6. Depends on: T4.
- T6. Derive one terminal cell metric from the shaped monospace font at the current backing scale and make the PTY grid, glyph placement, background rects, pointer hit testing, and wheel scrolling use it; delete the hardcoded cell constants. Covers R6, R7, AC7, AC8, AC9. Depends on: T4.
- T7. Remove the Option-modified global shortcut command, its binding, its settings handling, its global registration, and its overlay entry point, and keep old settings files loadable. Covers R8, AC10. Depends on: none.
- T8. Wire the file service and file tree state into application state and populate the Files section for the selected workspace, including the remote-workspace unavailable state. Covers R9, AC11. Depends on: T3.
- T9. Implement the editor pane: open a file, show path and content, report read failures, and keep the caret visible while scrolling. Covers R10, AC12, AC15. Depends on: T8.
- T10. Implement editing, the unsaved-changes marker, and saving with failure reporting. Covers R11, R12, AC13, AC14. Depends on: T9.
- T11. Add automated regression coverage for the pane tree, the shared hit-region contract, the cell metric derivation, the sidebar row model, and the file/editor document state transitions. Covers R4, R6, R9, R11.
- T12. Capture app-runtime evidence from the built release bundle for the sidebar, split, terminal, and editor surfaces. Covers AC1-AC15. Depends on: T10.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | release build and lint health of the whole crate | none |
| automated behavior | yes | layout, hit-region, cell metric, sidebar row, and document state regressions | none |
| app runtime | yes | the five reported user-visible surfaces in the real bundle | final visual taste judgment |

`automated behavior` is required for done: every defect in this PRD is a pure geometry, state, or model defect that a Rust unit test can hold, and the repository already tests layout and terminal behavior this way.
`app runtime` replaces the usual browser mode because this is a native macOS application with no browser surface; its evidence is a screenshot of the running signed bundle.

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R13 | the crate builds in release with the workspace lint settings and no new warnings, proving no surface was left half-migrated | yes | no |
| V2 | automated behavior | R4, R5, R6, R7, AC4, AC7, AC8, AC9 | pane-tree splitting, focus, zoom restoration, divider constraints, and the single cell metric are covered by tests that fail if geometry drifts between the PTY grid and the drawn grid again | yes | no |
| V3 | automated behavior | R1, R2, R3, R9, R10, R11, R12, AC2, AC11, AC13, AC14, AC15 | sidebar row and hit-region construction, file tree projection, and editor document open/edit/save/failure transitions are covered by tests that fail if a drawn row loses its hit region or a failure path silently degrades to an empty state | yes | no |
| V4 | automated behavior | R8, AC10 | the removed command is absent from the registry and a settings file still containing it loads without error | yes | no |
| V5 | app runtime | SC1, SC2, SC3, SC4, SC5, AC1, AC3, AC4, AC5, AC6, AC7, AC8, AC12, AC13, AC14 | the built and signed bundle, running as the only instance, visibly shows the sectioned clickable sidebar, an in-place split, a full-height scrolling terminal, and a file opened, edited, and saved from the tree | yes | no |

### 9.3 Human Verification

- Final visual taste judgment on the sidebar against the reference screenshot: spacing, weight, icon and badge treatment, and whether the sections read as clearly separated.
- Confirmation of the "option + f" assumption in 4.3: whether removing `⌥Tab` / "Show workspace status" is the removal the user meant.

## 10. Risks And Open Decisions

- The "option + f" reading is an assumption. If wrong, the correct fix is a one-commit revert plus removing the shortcut the user actually meant; no data or external effect is involved.
- Replacing the fixed geometry with a pane tree touches render, input, accessibility, and PTY sizing at once. Mitigation: the frame model and hit-region contract land first (T1) so the later tasks move onto an already-tested boundary.
- Font metric measurement can differ between the shaped monospace fallback fonts available on a machine. Mitigation: the metric is measured from the same shaped font the renderer uses, so the PTY grid and the drawing can only agree or fail together, and V2 holds that agreement.
- Editing files is a real write to the user's disk. Mitigation: writes happen only on an explicit save of an explicitly opened file, through the existing validated-path file service, and failures keep the buffer.
- Deferred decision: remote workspace file trees, code-editor features, and file mutation operations, each recorded as a non-goal in section 3 with its revisit condition.

## 11. Implementation Guardrails

- Do not expand scope beyond section 3, and do not add unapproved services, schemas, jobs, dependencies, or external calls.
- Do not implement remote SFTP browsing, syntax highlighting, multi-file tabs, undo history, or file create/rename/delete in this run.
- Do not touch credentials, secrets, or any path outside the selected workspace root; the file service's relative-path validation stays the only way files are addressed.
- Do not create a branch, commit, push, or pull request: delivery mode is `local`.
- engineering/principles.md rule 1: do not keep the old text-blob surface, the old fixed geometry, or the removed shortcut behind a flag, fallback, or compatibility shim; update every consumer and delete the old path.
- engineering/principles.md rule 2: do not add configuration, plugin points, or abstraction for surfaces this PRD does not require, such as theming or pluggable editors.
- engineering/principles.md rule 3: keep the app buildable and runnable at each task boundary; never leave the window in a non-working state between tasks.
- engineering/principles.md rule 4: never cover a failed file read, save, snapshot, or topology submission with an empty string, a default value, or a silent skip.
- engineering/principles.md rule 5: keep the domain layers separated - the file service and terminal own their domains, the frame model owns presentation, and the view does not read the filesystem or the terminal grid directly.
- engineering/principles.md rules 6 and 7: use the crate's existing dependencies (`alacritty_terminal`, `glyphon`, `portable-pty`, `objc2`) and the repository's existing `files.rs` and `navigator.rs` modules rather than adding packages or re-implementing them.
- engineering/principles.md rule 8: the pane tree, the frame model, and the cell metric are the long-term structures; do not lay a temporary geometry that is meant to be replaced later.
- engineering/principles.md rules 9 and 10: every new failure path emits a structured `event=<name> ...` line with the identifiers needed to locate it and is visible in the app's status area, and a no-op outcome (an empty workspace list, an empty directory) is reported as its own state rather than as an empty screen.
- engineering/principles.md rule 11: repeating an operation converges - reopening the same file, re-saving unchanged content, re-splitting, and re-applying the same Herdr snapshot must not duplicate panes, rows, or documents.
- engineering/principles.md rule 12 and practices/test.md: write a test only where a plausible regression would fail it; assert observable outcomes (rects, grid sizes, row and hit-region sets, document state) rather than internal call wiring, and mock nothing that this crate owns.
- engineering/principles.md rule 13: fix the class - one cell metric, one hit-region source, one layout tree - rather than patching the specific reported symptom.
- design/principles.md rule 1: the sidebar sections stay read views compared by field, so they are row lists with a state column, never always-open forms.
- design/principles.md rule 2: order the sidebar by the operator's workflow (spaces, then agents, then the selected workspace's files), not by the projection's internal structure.
- design/principles.md rule 3: the frequent actions - select a workspace, focus an agent, open a file, split a pane - are single click or single shortcut.
- design/principles.md rule 4: show derived state (agent phase, unsaved changes, focused pane, remote workspace) as computed state; never make the user infer it from raw fields.
- design/principles.md rule 5: one row shape and one section pattern across Workspaces, Agents, and Files; do not invent a second list pattern for one section.
- design/principles.md rule 6: state the consequence at any action that writes to the user's disk; this scope's only such action is save, which must show its result.
- design/principles.md rule 7: encode state and structure visually first - selection, focus, agent phase, unsaved changes, and remote status get color, weight, icon, or placement, with a short label only where the visual alone is ambiguous; no explanatory paragraphs in the layout.

## 12. Implementation Result Report Contract

Report:

- status: `Done`, `Partially Done`, or `Blocked`.
- user-visible changes across the sidebar, pane layout, terminal, shortcuts, and file/editor surfaces.
- the modules changed and each module's responsibility boundary after the change, and whether the approved structure (frame model, pane tree, single cell metric, wired file layer) was followed.
- task completion status for T1-T12 and R/AC/V coverage.
- verification evidence by mode, including the screenshot artifacts for V5 and the exact bundle they were taken from.
- automated tests added or updated and the regression risk each protects.
- the disposition of the "option + f" assumption and any other assumption made during implementation.
- deviations, remaining human review items, not-done items, and follow-up candidates.
- delivery: local mode, so no branch, commit, push, or PR is expected in this run.
