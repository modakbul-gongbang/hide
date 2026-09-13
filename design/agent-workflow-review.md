# Agent workflow design review

Status: design proposal, not an approved implementation contract.
The product code and installed app are unchanged by this review.
The editable source is [hide.pen](hide.pen), under `Review / 2026-09-13 Agent workflow /`.
The existing `Screen /` boards continue to describe the existing design; this proposal does not silently supersede them or the current contracts in `DESIGN.md`.

## Direction

The operator talks to one parent agent and can understand the work it delegates without visiting every child pane.
Three different relationships must stay distinct: Project to Workspace membership, parent to delegated agent ownership, and a Workspace's Git comparison against the primary branch.
The interface should answer what is running, what needs the operator, where it is running, and how to return to it.
Commit history, raw pane identifiers, allocation details, and instrumentation explanations belong in inspection surfaces.

All example messages, names, counts, PR numbers, and timestamps in the proposal are labelled design samples, not a live status report or content to ship as constants.
Existing `HideTheme` variables are reused; no application token or product code is added.
The latest user direction prioritizes a natural node-and-edge parent/child visualization and consistent visual UI across navigation surfaces.
Following the request for a less fragmented view, start with `01 All work full screen` and `02 Project full screen`.
Then inspect `Workbench visual`, `Delegation graph visual`, the component/state sheets and the earlier structural alternatives.
English navigation terms remain aligned with the product while Korean task names and message content exercise mixed-script layouts.
Korean labels in the proposal are copy candidates, not a decision to localize the entire application.

## Inspection and confidence

| Evidence | Finding | Limit |
| --- | --- | --- |
| User reference 1 | Workspace lineage connectors compete with disclosure, status marks, and provider icons; stopped children occupy substantial space | Pixel observation, not a proven geometry root cause |
| User references 2 and 3 | Overview emphasizes commits, tags, folded commits and a long PR list instead of active work | Observed supplied screenshots, not freshly navigated |
| User reference 4 | Pane child chips expose truncated fork identifiers; `Children unknown` appears beside known child entries | Known children and incomplete instrumentation can coexist; do not call their coexistence a data contradiction |
| User reference 5 and current `WorkspaceOutlineView.configure` | Desired Explorer Git decoration is absent from the current native row renderer; ordinary entry text uses primary color and the cell has icon/name slots | Source-verified; no dirty Explorer interaction was completed |
| User references 6 and 7 and current `HideUI.swift` | Search uses a generic kind icon and internal identifier subtitle; Recent Panels uses provider artwork but omits canonical status | Source and supplied-image agreement |
| Fresh native capture | One installed Hide app was identified and its window captured; Git has a large blank upper area and lower rows split branch/path text into narrow multiline columns | Installed screenshot, not proof that this checkout built the installed binary |
| Current `model.rs`, `sidebar.rs`, `HideUI.swift` search | Session identity exists; no persistent conversation-history reader or Sessions surface was found in the inspected source | This is a bounded search, not proof that no external provider can supply history |
| Current Pen inventory | Existing sheets cover panel/section headers, line/card rows, primitives and older as-built review rows; new workflows lacked dedicated component contracts | Six proposed masters added below |

Native observation used the installed bundle, an exact window capture, and verified Screen Recording/Accessibility permissions.
Attempts to navigate the installed window were refused by the automation tool because its inventory could not resolve the target for interaction; no click or keystroke was dispatched to Hide.
There was no app rebuild, installation, restart, pane creation, pane selection, process termination, or file operation in the product.
The native screenshot and tool evidence remain local under the review run directory, outside source control.
No performance, hover, keyboard, screen-reader, or new-feature native acceptance claim is made.

## Canvas map

Every name below has the prefix `Review / 2026-09-13 Agent workflow /`.
Node IDs identify editable boards in the one shared document.

| Board suffix | Node | Purpose |
| --- | --- | --- |
| Audit | `Mn89s` | Review direction and the seven requested areas |
| Proposal / Agent item component | `eo5fV` | Shared identity/state row and canonical status variants |
| Proposal / Workspace hierarchy | `QjBOk` | Expanded/collapsed parent and delegated-work grouping at 320pt |
| Proposal / Overview candidates | `BcACs` | Action list A versus primary comparison map B at 344pt |
| Proposal / Delegation structural alternatives | `tjKNd` | Earlier tree/map comparison; use the later graph visual for the latest visual direction |
| Proposal / Explorer changes | `Wyafr` | File letters, folder descendant decoration, selection, narrow/wide layouts |
| Proposal / Sessions | `Jamt8` | Workspace history list and user/assistant-only reader |
| Proposal / Search and recent | `WStaR` | One Agent item across search and recent navigation |
| Proposal / New agent and Quick workspace | `t6SuFD` | New Agent entry and project-independent workspace |
| Proposal / GitHub and Git details | `dlMBT` | Focused PR inspection, retained Git diagnostics, lookup states |
| Proposal / Team summary states | `x8Are` | None, one, many, unavailable, disconnected and escalated delegation |
| Proposal / Empty and unavailable | `TXiJD` | Empty, no results, partial, denied, remote and pending examples |
| Proposal / Explorer entry component | `f10VU` | Modified, untracked, added, renamed, conflict and clean variants |
| Proposal / Workspace work item component | `j62vn` | Normal, no PR, stale GitHub and no agent variants |
| Proposal / Session item component | `O2Egh` | Live, ended, no messages and partial-record variants |
| Proposal / Workbench visual | `QLNQG` | Full 1440pt workbench with shared agent identity, delegated group, parent summary and action-focused Overview |
| Proposal / Agent node component | `R0tOWH` | Reusable graph node with shared identity, ownership, direct-child count and selected/question/seen states |
| Proposal / Delegation graph visual | `nE4ek` | Parent/child/grandchild node-and-edge graph, selected-node inspector, Graph/List and viewport controls |
| Proposal / 01 All work full screen | `WZvs5` | Complete 1600pt application window: global attention, project team lanes, compact Git context and selected-parent message/history inspector |
| Proposal / 02 Project full screen | `psy2M` | Complete 1600pt project view: primary comparison table, delegation graph, selected-workspace files/history and agent/PR inspector |

There are 20 new review boards; existing current-screen boards and the previous review are retained.

## Whole-screen reading order

The original workbench demonstrates an active terminal, but is not the overview entry point for this proposal.
The two newer screens use the central application area for an integrated read view instead of squeezing the overview into a narrow side panel.
They are complementary navigation levels, not two competing layouts for the same scope.

`01 All work full screen` answers, in order: which parent needs my response, what each project is doing, which children the parent delegated to, and what the selected parent's last visible message says.
The global sidebar's project numbers count unique agents, not workspaces; accessible labels must state that unit.
The shared fixture has three projects, five workspaces and seven agents: two operator-demand parents, four Working agents and one Seen agent.
The same parent appears in the attention list, its relationship lane and the inspector, but contributes only once to identity-based totals.
Creator has one Question parent, two Working children and one Idle child; hide has one Working parent; presentation has one Approval parent and one Working child.
The global Git summary is explicitly project-scoped while the selected-parent file count is Workspace-scoped; creator's five changed files comprise two in master and three in review.

`02 Project full screen` answers which branch is primary, what each Workspace is doing relative to it, how the project's agents are related, and what files/messages belong to the selected work.
Selecting the review Workspace or its agent highlights the same subject and changes the lower files/history panels and right inspector to review; it does not filter the project-wide graph into a misleading partial tree.
The parent remains visible in master, even though the selected child is in review.
Comparison rows are Git comparisons, and graph edges are agent delegation; the interface never uses one connector to imply both.
For a Workspace with several agents, show a truthful agent count and explicit selection rather than pretending the representative task is the only agent.

All work is a proposed global navigation destination; project selection opens the project-level central view, and explicit agent-opening actions return to the active execution surface.
New Agent retains the relevant creation scope: global navigation offers the location chooser, while the project action starts with that project's Workspace choice.
The full-screen labels and navigation are proposals, not evidence these destinations already exist.
The complete-window designs reuse the Agent item, Agent node and Explorer entry masters and the existing token set.

Known empty, loading, partial, failed, disconnected and history-unavailable treatments inherit the state sheets in this review.
Unknown counts must remain unknown, unavailable message readers must show an unsupported/unavailable state, and no missing Git result may be rendered as clean or passing.
At more projects or children than fit, scroll the main content, keep attention reachable, and use explicit collapsed groups with truthful counts rather than shrinking all task titles to fit the viewport.
At smaller window widths, collapse the optional right inspector and open it on selection before reducing graph-node text sizes; the exact native width threshold remains an implementation decision.
The new screenshots verify the populated 1600pt layouts only; smaller-window behavior and live navigation are not verified by these static boards.

## Visual system and interaction details

Use the existing dark surface hierarchy: background for the working area, sidebar for navigation, panel for inspection, and elevated for selection and controls.
Use existing typography at 17pt for surface titles, 13pt for task identity, 12pt for controls, and 10pt for compact context; Korean task names wrap instead of being replaced by internal IDs.
Reserve blue, amber, green and red for semantic status; graph edges remain neutral so a connector cannot imply success or failure.
Selected nodes use both a boundary and a raised surface, independently of their activity color.
Graph cards earn their boundary because each is a selectable agent with its own identity and connection ports; ordinary navigation rows do not receive redundant boxes.
Use real icon, input, keycap, button and disclosure nodes rather than whitespace-aligned text approximations for the refined Search, Recent, New Agent and Explorer surfaces.
The full workbench is a layout demonstration, not a proposal to replace terminal rendering with message bubbles or to invent live terminal content.

The graph reads top to bottom with rounded orthogonal edges and fixed top/bottom center ports.
Each edge means one known direct delegation, never Git ancestry or project membership.
The selected child's inspector preserves the root context and exposes an explicit Open agent action; single selection only inspects.
Graph is the proposed primary visual mode following the user's latest direction; List is the keyboard-friendly and dense-team alternative, not a separate status model.
Keep deterministic sibling ordering, stable node placement on status-only updates, and no force-directed animation that moves the target while it is being inspected.
At large counts, preserve the selected root-to-node path, collapse sibling groups with truthful counts, and offer Fit to view, pan/zoom and List without hiding escalated attention.
Unknown relationships belong in a separately labelled unlinked group rather than fabricated edges; duplicate identities, missing ancestors and cycles require explicit data-quality handling before layout.
These large-team and malformed-data behaviors are handoff requirements, not a claim that the five-node static board implements a graph engine.
No animated motion, drag interaction, keyboard navigation or native accessibility behavior is verified by these drawings.

Design principles 5, 7, 8 and 12 are applied through shared component instances, visual ownership edges, restrained containers, and rendered mixed-script layout checks.
Principle 11 still requires a decision on unresolved structural alternatives before implementation; the user requested candidate drawings, not product implementation in this turn.

## Requested changes and implementation boundaries

### 1. Workspace lineage

Use a parent Agent item followed by one indented delegated-work group.
The group has a quiet left boundary; it does not connect to status marks or provider logos.
Independent parents share one leading column, so a reader can distinguish siblings from children.
Collapsed groups keep a canonical state breakdown; expansion changes neither execution nor read state.
At greater depth, open the full relationship view instead of squeezing each generation into less space.
Cross-workspace children retain their physical Workspace label, and a child appearing in two navigation contexts must not be counted twice.

Do not infer parentage from indentation, title similarity, branch names, or matching working directories.
Do not rename child completion to Done if the existing ownership/read projection says Idle or Seen.
The first proposal uses Working/Seen group counts, while each visible child retains its precise demand/status label.
The default collapsed treatment for stopped descendants is a proposal to decide before implementation, not authority to hide escalated attention.

### 2. Overview and GitHub

A is recommended: primary branch anchor, Needs You work, Working work, then collapsed merged work.
B uses a primary-to-workspace comparison structure for users who prioritize the relationship overview.
The B connector means comparison against the selected primary, not a Git commit-parent edge or evidence of where a branch was created.
Use the actual primary branch; never hard-code `main` or equate remote default and primary without the current policy.

Each item carries branch, representative agent, PR/CI, changed-file state, divergence, and an explicit next action.
Selection opens inspection; only a labelled Open/Return action changes the active pane and its read state.
Keep merged work collapsed by default and keep cleanup eligibility separate from PR merge status.
GitHub defaults to PRs matched to this project's registered workspaces, with stale/authentication/lookup limits available in details.
Do not use the latest-200 per-branch query as a repository-wide total or present absent check data as passing.
Do not equate passing checks with mergeability, review approval, or authorization to merge.

### 3. Parent-centric delegation

Keep the existing 28pt title row and 24pt conditional delegation row as the starting geometry.
Replace the child-name-chip strip with one summary control that opens a relationship detail.
The current contract explicitly chose name chips; the new proposal must be adopted before that contract is replaced.
The earlier A detail explores a tree beside an inspector and B explores a relationship map.
The later graph visual follows the user's explicit node-and-edge direction and is now the recommended visual starting point, with List retained for keyboard traversal and dense teams.
Viewport gestures, large-team collapse, focus order and native graph accessibility still require implementation decisions and real interaction checks.

Expose the parent, selected child's title and status, actual Workspace, and an explicit Open agent action.
Closing the detail returns focus to its invocation control; inspecting a child does not mark it read.
Unknown instrumentation is separate from unknown activity and from a confirmed count of zero.
Known children can be shown while explaining that more children may not be observable.
Use the existing stall escalation decision; do not introduce a new timeout or promote all child questions to operator attention.
Retired parents must not remain clickable live agents; retain only relationship information the history contract can actually supply.

### 4. Explorer Git decoration

Reserve a trailing Git-decoration slot while leaving normal file activation intact.
Use M, A, U, R and a conflict mark with semantic color; expose their full meaning in help/accessibility text.
Folders indicate descendant changes rather than pretending to be Modified files.
Deleted files belong in Changes, not as fabricated existing Explorer entries.
Unsaved editor state remains distinct from Git modification.
Non-Git and unsupported remote workspaces have no invented Git state; a failed Git read is an explicit failure rather than Clean.

The proposal is not approval to run Git per visible row or on hover.
The implementation must use the established change data and performance ownership contracts.
The 24pt sample row needs comparison with the current 22pt native tree geometry before adoption; no new spacing token was added in this design-only change.

### 5. Git absorption and Sessions

Keep Explorer for file navigation and Changes for reviewing modified files.
Move Git's base selection, divergence, upstream/pushed/fetch status, history, disk details and cleanup access into Overview inspection before removing its tab.
Maintain each existing action's scope and error recovery; do not discard features just because the tab is unpopular.
Saved Git-section selection needs an explicit migration to the corresponding Overview detail.

Sessions is a new read view scoped to the current Workspace, with provider, title, recorded time and a short conversation preview.
Show only user and assistant messages; do not expose tool calls, tool results, hidden reasoning, or raw terminal escape sequences.
Live status comes from the linked live agent; Ended describes a historical session, not the Done attention group.
Reading a session must not start a process or claim the session is resumable.
Offer Open current agent only when its live identity can be resolved.
The proposed wide reader can be a central document surface while the narrow Sessions list stays in the side panel; its final presentation must be selected before implementation.

Session identity alone does not provide historical content, durable lineage, completeness, permission, retention, or resume semantics.
Those data contracts are prerequisite work; do not mock the reader into production or scrape transient viewport text to pretend to offer complete history.
Read-only history should remain useful when the original pane has closed, but that requires a supported provider reader and an agreed retention policy.

### 6. One Agent item

The proposed master has status mark, task title, provider and Workspace context, with an optional trailing timestamp or shortcut.
The same underlying agent must keep the same title, provider identity and canonical status in Sidebar, Search, Recent Panels and delegation detail.
Use the actual supported provider artwork when implementing the existing badge slot; the proposal uses provider text, not invented brand artwork.
Search may group or rank matches, and Recent Panels must preserve MRU order; shared visuals do not imply shared sorting.
Keep File, Diff, Browser and plain Terminal results in Recent Panels with their own type identity and no fabricated agent status.
Move pane IDs to copyable inspection details instead of the primary search subtitle.
At narrow widths, retain status and meaningful task text; full titles/paths remain available through the shared tooltip and accessibility help.

### 7. New Agent and project-independent work

Remove Chat as a domain term from the proposed navigation, entry action, tooltip and shortcut description.
Use New Agent for starting an execution and Workspace for its working location.
Replace the separate Scratch domain with a fixed project-independent Workspace, provisionally called Quick workspace.
Existing Scratch contents, running sessions and files are preserved; the request is not authorization to erase them.
The current Workspace stays the default when starting an agent there, while Quick workspace offers a direct project-independent path.

The user suggested a temporary location such as `/tmp`; this design does not allocate or migrate any directory.
A directory in OS-managed temporary storage and a workspace expected to retain history have different lifetimes.
Decide that lifetime and recovery behavior before selecting the path.
Do not promise permanent file retention in a temporary directory or silently clean up active work.

## Additional findings

| Priority | Improvement | Why / acceptance focus |
| --- | --- | --- |
| P0 | Separate operator attention from delegated demand | A parent should not send the operator through every child question; escalations remain reachable |
| P0 | Preserve inspection versus focus | Opening a PR, row inspector or relationship modal must not acknowledge another agent |
| P0 | Treat incomplete instrumentation honestly | Known child entries plus incomplete observability need one coherent explanation, not zero or a false contradiction |
| P1 | Replace Git's wide field strip with vertical inspection | Fresh screenshot shows severe branch/path wrapping and large unused space |
| P1 | Give long identifiers progressive disclosure | Task titles get the main width; raw IDs and absolute paths stay in details |
| P1 | Separate project scope from workspace scope | Overview is project-scoped; Explorer, Changes and Sessions need a visible Workspace qualifier |
| P1 | One creation entry vocabulary | The existing plus and New chat row duplicate an action; use one consistent New Agent action and a location chooser |
| P1 | Persisted navigation migration | Removing Git and Scratch must retain meaningful selection and existing content |
| P1 | Keep stale PR state visibly qualified | A retained green check must not masquerade as a fresh successful lookup |
| P1 | Accessible graph/list alternatives | Keyboard traversal, full task labels, focus return, and non-color status meaning need actual native checks |
| P2 | Hide housekeeping behind its action | Allocation and cleanup are useful secondary details, not the first explanation of project activity |
| P2 | Preserve honest time semantics | Relative times need a defined event source; activity, last message and last fetch are not interchangeable |
| P2 | Dense and empty states both matter | Many descendants, long Korean titles, missing titles, no results, non-Git folders and remote failures are real states |

## Proposed masters and remaining coverage

| Proposed master | ID | Consumers / states |
| --- | --- | --- |
| Agent item | `Z5BtlX` | Sidebar, Search, Recent, delegation and live Session identity; Working, Question, Approval, Error, Done, Idle, Unknown, Disconnected |
| Team summary | `W2m8VC` | Pane delegation row; zero, one, many, unknown instrumentation, disconnected, escalation |
| Explorer entry | `mSu8p` | Explorer and state sheet; modified, added, untracked, renamed, conflict, clean |
| Workspace work item | `wa3TP` | Overview and state sheet; normal, no PR, stale query, no live agent |
| Session item | `ey7uz` | Sessions and state sheet; live, ended, no visible messages, partial record |
| Agent node | `uCQGy` | Graph nodes and state sheet; shared Agent item, selected boundary, ownership and direct-child count |

Masters are placed inside proposal sheets and referenced by the illustrated consumers and state examples.
Existing button, search, tab, tooltip, icon and badge owners remain the implementation starting point.
Explorer file examples now use the shared entry master, and graph nodes nest the common Agent item.
The complete migration must replace remaining equivalent manually drawn sample rows, tabs and summary controls with adopted masters; not every illustrative control on these concept boards is a component instance.
These are reviewable interaction/layout proposals, not a pixel-perfect native catalog or an implemented prototype.
Hover, pressed, focus and disabled treatments are specified using existing tokens; their live behavior is not tested by static Pen images.

## Decisions to make before implementation

| Decision | Recommendation | Alternative / unresolved part |
| --- | --- | --- |
| Overview structure | A: action-grouped work list | B: primary comparison map |
| Delegation detail | Node-and-edge Graph with adjacent inspector, following the latest user direction | List fallback and large-team keyboard/viewport behavior still need agreement |
| Project-independent workspace name | Quick workspace | General workspace; final product language remains undecided |
| Temporary workspace lifetime | Preserve existing contents and define retention explicitly | OS-temporary files versus persistent application-owned files |
| Sessions availability | Supported local provider history reader first, explicit unsupported states elsewhere | Provider coverage, remote access, retention and completeness are unresolved |
| Session reader placement | Central read-only document with Sessions list retained | Separate sheet; confirm against actual app width |
| Default descendant collapse | Keep active work visible and collapse stopped work | Existing disclosure preference and escalated attention must be preserved |

The node-and-edge visual direction is user-requested; detailed interactions and the remaining recommendations are design judgment, not blanket implementation approval.
Design principle 11 requires choosing a structural candidate before product implementation; it does not prevent drawing the candidates requested here.

## Handoff order

1. Review the Overview/delegation candidates and agree on vocabulary and temporary-workspace lifetime.
2. Adopt the shared Agent item and Workspace lineage, preserving status/read/ownership behavior across all consumers.
3. Adopt the parent summary and delegation inspector, including incomplete/retired/cross-workspace relationships.
4. Implement the selected Overview and focused PR/Git details; migrate every Git function before removing the tab.
5. Add Explorer decorations from supported change data, then verify native selection, long paths and conflict/remote states.
6. Establish the Sessions data contract, then implement the list and reader; do not make Git consolidation depend on unsupported history ingestion.
7. Replace New Chat/Scratch terminology and navigation with a content-preserving Workspace migration.

For each implementation change, update the owning `DESIGN.md` and active references together, promote accepted proposal masters into `Component /`, update the corresponding `Screen /` boards, and remove the adopted Review boards.
Do not promote all proposals automatically or treat this note as a human-approved PRD.
Read current architecture, status and performance contracts before changing their owning code; this note does not introduce new Herdr methods or a new runtime owner.

## Review checks

The native baseline was captured from one identified installed app.
The proposal boards were rendered with Pen and inspected for layout, mixed Korean/English text, and overflow; direct board references are in the canvas map.
Narrow panel examples include 320pt and 344pt, with a wider Explorer comparison; native 320/344/400pt acceptance remains implementation work.
Run `node scripts/gen-pen.mjs`, then `node scripts/check-design-contract.mjs` before committing the canvas.
The passing static checks prove token/band consistency, not design approval or real interaction behavior.
Keep native captures, rendered exports and verification transcripts local under `agents/runs/`, never in this committed design note or the canvas as embedded workstation screenshots.
