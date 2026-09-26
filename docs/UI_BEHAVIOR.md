# UI behavior

This document owns what Hide's UI does: the rules for how each screen and control behaves, independent of any platform's exact pixel values.
It replaces the behavioral content of the retired `DESIGN.md`.
Visual authority (what a control looks like) lives in the Pen library and `web/src/components/ui`/`web/src/components`, described in [DESIGN_WORKFLOW.md](DESIGN_WORKFLOW.md); numeric authority lives in `design/tokens.json`.
The native macOS shell (`macos/`) still ships until S6 and is frozen for this change: `HideTheme.swift` is not regenerated or hand-edited, and its own design-contract tests are gone.
Where a rule is currently native-only, its Swift owner is named below; a web owner is added wherever the web shell (`web/src/`) already implements the equivalent behavior.

## Web Workspace

The web shell's Workspace screen follows the approved S6 proposal, and its View areas follow the approved boards of PRD S7 (`agents/prd/workspace-views-layout/prd.md`).
Web owner: `web/src/WorkspaceScreen.tsx`, `web/src/ViewAreas.tsx`, `web/src/Tools.tsx`, `web/src/viewLayout.ts`, `web/src/viewDrag.ts`, `web/src/viewFocus.ts`.

The toolbar reads left to right: the path back (`Main / Project / Workspace`, naming the device when it is not this Mac), the layout switch, then the two tool toggles.
The layout switch offers three adjacent choices in one group - Agents only, Agents and Views, Views only - with the chosen one visually distinct from the others; the same three names appear in its menu and as palette commands, and each choice's tooltip is its accessible name.
Explorer and History are independent toggles that open and close on their own, drawn pressed while shown; with both shown they share the tool column, Explorer above History, each with its own close.
A right-click or the menu key on the toolbar offers the layouts, each tool, Copy Workspace path, and Open Project Overview.

Agents and Views sit side by side, each region with its own tabs, and a boundary between them drags with a guide line and lands once on release.
An empty Agent area offers New tab.
Changing the layout only changes space: it closes no tab, document, or pane, and choosing Views only never makes a split; a split comes only from a split command or a drop on an edge.

An Agent tab carries its provider's mark (Claude, Codex) or a neutral terminal mark for any other kind, and never replaces Herdr's own tab name; its tooltip and accessible name carry the kind, the full name, and the agent's state.
A View tab carries the file-type mark, and a diff tab the comparison mark, so a kind is never told by color alone.
Its title is cut at the tail to fit, and its tooltip and accessible name carry the kind, the full path, and `Preview` or `Unavailable` while the view is one, so a long path stays readable.
A dirty view shows a warning-colored mark after its title.
Closing a view is always called Close view, distinct from moving a file to the Trash and from closing a pane or tab.

### View areas

The Views region holds one or more View areas, each with its own tab bar above its own view, split left/right or up/down as often as the limits allow.
A split divides one area in two along one axis, and either half can split again along either axis, so every arrangement of side-by-side and stacked areas is a tree of halves.
One area is active: the next file opens there, the palette and the keyboard act on it, and only its active view's tab carries the accent indicator.
Every other area still shows its own active view's tab, with a primary title and no indicator, so the operator sees what each area holds and which one is in charge.
An area whose tabs outrun its width scrolls its own strip so the shown view's tab stays in sight whenever the shown view changes or the area is resized.
Clicking a tab or a view, or moving focus to another area from the palette, makes that area active.
The divider between two areas turns accent-colored on hover and keyboard focus, drags with a guide line, and lands once on release, so a drag never resizes a document or a terminal on every pointer move.
A focused divider moves with the arrow keys along its axis, one step and one change per press.
No area gets narrower or shorter than its minimum, a divider stops where either neighbour would, and each side of a split keeps between 15 and 85 percent of it.
An area whose last view leaves disappears, and its neighbour takes the space.
When the last view of the whole Views region closes, one empty area stays and says that no file or diff is open, with Show Explorer when the Explorer is hidden and Open file; the layout does not change on its own.

A Workspace holds at most 6 View areas, no area sits more than 3 splits deep, and at most 64 views are open at once.
A split past the area or depth limit is refused with its reason, and an open past the view limit says so and keeps every currently open view as it was.
Opening a file that is already shown moves to its view, so it is never refused.

### Opening, preview, and Open to the side

A single click on an Explorer file or a History row opens it in the active area's preview view (italic title), and the next single click replaces that preview in place, so browsing leaves one tab per area rather than a trail.
Each area has at most one preview, and a click never touches another area or a pinned view.
A double-click on the row or the tab, Keep open, or the first edit pins the preview where it is; because the first edit pins, a document that is dirty, saving, or whose save failed is never a preview in any area that shows it.
Opening a file that is already shown moves to its view instead of adding a tab, choosing the one used last when several views show it.
Opening a file from Agents only switches to Agents and Views and focuses the active View area.
Diffs are placed by the same rules.

Open to the side (from the Explorer's file menu, a History row's menu, or the palette) is the only way to show one file twice.
It puts a pinned second view in the area beside the active one, trying right, then left, then below, then above, or in a new area on the right when there is only one area; when that area already shows the file, its view is focused instead, and with no view open at all it opens in the empty area.
From the only area, Open to the side is a split, so it is offered only where Split right would be, and otherwise stays listed, disabled, with its reason.
Both views show one document: an edit in either appears in the other at once, and each keeps its own scroll position, cursor, and selection.
Closing one of them leaves the document, its text, and its unsaved state in the other; closing the last one goes through the same save and conflict protection every document close has.
Dragging a tab moves its view and never copies it.

### Dragging a view

A dragged View tab lifts a floating copy that follows the pointer, while the tab keeps its place and nothing on screen resizes.
Over a tab bar, an insertion line marks where the tab will land: in its own bar the drop reorders, and in another area's bar it moves the view there with no split and no copy; when that area already shows the same document, the moved view lands at the line and that area's view of the document gives way, so no area holds one document twice.
Over the left, right, top, or bottom edge of an area's content, the half of that area the drop would create is highlighted with a short label such as `Split right`, and the drop creates that area and moves the view into it.
Only one destination is highlighted at a time, and moving to another edge or bar replaces it.
Where the view cannot go - it is the only view of the area whose edge it is over, the area cannot be halved at its minimum size, the Workspace is at its area or depth limit, or the target is not a View area - no overlay appears and the pointer shows the forbidden cursor.
Escape, a release outside a valid target or outside the window, and a target that disappeared or became ineligible before the release all keep the original order and layout; only a valid drop changes the layout, once.
Nothing is resized, reattached, or saved while a drag is in progress, and the drag itself is never stored.
A drag never moves a view to another Workspace, never turns an Agent tab into a view, and never splits a terminal.

### The View tab menu

A right-click on a View tab, or the menu key while the tab has focus, opens its menu with these items in order: Keep open, Split right, Split left, Split up, Split down, Move right, Move left, Move up, Move down, Copy path, Reveal in Explorer, Close view.
Keep open appears only on a preview view.
A Move item appears only toward an area that exists in that direction, and moves the view into it without a split.
A Split item the Workspace cannot make stays listed, disabled, with its reason under it, the way every web menu draws an item its target cannot use.
The labels say where the view goes, never "Move to Group".
Close view closes the view and never the file on disk; the menu has no file deletion and never closes a pane or a tab.
Opening the menu moves no focus and changes nothing, and Escape closes it.
Every item is also a palette command for the active view, and the palette adds Open to the side, focus to the next or previous area, and resizing the active area, so every split, move, close, and resize can be done from the keyboard.
A palette command that cannot run now is drawn muted with its whole reason under its title, and picking it does nothing.

### View states

A view whose file is being read shows `Opening…`.
A restored view whose device or root is not ready says what it waits for (such as `Waiting for <device> to connect`) and reads its file by itself once that is ready.
A view whose file cannot be read shows why, with Close view and Retry, and its tab title reads as struck through; either action acts on that view alone and leaves the active area where it was.
Each state belongs to its view alone, so one missing file never blanks another view or area.
After a restart the app reopens the last Workspace as it was left: its areas and their sizes, each area's tabs in order with its preview, pinned views, and active view, the active area, the layout, and the tools; unsaved text returns from the browser's drafts, and Herdr's current tabs and panes are used as they are.
A first run, or a last Workspace that no longer exists, starts on Main.
A layout file that cannot be read is kept aside and the app starts on Main, where the operator picks a Workspace and continues with a new layout.

### Narrow windows

When the window cannot give the working regions their minimum beside the tool column, Explorer and History open as a temporary overlay above the working regions instead of a column; Escape or a click outside closes it and returns focus to the toggle that opened it.
When Agents and Views cannot both have their minimum width side by side, the region used last fills the width and an explicit switch leads to the other.
When the View areas cannot all have their minimum, only the active area shows, with an area switcher to the others.
Widening the window brings back the chosen layout, the area sizes, and the tool column, because none of these narrow arrangements is stored or sent to the core.

### Library masters

The View area masters in `design/hide-ui.lib.pen` are `Component / View tab`, `Component / View insertion line`, `Component / View split overlay`, `Component / View tab menu`, and `Component / View area message`.
Their sheets draw every state as refs: the View tab sheet draws preview, pinned, hover, active-in-the-active-area, active-in-another-area, dirty, unavailable, diff, a long title, and the floating drag copy; the placement sheet draws a reorder, a move into another area, a right and a down split, and an ineligible target; the tab menu sheet draws a preview's menu and a pinned view's menu with a disabled Split and its reason; the area states sheet draws the empty Views state and each view's opening, waiting, and unavailable states.

### Agent panes and the Agents explorer

Several View areas leave the Agent side as it was already drawn: the layout switch, the independent tool toggles, and the child chips below behave the same with one area or six.

A pane whose agent delegated work shows every direct child on one row under its header, each chip a status mark, the provider mark, and a capped title; the row scrolls sideways instead of growing, and a pane with no children has no row.
Its library masters are `Component / Pane child chip` and `Component / Pane child row`; the native pane header keeps its first child and `+N` until the native shell changes.
A chip opens the existing child at once; while that move is in flight the chip shows a pending mark and repeats of it are ignored, and a failure shows the core's reason under the header with Retry (when the core says it can be retried) and Dismiss.
A child pane has a compact Return mark in its identity row, named with the parent in its tooltip and accessible name.
The pane menu (from its overflow control or a right-click on the header) lists the parent, the other siblings, and the children as explicit Open items, then Copy pane name and Close pane; opening it moves no focus and marks nothing read.

The Agents explorer groups every current agent, this machine's and each connected device's, under Needs You, Done, Working, and Seen, and leaves an empty group out; a device's row names its device before the agent kind, and a device that is not connected lists nothing it only last reported.
Each group lists its root rows; a delegated row is drawn only beneath its parent, indented one step per level and muted, while the operator has that parent unfolded.
A group's heading counts every agent it speaks for, its roots and all their live descendants whether folded or not, so each agent is counted once, under its root's heading.
Descendants start folded; the parent's chevron folds and unfolds them, and the choice is the core's `expanded_agent_pane_ids`, so it survives a restart.
A folded parent with live descendants carries a badge after its title: one mark and count per state (error, approval, question, working, done), summed over every live descendant, or `↳N` when all of them are merely ready.
The badge is a button: a click, Enter or Space opens a list of the direct children with their status mark, name, status word, branch when it differs, and elapsed time; the arrow keys move the highlight, Enter or the highlighted row's arrow opens that child's pane, the last item unfolds the children in the list, and Escape closes it and returns focus to the parent row.
The list has no Stop action, drops a child the moment it leaves the projection, and closes when no child is left.

A root whose own turn is over while a descendant still works or asks is waiting on its children (docs/status-model.md): it stays in Working with its ring in the working color, and its badge, not its own sentence, says what is going on.

An agent row's first line is always its status mark, provider mark, stable task name, a branch chip only when a delegated row's checkout differs from its parent's, a device chip for a row on an SSH device, the badge, and the elapsed time.
A second line appears only when the row has something to say: a question, approval or error keeps its request in the warning color (red for an error) until it is resolved, however often the row is read; a row that changed since the operator last looked shows its sentence bright until it is read; and the selected or hovered row reveals its full sentence over up to two lines, with the whole of it in the row's tooltip.
Every other row is one line, and no row draws a progress number or step the agent did not report.
Web owner: `web/src/agentRow.ts` (rules, reusable by any list of agents), `web/src/components/agent-row.tsx`, `web/src/components/agent-children-popover.tsx`.

## Web Project Sessions

A Project's Sessions is a web screen of its own (PRD S8): the Overview's header offers `Sessions`, and the path back reads `Main / Project / Sessions`, naming the device the way Overview does.
Web owner: `web/src/SessionsScreen.tsx`, `web/src/sessions.ts`.

It lists the history of every Workspace the Project has, works for a Project with no Workspace, and never runs an agent or sends a session to a Workspace.
Above the list are the `All / Codex / Claude Code` choice, the search, and the count (`N sessions` or `N of M sessions` while a filter narrows it); a read in flight adds `Reading…` beside the count and keeps the rows.
Each row shows the provider mark and name, the time, the first request or title in at most two lines with the full text in its tooltip, and the checkout it ran in.
A session with neither request nor title reads `Untitled session`, muted, and a time it never carried is left out.
A row's accessible name reads provider, first request, checkout, time, and availability, in that order.
The open session's row is the list's one Tab stop; the arrows, Home, and End move between rows, ArrowDown from the search lands on that row, and Escape in the search clears it.
The provider choice is one Tab stop whose arrows choose the neighbouring provider.
An unreadable session dims only its own row, marks it unavailable, and keeps its reason, Retry, and Copy source location under it, copying the provider file's own path.
A session whose file is no longer found after the history listed it stays listed the same way, with an explanation that it may have been moved or deleted, and its last location to copy.
The list place shows one small mark per state: loading, no sessions yet, no matching sessions (with Clear filters), sessions could not be read (with the reason and Retry), and on a device Project, sessions unavailable with the device's reason and no Retry.
The open session shows its row's provider, checkout, and time with Copy source location, its request as the title, then each request and answer in order as plain text; injected context, where Project Memory travels, is not shown.
A session that cannot be opened shows the same reason in the detail place with Retry, which reads the history again and then the session.
When another window names another Project, this one says so and offers `Show this project's sessions`, which names this Project again only when chosen, so two windows never take it from each other.
The web shell has no Project Memory entry point, disabled control, or placeholder until the Memory stage (PRD S8); Memory is managed in the macOS app.

## Terminal image attachment boundary

Web owner: `web/src/attachments.ts`. Native owner: `TerminalFileDrop.swift`, `ImeTerminalView.swift`. Core owner: `herdr-core/src/runtime/attachments.rs`, `herdr-core/src/remote/attachments.rs`.

Dropping local file URLs into a visible terminal focuses that receiving pane and starts one attachment intent through the existing ordered writer.
Paste captures PNG or TIFF clipboard images at the same ingress; ordinary text and keys keep their existing terminal behavior.
The core reserves the original pane and connection generation before asynchronous image preparation, file validation, or transfer, so later focus changes cannot redirect the attachment.
Clipboard images are normalized into private PNG files off the input thread, while the core owns transfer state, held input, retry, and cancellation.
Local files keep their original paths after validation; remote attachments are uploaded through authenticated SFTP and insert only the resulting remote paths.
When the terminal advertises bracketed paste, each path has its own paste frame and the complete drop is enqueued once in file order; otherwise the paths are inserted as shell-quoted text with a trailing space.
Spaces and apostrophes survive; shell expansion characters are escaped, and paths containing control characters, symlinks, directories, and special files are refused with an actionable notice.
Nothing performs automatic Enter or replaces existing prompt text.
One transfer is admitted at a time, capped in file count and size; clipboard decoding is also capped by megapixels.
The original terminal's later input is held up to a fixed size, then explicitly refused rather than growing the queue or silently submitting an incomplete prompt.
Only a successful transfer to the original live generation releases the attachment followed by that held input.
A compact notice above the affected terminal shows preparation, upload, or failure and offers Retry where safe, or explicit cancellation that discards held input.
An original pane that closes or reconnects invalidates the intent; neither files nor old input are forwarded into a replacement session.
Files older than 24 hours are pruned on the next attachment attempt, not by a resident background cleaner, so successful local clipboard references remain readable after the paste.
Failed or cancelled transfers clean up only their own created files where the destination remains reachable, with deferred cleanup recorded diagnostically.
It inserts paths even when a provider cannot decode the referenced file; provider validation remains visible in its own composer.
Hide provides no thumbnail shelf, attachment membership, or synchronized removal; subsequent editing and submission remain native provider operations, and pasting a path is not proof of image acceptance.
Reintroducing a shared attachment shelf requires a supported provider contract for stable attachment identity, idempotent add/remove, native draft changes, and accepted submission events; both surfaces would have to reflect the same attachment membership, and the shelf would have to clear only after confirmed submission, preserving items on failure.

## Project Home

Native owner: `HideTheme.Home` and the SwiftUI Project Home views.
Web owner: `web/src/ProjectOverview.tsx` (the Project Overview screen), `web/src/projectBoard.ts` (the board rules); a card's agent row is the Agents list's `web/src/components/agent-row.tsx`.
Both shells place a checkout by the same rules; `web/src/projectBoard.test.ts` carries the Swift `ProjectHomeTests` cases.

Project Home uses the shared tab choice, badges, agent identity marks, settings field, icon buttons, and command tooltip.
Tasks is the session default, with ad hoc requests above four Git-derived columns; Agents reuses each card in three canonical lifecycle columns.
A checkout's stage is merged when its worktree or its pull request is merged, review with an open pull request, working with changed files or commits ahead, and ready otherwise; agents and the issue's Project status never move it.
Merged is Git ancestry against the base the core resolves, so a branch with no commits of its own reads as merged once its base resolves.
The ad hoc strip holds the checkouts that are not linked worktrees (the primary checkout, a folder) and only while an agent works in them; an open issue no checkout is linked to is a backlog card in ready.
Needs You uses the warning halo and an error uses danger, and raises the card to the top of its own column without moving it out of its Git column.
Only the current delivery fact and linked issue appear in the footer; an issue whose Project status names another stage carries a mismatch chip whose tooltip names both.
GitHub's age appears only in the issue chip's tooltip, as the last successful read, never as a banner.
Merged and Seen columns start collapsed and list only their names while folded.
The board scrolls horizontally below its column minimum, and titles wrap to two lines; branch labels truncate at the tail with the full text in a shared tooltip and accessibility help.

On the web, the board is the Project Overview: the sidebar's project name (a plain folder's one row opens its checkout instead), Main's project row, the palette and the Workspace toolbar menu open it, and ⌘⇧H opens it for the checkout in front.
Escape, once no dialog or menu is open, returns to the Workspace in front, or to Main when there is none; an agent row opens its pane and a card header opens its checkout.
The header carries the path back, the Tasks/Agents choice, the worktree count, the open pull-request count once GitHub has answered, main's distance behind origin only above zero, Sessions, and New agent.
New agent opens the New worktree dialog on a Git project and the folder's Workspace otherwise.
A project with no agent at all shows only an empty state with New agent, a folder with agents only the ad hoc strip, and before the first snapshot the shell's own connecting state shows instead.
While hided or a device is unreachable the board keeps the last snapshot and the existing connection or device line is the only signal.
A card's agent row follows the Agents list's row rules above (`web/src/agentRow.ts`): the same first line, second line and branch chip, and the core's waiting-on-children ring.
The whole lineage is drawn inside its card whatever the sidebar has folded, so a card row carries no chevron and no descendant badge.

## Explorer file management

Web owner: `web/src/ExplorerTree.tsx`, `web/src/explorer.ts`. Native owner: `WorkspaceOutlineView.swift`, `WorkspaceOutlinePresentation.swift`, `changes.rs`.

The tree's context menu follows VS Code's order: New File, New Folder, a separator, then on a file row Open with Default App, Open in Browser Pane and a separator, then Reveal in Finder, Copy Path, Copy Relative Path, a separator, Rename, a separator, Delete.
A folder row has no open items, because its open is Reveal in Finder; the empty area below the rows stands for the root and offers only the two creations; a remote tree is read-only and offers only the two copies.
The item set the menu offers is a presentation decision a test can check directly, not something the platform decides implicitly.

Open with Default App hands the file to the OS through the existing external opener, and a refusal is reported with the path and the reason.
Open in Browser Pane stays in the menu whether or not it can act; when it cannot, the item is disabled with its one reason in the order the operator can act on it: remote files open on their device, opening in progress, Node.js is not on PATH, not connected to Herdr, no focused pane to open beside.
The menu does not auto-enable; the disabled state is the presentation's decision.
The outcome of an open is a notice: the host's own sentence when it refused, a message that no chromux profile is running with the launch command to fix it, or a timeout message after thirty seconds.
`docs/BROWSER_PANES.md` owns how the pane is opened and which profile is chosen.

Delete has two entry points, the menu item and a delete shortcut while the tree holds the keyboard, and both end in the same confirmation: an alert asking to move the item to Trash, telling the operator everything in a folder goes too, and that the item can be restored from Finder, with Cancel as the default action and Move to Trash as the destructive one.
Nothing reaches the core without that alert; Cancel and Escape send nothing.
The delete shortcut is scoped to the tree so a pane command cannot be rebound onto it, and it never reaches the terminal by mistake.
The item goes to the OS Trash, never to a permanent delete; the selection moves to the next sibling, else the previous one, else the parent, decided by the tree and carried in the same event so the cursor lands in the same frame as the removal.
The event also carries the item's identity as read when the prompt opened, and the core refuses an item that was replaced at that path while the modal was open, so what leaves is what the modal named.
A failed move keeps the item and puts the reason on the row under it, the way a refused name is shown.

New File, New Folder, and Rename take the name in the row itself: a draft row is inserted at the top of the target folder, or the item's own label becomes the field.
Enter sends, Escape and any other loss of focus cancel, and an unchanged rename closes the field without asking anything.
An empty name, a name with a path separator, and a name already used in that folder are refused before the round trip; the field stays and the reason is one row directly under it.
The field draws on an elevated surface so it reads as an input among labels; nothing else about the row changes.

The filesystem change is the core's: one event carries the request, the core refuses paths outside the focused checkout and any overwrite, runs the exclusive call off the runtime mutex, and settles one operation slot.
The tree reads a finished slot to re-read only the folders it touched, keeping every loaded folder and its expansion, and a failed slot to place the reason under the row the change started from.
The selection moves to the new or moved item because the core sets it explicitly; expanded folders and open file tabs inside a renamed folder follow it.

A drag moves one item inside the tree.
Dropping on a folder puts the item inside it, on a file puts it beside that file, and on the empty area puts it at the root; the receiving folder row is what highlights.
The same parent, the item itself, and a folder inside the item show no drop indicator and accept nothing.
The drag never leaves a copy that a different app (such as Finder) could read as a file.

Git status decorates each row with one status mark: Modified, Added, Untracked, Renamed, and Conflict render as `M`, `A`, `U`, `R`, and `!` with a semantic color and a matching status name in tooltip and accessibility help.
A folder with any changed descendant renders a dot mark; the mark describes derived folder state and never relabels the folder as a modified file.
Deleted descendants still mark an existing ancestor folder but never create a file row that no longer exists.
Clean and unavailable decoration both reserve the slot, while loading and failure are distinguished by a panel notice above the still-usable tree.
The decoration is not a control and cannot intercept file open, disclosure, inline editing, drag, keyboard navigation, or the context menu.

Git state comes from the root-scoped History projection.
Rename keeps both previous and current relative paths, conflict remains an independent status, and folder state is derived from the complete changed set rather than only loaded outline children.
Explorer visibility reuses the History reader's bounded refresh outside the runtime mutex.
Switching Workspaces replaces the decoration root, and no per-row, hover, selection, or scroll path starts Git.

## Editor preview tab

Native owner: `runtime/editor.rs` (`place_editor_tab`, `promote_editor_tab`), `EditorTabSnapshot.preview`. The web Workspace's one-preview-per-View-area model is the same idea applied per area; see [Web Workspace](#web-workspace).

A single click on an Explorer file or a History row opens it in the checkout's one preview tab (VS Code's model): the strip draws the title in italic, and the next single click replaces the tab in the same slot instead of adding one.
The core owns the preview flag and decides replacement and promotion; the shell only says what the click meant.
Promotion happens in the same slot, on four triggers: a double-click on the Explorer row, a double-click on the tab title, the first edit, and Keep Open; a drag to a new slot promotes as well.
A dirty tab is never replaced: the core promotes it where it sits and opens the new preview beside it.
Every other entry point - opening a file by path, Reopen Closed Tab, a Markdown or terminal link, a file the Explorer just created - opens an ordinary tab, and a single click on a file that already has a tab focuses it without touching the slot.
A replaced preview tab is not a close: its document, mode, and wrap state are dropped and nothing enters Recent Closed; closing the tab yourself records it as any file tab.
Editor tabs stay ephemeral, so the preview flag is never persisted.
The tooltip and the accessibility label read `name · Preview` while the tab is one and drop the suffix on promotion; the tab's colors, close button, and its Recent Panels row are the ordinary tab's.

## File document toolbar and Markdown

Native owner: `EditorViewerOverlay.swift`, `PDFDocumentView.swift`, `MarkdownLiveEditor.swift`, `MarkdownLiveSource.swift`, `MarkdownListEditing.swift`. Web owner for the editor surface: `web/src/Editor.tsx`.

The central file surface uses one document toolbar, preserving the tab strip and Explorer.
The current folder and filename give context; Find uses the platform's native find bar, Wrap changes the source text container, and reveal actions target Explorer and Finder.
Unsaved drafts and the existing read-only/conflict notices remain visible in every mode.
Diff tabs retain their own viewer.

The core names each open file's kind (text, markdown, image, pdf, binary), and the overlay picks the adapter from it; the toolbar is the same bar in every kind, with controls a kind cannot use taken away rather than left dead.
A PDF (recognised by its signature whatever its name) shows in a continuous, width-fitted, text-selectable, non-editable view.
Its toolbar keeps the breadcrumb and the two reveals, shows Find disabled with the reason that Find is unavailable for PDF, and hides Wrap, the Markdown mode group, and Unsaved, which a PDF can never earn.
A PDF that cannot be decoded, cannot be read, or is password-protected shows a `PDF unavailable` state with the reason under the same toolbar.
An image hides Wrap as well; a file that is not UTF-8 shows a `Preview only` state explaining the file type cannot be shown as text, and keeps Wrap disabled beside a disabled Find.

Markdown files alone show the centered Live/Source choice, and Live is the default.
Both are editors over the same draft: Live draws the formatting in place and hides the markup on every line the caret is not on (the way Obsidian's Live Preview does); Source is the monospaced editor with its line-number ruler and Wrap toggle.
The core owns mode and source wrapping per open file tab; another tab has independent choices, returning to a tab restores them, and close/reopen or app restart starts Live with source wrapping off.
Autosave captures its file identity when scheduled so a subsequent tab selection cannot redirect the write.
Closing a file tab carries that tab's matching pending save in the same close intent, and the tab remains open with a visible error if the exact path and contents cannot be saved.
The editor retains only its latest unacknowledged draft while older core snapshots arrive, preventing a snapshot echo from moving the caret or replacing newer input.

In the Live view, the line holding the caret, and every line a selection crosses, shows its source; a fenced code block is one unit, so a caret anywhere inside it shows both fences.
Markup hides again the moment the caret leaves, with no animation.
Headings hide their hashes; bold, italic, and strikethrough hide their delimiters; inline code and fenced blocks use the editor's monospaced font; unordered markers draw as a bullet and ordered markers keep their digits, both with a hanging indent; a quote indents behind a bar and hides its `>`; a link shows its text in the accent color with the brackets and URL hidden; a `---` line draws as a rule.
Tables, images, HTML, footnotes, and task lists are not drawn: they stay monospaced source, editable in place, and a parse the view cannot use leaves the whole document monospaced with the reason in the notice bar while typing continues.
An empty Markdown file is an empty Live editor with the caret in it.
Links open on modifier-click only: HTTP(S) in the external browser, a relative file inside the current checkout as an Explorer reveal, and anything else as a caller-visible notice; a plain click places the caret.
Raw HTML is inert literal text, never a browser execution surface, and no image resource is read or fetched.
A document over 256 KB opens in Source with Live disabled and a notice that Live preview is off for files over that size, because Live re-parses the whole document after each edit.
That re-parse runs off the main thread, one at a time, with a burst of keystrokes coalescing into at most one more; attributes are re-applied only over the region whose plan changed.
No re-parse lands while an IME composition is marked, so composed input is not interrupted.

In both modes a Markdown document answers list-editing keys the same way: Enter after an item's text starts the next item with the same marker or the next number; Enter or Backspace on an empty item removes its marker and leaves the list; Tab and Shift-Tab move an item one level, with a numbered item counting in the column it joins and the column it left renumbering from 1; a lone `1. ` line stays as typed on Enter because it may be text.
None of this runs while a composition is marked, and a task box is not treated as a list item.
The text that results is ordinary Markdown, with nothing hidden or special in it.

## Projects and checkout context

Native owner (sidebar tree): `SidebarPresentation.swift`, `sidebar.rs`. Native owner (Overview panel): `OverviewPresentation.swift`, `CheckoutOverview.swift`, `project_context.rs`, `worktrees.rs`, `disk.rs`, `worktree_cleanup.rs`, `runtime/projects.rs`. Web owner: `web/src/sidebar.tsx`, `web/src/projects.ts`.

### Sidebar hierarchy

The sidebar hierarchy is Project > Workspace > Agents; a Workspace corresponds to one checkout path, including a plain folder.
Two checkouts of one repository share a project cycle.
Each checkout row shows a kind glyph, selected in priority order: the pull-request lifecycle icon when current GitHub data has a pull request, then branch, home for the primary checkout, commit for detached HEAD, or folder for a plain folder.
Open, draft, merged, and closed pull requests keep their own lifecycle shapes and colors, stale GitHub data mutes only the icon, an unavailable GitHub lookup falls back to the branch glyph, and a missing folder colors its branch glyph as danger and omits the age.
Workspaces with nested agent rows toggle disclosure across the whole row; workspaces without nested agent rows open on click.
In the web shell a checkout row always opens its checkout: a trailing chevron, drawn only while agents run there, opens and closes their rows, and the row's `⋯` menu takes the last-commit age's place while the pointer is over the row.
A web checkout's agent rows start closed, so its second line names them, and the checkouts the operator opens are kept in the core's ui state across launches; the Swift shell keeps its own disclosure until it is removed.
A web project row folds its checkouts from its leading chevron, and the rest of the row opens the project's Overview; both folds are this machine's, so a selected SSH device's tree is drawn with nothing folded.
A plain folder, a project that is not a Git repository and holds one checkout, is one web row instead of a project row over an identical checkout row.
Its first line is the project's folder glyph, name and activity, set in the checkout row's columns with the activity where the age stands; its second line and trailing chevron are the checkout's.
It has no fold of its own and keeps the fold's lane; the row opens the checkout and is marked while that checkout is in front, its menu lists the project's items and then the checkout's, and its Overview is reached from Main, the palette or the Workspace toolbar.
While a checkout's agent rows are closed, its second line names them, the representative agent's mark and provider and `+N` for the rest, before the purpose; a checkout with neither, or one whose Git facts have not been read yet, is one line.
Workspace disclosure persists across launches and hides only the nested agent rows, preserving selection, running panes, and raised attention rows.
An agent row's title is its identity label at both densities: the rolling task, or the workspace label when no task exists; a Herdr agent name remains a control identifier and never becomes display copy.
A row whose descendants are folded, and every raised row, wears a descendant badge counting live descendants by state before the elapsed time; opening the fold removes the badge because the opened rows carry their own marks.

### Purpose, pinning, and PR chrome

A checkout's one-line purpose is set from the checkout row's or Overview header's `Set purpose…` context item, with a character count and a warning near the limit; saving an empty purpose clears it, and display falls back through branch description, representative agent title, and pull-request title in that order.
A registered project can be pinned from its row menu or right-click menu; pinned projects are drawn once under a `Pinned N` section between the raised groups and the activity-ordered project list, in the tree's own order (device first, then latest activity), and only while at least one project is pinned.
The pin lives on the project's registration and survives a relaunch; removing the registration takes the pin with it.
A pinned project is exempt from its device's inactive fold whatever its activity; its own stale worktrees still fold behind their own `Inactive N` row.
PR lifecycle color is a semantic-color exception to otherwise neutral chrome: Open, Merged, Closed, and Draft each keep a fixed color shared between the sidebar glyph, the Overview popover header, and the state badge, including during hover and selection; review decisions and CI keep their own separate status meanings.
Clicking the lifecycle icon, or choosing `Open PR #N` from the checkout context menu, opens that pull request in GitHub; the rest of the row keeps its disclosure or selection action.

### Project ordering and inactive folding

Projects and their checkouts sort by the latest authoritative agent activity timestamp or Git commit timestamp, descending; missing activity sorts after known activity, and no UI interaction or local clock invents recency.
Activity orders projects inside one device group and never across two.
A project's merged, closed, or seven-day-inactive secondary checkouts move behind a trailing `Inactive N` disclosure, while its primary checkout and every checkout with live work, local changes, unpushed commits, or current focus remain visible.
When every checkout in a project is inactive, the project itself moves behind the device's `Inactive projects N` disclosure.
Both folds default closed and remember their expansion independently.
Search continues to index the complete project tree; choosing a folded result brings the focused row back into the active list without opening either fold.

### Right panel Overview

The right panel order is Overview, Explorer, History, then Sessions.
Overview is project-scoped and is one list: the current Project's worktrees as groups, each holding the agents working in it, under a strip of derived project facts.
It has no mode, no graph, and no inspector; what a group or row has to say is on its own line, and what an operator would look up sits in a tooltip or an existing popover.
The top block carries the project name, a workspace/inactive count, a refresh action, and a stat strip.
The strip's first row is always drawn for a Git project: allocated disk (opens the disk popover) and open PR count (opens the GitHub popover); zero open pull requests is a measured value and is drawn.
The second row holds only cells with something to act on (main behind origin, merged-to-clean-up) and is absent when neither applies; behind is read-only because Hide does not fetch.
Every cell follows one glyph language: a number when known, a pending mark while being read, a warning mark when it cannot be read, and absence when it does not apply; the reason lives in the tooltip and popover, never on the surface.
A plain folder project has only its size in the strip and only its size on its group header.

A group header shows the disclosure chevron, the checkout kind glyph, the branch, and a one-line purpose (falling back to the PR title, then to the branch alone); the full purpose is the header tooltip when the visible copy truncates.
Its badge strip is in fixed order: pull request, checks, files, behind, ahead, allocated size, staying on one line when it fits and wrapping without truncating the pull-request state at a narrow width.
Checks are shown as passing, failing, or pending, and the badge is absent when the pull request has no checks.
Files are Clean, a count, a pending mark, an unreadable mark, or missing, with matching color.
Behind is shown only above zero, in warning; ahead is shown only above zero and only without a pull request.
The pull-request and checks badges open the GitHub popover, `N files` opens History on that checkout, and Clean does not activate.
Groups sit in one fixed order: the primary checkout first, then linked worktrees oldest first, then an `Inactive N` fold sharing the sidebar's fold state.
No agent state and no search reorders a group.

An agent row is mark, badge, title, optional detail, and an open action, and the whole row is the button.
A delegated child is indented under its parent; a child delegated into another worktree stands in its own group naming its parent and parent branch.
An empty group has one row offering to start an agent, with the same Terminal only / Claude / Codex choice the header's `New agent here` offers.
Search matches an agent's title, its state sentence, and a branch, keeping the matching rows with their group header; no match shows a clear-search action.
While the live agent projection is unavailable a caption above the search says so and the last known rows stay clickable; a value Hide cannot read shows as unreadable and never as a false zero.
Only a row click, the header click, the `N files` chip, and the menus' explicit actions change pane focus, checkout focus, the panel section, or read state; scrolling, folding, searching, and refreshing never do.

### Disk allocation and cleanup

Allocated-on-disk sums main, linked worktree folders, and the shared Git directory once; nested roots belong to the longest matching root, hard links share one inode allocation, and descendant symlinks are not followed.
An incomplete measurement has no total; the UI separates the confirmed subtotal from unavailable target measurements, and allocated blocks are not a promise of reclaimable space.
Cleanup opens a review sheet with separate Available and Excluded groups, exact branch and folder, allocated size or failure, and a target-specific exclusion reason.
Nothing is preselected, and Remove is disabled until a user explicitly checks an eligible folder.
Main/current, dirty/untracked, live-pane-use, locked, nested, detached, unknown, and not-confirmed-merged targets are excluded; only clean, unused linked worktrees merged into local main can be removed, without force.
An ordinary merge is proven by Git ancestry; a squash merge requires the exact GitHub pull request head commit to equal the reviewed worktree HEAD and its merge commit to already be an ancestor of local main.
Removal is one `git worktree remove`, so every build cache a checkout owns is deleted with it and nothing outside the folder is touched.
Confirm rechecks current Git and Herdr state before each target and refuses changed state with a Review-again path.
Completion lists individual removed/refused outcomes, and repeating the same completed intent does not repeat removal; Review and Cancel perform no filesystem mutations.

### Removing a project's registration

`Remove project…` removes only Hide's registration and never deletes files, worktrees, sessions, or Herdr workspaces.
A project Herdr has no pane in is confirmed with registration-only copy; a project with panes is not refused, and the confirmation names the pane and running-agent counts, with the parenthetical omitted at zero running agents.
On confirmation the core closes every pane in the project's checkouts and waits for confirmation before removing the registration and its row; a timeout or refusal leaves the project registered with the reason in the error banner, and a repeated request continues from the panes that remain.
Removing the project that holds the focused checkout moves focus and pane selection to the next project.
An add that lands mid-removal cancels the removal and says so, rather than losing the project it just opened a pane in; a completed removal disappears from the snapshot and a repeated request is a quiet no-op.

## Recent navigation

Native owner: `AgentMRU.swift`, `ShellModel.swift`, `ShellModelNavigation.swift`. Web owner: `web/src/recent.ts`.

Cycling recent surfaces walks every unified surface in recent-use order, across every project, checkout, and device the session holds: terminal, Browser plugin, file/editor, and diff tabs.
The overlay ("Recent Panels") returns to the actually previous surface on a single chord, and repeated chords toggle between the last two surfaces; holding the modifier while repeating the chord walks older visits rather than tab-strip or agent-list order.
A second cycle scopes to projects globally and restores each project's last used surface.
Holding the chord's modifier previews; releasing it commits; Escape keeps the original selection; a menu action commits immediately.
Reopen Closed Tab is disabled when the session-local recent-close stack is empty or a restore is already running, and restoration works regardless of which surface currently owns focus.
Restoration is one action with no confirmation: an in-flight pane shows inline progress, and a restore without a target pane shows a compact inline warning.
Missing cwd, an unavailable prior conversation, a pruned Browser pane, a missing file, and a retryable failure all use the same inline notice vocabulary, without a banner, card, or modal.
A definitive close refusal removes its reserved reopen entry, while an unconfirmed result keeps the entry and explains inline that Hide could not determine whether the item closed.
With no other project or tab available, navigation keeps the current selection without a modal; selecting an empty project shows its existing empty state.
Automatic pruning and concurrent-selection recovery use structured diagnostics without a modal.

The project identity is scoped by device, following the sidebar's Project > Workspace > Agents hierarchy; two checkouts of one repository share a project cycle, and panel history is one order over every project, narrowed to a project for its own last-surface lookup.
Both the recent-panel and project switchers show at most nine rows around the highlight.
Project rows show the last surface and checkout; panel rows show their project and checkout (collapsed to the checkout alone when both share a name) and their surface type.
Remote rows carry a separate trailing `Remote · <device label>` badge with the agent mark, title, checkout, and dirty indicator intact; local rows have no extra badge, and a device absent from the registration map is identified by its actual remote ID rather than presented as local.
A panel row whose tab holds exactly one agent pane is titled by that agent's identity with its status mark; a tab with no agent or several keeps the Herdr tab label.
History is session-local and retains only existing projects and surfaces; a deleted highlight moves to the next surviving entry without reordering the held cycle, and if none survives, the cycle cancels and keeps the current selection.

## Device picker

Web owner: `web/src/DevicePicker.tsx`. Native owner: the sidebar device trigger and its themed popover.

The bottom-sidebar device trigger shows a laptop-or-server icon and the selected device name, and opens a popover (not a native system menu).
The list is one flat two-line row per device: the actual device name above, then Local or Remote, connection state, and available agent count below.
Remote unavailable rows say `Not connected` and never present a stale count as current.
The selected device shows a selected wash and a checkmark; keyboard focus is separate and does not change the device until activation.
Up and Down move focus, Return or a click selects, and Escape dismisses without a selection change.
Empty lists say `No devices available`, long names truncate in their title line, and the complete identity remains in the row's accessibility label.
The list scrolls once its rows exceed the shared height cap.
The web shell draws the same trigger and list at the bottom of its sidebar from the shared tokens, with line icons standing in for the native SF Symbols.

## Weekly usage

Web owner: `web/src/components/weekly-usage.tsx`, `web/src/usage.ts`. Native owner: the sidebar utility bar's usage button and `HideUsagePopover`.
The core reads the numbers and names each row's state (`navigator.provider_usage`, [AI_PROVIDERS.md: weekly usage display](AI_PROVIDERS.md#weekly-usage-display)); the shells only draw them.

The sidebar footer carries one chip per provider at its right, beside the device picker: the provider mark and the rounded percent of the seven-day window.
A percent reads in the success color below 70, the warning color from 70 and the destructive color from 90.
A provider that is loading or unavailable is a dimmed mark with no percent; a stale or fallback reading keeps its percent.
The chips' accessible name lists every provider with its reading.
Clicking the chips opens the Weekly Usage popover above them, titled `Weekly Usage` with `7 days`: one row per provider with its mark, name, the time left before the reset (`in 5d 4h`, `in 3h 12m`, `in 7m`), the percent and a bar, and each scoped bucket such as Fable indented under its provider with `└`.
A loading row reads `…` and an unavailable row reads `Unavailable`, each with the core's message in place of the bar; a stale or fallback row keeps its bar and shows its message under it.
No usage state becomes a banner, toast or alert (design principle 13).
The countdown advances once a minute while the popover is open, and nothing ticks while it is closed.

The web page tells the core when a read is worth making, through two hints on `ui_state_update`: `usage_window_visible` follows the page's visibility and is sent on every live connection and on each change, and `usage_popover_open` is sent when the popover opens and again when it closes or leaves the screen.
The core keeps one value of each, so with several pages open the last page to report decides.

## Search keyboard navigation

Native owner: Command+K (agent/workspace search) and Command+P (file search). Web owner: `web/src/search.ts`, `web/src/Palette.tsx`, `web/src/components/search-field.tsx`.

Agent/workspace search and file search share the same focused query field and first-result selection behavior.
An agent result is titled by the identity every other surface uses and subtitled by the row's second line, falling back to the status word when the state chose no sentence; the pane id leaves the printed row but still matches the query and is read by accessibility, so a result can be found by title, sentence, or id.
On the web, the sidebar's `Search` field with its `⌘K` keycap opens the same palette Command+K opens, and the query row carries an `Esc` keycap.
Results sit under headers in the form `<project> > AGENTS` (an agent under the first project whose checkouts hold its pane, `AGENTS` when none does), `WORKSPACE > COMMANDS`, `WORKSPACES > PROJECTS`, and `WORKSPACES > CHECKOUTS`.
A group stands where its best result ranked and keeps its results in rank order, so grouping never moves the best match off the first row.
An agent row is the agent's own mark, its title, and the state line under it; a project or checkout row carries its path under the title; the selected row shows `↵`.
With nothing to search the list says `No agents or workspaces yet`, and a query with no match says `No matching agents or workspaces`.
Up and Down move the selection in display order, stopping at either end, while typing continues in the query field.
Return executes the highlighted result through the existing agent, checkout, or file-opening action; Escape closes the sheet.
The selected row scrolls into view.
Filtering preserves a surviving selection by identity; a retired selection moves to the first remaining result.
Empty results have no selection, and arrows or Return require no modal acknowledgement.
A stale result is checked against the live result set before execution, and file search never executes results from a previous query or checkout while its asynchronous index is updating.

## Pane header lineage and ownership

Native owner: `PaneLineageHeader.swift`, `ShellModel.swift`, and the [agent workflow contract](../design/agent-workflow-review.md), which is the native shell's authoritative lineage/ownership document.
Web owner: `web/src/PaneRelations.tsx`, `web/src/lineage.ts`, and the [Agent panes and the Agents explorer](#agent-panes-and-the-agents-explorer) subsection of Web Workspace above.

The pane header keeps one identity row naming the pane and its status; a pane with children gains a second row for child chips that exists only when there are children.
An authoritative parent becomes a compact Return control in the first row, with icon-only fallback before current identity or actions are truncated.
A pane with children names the first direct child in the second row, and the remaining children fold into an adjacent `+N` relationship control opened by a bounded scrolling sheet; pending navigation disables child changes, and Retry remains attached to the inspected target that failed.
The relationship sheet inspects on row selection and navigates only through its explicit Open action.
A relationship Open or parent Return publishes one request-scoped pending state; the same target cannot dispatch again while that request is pending, and Retry starts a new request only after the prior one has settled.
Target retirement before dispatch, and a core-owned refusal, timeout, or remote-control failure, keep the current pane geometry and tab topology, show the scoped reason, offer Retry when the outcome is retryable, and offer Dismiss to clear only the notice.
The shell never derives success from an old focused layout or optimistic remote selection, never attributes an unrelated global error to the control, and never sends a second focus event as rollback; a canvas notice preserves pending and failed feedback after successful navigation removes the source header or sheet from view.
A root with no parent carries no Return control, following the rule that a control with nothing to do is not drawn.

Ownership is drawn as emphasis, not as a new color or container: the operator's own rows are bright, delegated rows are subdued, and nothing new is introduced, because a delegated row is simply never emphasized.
A child's question or completion reaches the operator through its ancestors: the ancestor row turns unread and its descendant badge changes, and the ancestor's own group does not move.
An uninstrumented mark (agent detected but its subagents not visible to Hide) is drawn only where an agent was detected, is a mark plus an accessible name and never a color alone, and its subagent count sits beside it as a badge; a count Hide cannot read is drawn as unknown and never as a zero, because a zero claims the agent is working alone.
An Overview agent row reuses the same agent identity and state presentation as the sidebar and relationship sheet; a missing row means the current live projection has no agent there, and an uninstrumented mark never means zero.
The header wash marks the pane Hide is showing, while the outer primary indicator marks the terminal that owns the native keyboard responder; moving keyboard focus into Overview keeps the shown wash and removes the terminal outline.
Unread weight is never reused to mean parent, child, delegated, or selected.

## Keycaps, tooltips, and icon buttons

Every icon-only control has a tooltip and an accessible name carrying the same words as the tooltip.
A chorded tooltip reads the label followed by the shortcut chord; a chordless control shows only the label.
There is no native platform tooltip layered underneath the shared one; the shared tooltip is the only tooltip in the main shell.
Tooltip hover has a short reveal delay, and an exact modifier hold reveals shortcut hints faster than a hover tooltip does; releasing the modifier, deactivating the app, or opening a sheet clears hints.
Pane focus, active tab, tab order, zoom state, and disappearing anchors all update which controls can show a hint or tooltip; pointer exit, mouse down, scroll, key down, losing key window status, and anchor removal all dismiss an open tooltip.

Destructive buttons are named by their result (`Move to Trash`, `Close 3 panes and remove`, `Stop work and close`), never by a generic "Delete" or "OK" that hides the consequence; the non-destructive option is the default/cancel action.
Escape closes the innermost open layer and returns focus to whatever held it before that layer opened, including a terminal that was focused when a sheet, menu, or overlay opened over it.
A disabled control cannot activate, and destructive meaning always comes from the control's role rather than from its text color alone.
Hover and focus are local presentation state: they never publish core snapshots, dispatch core events, or trigger Git/disk work by themselves.
Pending operations show explicit progress and remain disabled for the duration; a shared control style never invents its own pending or error state independent of the actual operation.

## Sheets, overlays, and abnormal states

Search, New Agent, Settings, and file search each host the same tooltip overlay and the same sheet container language: headline titles, supporting text, and consistent spacing.
The Settings sheet grows with the presenting window up to a maximum, so an ordinary window reads a tab without scrolling; the Settings scene (its own window) keeps its smallest size.
A selected provider card uses a stronger fill and border while keeping its status readable; disabled Start/Add controls keep their existing enablement conditions.

Loading, local-only, no-workspace, disconnected, and unreadable states are each distinct and each drawn in its smallest form: a badge or a dimmed row before a banner, following design principle 9.
A value Hide cannot read is shown as unreadable (a question mark or equivalent), never as a false zero or a silently empty state.
An alert, banner, or sheet for a failure is a deliberate product decision, never a default: a failure the operator cannot act on goes to the diagnostic log instead (design principle 13).
Retryable failures offer Retry attached to the exact target that failed; a definitive refusal and an unconfirmed/timed-out result are told apart in their copy, because only the definitive case can safely remove state (such as a reopen entry or a pending pane).
