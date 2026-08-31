---
topic: "hide workbench redesign"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "This changes daily native macOS navigation, editing, keyboard behavior, and persisted UI state without touching production data, authentication, or external side effects."
source_intake: "current conversation"
created_at: "2026-08-31"
updated_at: "2026-08-31"
---

# PRD: hide workbench redesign

## 1. Summary

Redesign hide's workbench so the right panel is a dense, responsive file tree, while the editable file viewer opens over the central terminal pane area instead of competing for width below the tree.
Add independent, persistent controls for the left sidebar and right workbench, with standard shortcuts and automatic reopening when a hidden panel receives focus.
Preserve the existing core-owned runtime model, local edit/save/conflict behavior, remote read-only boundary, dark `HideTheme`, and concurrent branch ownership limits.

Approval checklist:

- Scope: file-tree quality, central file-viewer overlay, and independent left/right panel visibility only. See sections 3 and 6.
- Structure: an AppKit outline adapter with lazy directory loading, an independent viewer overlay, and two core-owned persisted visibility fields. See section 5.
- Product decisions: full central-pane overlay, editing and saving retained, and a curated Seti icon subset with fallback. See section 4.3.
- Verification: automated behavior, native desktop screenshots and interaction, runtime performance sampling, and source-ownership checks are required. See section 9.
- Delivery mode: local semantic commit on `prd/workbench-redesign`; no push, PR, CI, merge, or public delivery.

## 2. Problem, Goal, And Users

The current workbench uses a recursively materialized SwiftUI `OutlineGroup` with default blue folders, oversized rows, no selected or hover treatment, no indent guides, and an informational overlay that collides with the tree.
The file viewer shares the narrow right column below the tree, so both navigation and editing become cramped and long lines wrap excessively.
The fixed left and right panels also consume terminal space even when the user does not need them.

The goal is a native macOS developer-tool workflow in which a developer can scan and navigate a large workspace quickly, open and edit a file in a readable central overlay without losing access to the tree, and reclaim terminal space by independently hiding either side panel.

### 2.1 User Scenarios

- SC1. Navigate a large local workspace.
  Actors: a developer using hide on macOS.
  Primary path: the developer expands folders, moves through rows with pointer or keyboard, recognizes file types by icon and color, and opens a file without the tree stalling.
  Failure state: an unreadable or disappeared directory produces a visible unavailable state or error instead of an empty success state.
  Recovery: the developer collapses or re-expands the directory or selects another reachable folder without restarting hide.
  Reach: use a dedicated local test workspace containing nested directories, common source extensions, unknown extensions, and enough files to exercise virtualization.

- SC2. View and edit a file over the terminal area.
  Actors: a developer using the workbench and terminal panes.
  Primary path: selecting a local file opens an overlay across the central pane area while both side panels remain usable; selecting another file updates the same overlay; editable text and Markdown can still be saved.
  Failure state: unsupported, read-only, image-decode, save-conflict, and external-change states remain explicit and actionable.
  Recovery: Esc or the visible close control dismisses the overlay without discarding the current draft; selecting the file again restores the current in-memory editing state.
  Reach: use a dedicated local test workspace with editable text, Markdown, image, unsupported, read-only, and externally modified files.

- SC3. Reclaim terminal space with independent panels.
  Actors: a developer switching between navigation-heavy and terminal-heavy work.
  Primary path: toolbar controls or standard keyboard shortcuts hide and show the left sidebar and right workbench independently, including the state where both are hidden.
  Failure state: hidden panels do not leave dead split-view gaps, cover controls, or make their recovery action unreachable.
  Recovery: focusing a hidden panel opens it automatically, and the toolbar controls remain available regardless of visibility.
  Reach: start with both panels visible, exercise all four visibility combinations, then relaunch hide using the same isolated UI-state file.

## 3. Scope And Non-Goals

In scope:

- Replace the local SwiftUI `OutlineGroup` path with a reusable AppKit `NSOutlineView` adapter that reuses rows and loads directory children only when needed.
- Give the tree compact `HideTheme`-based rows, disclosure affordances, selection, hover, 8-point indentation, visible indent structure, pointer interaction, keyboard navigation, and accessibility labels.
- Bundle a curated MIT-licensed Seti icon subset for the extensions and filenames hide currently understands plus common repository files, with one generic file fallback and one folder treatment.
- Remove or relocate the overlapping `Local, existing files only` message so no explanatory text floats over tree content.
- Keep the right workbench tree-only and move the existing local viewer/editor into an overlay hosted over the complete central terminal pane region.
- Keep edit, Save, Markdown preview/edit, image preview, read-only notice, diff summary, conflict resolution, and unsupported-file behavior.
- Add Esc and a visible close control that hide the overlay without discarding the current in-memory draft.
- Add independent left-sidebar and right-workbench controls, `Cmd+B` and `Cmd+Option+B`, automatic opening on focus, and restart persistence through core `ui_state`.
- Add caller-observable automated coverage, a real signed development-app run, native screenshots, interaction evidence, and performance sampling.

Non-goals and deferred decisions:

- No changes to sidebar sections, agent rows, `AgentBadge`, New Workspace, New Agent, Pet, dashboard, search grouping, terminal lifecycle, PTY attachment, or pane switching owned by `prd/hide-ux-followup`.
  Consequence: those surfaces retain their concurrent branch behavior.
  Rationale: the user assigned them to another worktree.
  Revisit condition: resolve integration conflicts after both local branches are complete.
- No remote-file editing or recursive remote explorer in this change.
  Consequence: remote files remain the existing read-only, top-level representation and remote terminal remains the write owner.
  Rationale: SSH ownership and remote transport are separate product boundaries.
  Revisit condition: a separately approved remote editing contract.
- No multi-file editor tabs, multi-buffer draft store, search/filter input, drag and drop, rename, create, delete, context menus, or compact-folder collapsing.
  Consequence: only one core editor buffer remains active, and changing files follows the existing single-buffer model.
  Rationale: none is required by the three requested redesigns, and each adds a separate file-operation or persistence contract.
  Revisit condition: explicit follow-up requirements for those workflows.
- No full Seti distribution.
  Consequence: uncommon extensions use the generic fallback icon.
  Rationale: the user chose a curated subset to keep bundle size and mapping maintenance bounded.
  Revisit condition: observed fallback frequency justifies more mappings.
- No modification of the frozen Stage 0 spike record or retired root `src/` shell.
- No installation replacement, app termination, or reuse of an app instance started by another session without first establishing ownership.
- No push, PR, CI watch, merge, public-repository update, force push, or history rewrite.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
The implementation can create its own isolated workspace and state file, and no credentials, billing, account ownership, production data, or user-only asset is needed.

### 4.2 Human Decisions Before PRD Approval

None required.
The user explicitly accepted all three recommended choices: central-pane-area overlay with side panels retained, editing and Save retained, and a curated Seti subset with fallback.

### 4.3 Decision Traceability For Fidelity Review

- User request: implement all three workbench changes through `$please`: file-tree quality, a pane-area file-viewer overlay, and independent left/right panel hiding. Represented by R1-R9, AC1-AC12, T1-T5, and V1-V5.
- User source priority: the user-authored text in `hide-workbench-request.md` controls intent; its code observations and completed open-source research are inputs, not conclusions to copy. Represented by the architecture decision below and guardrails G1-G4.
- User decision on viewer placement: accepted the recommendation to cover the full central terminal pane area while leaving both side panels available. Represented by R4, AC5, SC2, and V3.
- User decision on viewer capability: accepted retaining editing and Save. Represented by R5, AC6-AC7, SC2, and V2-V3.
- User decision on icons: accepted a curated Seti subset plus generic fallback rather than the full Seti distribution. Represented by R3, AC4, T1, and V1-V3.
- User safety decision: work only in `/Users/hoyeonlee/projects/herdr-ide.worktrees/workbench` on `prd/workbench-redesign`, preserve all existing Herdr sessions, and publish nothing. Represented by R9, AC11-AC12, T5, V5, and guardrails G5-G8.
- User ownership decision: `WorkbenchPanel.swift` and new tree/viewer files are owned here; shared container changes must be minimal; forbidden `HideUI.swift` sidebar, agent-row, `AgentBadge`, New Workspace, and New Agent regions and unrelated `herdr-core` work remain untouched. Represented by R8, AC10-AC11, and V5.
- Agent technical decision: choose `NSOutlineView` over a manually flattened `LazyVStack` because the requested row reuse, disclosure, keyboard navigation, selection, and exact row geometry are native behaviors in one established component; pair it with lazy child enumeration so virtualization does not leave the current recursive filesystem load in place. Represented by R1-R2, AC1-AC3, and T1.
- Agent-owned reversible assumption: provide both persistent toolbar buttons and the conventional `Cmd+B` and `Cmd+Option+B` shortcuts, and automatically reveal a panel when focus targets it. Represented by R7, AC8-AC9, SC3, and V2-V3.
- User decision clarification: the curated, extensible icon mapping prioritizes extensions actually encountered in this repository, including `swift`, `rs`, `ts`, `js`, `md`, `json`, `toml`, `yml`, `sh`, `py`, and `png`, plus common Git/config filenames, other common image types, generic text/file, and folder states. Represented by R3, AC4, T1, and the result report contract.
- Agent-owned reversible assumption: Esc hides the overlay but does not clear the core editor or draft; the visible close control has the same behavior. Represented by R5, AC7, and SC2.
- Accepted existing product boundary: local files remain the only editable files; remote files remain read-only and terminal-owned. Represented by R6 and the remote-editing non-goal.
- Rejected option: keep the current `OutlineGroup` and merely restyle it. Rejected because it leaves non-virtualized row creation, incomplete keyboard behavior, and the recursive rendering path intact.
- Rejected option: bundle all Seti icons. Rejected by the user's choice of the recommended curated subset.
- Rejected option: make the overlay read-only or restrict it to one selected terminal tile. Rejected by the user's accepted recommendations.
- Principles intake: `/Users/hoyeonlee/projects/oh-my-principle/engineering/principles.md` and `/Users/hoyeonlee/projects/oh-my-principle/design/principles.md` were read from the current local checkout on 2026-08-31. `sasu principles list --json` reported no repositories in `agents/config.json`, so no additional declared-domain source commit exists.
- Design-skill disposition: `design-taste-frontend` identifies code editors and dense product UI as out of scope. Only its redesign audit, existing-token preservation, interaction-state, accessibility, and performance checks are applied; web marketing layout rules are not applied.

## 5. Major Technical Structure Changes

- Replace the SwiftUI-owned recursive local tree renderer with an AppKit outline boundary: a SwiftUI representable hosts `NSOutlineView`, while a focused filesystem data source lazily enumerates and caches children by path.
- Separate the current combined tree/viewer composition into a tree-only workbench and a viewer overlay hosted by the central pane container.
- Extend core-owned UI state and its stable persistence contract with independent left-sidebar and right-workbench visibility, using backward-compatible defaults of visible for existing state files.
- Keep the shell as a renderer and event dispatcher: panel toggles, focus-driven reopen, file selection, and overlay visibility changes dispatch typed events or use snapshot state rather than becoming shell-only authority.
- Add a curated Seti resource and attribution boundary under the existing SwiftPM resource and third-party-notice conventions.
- No new service, database, network, auth, payment, job, queue, or external runtime dependency is introduced.

## 6. Requirements

- R1. The local workspace tree must use `NSOutlineView` row reuse and enumerate a directory's children only when that directory is expanded or otherwise queried by the outline data source; no per-row subprocess or core-runtime lock work is allowed.
- R2. The tree must be dense and navigable: compact row height, 8-point indentation, native disclosure, visible indent structure, hover, selected state, pointer activation, arrow-key traversal, Return/open behavior, and accessible labels must use existing `HideTheme` typography, color, spacing, radius, and accent conventions.
- R3. File and folder identity must use one easily extensible curated Seti-derived icon/color mapping with a deterministic generic fallback, prioritizing repository extensions such as `swift`, `rs`, `ts`, `js`, `md`, `json`, `toml`, `yml`, `sh`, `py`, and `png`; assets must be bundled through SwiftPM and accompanied by an MIT notice and source attribution under `macos/Resources/THIRD_PARTY_NOTICES/`.
- R4. Selecting a local file must show the existing viewer as an overlay over the full central terminal pane region, below the product toolbar and above the status bar, while both side panels remain visible and interactive.
- R5. The overlay must retain local text editing, `Cmd+S`, Save enablement, Markdown preview/edit, image preview, diff count, read-only and unsupported states, external-change conflict recovery, a visible close control, and Esc dismissal without discarding the current in-memory draft.
- R6. Remote context must retain its explicit read-only and terminal-owned editing boundary; the redesign must not suggest that remote writes are available.
- R7. The left sidebar and right workbench must each be independently visible or hidden through persistent toolbar controls and `Cmd+B` / `Cmd+Option+B`; focusing a hidden panel must reveal it before applying focus.
- R8. Panel visibility must be core-owned, included in the revisioned UI snapshot, persisted atomically in `ui_state`, default visible for existing/missing state fields, and survive a relaunch without resetting unrelated persisted fields.
- R9. Implementation must preserve the concurrent ownership boundary, avoid unrelated `HideUI.swift` and `herdr-core` changes, leave existing Herdr sessions and foreign app processes untouched, and remain local to the named worktree and branch with no public delivery.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | Expanding one directory in a large test workspace enumerates only the root and directories needed for the visible expansion path, while scrolling realizes reusable outline rows instead of constructing the full expanded SwiftUI hierarchy | machine | - |
| AC2 | The rendered tree has compact themed rows, 8-point hierarchy indentation, visible disclosure and indent structure, distinct hover and selection, no default blue folder treatment, and no helper text overlapping file rows | judged | native screenshot set: empty, populated, hovered, selected, and nested-expanded tree at the development app's real window size |
| AC3 | Pointer and keyboard navigation can expand, collapse, move between, and open rows, and assistive identifiers distinguish nodes with repeated names | machine | - |
| AC4 | Every curated filename or extension, including the user-prioritized repository extensions, maps to its intended bundled Seti icon/color, an unknown extension maps to the generic fallback, adding a later mapping requires changing one centralized mapping boundary, and the bundled files have a complete MIT source notice under `macos/Resources/THIRD_PARTY_NOTICES/` | machine | - |
| AC5 | Selecting a local file opens a readable overlay across the complete central terminal pane region while the left sidebar and right workbench remain visible, and selecting another tree file updates that overlay | judged | scripted native run and screenshots showing terminal-before, overlay-open, and second-file-selected states with both side panels present |
| AC6 | Text and Markdown edits, Save and `Cmd+S`, image preview, read-only/unsupported states, diff summary, and file-conflict recovery continue to produce their existing observable outcomes in the overlay | machine | - |
| AC7 | Esc and the close control dismiss the overlay without losing the current in-memory draft, and selecting the same file again restores that draft during the process lifetime | machine | - |
| AC8 | Toolbar controls and `Cmd+B` / `Cmd+Option+B` independently reach all four left/right panel visibility combinations without a dead split-view gap, and a focus request reveals the corresponding hidden panel | judged | scripted native interaction and screenshots for left-only, right-only, both-visible, and both-hidden states plus focus-driven reopen |
| AC9 | The selected panel visibility combination survives a clean app relaunch through core `ui_state`, while selected file, expanded paths, shortcuts, pet state, workspace/device registrations, accent, font size, and warning preferences retain their previous values | machine | - |
| AC10 | Core remains the authority for persisted visibility and overlay/editor state, and no filesystem enumeration, subprocess, blocking I/O, or large serialization is added while holding the runtime mutex | machine | - |
| AC11 | The final run-owned diff does not modify the forbidden sidebar, agent-row, `AgentBadge`, New Workspace, New Agent, Pet, dashboard, search-grouping, terminal-lifecycle, PTY-attach, or pane-switching ownership surfaces | machine | - |
| AC12 | Native verification uses exactly one identified signed development-app instance and an isolated test workspace/state, captures real macOS screenshots, samples the running app and `herdr server` for performance claims, does not terminate or move foreign processes or sessions, and creates no push or PR | judged | process inventory, bundle identity, isolated test-resource ledger, native screenshot set, sample summaries, and final git/delivery evidence |

## 8. PRD-Level Tasks

- T1. Build the lazy AppKit outline boundary, themed reusable rows, curated Seti resource mapping, fallbacks, and third-party notice. Covers R1-R3, AC1-AC4.
- T2. Separate the editor/viewer from the tree and host it as a central-pane overlay with preserved edit/save/preview/conflict behavior and draft-preserving dismissal. Covers R4-R6, AC5-AC7. Depends on: T1.
- T3. Add core-owned, persisted independent panel visibility, toolbar controls, shortcuts, and focus-driven reopen using only the minimal shared-container edits. Covers R7-R9, AC8-AC11. Depends on: T2.
- T4. Add high-value regression coverage for lazy enumeration, icon mapping/fallback, keyboard actions, overlay dismissal/draft retention, existing viewer behavior, independent visibility, persistence compatibility, and unrelated-state retention. Covers R1-R9, AC1, AC3-AC11. Depends on: T3.
- T5. Build and sign the development app, create an isolated test workspace and UI-state location, prove the native flows with one owned instance and real screenshots, sample the app and `herdr server`, inspect the final ownership diff, clean only resources created by this run, and leave local delivery evidence. Covers R1-R9, AC1-AC12. Depends on: T4.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Swift, Rust, resource, license, and repository health | none |
| automated behavior | yes | lazy tree behavior, mappings, editor preservation, keyboard actions, UI-state persistence, and regression protection | none |
| native desktop runtime | yes | real tree, overlay, panel states, keyboard/focus behavior, accessibility targeting, and screenshots | final visual taste remains human review |
| runtime performance | yes | large-tree interaction and absence of new app/core blocking | none |
| source and delivery hygiene | yes | ownership boundary, isolated resources, branch, local-only delivery, and no foreign-session interference | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R9, AC4, AC10-AC11 | Rust and Swift builds, tests, resource processing, license checks, and diff hygiene remain green without new warnings caused by the change | yes | no |
| V2 | automated behavior | SC1-SC3, R1-R8, AC1, AC3-AC4, AC6-AC7, AC9-AC10 | caller-observable tests prevent eager descendant loading, icon fallback, navigation, draft loss, viewer regression, and persisted-panel-state regression | yes | no |
| V3 | native desktop runtime | SC1-SC3, R2-R7, AC2, AC5-AC8, AC12 | one owned signed development app visibly provides the dense tree, central overlay, preserved editing states, and every panel combination with usable keyboard and pointer recovery | yes | no |
| V4 | runtime performance | SC1, R1, R9, AC1, AC10, AC12 | sampling the owned app and `herdr server` during large-tree expansion and scrolling shows no new filesystem, subprocess, serialization, or runtime-mutex wait dominating the interaction | yes | no |
| V5 | source and delivery hygiene | R8-R9, AC11-AC12 | the final changed-file and hunk inventory respects concurrent ownership, uses only isolated test resources, leaves existing sessions/processes untouched, and records a local commit with no push or PR | yes | no |

### 9.3 Human Verification

- Review the final native screenshots for whether the compact tree, Seti subset, selection hierarchy, overlay proportions, and toolbar controls feel coherent with hide's existing dark product language.
- Review the minimal shared `HideUI.swift`, `ShellView.swift`, and `herdr-core` hunks during later branch integration, especially where the concurrent `prd/hide-ux-followup` branch also changed container or snapshot code.

## 10. Risks And Open Decisions

- `HideUI.swift` and `herdr-core` are shared integration surfaces.
  Mitigation: limit edits to top-level panel composition, toolbar layout controls, snapshot/UI-state fields, typed events, and direct tests; report every shared-file hunk explicitly.
- An `NSOutlineView` wrapper can become a second source of selection or expansion truth.
  Mitigation: AppKit owns only transient row/view mechanics; selected file, expanded paths, overlay/editor state, panel visibility, and persisted state remain core-owned and flow through typed events/snapshots.
- Lazy enumeration can show stale children after external filesystem changes.
  Mitigation: define cache invalidation on root change and explicit re-expansion/reload; do not claim live filesystem watching in this PRD.
- Curated Seti coverage can make uncommon files less distinctive.
  Mitigation: deterministic generic fallback, a compact centralized mapping, and follow-up expansion based on observed misses.
- Overlaying the full central area hides terminal content while open.
  Mitigation: side panels and toolbar remain usable, close is always visible, Esc is supported, and the overlay does not destroy terminal state.
- The existing single editor buffer may lose an unsaved draft when opening a different file, depending on current core behavior.
  This PRD does not add multi-buffer persistence or a new file-switch confirmation because neither was part of the requested redesign.
  Esc dismissal itself must preserve the draft.
- Native verification can conflict with another session using `/Applications/hide.app`.
  Mitigation: identify ownership before launch or replacement, prefer an isolated signed development bundle and state file, and stop rather than terminate a foreign instance.
- No blocking product or implementation decision remains.
  Final visual taste and later branch-conflict resolution remain explicit human review surfaces.

## 11. Implementation Guardrails

- G1. Engineering principle 1: remove the obsolete combined tree-plus-viewer composition and `OutlineGroup` path in the same change; do not leave compatibility renderers or dead helpers.
- G2. Engineering principle 2: implement only the smallest complete AppKit outline, overlay, and two-state persistence model required here; do not add speculative file operations or editor abstractions.
- G3. Engineering principles 5 and 7: keep filesystem tree data, icon presentation, viewer presentation, panel composition, and core persistence modular, and extend existing `HideTheme`, resource, snapshot, event, and persistence paths instead of creating parallel systems.
- G4. Engineering principles 4, 9, and 10: directory, asset, state-decode, state-save, file-open, and overlay failures must remain explicit and observable from the UI or structured diagnostics; no empty-success fallback.
- G5. Engineering principle 11: state writes, repeated toggles, relaunch, and repeated resource setup must converge without duplicated assets, notices, or corrupted state.
- G6. Engineering principle 12 and `engineering/practices/test.md`: tests must assert caller-visible lazy loading, mapping, editing, keyboard, and persistence outcomes rather than AppKit wiring or private helper calls.
- G7. Engineering principle 13: fix eager tree construction and default styling as classes of failure, not only the supplied screenshot or one repository shape.
- G8. Design principles 2, 3, 5, and 7: organize controls around the developer's navigation/editing workflow, keep frequent file and panel actions one click or shortcut away, follow hide's existing tokens, and encode hierarchy and state through geometry, icon, color, and selection before explanatory text.
- G9. Do not touch the forbidden concurrent-owner sections or broaden `herdr-core` beyond the minimal typed editor/overlay and visibility state required by R5, R7, and R8.
- G10. Never hold the runtime mutex across filesystem enumeration, subprocess work, blocking I/O, or large serialization, and never fork a subprocess from a tree tick, row, or input event.
- G11. Do not edit generated files or `CHANGELOG.md`, and do not modify the frozen Stage 0 spike record.
- G12. Do not close, move, rename, or reuse existing Herdr workspaces, tabs, panes, agents, or sessions.
- G13. Before native UI claims, confirm exactly one owned app instance and whether it is the signed development build or installed bundle; use a real macOS screenshot, not code inspection or process existence.
- G14. If an installed bundle is explicitly replaced later, delete the old bundle before copying; do not overwrite a signed bundle in place.
- G15. Do not push, create or update a PR, watch CI, merge, force-push, rewrite history, or touch a public repository.
- G16. Commit only this run's coherent work with product-focused metadata and no agent, model, vendor, or tool attribution.

## 12. Implementation Result Report Contract

The implementing agent must report:

- Status: `Done`, `Partially Done`, or `Blocked`.
- Human decisions replaced by assumptions, listed first, including shortcuts and Esc draft retention; report the curated icon coverage separately as an explicit user decision.
- User-visible tree, overlay, editing, and panel behavior changes.
- Actual file/module structure and the responsibility of the AppKit outline adapter, tree data source, icon mapper/resources, viewer overlay, shared panel container, CoreBridge, and core persistence/event changes.
- Whether the section 5 structure and every ownership guardrail were followed.
- A specific inventory of every hunk in shared `HideUI.swift`, `ShellView.swift`, and `herdr-core` files and why it was necessary for later human integration.
- T1-T5 and R1-R9 completion, plus AC1-AC12 and V1-V5 coverage.
- Build/static, automated behavior, native desktop runtime, runtime performance, and source/delivery hygiene evidence, including commands, exit results, app PID/bundle identity, screenshot paths, sample summaries, and isolated-resource cleanup.
- Automated tests added or updated and the regression risk each protects.
- The exact curated Seti filenames and extensions, why each was included, how the generic fallback works, and where a future mapping is added.
- Acceptance and fidelity verdicts, timing, ledger findings, completion fingerprint, receipt path, and evidence that finalize executed no tests or external commands.
- Local delivery commit and branch, with explicit confirmation that no push, PR, CI, merge, public repository, or foreign Herdr session was touched.
- Deviations, unresolved risks, remaining human visual review, and follow-up candidates.
