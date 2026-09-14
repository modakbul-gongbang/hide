# Agent workflow UI contract

This document names the implemented Agent workflow surfaces in `design/hide.pen` and their code owners.
The canvas contains current `Screen /` boards and reusable `Component /` masters only.
The former Final, R2, R3, handoff candidates, and the Overview family-only comparison are historical and must not be reconstructed.

## Product decisions

- Agents opens in `My Work` for each app session and offers `All` as an explicit scope change.
- `My Work` excludes rows whose core-final ownership is Delegated, while hard escalations and visible orphans remain operator-owned and visible.
- Both scopes preserve Needs You, Done, Working, and Seen instead of replacing status groups with a relationship tree.
- Overview opens in Tasks and shows the current Project's live task forest across its Workspaces.
- Git remains a separate Overview mode because commit ancestry and agent delegation are different relations.
- Selecting an Overview row changes inspection only.
- The row's Open control is the only action that shows its pane and changes read state.
- The pane header distinguishes the pane shown by Hide from the terminal that owns the native keyboard responder.
- Explorer decorates its existing native file tree from the normalized Changes projection and does not own another Git reader.
- Provider artwork, agent identity, status, ownership, instrumentation, and lineage come from the same canonical projection on every surface.

`My Work` is intentionally session-local.
No runtime preference, migration, or cross-launch persistence is implied.
The current Herdr/Sasu dispatch contract may omit parent lineage, so Hide filters only ownership the core can prove and never claims every supervisor-created worker is identifiable.

## Canvas map

| Board | ID |
| --- | --- |
| Screen / Workbench / Parent | `wzrRI` |
| Screen / Workbench / Delegated child tab | `IhnIO` |
| Screen / Explorer / Git in workbench | `n2oTPf` |
| Screen / Overview / Inspect without moving | `AZvEA` |
| Screen / Agents / My Work status groups | `Snwg0` |
| Screen / Pane / Relationship modal | `eXxKb` |
| Screen / Overview / Project task forest | `c3Lf5` |
| Screen / Overview / Availability states | `m5T3n1` |
| Component / Spec / Pane header and focus | `mR198` |
| Component / Spec / Narrow and non-agent headers | `b6Nt3Q` |
| Component / Spec / Shared agent identity | `ZjPJ6` |
| Component / Spec / Hierarchy widths | `INj5G` |
| Component / Spec / Workspace disclosure | `g7trZv` |
| Component / Spec / Tree interaction states | `m0gZNn` |
| Component / Spec / Browser file diff toolbars | `p0Do2` |
| Component / Spec / Relationship node | `d8bYuE` |
| Component / Spec / Search and Recent Sessions identity | `dexoX` |
| Component / Spec / Explorer Git states | `BMEZi` |
| Component / Spec / Relationship action states | `ZEwQt` (live draft, not yet persisted) |

The deleted `Review / UI Handoff / 00 Start here` and `90 Shared dependencies` boards were navigation aids, not product masters.
Their adopted reusable content now lives in the Component band and all changed product compositions live in the Screen band.

## Master ownership

| Master | Code owner | Contract |
| --- | --- | --- |
| Agent identity `HXWFK` | `AgentRow.swift`, `AgentRowPresentation`, `AgentNavigatorRow` | One provider artwork, title, canonical state, ownership, and instrumentation identity across live lists, search, relationship, and Overview. |
| Tree row `n1zn1O`, lineage segment `N6BTh4` | `LineageGuideView.swift`, `SidebarGrouping.swift`, `HideUI.swift` | Disclosure and selection remain separate, and 18pt lineage columns remain aligned through multiline rows. |
| Workspace row `pPtY6` | `HideUI.swift`, `CheckoutCardPresentation.swift` | Workspace membership and agent delegation stay distinct, and expanding a Workspace changes no pane, tab, or read state. |
| Focused pane `ZvLjg`, icon header `Z3BnL` | `ShellView.swift`, `PaneLineageHeader.swift` | The 28pt identity row, optional 24pt child row, shown wash, responder outline, zoom, and overflow use existing typed intents. |
| Named child chip `j0Sji` | `PaneChildRow`, `PaneLineagePresentation` | One direct child is named and remaining direct children fold into an honest `+N`. |
| Workbench `OwIFR` | `HideMainView`, `HideTabStrip`, `ShellModel.swift` | A delegated child occupies its own tab and returning chooses the authoritative parent placement. |
| Browser chrome `KUcQU`, address toolbar `eMlZD`, document toolbar `hxdu7` | `BrowserPaneView.swift`, `EditorViewerOverlay.swift` | Browser, file, and diff surfaces keep their own controls and never inherit agent-only actions. |
| Graph node `FKLct` | `PaneRelationshipSheet`, `CorePaneChildren`, `CoreLineageStep` | Node selection inspects, Open navigates, and unavailable lineage stays explicit. |
| Relationship action `p7Vim` (live draft, not yet persisted) | `ShellModel.swift`, `PaneLineageHeader.swift`, `HideUI.swift`, `CoreBridge.swift`, `runtime.rs` | One request ID carries Ready, Pending, Target unavailable, and Open failed outcomes across the sheet, Return control, direct child chip, and retained canvas. |
| Project task overview `G4Sj9`, task item `AdQ5R` | `CheckoutOverview.swift`, `OverviewPresentation.swift` | The current Project's authoritative live forest crosses Workspaces without guessing missing parent links. |
| Explorer Git row `mSu8p`, panel `Wo6qx` | `WorkspaceOutlineView.swift`, `WorkspaceOutlinePresentation.swift`, `changes.rs` | A fixed status slot renders M, A, U, R, conflict, folder-changed, and clean states without changing file-tree interaction. |

## Agents scope and lineage

Agents uses the same canonical visible list for rows and numeric shortcuts.
The scope control does not change the pane, tab, disclosure, inspection, or read state.
Switching from `My Work` to `All` restores delegated rows in their original status groups.
When only delegated work exists, the empty My Work state offers a direct All action.
Loading, confirmed no agents, filter-empty, disconnected, and populated states remain distinct.
During disconnection the last known rows remain visible beneath an explicit stale-data notice.

Ownership is derived from the core's final `delegated` value rather than from depth.
Depth zero is not ownership, because a hard escalation can clear delegation and an orphan must remain visible.
Missing parent lineage never becomes evidence that a pane was independently started.

## Pane header and focus

The first row is 28pt and carries identity plus pane actions.
The second row is 24pt and exists only when the pane has known child or subagent work; instrumentation uncertainty stays in the first row as a help icon.
A child pane keeps a compact authoritative parent return control in the first row.
The child row shows the first known direct child and folds the rest into `+N` and the relationship sheet.
Selecting a relationship row changes the sheet's inspection only, while Open sends the existing pane-selection intent.
Open and parent Return remain pending until the core publishes the outcome for that exact request ID and target.
The same pending intent blocks duplicate execution.
An unavailable target is refused before dispatch, while a core refusal, timeout, retirement, or remote-control failure ends only the matching request.
Both paths keep pane geometry and tab topology intact, display a scoped reason, and expose Retry only when the core outcome permits it.
Retry starts a new request after the failed one has settled.
The result remains visible in the retained canvas when navigation removes the source sheet or header, including direct child-chip navigation.
The shell does not treat `lastError`, an inactive historical layout, or optimistic remote navigation as success or failure, and it emits no rollback focus event or UI-owned timeout.

The B24 Pen sheet `ZEwQt`, master `p7Vim`, modal states `dFThD`, child Open states `V4miTY`, and parent Return states `u9CJj` remain in the live `hide.pen - Edited` document and the run exports.
They are not yet present in the worktree file because the non-foreground app connection cannot Save As to the worktree path.
Do not treat the IDs as adopted canvas references until a separately coordinated Pen Save As persists them and the generator/checkers pass on that saved file.

The header wash marks the pane currently shown by Hide.
The outer primary hairline marks the actual terminal responder.
Moving keyboard focus into Overview retains the shown wash and removes the terminal outline.
Unread weight is not reused to mean parent, child, delegated, or selected.

Zoom or Restore stays at the right edge in every state.
Fork, ports, sibling access, and other secondary capabilities live in the existing overflow menu.
Close remains separate and preserves the core-owned consequence check.
At narrow widths the parent name falls back to its icon before current identity or actions are lost.

## Overview task forest

Tasks is the default Overview mode and Git is the alternate mode.
The task forest is built from current Project checkouts and canonical live agents.
Only authoritative child IDs create edges.
Unknown parents, visible orphans, and cycles remain visible as roots instead of disappearing.
Root tasks and children that cross a Workspace show their Workspace location.
Search retains every matched task and its known ancestors.
No-results keeps the prior inspector and offers query clearing rather than inventing a new selection.

Row selection and keyboard Return inspect.
The trailing pane control and the inspector's Open action navigate.
The inspector shows canonical identity, status, Workspace, orphan or disconnected facts, and Workspace details.
Workspace changed-file counts are never presented as one agent's output.
The Git view preserves GitHub, disk, cleanup, ancestry, worktree selection, and existing explicit actions.

## Explorer Git decorations

The native outline keeps 22pt Seti rows and reserves one 12pt Git slot at the trailing edge.
The filename truncates before that slot and the row's file open, disclosure, rename, drag, keyboard, and context-menu owners remain unchanged.
The badge is informative and intercepts no click.
The tooltip and accessibility label contain the full relative path and status name.

| Mark | Meaning |
| --- | --- |
| M | Modified |
| A | Added |
| U | Untracked |
| R | Renamed, with previous and current relative paths retained by the projection |
| ! | An actual Git conflict |
| `●` | A folder with at least one changed descendant |
| Empty slot | Confirmed clean or no decoration available |

Deleted paths remain in Changes and do not become fake Explorer rows.
An unsaved editor buffer remains editor state and is not relabeled as Git M.
Folder aggregation derives from the complete changed set, including deleted descendants that the current tree cannot enumerate.
The highest-risk descendant controls the folder's semantic help while the visible folder mark remains `●`.

`ChangedFileStatus` carries Modified, Added, Deleted, Untracked, Renamed, and Conflict.
`ChangedFileSnapshot.previous_relative_path` carries the source side of a rename while `relative_path` remains the destination that can be opened.
The Changes reader requests Git status while Explorer is visible or a Changes/diff surface needs it.
It refreshes at the existing two-second bound, runs outside the runtime mutex, and performs no row, hover, or scroll subprocess.
Changing the focused checkout replaces the root-scoped decoration set, so one Workspace never paints another's status.
Git loading or failure appears above the file tree and never erases usable file navigation.

## Verification contract

Static acceptance runs `node scripts/gen-pen.mjs`, `node scripts/check-pen.mjs`, and `node scripts/check-design-contract.mjs`.
Code acceptance runs both required Rust commands and both required Swift commands from the implementation worktree.
Native acceptance uses exactly one identified candidate Hide app and one isolated Herdr server.
Screenshots must prove My Work and All, task inspection versus Open, shown versus responder focus, narrow header behavior, and Explorer rename/conflict/folder decorations.
Idle and driven observations must record their load and must not turn one bounded run into a universal performance claim.

This contract applies Design principles 2, 4, 7, 9, and 12 by matching the operator workflow, rendering derived ownership and Git state, separating every availability state, and checking Korean and English at real widths.
It applies Engineering principles 4, 5, 7, and 13 by keeping failures caller-visible, extending the existing projection and readers, and fixing rename, conflict, lineage, and root-isolation classes rather than individual screenshots.
