# UI behavior

This document owns what Hide's UI does: the rules for how each screen and control behaves, independent of any platform's exact pixel values.
It replaces the behavioral content of the retired `DESIGN.md`.
Visual authority (what a control looks like) lives in the Pen library and `web/src/components/ui`/`web/src/components`, described in [DESIGN_WORKFLOW.md](DESIGN_WORKFLOW.md); numeric authority lives in `design/tokens.json`.
Each rule below names the code that owns it: a web owner in `web/src/` and, where the behavior is core-decided, a core owner in `herdr-core/src/`.

## Web Workspace

The web shell's Workspace screen follows the approved S6 proposal, its View areas follow the approved boards of PRD S7 (`agents/prd/workspace-views-layout/prd.md`), and its side panel follows the operator's decisions on issue 170, the newest ("Side panel hierarchy, revised") first: every control sits once, on the container it changes.
Web owner: `web/src/WorkspaceScreen.tsx`, `web/src/ViewAreas.tsx`, `web/src/Tools.tsx`, `web/src/viewLayout.ts`, `web/src/viewDrag.ts`, `web/src/viewFocus.ts`.

The toolbar spans only the agent column and holds the path back (`Home / Project / Workspace`, where `Home` opens the Overview), led by the device's colored band when the Workspace is not this Mac's; a tab in Home reads `Home / ~/hide`, since Home is no project. It has no tool toggles.
The side panel toggle sits at the Workspace's top right in both states: at the toolbar's right end while the panel is closed, and at the right end of the panel's first row while it shows.
It is drawn pressed while the panel shows, and ⌘⇧B toggles the panel too, restoring its views and the tool column as they were before hiding; while the panel is closed with views open, the toggle carries a badge with their count, also given as its accessible description.
The toggle, the tool column's toggle, Pin, and Expand each keep one accessible name and say their state as pressed or not; their tooltips say what a press does.
A right-click or the menu key on the toolbar offers the three panel states (Side panel closed, open, and expanded), Pin or Unpin side panel, Copy Workspace path, and Open Project Overview; the palette offers the other two states, Pin or Unpin, and Show or Hide Explorer and History as commands.
An empty Agent area offers New tab.
The area is empty only when the checkout has no tab: a checkout whose only tab holds delegated children keeps that tab off the strip and still draws it on the canvas, so the agent chosen from the sidebar there opens on its pane.

### The side panel

The Workspace holds the agent column (the toolbar, then the Agent area), always the Workspace's full width, and the side panel on its right edge at the Workspace's full height, up to the toolbar's row, over the agents.
The panel is closed, open at its width, or expanded over the whole body; the state is stored per Workspace and survives a restart, and a Workspace seen for the first time starts closed.
Open, the panel floats over the right part of the agent column and the Agent area keeps its full size underneath, so opening, closing, resizing and expanding the panel never resizes a terminal.
The panel is a `--card` surface with a `--border` hairline and a `--radius-lg` top-left corner, and no shadow; a `--spacing-sm` gap in `--background` on its left separates it from the agents.
Everything inside it sits on `--card`: the tabs, the document header, the editor, the diff, a page and a loading view, so a shown View tab is marked by its indicator and title alone, never by a surface of its own.
The pane header actions and the right part of an agent's lines under an open panel stay under it; Pin is the remedy.
Pin docks the panel instead: the agents end at its left edge and their terminals resize once to fit, and Unpin gives them the body's width back.
Pin is not a fourth state: it is stored per Workspace with the width, and a pinned panel still closes, opens, and expands, its agents keeping their docked width under an expanded panel so expanding and restoring resize nothing.
The gap on the panel's left is its resize grip (`Component / Side panel grip`): nothing at rest, and on hover, keyboard focus and while dragging a hairline centred in the gap with a small ⇆ pill at its middle that overlaps the card's edge; the grip travels with the pointer as the guide and lands once on release, one step per arrow key while focused, and neither the panel nor the agents to its left narrower than the area minimum.
Every panel that has a width resizes this way, floating or pinned, the tools-only panel included; the tools-only panel keeps a width of its own, the tool column's until it is first resized, and both widths are the Workspace's shares of the body and survive a restart.
The agents left of the panel are live: clicking a pane or a tab there focuses it and typing goes to it while the panel stays up, and chords such as ⌘F, ⌘T and ⌥W act where the keyboard is, the panel's View area or the pane.
While an expanded panel covers them, the agents take no pointer or keyboard, so Tab never walks into a terminal out of sight.

The panel's first row sits at the toolbar row's height and holds each top area's View tabs with their kind marks, each area's tabs followed by its own New tab (a new browser display in that area, see Browser displays), and, at its right end, the panel actions: the tool column's toggle (pressed while the tools show), Expand (only while a view is open), Pin, and the panel toggle.
Its second row is level with the agents' tab strip: the active document's header over each View area, naming a file from its checkout and cutting a long path at its start so the file name stays, and over the tool column the Explorer and History icon tabs, the shown one marked, named Explorer and History in their tooltips and accessible names.
The tool column holds one tool at a time, with no title row and no close: a tab swaps the tool, and the column's toggle hides and shows it, keeping the tool it held.
With stacked View areas each area keeps its own tab strip, and the first row holds the top area's tabs and the panel actions.
With no view open and the tools shown, the panel is only the tool column, including after the last view closes, and its first row holds the tool tabs, Pin, and the panel toggle, so the tools alone can be pinned beside the agents.
⌘E opens a closed panel with tools shown, closes a tools-only panel, and toggles the tool column beside views, keeping the chosen tool.
The sidebar's Projects | Agents switch has no default chord and can be bound in Settings, Shortcuts; a bound chord shows in the tabs' hint.
Panel content is views or tools only; closing the last view with tools hidden closes the panel in the core, and hiding the tools-only column does the same.
There is no empty panel body or empty-panel New tab button.
Only views expand: an expanded panel with no view open is drawn at its width, so the agents stay in reach.

Opening a file, a diff, or a page while the panel is closed opens it, with the active View area focused; revealing a file shows the tool column on the Explorer, and opens a closed panel.
Choosing an agent or a tab from the sidebar, the palette, or a tab cycle closes an unpinned panel and brings a pinned expanded panel back to its width, so the chosen agent is in sight; a pinned open panel stays beside the agents (in a window too narrow for both it closes too; see Narrow windows), and a choice made in the Agent area on screen beside the panel moves nothing.
Closing the panel closes no view, document, or pane, and the keyboard goes back to the focused pane.
Changing the panel's state only changes space, and expanding never makes a split; a split comes only from a split command or a drop on an edge.
Inside the panel the View areas behave as they do anywhere else: tabs, splits, preview, dirty state and browser displays, each page inside the panel's bounds.

An Agent tab shows the focused pane's status mark, provider logo, then title, using the sidebar row's title and mark rules.
A custom Herdr label wins; an empty, numeric, or `Tab N` label instead follows the focused pane's agent title (its task, or its provider's name), foreground process name, then `Tab N` using Herdr's stable number; a Herdr workspace label is never a tab or agent name.
A plain terminal tab shows the terminal icon and that name; terminal titles are never used to guess a process.
Local and remote process names are read only for focused panes of attached tabs, outside the runtime lock, on focus or agent-state changes and a 30-second recheck.
The foreground process is the process-group leader, or the last returned process when the leader is absent; its name is the basename of `argv0`, falling back to `name` only when `argv0` is empty, and to `Tab N` when both are empty.
The tooltip and accessible name carry the kind, full name and agent state; widths, scrolling, truncation and close behavior stay the same.
Every Agent tab reuses the same name and Rename field in its area menu described below.
Rename opens an inline field with the displayed name selected: Enter saves to that host's Herdr, an empty name restores automatic naming, and Escape or blur cancels editing.
While saving, the committed name stays unchanged; a refusal or timeout keeps the entered text with “이름을 저장하지 못했습니다 · 다시 시도” below it, Enter retries, and Escape returns to the prior name.
Failure details go to diagnostics, with no banner; a successful name survives reconnect while Herdr keeps the tab.
Copy name copies the displayed name.
A View tab carries the file-type mark, and a diff tab the comparison mark, so a kind is never told by color alone.
Its title is cut at the tail to fit, and its tooltip and accessible name carry the kind, the full path, and `Preview` or `Unavailable` while the view is one, so a long path stays readable.
A dirty view shows a warning-colored mark after its title.
Closing a view is always called Close view, distinct from moving a file to the Trash and from closing a pane or tab.
The close chord, ⌘W in the desktop app and ⌥W in a browser, closes the smallest unit that holds the keyboard: the focused View area's active display, or the focused terminal pane.
The page records its last focused region as View area, pane, tools, or none from focus events, including native browser-page focus; the chord does not walk the active DOM element or choose a region from panel size.
When the desktop app hands the keyboard back from a native page to deliver a menu command, the focus the shell's last element regains is not a move while that command runs, so ⌘W and ⌘T pressed in a page act on that page's View area; once the command has run, the keyboard's owner is the shell element holding it, so after any other command from a page the next chord acts where the operator now types.
With tools or nothing focused, nothing closes and a diagnostic states why.
Tabs close only from their own close control or menu; closing a tab's only pane still removes its tab through Herdr.
An unavailable or retired keyboard target never falls through to closing a different pane or a whole tab.
When a connected pane selects, splits, moves, or closes a View through `hide view`, the same View layout rules apply to that pane's Workspace even while another Workspace is in front.
These commands do not move the keyboard target; a close that would lose the last View of an unsaved document reports the refusal and keeps the draft.
`hide file open` and `hide diff open` put the calling pane's file or changed-file diff into its own Workspace without selecting that Workspace by default.
An explicit local `--reveal` brings that Workspace and the opened View forward, opening its side panel if it was closed; without it, the side panel keeps its state.

### Agent areas

Electron’s numbered tab shortcuts and hold keycaps share the saved Agent area tree order, then each bar’s left-to-right tab order.
Moving or reordering a tab updates both numbers together; waiting overflow tabs have no number.

At most 64 normal Agent tabs are placed, in up to six areas and three split levels.
New tab, Reopen that needs a tab, and protected replacement close count pending admissions and refuse before any external effect when full; the existing one-line notice asks the operator to close a tab.
After an ambiguous creation reply, Hide checks the request marker once without resending the mutation.
An unconfirmed creation retains its request-specific place and reports that uncertainty in the same notice; elapsed time alone never frees the place.
External tabs beyond the cap remain in Herdr topology, with their waiting count in that same notice, and enter the active area in authoritative order when a slot opens.
A waiting tab cannot take Hide's keyboard or active-tab selection.
This boundary is an Observer-approved, user-vetoable implementation assumption from the Agent groups contract.

The Agent column has its own area tree, separate from the side panel's View tree.
Each area has a tab bar, a New tab button and the active tab's live pane canvas; dividers separate areas.
Only the active area's selected tab carries the accent; clicking a tab or pane activates its area and sends the keyboard to that pane.
All shown tabs stay attached and awake, while only the active area's tab receives read and sleep-visit updates.
Pane headers, child chips, relationship controls and pane splits remain inside each canvas; find belongs to the focused pane.
⌘F on a full-screen Claude Code or Codex pane opens that agent's own search over its whole conversation in the pane, with the agent's own keys and count (Claude Code: type, Enter, `n`/`N`; Codex: type, Enter, Ctrl+P), and no find bar appears; any other pane, and an agent drawing inline, gets the find bar.

An Agent area's tabs share its bar the way a browser's tabs do (the Pen library's `Component / Adaptive Work Tab`): each asks for the preferred width, and all shrink alike while an equal share still holds the title minimum.
Below that every tab keeps only its marks at the icon identity width, the selected one adding its close control, and the strip scrolls once even the marks overflow.
Agent and View areas share the drag, divider and narrow-window controls described below.
A drag keeps its original tab in place and changes no terminal size until a valid drop.
Dropping on a tab bar reorders or moves the tab; dropping on a content edge highlights the new half with Split left/right/up/down and creates another area on release.
Moving the last tab out collapses its area, as does closing it or its disappearance from Herdr.
A sole empty area shows No agent tab is open and New tab.
Agent tabs cannot enter the View column, and a sole tab cannot split its own area.
Invalid size, area or depth limits show the forbidden cursor without an overlay; Escape, outside release and a vanished target leave the layout unchanged.
Each area scrolls its selected tab into view; when the column is too narrow, an area switcher shows one area without changing the saved tree.

New tab, Split right/left/up/down, available directional Move commands, Rename…, Copy name and Close tab… form each tab menu.
A disabled split explains its reason, and opening the menu changes no selection.
The palette opened from an Agent pane adds these area commands and next/previous area focus and grow/shrink commands, including unavailable reasons.
An area's New tab adds at its end.
The new tab chord, ⌘T in the desktop app and ⌥T in a browser, opens where the keyboard is: with a panel View area focused it is that area's New tab, with an Agent pane focused it adds an agent tab at the end of the area showing that pane, and anywhere else (the sidebar, the Overview, the tools, the Agent controls) it adds one to the Agent active area.
The View and Agent columns keep separate active areas, so the chord follows the recorded keyboard owner, never whichever column's active area changed last.
Externally created tabs append to the active area without changing the shown tab; Herdr's own reorder never changes Hide's area order.
Delegated-only tabs remain absent from every bar and occupy the active canvas when chosen from the sidebar.
Choosing a normal tab elsewhere activates its owning area.

The tree, ratios, ordered membership and active selections return after restart.
Missing tabs are removed, previously unplaced tabs append, and Reopen closed tab uses the former area when it survives, otherwise the active area.
An older build may discard Agent layout state; returning starts with all current tabs in one area and keeps the View layout.
SSH device Workspaces use the same component with one area, retain device Herdr reorder, and disable splitting with a local-Workspaces-only reason.

Closing a primary Herdr workspace's last tab or last pane while a linked worktree remains first creates a shell at the checkout root in the same area and position.
The existing close guards run first, and linked workspaces remain untouched.
If shell creation fails, nothing closes; if closing is refused, the shell remains.
Retry close reuses that intent's shell, while Dismiss ends the failed intent; detailed Herdr failures are diagnostic-only.

### View areas

The side panel holds one or more View areas, each with its own tab bar above its own view, split left/right or up/down as often as the limits allow.
A split divides one area in two along one axis, and either half can split again along either axis, so every arrangement of side-by-side and stacked areas is a tree of halves.
One area is active: the next file opens there, the palette and the keyboard act on it, and only its active view's tab carries the accent indicator.
Every other area still shows its own active view's tab, with a primary title and no indicator, so the operator sees what each area holds and which one is in charge.
An area's tabs each ask for the preferred width and shrink alike down to the title minimum; a file's type mark does not tell files apart, so a View tab keeps its title and never turns to marks the way an Agent tab does.
An area whose tabs outrun its width even at that minimum scrolls its own strip so the shown view's tab stays in sight whenever the shown view changes or the area is resized.
Clicking a tab or a view, or moving focus to another area from the palette, makes that area active.
The divider between two areas turns accent-colored on hover and keyboard focus, drags with a guide line, and lands once on release, so a drag never resizes a document or a terminal on every pointer move.
A focused divider moves with the arrow keys along its axis, one step and one change per press.
No area gets narrower or shorter than its minimum, a divider stops where either neighbour would, and each side of a split keeps between 15 and 85 percent of it.
An area whose last view leaves disappears, and its neighbour takes the space.
When the last view closes, tools stay visible if enabled; otherwise the panel closes, with that state saved for the Workspace.

A Workspace holds at most 6 View areas, no area sits more than 3 splits deep, and at most 64 views are open at once.
A split past the area or depth limit is refused with its reason, and an open past the view limit says so and keeps every currently open view as it was.
Opening a file that is already shown moves to its view, so it is never refused.

### Opening, preview, and Open to the side

A single click on an Explorer file or a History row opens it in the active area's preview view (italic title), and the next single click replaces that preview in place, so browsing leaves one tab per area rather than a trail.
Each area has at most one preview, and a click never touches another area or a pinned view.
A double-click on the row or the tab, Keep open, or the first edit pins the preview where it is; because the first edit pins, a document that is dirty, saving, or whose save failed is never a preview in any area that shows it.
Opening a file that is already shown moves to its view instead of adding a tab, choosing the one used last when several views show it.
Opening a file while the side panel is closed opens the panel first.
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
After a restart the app reopens the last Workspace as it was left: its areas and their sizes, each area's tabs in order with its preview, pinned views, and active view, the active area, the side panel's state, width and Pin, and the tools; unsaved text returns from the browser's drafts, and Herdr's current tabs and panes are used as they are.
A first run, or a last Workspace that no longer exists, starts on the Overview.
A layout file that cannot be read is kept aside and the app starts on the Overview, where the operator picks a Workspace and continues with a new layout.
A Workspace stored before the side panel, with a layout instead of a panel state, restarts into the nearest panel state: Agents only as a closed panel (an open one when its View areas floated over the agents), Agents and Views as a pinned open panel as wide as its View region was, and Views only as an expanded panel.

### Browser displays

A View area's new tab offers only what can become a display, never a tool or a pane.
The View strip’s +, its New tab menu item, and the new tab chord while the area holds the keyboard open a distinct browser display in that area with no address and the address field focused.
Its body shows Open with File (⌘P), opening the file palette, and Diff only while the checkout has changes, opening a palette of changed files.
Picking either replaces that empty tab in place; entering a URL navigates the same tab, and closing an untouched tab creates no recovery entry.
The new-tab page offers no tools or panes; ⌘P remains the file palette everywhere.

A web page is a view like a file: it opens in the active area, has a tab, splits, moves, and closes like one, and comes back after a restart at the address it last showed.
A page with an address carries a globe mark and the page's title, else its host, else a local file's name; its tooltip and accessible name carry `Page`, the title, and the full address.
It opens from `hide browser open` in a connected Herdr pane or a shell inside a registered checkout, from Open in Browser on an HTML file in the Explorer's menu (listed after Open to the side), from a page that asks for a new window, and from the address field.
On a connected SSH device, the native page uses that device's localhost or consented checkout resources; a route failure appears in the page's existing failure state.
Opening an address the Workspace already shows moves to that view and loads it again instead of adding a second one.
The view's own toolbar holds Back, Forward, Reload (Stop while the page loads) and the address, which shows a web address without its scheme until it is focused; focusing it selects the whole address, Return loads what was typed, and Escape puts the page's address back.
A page that cannot load says so in its place with the address and the reason, and Reload tries again; nothing else on screen changes.
While the palette, a menu, or a dialog covers a page, the page is shown as a still picture of itself, so the overlay draws over it, and it comes back live when the overlay closes.
While the shell drags something (a tab, a divider, the side panel's edge, an Explorer item), every page is shown as its still, so the guide or preview draws over it and a drop lands in the shell rather than the page; the pages come back live at release, and a drag inside a page is the page's own.
In a plain browser tab the view keeps its address on the toolbar row, level with a document header beside it, and below it reads `Pages open in the hide desktop app.`; a web address offers Open in browser, and nothing else is drawn in its place.
[BROWSER_DISPLAYS.md](BROWSER_DISPLAYS.md) owns which addresses a page may hold, the `file:` boundary, and the page's lifetime.

### Narrow windows

When the body cannot give the agents their minimum beside the open panel, the panel takes the whole Workspace as if expanded, without Expand or Pin, and a pinned panel floats there instead of docking; it then covers the agents like an unpinned one, so an agent or a tab of that Workspace chosen from the sidebar, the palette, or a tab cycle closes it and uncovers the chosen agent, the Pin kept for when the panel opens again.
Only the Workspace on screen is affected: another Workspace's panel stays as it was, so an agent chosen in another Workspace whose pinned panel is open lands under that panel in a narrow window until the toggle or ⌘⇧B closes it.
From the Overview or a project's Overview, an agent of the Workspace last on screen is treated as that Workspace last was: if the window was narrowed meanwhile, the agent lands under its pinned panel until the toggle closes it, and if it was widened, its pinned panel closes once, the Pin kept.
In a narrow window the tool opens as a temporary overlay over the View area's right side, below the first row, instead of a column, drawn with a border and no shadow and carrying the tool tabs; it is closed until the tool column's toggle, a tool command, or ⌘E asks for it, including the press that opened a closed panel, and the toggle reads pressed while it shows; Escape, the toggle again, or a click outside closes it and returns focus to what opened it.
Hiding and restoring the whole panel keeps whether its temporary tool overlay was open.
When the View areas cannot all have their minimum, only the active area shows, with an area switcher to the others.
Widening the window brings back the panel's stored state, width and Pin, the area sizes, and the tool column, because none of these narrow arrangements is stored; the core is told only whether the panel covers the whole body, and only when that changes.

### Library masters

The side panel's masters in `design/hide-ui.lib.pen` are on `Component / Side panel`: `Component / Side panel toggle` with its open-view count, `Component / Side panel tool tabs`, `Component / Side panel actions` in each of its states (tools hidden, pinned, expanded, no view open, narrow), and the panel itself over the agents, pinned with the grip hovered, with the tools hidden, and as the tool column alone; `Screen / Workspace` in `design/hide-screens.pen` draws it open and closed in Dark and Light.
The View area masters in `design/hide-ui.lib.pen` are `Component / View tab`, `Component / View insertion line`, `Component / View split overlay`, `Component / View tab menu`, and `Component / View area message`.
Their sheets draw every state as refs: the View tab sheet draws preview, pinned, hover, active-in-the-active-area, active-in-another-area, dirty, unavailable, diff, a long title, and the floating drag copy; the placement sheet draws a reorder, a move into another area, a right and a down split, and an ineligible target; the tab menu sheet draws a preview's menu and a pinned view's menu with a disabled Split and its reason; the area states sheet draws each view's opening, waiting, and unavailable states.
The browser display's toolbar and its loading, load failed, and plain browser tab states are on `Component / Browser file diff toolbars`.

### Agent panes and the Agents explorer

Several View areas leave the Agent side as it was already drawn: the side panel toggle, the tool column, and the child chips below behave the same with one area or six.

A pane whose agent delegated work shows every direct child on one row under its header, each chip a status mark, the provider mark, and a capped title; the row scrolls sideways instead of growing, and a pane with no children has no row.
Its library masters are `Component / Pane child chip` and `Component / Pane child row`.
A chip opens the existing child at once; while that move is in flight the chip shows a pending mark and repeats of it are ignored, and a failure shows the core's reason under the header with Retry (when the core says it can be retried) and Dismiss.
A child pane has a compact Return mark in its identity row, named with the parent in its tooltip and accessible name.
The pane menu (from its overflow control or a right-click on the header) lists the parent, the other siblings, and the children as explicit Open items, then Copy pane name and Close pane; opening it moves no focus and marks nothing read.

A pane whose agent sleeps (PRD agent-sleep) shows its state in place of the terminal, which stays hidden until the agent is back because the shell under it is not what the operator was talking to: Sleeping with the last progress line and Wake agent; Waking… with how old the resumed conversation is; or `Couldn’t resume this conversation` with the core's plain reason, Retry, and Start new session.
The header caption reads `☾ sleeping · 22h`, `☾ waking…` or `could not resume`, and typed input to the pane goes nowhere.
Opening the pane's tab by a committed move (a row, a tab, a checkout, a relation) wakes it in the same pane with its conversation; a Recent Panels preview does not, and neither does a click inside the tab already on screen.
The pane menu offers Sleep agent on a local agent pane that is awake, disabled with the core's reason when the agent is working, waiting for the operator, of another kind, or has no conversation Herdr reported.
Web owner: `web/src/PaneView.tsx` (`SleepBody`), `web/src/sleep.ts`, `web/src/PaneRelations.tsx`.

The Agents explorer groups every current agent, this machine's and each connected device's, under Needs You, Done, Working, and Seen, and leaves an empty group out; a device's row names its device before the agent kind, and a device that is not connected lists nothing it only last reported.
Each group lists its root rows; a delegated row is drawn only beneath its parent, indented one step per level and muted, while the operator has that parent unfolded.
A group's heading counts every agent it speaks for, its roots and all their live descendants whether folded or not, so each agent is counted once, under its root's heading.
In the desktop app ⌥1 to ⌥9 open the first to ninth row the list draws, top to bottom across the groups, exactly as clicking that row does; the order is the list's whether or not the sidebar shows it, a folded child takes no number, and a number with no row does nothing.
Descendants start folded; the parent's chevron folds and unfolds them, and the choice is the core's `expanded_agent_pane_ids`, so it survives a restart and is the same fold wherever the parent is drawn, in Agents or under its checkout in Projects.
The chevron is at the row's right end and exists only on a row with children: folded, it is always shown; unfolded, it shows under the pointer, while focus is inside the row, and always on an input with no hover, in a slot kept at rest so nothing beside it moves.
A leaf row keeps the same slot empty, so every agent row's time ends on one column and every chevron stands on one.
A folded parent with live descendants carries a badge after its title: one mark and count per state (error, approval, question, working, done), summed over every live descendant, or `↳N` when all of them are merely ready.
The badge is a button: a click, Enter or Space opens a list of the direct children with their status mark, name, status word, branch when it differs, and elapsed time; the arrow keys move the highlight, Enter or the highlighted row's arrow opens that child's pane, the last item unfolds the children in the list, and Escape closes it and returns focus to the parent row.
The list has no Stop action, drops a child the moment it leaves the projection, and closes when no child is left.
A folded root keeps descendants in its own checkout in that badge and draws one compact line for each other checkout below the root, ordered by its representative child's state and then branch.
Each line shows the representative status mark, branch, a pull-request number only when current GitHub facts contain one, the server-glyph device chip when it runs elsewhere, and `+N` for further descendants in that checkout; at most three lines are drawn, followed by one overflow line.
The representative is the most actionable child in Needs You, Done, Working, then Seen order, so the line is a derived summary and never an invented status.

A root whose own turn is over while a descendant still works or asks is waiting on its children (docs/status-model.md): it stays in Working with its ring in the working color, and its badge, not its own sentence, says what is going on.

A sidebar agent row's first line is always its status mark, provider mark, stable task name, a branch chip only when a delegated row's checkout differs from its parent's and the row above does not already name that checkout, a device chip for a row on an SSH device, the badge, the elapsed time, and a parent's chevron; the elapsed time is always drawn and never gives way to a control, and an agent whose elapsed time was never reported shows none rather than a made-up `0s`.
The device chip is the existing Badge treatment with a server glyph and the device's real display name, never a connection state inferred by the web shell.
Its second line exists only when the row has something to say, and from the moment it does: a question, approval or error keeps its request in the warning color (red for an error) until it is resolved, however often the row is read, and a row that changed since the operator last looked shows its sentence bright until it is read; either is one line, cut at its end.
A quiet sentence is never drawn on the row; the row's tooltip carries it with the full title.
In Agents a root row adds a fixed context line naming its project and checkout (`project › checkout`, the project alone for a plain folder), since the list does not otherwise say where the agent works; a row under a checkout in Projects has none, because the rows above it say it.
Hover, keyboard focus, selection and an open badge list change a fill, a ring and a chevron's opacity only; they never add a line or change a row's height, so the row below never moves.
A sidebar agent row is 28 high, or 44 with its second line: line one is 20 and line two 16, and the title is 12/400 in every state, a change brightening it rather than thickening it.
A row's height is a minimum, not a cap: a row grows to hold its lines rather than letting them run into the next row, and it still holds still under hover and focus.
No row draws a progress number or step the agent did not report.
The Project Overview's agent rows keep their own density: a quiet sentence is revealed on the selected or hovered row over up to two lines, with the whole of it in the tooltip.
Web owner: `web/src/agentRow.ts` (rules, reusable by any list of agents), `web/src/components/sidebar-agent-row.tsx` (the sidebar row), `web/src/components/agent-row.tsx` (the Overview row and the descendant badge both draw), `web/src/components/agent-children-popover.tsx`.

## Web Project Sessions

A Project's Sessions is the Project Overview's Sessions tab (PRD S8), under the Overview's own path back, title and facts line.
Web owner: `web/src/ProjectSessions.tsx`, `web/src/sessions.ts`.

It lists the history of every Workspace the Project has, works for a Project with no Workspace, and never runs an agent or sends a session to a Workspace.
Above the list are the `All / Codex / Claude Code` choice, the search, and the count (`N sessions` or `N of M sessions` while a filter narrows it); a read in flight adds `Reading…` beside the count and keeps the rows.
Each row shows the provider mark and name, the time, the first request or title in at most two lines with the full text in its tooltip, and the checkout it ran in.
A session with neither request nor title reads `Untitled session`, muted, and a time it never carried is left out.
A row's accessible name reads provider, first request, checkout, time, and availability, in that order.
The open session's row is the list's one Tab stop; the arrows, Home, and End move between rows, ArrowDown from the search lands on that row, and Escape in the search clears it, so only an Escape in an empty search leaves the Overview.
The provider choice is one Tab stop whose arrows choose the neighbouring provider.
An unreadable session dims only its own row, marks it unavailable, and keeps its reason, Retry, and Copy source location under it, copying the provider file's own path.
A session whose file is no longer found after the history listed it stays listed the same way, with an explanation that it may have been moved or deleted, and its last location to copy.
The list place shows one small mark per state: loading, no sessions yet, no matching sessions (with Clear filters), sessions could not be read (with the reason and Retry), and on a device Project, sessions unavailable with the device's reason and no Retry.
The open session shows its row's provider, checkout, and time with Copy source location, its request as the title, then each request and answer in order as plain text; injected context, where Project Memory travels, is not shown.
A session that cannot be opened shows the same reason in the detail place with Retry, which reads the history again and then the session.
When another window names another Project, this one says so and offers `Show this project's sessions`, which names this Project again only when chosen, so two windows never take it from each other.
The web shell has no Project Memory entry point, disabled control, or placeholder until the Memory stage (PRD S8); Memory currently has no UI in any shell and works only through the `UserPromptSubmit`/`SessionStart` hooks (see [PERFORMANCE_TESTING.md: Project Memory cost contract](PERFORMANCE_TESTING.md#project-memory-cost-contract)).

## Terminal image attachment boundary

Web owner: `web/src/attachments.ts`, `web/src/PaneView.tsx`. Core owner: `herdr-core/src/runtime/attachments.rs`, `herdr-core/src/remote/attachments.rs`.

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

Web owner: `web/src/ProjectOverview.tsx` (the Project Overview screen), `web/src/MainScreen.tsx` (the Overview of every project), `web/src/OverviewLenses.tsx` (the tiles, the checkout lanes and the lineage both scopes draw), `web/src/overviewLens.ts` (their rules: buckets, tile values, lane order, columns and folds, lineage rows, the entry lane), `web/src/IssuesView.tsx` (the Issues view both scopes draw, with its panel beside the board), `web/src/TaskBoards.tsx` (the Board, List and Dependencies modes and the issue card, which the Overview of every project calls Tasks), `web/src/IssuePanel.tsx` (the issue panel), `web/src/issueDetails.ts` (the issue reads the panel and the preview share), `web/src/MarkdownText.tsx` (an issue's body in Markdown), `web/src/IssueDialogs.tsx` (New issue and Start), `web/src/issueStart.ts` (the Start dialog's first name and prompt), `web/src/PullRequestsView.tsx` (the PRs view), `web/src/PrDialogs.tsx` (이슈 잇기 and 맡기기), `web/src/prDelegate.ts` (맡기기's first prompt), `web/src/projectBoard.ts` (the board rules, the PRs tab's groups included); a card's agent row is the Agents list's `web/src/components/agent-row.tsx`.
The web boards follow PRD task-agents-views (`agents/prd/task-agents-views/prd.md`), reworked issue-first on 2026-09-28: work starts from an issue, and a card reads issue, then agents, then pull request.
The project's page is laid out by PRD overview-lenses-tiles-agents (`agents/prd/overview-lenses-tiles-agents/prd.md`): tiles where the tab row was, and an Agents view of checkout lanes or lineages in place of the agent inbox.
Its PRs tile and view, 이슈 잇기 and 맡기기 are PRD overview-lenses-prs's (`agents/prd/overview-lenses-prs/prd.md`).

Project Home uses the shared tab choice, badges, agent identity marks, settings field, icon buttons, and command tooltip.

### Tiles and the first screen

Under the facts line stand four tiles of one width, `Agents · Issues · PRs · Sessions`, in the place of a tab row; the chosen one is outlined and a click on a tile opens its view.
A tile holds its name, a yellow badge of how many agents it is the operator's turn with (none at zero), a large number and a small unit, and one bar; it carries no sentence.
Agents counts the project's agents, its badge the ones asking or finished and not yet looked at, and its bar splits them into 내 차례, 일하는 중, 자식 대기 and 쉬는 중.
Issues counts the open issues, `열림`, and its bar splits them into 백로그, 진행 중 and 리뷰.
PRs counts the open pull requests, `열림`, its badge the ones that are the operator's turn, and its bar splits them into 내 차례, 에이전트가 고치는 중 and CI 실패; its badge's breakdown is `리뷰 · 초안 · 끝난 에이전트 확인`.
Sessions counts the project's sessions updated today by this machine's date, `오늘`, and its bar splits them into Claude and Codex; opening the Overview reads the project's session history once to fill it.
Resting on a bar shows its legend, each part's name and count, and resting on a badge its breakdown (`질문 1 · 승인 1 · 끝남 1`).
A value not yet read leaves the number and the bar out, and zero is drawn as zero; when a source read fails, a ⚠ stands by the tile's name and resting on it says what failed and how old the value is, with the reason in the diagnostic log and no banner.
The Agents tile's agents are the device's live rows, so it has no last value to age: while the device cannot answer, its ⚠ says why and the number stays empty.

Every way into a project's Overview, the project row, its Overview row, the palette and ⌘⇧H, opens Agents in its checkout mode with the lane of the checkout in front selected (outlined and scrolled into view), or main's lane when the checkout in front is elsewhere.
Only Recent Panels (`` ⌥` `` in a browser, `⌃Tab` in the desktop app) brings an Overview back as it was left, its tile, modes, selected lane and opened folds; the view lives on the screen, not in stored settings.
The PRs view opens only from its tile, a PR chip, `이슈 없는 PR N`, the sidebar's PR card and Recent Panels.
The facts line's right end carries the chosen view's mode control: `체크아웃 · 계보` for Agents, the filter and `Board · List · Dependencies` for Issues, and nothing for PRs.

### Agents: checkout lanes

The checkout mode draws one lane per checkout: the head on the left, the agents working in it to the right.
main's lane is pinned on top, then the lanes with an agent whose turn it is, then the working ones, then the resting ones, the most recently active first within each; inside a lane the agents go the operator's turn, working, waiting on children, resting.
A delegation is a line from the parent's node to the child's: down across lanes when the child works in another checkout, and a right arrow within one lane.
An Implementor delegated from main stands in its worktree's lane in its Observer's column, and a line keeps its column free in every lane it crosses, so no line runs through another agent.
A worktree head reads the kind glyph in its pull request's colour and the branch in mono, the purpose (else the pull request's title, else nothing), and a third line of the issue chip, the PR chip, `↑N ↓N` and the changed files in warning when dirty; main's head reads the house, main, its purpose and `에이전트 N`.
A worktree whose Git state has not been read shows `?` where the files go, never a false zero; before GitHub answers there is no PR chip and no PR colour, and the lane stands on Git facts alone.
Resting on a head brightens it and opens the checkout card: the path, the base and `↑N ↓N`, the changed files, the last commit's age and the pull request with its checks; the card's `↵ Workspace` is the head's click, which opens that checkout's Workspace, main's included.
The issue chip opens the Issues view with that issue's panel, the PR chip opens the pull request's row on the PRs view, unfolded, and a ⌘-click anywhere on a lane or node, the PR chip's included, opens GitHub.
A merged worktree is dimmed with the purple merge glyph and a folder-less one reads `× 폴더 없음`; both carry the one word `정리` at the head's right, whose tooltip says what it removes, and whose click opens the existing Delete worktree dialog for it.
Worktrees with no agent fold into one line, `에이전트 없는 워크트리 N`, and merged or folder-less worktrees whose agents only rest into another, `정리할 것 N`; a click unfolds the line in place, a line at zero is not drawn, and the facts line's `N merged → 정리` opens the checkout mode with `정리할 것` unfolded.
On the Overview of every project, each project's main lane is ranked by its agents like any other lane, first among equals, and an idle main folds with the worktrees that have no agent.

A node reads the status mark, the provider mark, the title and the age on one line, and under it the line the core makes: the question in yellow with a yellow outline when it is the operator's turn, the result after ✓ for a finished agent not yet looked at, `일하는 중 N · 물음 N · 끝남 N` for an agent waiting on its children, the progress line for a working one, and nothing, dimmed, for a resting one.
Colour belongs to the operator's turn alone.
Resting on a node brightens its background; resting half a second on its line opens everything the agent last said (the snapshot's `message`, the whole hook sentences, not the line cut to the node), with `↵ 패널에서 답하기`, the node's own click.
A node's click is one event that opens the agent's pane.

### Agents: lineage

The lineage mode draws one row per lineage, in columns `Observer · 보통 main`, `Implementor · 워크트리` and `하위 에이전트`: the lineages with an asking agent first, then working, then resting, an arrow from parent to child, and an agent with no parent in the first column.
A lineage node carries a third line of chips, the checkout (house or branch), the issue and the pull request: the checkout chip opens the Workspace, the issue chip the Issues view with that issue's panel, the PR chip the pull request's row on the PRs view (⌘-click GitHub), and resting half a second on each opens the checkout card, the issue's id, state, title and age, or the PR card.
Resting lineages fold into `쉬는 에이전트 N` and the ones in worktrees there only to be removed into `정리할 것 N`.

Nodes, lane heads and fold lines take focus; the arrow keys move between them by where they are drawn, Enter is the click, and Escape leaves the Overview as before.
Hover, focus and a half-second rest are local: they publish no snapshot, dispatch no core event, and start no Git or disk work.
Every icon button and chip has an accessible name, the same words as its tooltip.

### Issues

The Issues tile opens a board of issues, laid out by PRD overview-lenses-issues (`agents/prd/overview-lenses-issues/prd.md`): four columns, `백로그 · 진행 중 · 리뷰 · 완료`, each headed `name count`, on a page that scrolls as one; a column grows with its cards and its head stays in view while the page scrolls under it.
백로그 always stands, so a new issue has somewhere to go; another column shows only while it holds a card or a line of work with no issue.

Every project on this Mac has one issue source, chosen in Settings › Issues: GitHub issues, read and written through the operator's own `gh`, or Local issues, which Hide keeps in `local-issues.json` in its state directory, numbered per project and shown as `L-N`.
A project's source defaults to GitHub when its repository reads as a GitHub repository and to Local otherwise (a folder, a repository with no GitHub remote, or `gh` not installed); a project on a device has no source here.
The core hands the web a source-neutral task (the id the source shows, URL, title, open or closed, when it last changed), so no view reads a GitHub shape, and a new source is one more adapter in `herdr-core/src/tasks.rs`.

Every card is an issue; a worktree or a pull request is never a card of its own.
An issue's stage is its checkout's: 완료 when the worktree or its pull request is merged, 리뷰 with an open pull request, and 진행 중 otherwise; agents and the issue's own state never move it.
An issue is linked to a checkout by the branch's issue link or by a closing reference in the pull request's body, and a pull request that closes two issues shows the same chip on both cards.
Merged is Git ancestry against the base the core resolves, so a branch with no commits of its own reads as merged once its base resolves; for that reason a Local issue is never closed by a merge, only by the operator.
백로그 holds the open issues no checkout works on, most recently changed first; past 20 cards the rest wait behind `+N · 최근 갱신 순`.
The primary checkout or a folder is a card only while an agent works there on a linked issue; an agent there with no issue is on the Agents view, not an issue.
A worktree with no issue is one line at the foot of 진행 중, `이슈 없는 워크트리 N`, whose popover says it goes to Agents › 체크아웃 and names them, and whose click opens that mode.
A pull request with no issue is one line at the foot of 리뷰, `이슈 없는 PR N`, whose popover says it goes to the PRs view and names them, and whose click opens that view, where each has an issue cell to link.
A line at zero is not drawn, and done work with no issue is not shown.
완료 starts folded to one line per issue, the glyph, the id, the title and the number of the pull request that closed it, and resting on that number says `PR #N 머지 · 날짜`; a line opens the issue's panel, and the head unfolds the column into cards (on the Overview of every project, one line per project with its count).

A card's head is the source glyph, the id and at most two labels (GitHub, once the issue has been read), then the title in at most two lines.
A backlog card stops there, with a lock and the ids of the open issues it waits on in warning when it is blocked; starting it is never refused, only warned.
An in-progress card adds the checkout chip (the branch, `↑N`, `N files`) and the PR chip, and at most two agent rows, the ones that need the operator first, and `+N` for the rest; a review card adds the CI mark and the review GitHub asks for in one word.
Only a card whose agent asks or has finished is outlined in warning, its question line in warning, and it rises to the top of its column; a done card is dimmed, and no other card has colour.
Hover or focus fills the id line's reserved slot without changing the card's height: `▷ 시작` and `S` on a backlog issue, the Workspace icon and `O` in progress, the PR icon in review, a Local issue's edit icon, and `⋯` with 시작, Workspace, GitHub, 편집 and a Local issue's close or reopen; each button's popover says what it does.
Resting half a second on the id opens the issue's preview (id, labels, state, title, the body's first three lines as plain words, the author, the date and the comment count); the preview reads the issue once, at most one read at a time.
Resting on an agent row's line opens what that agent last said, on the checkout chip the checkout card, and on the PR chip the PR card.
A card's empty space and its title open the issue panel, an agent row that agent's pane, the checkout chip its Workspace, the PR chip the pull request's row on the PRs view (⌘-click GitHub), and a ⌘-click on the id GitHub; every area has one destination.
When the source cannot be read the board keeps the last issues it read, each card carries a small ⚠ whose popover says `GitHub 읽기 실패 · N분 전 값 · 이유는 로그에`, and 백로그's head and the Issues tile carry the same mark; there is no banner, and the reason is in the diagnostic log.

The issue panel opens to the right of the board, which stays in the width left to it; its head is the source glyph, the id, the source's name, Open or Closed and ×, then the title, then an action line: `▷ 시작` and `S` on a backlog issue, Workspace and `O` in progress, the pull request in review, beside it GitHub (a GitHub issue) or edit (a Local one), and `⋯` at its end.
Its properties are the stage, the labels, the author and date and the assignees for a GitHub issue, the day a Local issue was made, when it last changed, and what blocks it; a property with no value has no row.
`이 이슈로 한 일` is the checkout line with its Workspace button, every agent working there with a delegated one indented, and the pull request with its title and the review asked for or its CI; with none of them the section is not drawn.
The body is drawn as Markdown, and under it a GitHub issue shows `댓글 N`, the latest three comments and `쓰기는 GitHub에서`; a Local issue has no comments.
Opening the panel reads the issue's body, labels, author, assignees and comments once, on a worker off the core's lock; while it reads, the body and those properties are skeletons and the rest stands on the snapshot, and an issue opened again shows what was read before while it reads again.
A failed read puts one line of why and `재시도` in the body's place and leaves the rest of the panel standing; `재시도` reads that issue again.
A Local issue's title or body edits in place on a click or the edit icon: ⌘↵ saves, Escape cancels, the card changes only once the save is answered, and a refused save keeps the text with the reason in place.
The arrow keys move the panel with the card: ↑↓ within a column, ←→ to the card at the same height in the next column, staying at a column's end; Escape closes the panel first and leaves the Overview after, and choosing another tile closes it.
On the board a card takes focus, the arrows move between cards, Enter opens the panel, Space the preview, `S` starts, `O` opens the Workspace, and `C` makes a new issue.
Hover, focus and a half-second rest publish no snapshot and start no Git or disk work; the preview's one read is the only event they send.

The header's primary action is `새 이슈` (`C` whenever the page itself has the keyboard); New agent, which starts work with no issue, is the quiet one beside it.
New issue makes the issue in the page's project's source, and its `어디에` list moves it to another project on this Mac; `만들고 바로 시작` goes on to the Start dialog once the source has the issue.
The Start dialog turns an issue into work in one step: a worktree named for the issue, its base, the agent, and the agent's first prompt.
The name opens as the issue number and the title's English words (`192-hided-sigterm-handler`, `L-3-...`, or `issue-192` when the title has none), and when Settings › Issues allows it the background AI's name replaces it once it answers, unless the operator has typed; `↺ AI 이름으로` brings the AI's name back.
A name that is already a branch says so, and when a worktree has it the primary button opens that worktree instead.
The first prompt is filled from the issue's body, which the dialog reads when it opens, names the issue, and for a GitHub issue asks for a pull request that closes it when Settings › Issues says so; the operator edits it before starting.
Starting creates the worktree, writes the link into it, starts the agent with the prompt as its first instruction, then brings the new pane's Workspace to the front; a folder project has no worktree, so its agent starts in the folder with the prompt.

The facts line's right end carries the filter and `Board | List | Dependencies` (on the Overview of every project, the right of its tab row).
The filter keeps the cards whose id or title holds every word typed and, with `내 차례만`, only the ones waiting on the operator; the lines of work with no issue stay, and with no card left 백로그 says `필터에 맞는 이슈 없음` with `필터 지우기`.
A project's filter lives with its Overview's lens and comes back with it; the Overview of every project keeps its own while the page is open.
The mode is a mode of the Issues view, not a tab; it belongs to the page, so the Overview of every project's Tasks and every entry into a project keep it.
List draws the same cards one row each, grouped by stage with the moving work first (진행 중, 리뷰, 백로그, 완료 folded): the stage glyph, the id and title, a `질문` or `확인` badge on a row waiting on the operator, and on the right the agents' marks, the PR chip, the branch, `↑N` and the age; a row with agents unfolds them under it, one waiting on the operator starts unfolded, and a row's click opens the issue panel.
Dependencies draws the Board's cards left to right with a quiet stage word at each card's top right: an issue sits one column right of the longest chain of issues it waits on, and an arrow runs from the blocker's right middle to the blocked card's left middle; a card's click opens the issue panel.
Arrows carry no label; one legend line above the graph says the left issue has to finish first.
A blocked card is dimmed with its lock line, a done card is dimmed, and a card waiting on the operator keeps the Board's warning outline.
Issues with no relation in scope gather below the graph under `관계 없는 태스크`.
A blocker outside the scope, or one the source says is closed, is no arrow; an open one outside the scope is still named on the lock line.
On the Overview each card carries its project beside its id and an arrow crosses projects.
When the source answers the issues but not their dependencies, the last blockers read stay and the cards carry the same warning mark as a failed read.

### PRs

The PRs tile opens the project's pull requests grouped by whose move it is: `내 차례`, `에이전트가 고치는 중`, `CI 실패 · 맡은 에이전트 없음` and `최근 머지`, each headed `name count`, a group with no row not drawn.
A merged pull request is 최근 머지; an open one whose branch's checkout has a working agent (a parent waiting on its working children included) is 에이전트가 고치는 중; otherwise failed checks or a change request make it CI 실패; every other open one, asking for review, approved, a draft, or with an agent there finished and not looked at, is the operator's.
A closed pull request that was not merged is not shown.
최근 머지 starts folded and its head unfolds it: every merged pull request whose worktree record is still here, and the others merged in the last 14 days, newest merge first, each row dimmed.
The core sends the web only these pull requests, from the list `gh` already read; nothing more is read for the view.

A row reads, left to right, `▸`, the state glyph in its lifecycle colour, the number, the title, the issue cell, a yellow `확인` while an agent there finished unseen, then the agents' marks (three and `+N`), the branch in mono, the CI mark, the review in one word and the time.
The issue cell is the issue's chip, from the branch's issue link or else the first issue the body closes, and a dotted circle when there is none; only `변경 요청` and `확인` are yellow.
The row's click and Enter unfold it: the branch's agents under their ancestors, root first, the operator's turn on its yellow line, then GitHub, Workspace and, with no issue, 이슈 잇기 as icon buttons.
→ unfolds, ← folds, ↑↓ move between rows, and ⌘↵ or a ⌘-click is GitHub.
Under the pointer or the keyboard the time's fixed slot holds the row's buttons and nothing moves: `▷ 맡기기` on a failing or change-requested pull request with no working agent, `정리` on a merged one whose worktree is still here, otherwise the GitHub icon and `⋯` with 맡기기, 이슈 잇기 and 브랜치 이름 복사.
Resting half a second on the issue cell opens the issue's preview, on the number the PR card, on the branch the checkout card, and on an agent's mark everything that agent last said; an empty issue cell turns into the 이슈 잇기 icon under the pointer, whose popover says `이 PR을 이슈에 잇는다. 이을 이슈가 없으면 PR 제목 · 본문으로 새로 만든다`.
The issue chip opens the issue's panel, the branch its Workspace, the CI mark the checks on GitHub, the review word the review on GitHub, and one agent's mark its pane (several unfold the row).
Until GitHub has answered the view is three skeleton rows and the tile has no number; when a read fails the last pull requests stay, the PRs tile carries ⚠ whose popover says `GitHub 읽기 실패 · N분 전 값 · 이유는 로그에`, and there is no banner.
A repository with no GitHub remote has no pull requests, which is an answer: the tile reads 0.
Hover, focus, unfolding and a half-second rest are the screen's own state and publish nothing; the view's state rides on the screen, so Recent Panels brings back its unfolded rows.

이슈 잇기 opens the project source's open issues, searchable, with `새 이슈 만들기` last; closed issues and another repository's are not in it.
A GitHub issue asks once, `PR #N 본문에 "Closes #M"을 씁니다. 머지되면 GitHub가 이슈를 닫습니다.`, with `그만두기` first and `본문에 쓰기`; confirmed, Hide links the branch to the issue (its issue link and the Workspace token, when the branch has a worktree here) and writes the line at the end of the body after a blank line, reading the body just before, and the issue cell fills.
When the body already closes that issue by a closing keyword, nothing is written and the link succeeds, so a retry never adds a second line.
A failed body write keeps the link and the filled cell; the dialog stays with one line of why and `본문 다시 쓰기`, which writes only the body, and `닫기`.
A Local issue is Hide's link alone: choosing it asks nothing and writes nothing to GitHub, and the dialog appears only when the link fails.
`새 이슈 만들기` opens the pull request's title and body (read when it opens) in editable fields and one confirmation, `이슈를 만들고 PR #N 본문에 "Closes #(새 번호)"를 씁니다`, `만들고 쓰기` then makes the issue and writes the body; when the body write fails the issue stays made and linked, the dialog says `이슈 #M은 만들었고 PR 본문 쓰기는 실패했습니다`, and `본문 다시 쓰기` writes only the body.
On a Local source the same form says `Local 이슈를 만들어 이 PR에 잇습니다. GitHub에는 쓰지 않습니다.`, and `만들고 잇기` makes a Local issue and links it, with nothing written to GitHub.

`▷ 맡기기` opens the Start dialog on the pull request's branch: the worktree is the branch, shown and fixed, the agent Claude or Codex, and the first prompt the failed checks by name with their links and the change requests as their reviewers wrote them, which the dialog reads when it opens and the operator can edit.
With no checkout of the branch here the dialog says `이 브랜치의 워크트리를 만들고 시작합니다`, and the start fetches the branch from origin and makes a worktree that tracks it; it never makes a new branch.
A failed read of the checks and reviews leaves the prompt empty with why and `다시 읽기`, and does not hold back the start.
Started, the agent works in that checkout and the screen goes to its pane; while it works the pull request is 에이전트가 고치는 중, and once it finishes it is the operator's with `확인` until its pane is looked at.
`정리` on a merged row opens the existing Delete worktree dialog for its worktree, unchanged: the branch kept unless asked, its panes closed by `Close N panes and delete`, a running agent a warning line, and its cancel changing nothing; a row with no worktree record has no `정리`.

### Scopes

On the web, the sidebar picks the scope; a project's tiles and the Home Overview's tabs pick the view.
A device's Home Overview is that device's projects, a project is its Overview, and a checkout is its Workspace, which has no views of its own; the sidebar's Home row opens the first and a project's Overview row the second.
The Home Overview is of the device its screen names, else the device in front, and a device removed since then leaves it for the device in front; its title reads `Home` and the device's name.
The board is the Project Overview: the sidebar's project name or its Overview child (a plain folder's one row opens its checkout instead), the Overview's project row, the palette and the Workspace toolbar menu open it, and ⌘⇧H opens it for the checkout in front.
Escape, once no dialog or menu is open and no text field holds text, returns to the Workspace in front, or to the Home Overview when there is none.
The title row carries the path back (`Home / Project`, where Home is the project's device's), New agent and 새 이슈; directly under it is one line of facts, then the tiles, which show even while the project has no agent.
The facts line holds only facts about storage: for a Git project the worktree count, the disk every worktree and the shared Git directory occupy, main's distance behind origin only above zero, and `N merged → 정리` only above zero; the open issues and pull requests are counted on the tiles, not here.
Opening a local Git project's Overview asks the core to measure its disk and to read its issues; the size reads `… GB` while that runs and is left out, with the reason only in the diagnostic log, when a part cannot be read, and there is no refresh control.
The Home Overview keeps its tab row, `Tasks · Agents · Projects`: the Tasks board mixes the device's projects' issues, Agents is the same checkout lanes or lineages over those projects with the project's name above each lane head, and Projects is the device's registered projects; the device's Home folder is none of them.
Its Agents tab carries the count of agents it is the operator's turn with; there is no band under the header.
Its title row carries Add project in the desktop app and 새 이슈 (for the project in front, else the first one with a source), and its facts line the project count, the open issues once every source has answered, and, only when every project can give its part, the open pull-request and merged totals.
New agent opens the New worktree dialog on a Git project and the folder's Workspace otherwise.
Before the first snapshot the shell's own connecting state shows instead.
While hided or a device is unreachable the board keeps the last snapshot and the existing connection or device line is the only signal.
A card's agent row follows the Agents list's row rules above (`web/src/agentRow.ts`): the same first line, second line and branch chip, and the core's waiting-on-children ring.
A card row carries no chevron and no descendant badge.

## Explorer file management

Web owner: `web/src/ExplorerTree.tsx`, `web/src/explorer.ts`. Core owner: `herdr-core/src/changes.rs`.

The tree's context menu follows VS Code's order: New File, New Folder, a separator, then on a file row Open with Default App and a separator, then Reveal in Finder, Copy Path, Copy Relative Path, a separator, Rename, a separator, Delete.
A folder row has no open items, because its open is Reveal in Finder; the empty area below the rows stands for the root and offers only the two creations; a remote tree is read-only and offers only the two copies.
The item set the menu offers is a presentation decision a test can check directly, not something the platform decides implicitly.

Open with Default App hands the file to the OS through the existing external opener, and a refusal is reported with the path and the reason.

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

Core owner: `herdr-core/src/runtime/editor.rs` (`place_editor_tab`, `promote_editor_tab`), `EditorTabSnapshot.preview`. The web Workspace's one-preview-per-View-area model is the same idea applied per area; see [Web Workspace](#web-workspace).

A single click on an Explorer file or a History row opens it in the checkout's one preview tab (VS Code's model): the strip draws the title in italic, and the next single click replaces the tab in the same slot instead of adding one.
The core owns the preview flag and decides replacement and promotion; the shell only says what the click meant.
Promotion happens in the same slot, on four triggers: a double-click on the Explorer row, a double-click on the tab title, the first edit, and Keep Open; a drag to a new slot promotes as well.
A dirty tab is never replaced: the core promotes it where it sits and opens the new preview beside it.
Every other entry point - opening a file by path, Reopen Closed Tab, a Markdown or terminal link, a file the Explorer just created - opens an ordinary tab, and a single click on a file that already has a tab focuses it without touching the slot.
A replaced preview tab is not a close: its document, mode, and wrap state are dropped and nothing enters Recent Closed; closing the tab yourself records it as any file tab.
Editor tabs stay ephemeral, so the preview flag is never persisted.
The tooltip and the accessibility label read `name · Preview` while the tab is one and drop the suffix on promotion; the tab's colors, close button, and its Recent Panels row are the ordinary tab's.

## File document toolbar and Markdown

Web owner: `web/src/Editor.tsx`, `web/src/viewers/FileViewer.tsx`.

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

Core owner: `herdr-core/src/sidebar.rs`, `herdr-core/src/project_context.rs`, `herdr-core/src/worktrees.rs`, `herdr-core/src/disk.rs`, `herdr-core/src/worktree_cleanup.rs`, `herdr-core/src/runtime/projects.rs`. Web owner: `web/src/sidebar.tsx`, `web/src/projects.ts`.

### Sidebar type, rows and width

The sidebar sets its words on three sizes: a project name 13/600, a checkout name, an agent title and a count 12/400, and a second line and a time 11; a section header is 10/500 in sentence case (`Projects · Recent activity · 5`).
A project row, the Home row and a one-line plain folder are 36 high, a checkout row 32 or 48 with its second line, an agent row 28 or 44.
A checkout name mutes its prefix up to and including the first slash (`prd/`), so the part that tells checkouts apart reads first; a name with no slash, or one that starts or ends with it, has no prefix.
The checkout whose Workspace is in front turns its name 500 and nothing else about the row.
The sidebar keeps these sizes at every Appearance font size, because its rows are scanned, not read; the font size scales the rest of the interface.
The sidebar's right edge sets the width of its content column, the lists right of the device rail while one shows (see Device rail): under the pointer it shows a line and a column cursor, a drag moves the sidebar over the center between 220 and 440 and stops at either bound, and the release stores the width in the core's ui state, where it survives a reload and a relaunch; the center and its terminals take the new width once, on the release, never on each pointer move.
A double-click on the edge returns the width to 292, and a press that does not move changes nothing.
A width outside the bounds is refused into the diagnostic log: a sent one leaves the width as it was, and a stored one opens the sidebar at 292.

### Sidebar hierarchy

The sidebar hierarchy is Project > Workspace > Agents; a Workspace corresponds to one checkout path, including a plain folder.
Two checkouts of one repository share a project cycle.
Each checkout row shows a kind glyph: home for the stored primary checkout, otherwise the pull-request lifecycle icon when current GitHub data has a pull request, then branch, commit for detached HEAD, or folder for a plain folder.
The primary choice belongs to the registered project and survives restart; a registration without a choice defaults to its root checkout.
Changing it moves the home glyph and first position together without moving focus; a vanished choice stays stored and is marked again when that checkout returns.
A checkout row's `Set as default checkout` sets it; the item is disabled with its reason on the checkout that already is the default, a plain folder, a checkout whose folder is missing, a project without a registration (pin it first), and a device's checkout, which is read-only for this action.
Remote catalogs still derive home from Git-root facts and do not yet display the primary checkout stored by the remote core.
Open, draft, merged, and closed pull requests keep their own lifecycle shapes and colors, stale GitHub data mutes only the icon, an unavailable GitHub lookup falls back to the branch glyph, and a missing folder colors its branch glyph as danger and omits the age.
In the web shell every row reads on the left and ends the same way on the right: its time or its status badge, then a fold slot kept at rest, so nothing moves when a control shows and the times and badges of project, checkout and agent rows end on one column while their chevrons stand on another.
A folded chevron is always shown; an unfolded one shows under the pointer, while focus is inside the row, while its menu is open, and always on an input with no hover.
Nothing on a row stands for its menu: a right-click, or the menu key or ⇧F10 on the focused row, opens it.
Project, checkout and agent rows each have one (PRD sidebar-context-menus), and every other item runs at once; only `Remove project…`, `Delete worktree…` and `Close tab…` go through their existing confirmations.
A project row's menu is `Open Overview`, `New worktree…`, `New tab in main` (a new tab in the checkout the home glyph marks, brought to the front), then `Reveal in Finder` and `Copy path`, then `Pin` or `Unpin` and `Remove project…`, on every project row, registered or not.
A checkout row's menu is `Open` (the row's open, without unfolding its agents), `New tab here`, `Open pull request #n` while GitHub knows one, then `Set purpose…`, `Set as default checkout`, `Copy branch name`, `Copy path` and `Reveal in Finder`, then `Delete worktree…` in the destructive color on a linked worktree.
An agent row's menu, in Agents and under an opened checkout, is `Show` (the row's own open, with the ⌥n that selects the same row where the host has one), then `Copy title` and `Copy session id` (the conversation id Herdr recorded, disabled when it recorded none), then `Close tab…`, which closes the tab holding the agent's pane, wherever it is, through the tab close flow; Herdr 0.9.1 can neither mark a pane seen nor stop an agent, so neither is offered.
`New tab in main` and `New tab here` show the registry's new-tab chord; `Reveal in Finder` is the desktop app's only (a browser tab lists no such item) and shows the folder in Finder without opening anything.
On a device's rows `Reveal in Finder` and `Set as default checkout` are disabled with the reason; the rest act on that device as they do here.
`Delete worktree…` is never disabled on a linked worktree, on any device and before its Git state has been read; its confirmation says what would be lost and holds the choices, and reads `Reading the worktree's Git state…` until the row arrives.
The confirmation lists the folder's removal, the panes that close, one line naming every agent those panes stop with its state, and the core's warnings (uncommitted files with their count, a worktree inside it, the base branch, Git status unavailable, commits not merged, not pushed).
`Also delete branch <name>` deletes the branch with `git branch -d` when Git counts it merged, and otherwise with `git branch -D`, saying how many commits not on the base go with it or that Git could not tell; it is not offered for the base branch or a missing folder.
A folder that holds uncommitted files, a worktree inside it, or a status Git could not read shows a `Discard …` checkbox, and Delete stays disabled until it is ticked, because that loss cannot be undone; ticked, the folder is removed with `git worktree remove --force`.
Files that change after the confirmation, typically written by an agent as it stops, stop an unticked removal with that reason and keep the worktree; every refusal and failure is shown in the dialog above the choices, and Delete tries again on the row as it is then.
From the confirmation until Git answers, the checkout row and its agent rows are dimmed with a spinner where the badge was, the row opens nothing, and a right-click or the menu key on it offers no menu at all rather than an empty one; the row leaves the sidebar as the removal finishes, and a failure gives it back as it was.
A copy that the clipboard refuses goes to the diagnostic log.
A web checkout row opens its checkout and unfolds its agent rows in one event; activating its already selected, unfolded Workspace folds the agents while keeping that Workspace in front.
Click, Enter and Space have the same behavior, and a checkout without agents only opens.
Its chevron changes disclosure alone, and other checkouts keep their own expansion.
The core admits the workspace/checkout pair before changing focus or expansion and publishes both in the same snapshot; a stale pair changes neither and leaves a diagnostic.
A later Herdr projection failure retains these accepted values under the existing focus policy.
The last-commit age stays in place whatever the pointer does and while the menu is open.
An opened checkout and its agent rows share one small group fill; no card border nests inside another.
A web checkout's agent rows start closed, so its status badge counts them, and the checkouts the operator opens are kept in the core's ui state across launches.
A status badge counts agents under the mark each agent's own row draws, one mark and count per state, worst first (`× ! ? ● ✓ ○`), with idle agents included and zero states left out (docs/status-model.md, Workspace aggregation).
A web project row takes the checkout row's rule: it opens the project's Overview and unfolds its checkouts, and activating it while that Overview is in front and the project unfolded folds the project while keeping the Overview in front; the Overview is the web shell's own screen, so the fold is the click's one core event.
Its chevron on the right folds and unfolds alone, without navigating; both folds are this machine's, so a selected SSH device's tree is drawn with nothing folded and its project row only opens the Overview.
A web project row carries no time: it ends in its checkouts' badges added up, which stay while its checkouts are open because they are the project's own summary, and a project with no agent draws none.
An opened checkout's parent agent folds its children with the lineage chevron and badge the Agents list uses, from the same core state; a child working in another checkout is also drawn as a root in that checkout, so folding a parent never hides where an agent runs, and a selected SSH device's lineage is drawn unfolded with no chevron.
A checkout whose root came from another checkout prefixes its purpose line with the Return glyph and the parent checkout's branch, adding `+N` when more than one external root raised it.
Folding a project by its row or chevron, a parent by its chevron, or using a checkout chevron changes the list only: the center, the focused pane and tab, read state, groups and running processes stay as they were.
Before the first snapshot arrives the Agents and Projects lists say they are connecting rather than drawing an empty list, and in the desktop app a local Projects list with no registered project offers Add project.
The Home row stands for the device's Home (see Home) at every device count (PRD home-device-rail D-13): the house glyph, `Home`, and `N projects`, the device's registered projects with its Home not among them (no count before the first snapshot), in the project row's height, font and focus ring.
It opens the device's Home Overview by click, Enter or Space and carries the selected fill while that screen is in front; it is drawn before the device has a Home folder, since the count comes from the registrations.
The agents running in the Home are its child rows, opened as an agent row opens, and a `+` shown under the pointer, `New tab in Home`, opens a new tab in the Home and brings its pane forward once it is listed; a refusal, such as a `~/hide` that is not Hide's, shows the core's reason in a caption under that Home row until the next start or a click on the row.
With no remote device there is no rail and the sidebar's top is fixed above both lists: the Home row, then the `Projects | Agents` tab strip, Projects first and shown at launch, the choice kept for the session, ending in Add project (on Projects only, where the Agents tab leaves its place empty, and only in the desktop app) and Search, icons whose hints read `Add project` and `Search` with their chords.
With the rail there is no tab strip: the top line names what is in front, `This Mac`, a device's name with a smaller `Remote`, or `Inbox` with `모든 기기`, and ends in Add project (not while the Inbox is in front) and Search; the rail's selection decides the list below.
Search opens the ⌘K palette and Add project the Add a project dialog (see Adding a project), on this machine or a selected SSH device alike; there is no Search field row and no bottom new-workspace button.
The Herdr status line sits under the top and above the list, and is not shown while a device is in front.
The web Projects list is the scope picker, starting at its first project.
One row carries the selected fill at a time, the row of the scope the center shows: the Home row while its Home Overview is in front, a Git project’s Overview child on its Overview, or the focused checkout and its open agent row only while a Workspace is in front.
An expanded Git project starts with an Overview row using the checkout row’s columns, single-line height, font and focus ring, with a layout-dashboard glyph and no badge, time or chevron.
Click, Enter or Space opens the same Overview as the project name without changing the fold; only the Overview child carries its selection fill, and folding the project hides the child too.
A plain folder, a project that is not a Git repository and holds one checkout, is one web row instead of a project row over an identical checkout row.
Its first line is the project's folder glyph, name and status badge, set in the checkout row's columns, and the badge stays while its agent rows are open, as a project's does; its second line and trailing chevron are the checkout's, and a plain folder has no commit age.
It has no project fold of its own and keeps the checkout row's right slots; the row opens the checkout and is marked while that checkout's Workspace or the project's Overview is in front, its menu lists the project's items and then the checkout's own `Open`, `Open pull request #n` and `Set purpose…` (the folder is the checkout, so its new tab, path and Finder items are the project's), and its Overview is reached from the Overview, the palette or the Workspace toolbar.
While a checkout's agent rows are closed, its status badge ends line one; opening them takes the badge away, since their own marks now speak, and changes nothing else on the row.
A checkout's second line is its purpose, after the parent checkout it was raised from when there is one, with the last-commit age ending it on the time column; it is drawn only while the checkout has a purpose or a raising parent, so a checkout with agents and neither is one line.
A checkout with neither, or one whose Git facts have not been read yet, is one line, with its age on that line.
Workspace disclosure persists across launches and hides only the nested agent rows, preserving selection, running panes, and raised attention rows.
The web shell draws the raised groups at the top of the Projects list as `Needs You · N` then `Done · N`, each left out while empty: the Needs You or Done agents whose pane a listed project's checkout or the Home owns, in the core's order, on the Agents list's own row with its place line, drawn whatever their project, checkout or parent has folded, never unfolded themselves, and opened as the Agents row opens.
With no rail the raised groups stand under the tab strip; with the rail a device's list is its `Needs You · N` group, then the Home row, then Pinned and the projects, with no Done group, and no row there names the device, since the whole list is that device's (PRD home-device-rail B6).
An agent row's title is its identity label at both densities: the rolling task, or the provider's name when no task exists; a Herdr agent name and a Herdr workspace label never become display copy.
A row whose descendants are folded, and every raised row, wears a descendant badge counting live descendants by state before the elapsed time; opening the fold removes the badge because the opened rows carry their own marks.

### Purpose, pinning, and PR chrome

A checkout's one-line purpose is set from the checkout row's or Overview header's `Set purpose…` context item, with a character count and a warning near the limit; saving an empty purpose clears it, and display falls back through branch description, representative agent title, and pull-request title in that order.
Any project row can be pinned from its right-click menu: a row Herdr shows without a registration is registered with its device and root by the same Pin, so pinning never needs a separate add; pinned projects are drawn once under a `Pinned N` section between the raised groups and the activity-ordered project list, in the tree's own order (device first, then latest activity), and only while at least one project is pinned.
The pin lives on the project's registration and survives a relaunch; removing the registration takes the pin with it.
A pinned project is exempt from its device's inactive fold whatever its activity; its own stale worktrees still fold behind their own `Inactive N` row.
PR lifecycle color is a semantic-color exception to otherwise neutral chrome: Open, Merged, Closed, and Draft each keep a fixed color shared between the sidebar glyph, the Overview popover header, and the state badge, including during hover and selection; review decisions and CI keep their own separate status meanings.
On the web a checkout row whose glyph is a pull request's lifecycle makes that glyph a button (PRD checkout-pr-glyph-card): a ring in the pull request's color under the pointer and the pointer cursor say so, a click opens the pull request as a browser display of the Workspace in front (an address the Workspace already shows is brought forward instead), and ⌘-click opens it in the default browser.
While no Workspace is in front (the Overview, a project's Overview) or the checkout is an SSH device's, the click opens the default browser too.
The press never reaches the row, so the checkout neither opens nor unfolds; every other kind glyph (home, branch, commit, folder) is inert and a click there is the row's.
The row's menu offers `Open pull request #n` after `Open` and `New tab here` while GitHub knows one, with the same open as a plain click.
Hovering or keyboard-focusing a checkout row opens a card after the tooltip's delay, in place of the text tooltip (`Component / PR hover card`, `web/src/components/pr-card.tsx`): the pull request's badge, its number and `Open PR ↗` (the card's one control, opened as the glyph is), the title on up to two lines, a rule, then `Review`, `Checks`, `Branch`, `Agents`, `Commit` and `Path`, each row present only where the snapshot has the value.
The badge reads the lifecycle word (`Merged`, `Closed`, `Draft`, `Open`) or, under review, the decision (`Approved` in success, `Changes requested` in destructive, `Review required` muted), with `Draft` beside a draft's decision; `Review` repeats the decision, `Checks` reads `Passing`, `Failed` or `Pending` and is left out while the checks are none or unknown; `Branch` is the worktree's branch or `Detached HEAD at <short sha>`; `Agents` draws the sidebar badge's marks and counts while any agent runs there; `Commit` is the last commit's age once Git has been read; `Path` is the checkout's full path in mono.
A checkout with no pull request has the same card without the badge line; a folder that is gone has `Folder missing` in the danger color over its path alone; a card that would hold one value and no header (a plain folder with no agents) is the plain text tooltip with that value.
On a local Git project's checkout the card's head adds `PRs 탭에서 보기`, whose popover says `이 PR의 이슈와 에이전트 계보`, and whose click opens that project's PRs view at the pull request's row, unfolded.
The card stays while the pointer crosses onto it, closes when the pointer leaves the row and the card, on Escape, and on a press on the row; a screen reader reads the row's detail sentence (the pull request, the agents by state, the branch and the path) as before.

### Project ordering and inactive folding

Projects and their checkouts sort by the latest authoritative agent activity timestamp or Git commit timestamp, descending; missing activity sorts after known activity, and no UI interaction or local clock invents recency.
Inside a project the primary checkout comes first whatever its activity, because it is the one checkout that never folds, and the rest follow in that activity order.
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
Removal moves the folder into the repository's Git directory and has `git worktree remove` drop its registration, so every build cache a checkout owns goes with it and is deleted in the background, and nothing outside the folder is touched.
Confirm rechecks current Git and Herdr state before each target and refuses changed state with a Review-again path.
Completion lists individual removed/refused outcomes, and repeating the same completed intent does not repeat removal; Review and Cancel perform no filesystem mutations.

### Adding a project

Web owner: `web/src/AddProjectDialog.tsx`, `web/src/addProject.ts`; desktop owner: the `hide:pick-folder` channel in `desktop/src/main/host.ts`.
Add project opens one centered `Add a project` dialog from each of its entry points: the sidebar strip's +, the Overview's title row and empty state, the empty local Projects list, ⌘⇧N and the File menu; Escape and × close it.
Only the desktop app offers it, because the native folder picker is how a local project is added: a plain browser tab draws none of those entry points and has no chord for it.
The Host selector lists this Mac and every registered device, and starts on the focused device.
On this Mac the primary `Browse folder` row holds the keyboard, so Enter activates it; it opens macOS's own folder picker as a sheet on the window, which can also make a new folder, and a cancelled pick leaves the dialog as it was.
The chosen folder is registered with one `create_workspace`; the dialog closes when the project appears, and says `Adding <folder>…` until then.
A refusal stays inside the dialog as an alert naming the folder, and the dialog stays open for another pick: `already_registered` from the shell's own registrations, before anything is sent, and `outside_home`, `home_root`, `not_found`, `not_a_directory` or `invalid_path` from hided's `$HOME` line, or the core's own error.
A device's folders are not this Mac's to browse, so on a device the dialog shows a `~/` path field instead, sent to that device's helper through the same event, with its refusal shown the same way.
On this Mac an `Other ways to add` group sits under Browse folder, one row per way, each opening its own view of the same dialog with a `Back` that returns to the first view and puts the keyboard on the row it came from.
`Clone from URL` (web owner: `web/src/CloneFromUrl.tsx`) shows a `Git URL` field that holds the keyboard, and a `Parent folder` field with a button that opens the same native folder picker; the parent starts beside the most recently added local project, else at home (`~`).
The folder the URL names is shown as `Clones into a new folder <name>`, and hided is asked whether it is free under that parent, so `Clone` is enabled only for an https, ssh (`git@host:path`, `ssh://`) or `file://` URL, a parent inside home, and a name nothing holds there yet (`already_exists` covers even an empty folder, since a clone never writes into a folder it did not make); each refusal is shown inline under its field.
While the clone runs the dialog shows Git's stage and percent with a progress bar and a `Cancel` button, and the fields and `Back` wait; closing the dialog leaves the clone running, and opening it again shows it.
A finished clone is registered like a picked folder and the dialog closes when the project appears, or says it could not be added and that the repository is on disk for Browse folder; a failure (authentication, a stalled transfer, a missing repository) stays inline in plain words with `Clone` enabled again, and a cancel says nothing was kept.
`Create new project` (web owner: `web/src/CreateProjectView.tsx`) shows a focused `Name` field and a location row naming the folder it goes in (`Git repository in ~/…`) with the full path under it, which follows the name as it is typed.
The folder starts as the parent of the most recently added project on this Mac, else home; the location row opens the native folder picker to choose another.
hided answers a `project_target` probe for every name and folder on the same `$HOME` line the create is checked on, so a name that is not one folder, a folder outside home, something already standing at the path (`already_exists`) or a project already added there is said inline and `Create project` stays disabled until the path is free.
`Create project` (or Enter) sends one `create_workspace` with `new_folder`; the core makes the folder, runs `git init` in it and registers it, and the dialog closes when the project appears or keeps the failure with its reason.
A folder holding nothing, or only `.git` and a Finder `.DS_Store`, is what a create that failed after making its folder leaves, so it is not `already_exists`: the view says the project is made in it, and a retry continues there rather than being refused or making a second folder; a failed create never deletes the folder it made.

### Removing a project's registration

`Remove project…` removes only Hide's registration and never deletes files, worktrees, sessions, or Herdr workspaces.
A project Herdr has no pane in is confirmed with registration-only copy; a project with panes is not refused, and the confirmation names the pane and running-agent counts, with the parenthetical omitted at zero running agents.
On confirmation the core closes every pane in the project's checkouts and waits for confirmation before removing the registration and its row; a timeout or refusal leaves the project registered with the reason in the error banner, and a repeated request continues from the panes that remain.
Removing the project that holds the focused checkout moves focus and pane selection to the next project.
An add that lands mid-removal cancels the removal and says so, rather than losing the project it just opened a pane in; a completed removal disappears from the snapshot and a repeated request is a quiet no-op.
A row Herdr shows without a registration offers `Remove project…` too: its confirmation counts its panes the same way and says Hide keeps no registration for it, the confirmation closes those panes, and the row leaves once Herdr drops its workspace; a timeout leaves the row with the reason in the error banner, and repeating the removal closes the panes that remain.

## Home

Core owner: `herdr-core/src/runtime/home.rs`, `hide-host/src/home.rs` (see ARCHITECTURE.md, Home). Web owner: the Home row in `web/src/sidebar.tsx`, `web/src/devices.ts`.

Each device has one Home, `~/hide` in that account's home directory, for work that belongs to no project or to several.
Nothing is made on a device until its Home is first used: the first agent or tab started there makes `~/hide` with one link per project registered on that device and Hide's `AGENTS.md` with a `CLAUDE.md` link to it, and a device whose Home was never used has no `~/hide`.
Once a device has a Home, registering a project there adds its link and removing the registration removes only that link, never the project's folder; two projects with one folder name get distinct link names.
A link whose project folder moved away is dropped at the next sync with nothing on screen, only a diagnostic line.
A `~/hide` that Hide did not make is left untouched, and the start that wanted it says so where it was asked for, in the start panel or under the Home row for its `+`, with what to do.
A Home agent reads and edits the projects through their links, the change shows on that project's checkout row, and its row stays under the Home row.

## Recent navigation

Web owner: `web/src/recent.ts`, `web/src/keyboard.ts`, and `CycleOverlay` in `web/src/Overlays.tsx`.

Cycling recent surfaces walks every unified surface in recent-use order, across every project, checkout, and device the session holds: terminal, file/editor, and diff tabs, and on the web every View-area display (file, diff, and browser) of this machine and every Herdr tab of each connected device, whose displays the snapshot does not carry (PRD home-device-rail D-16).
On the web, each device's Home Overview and each Project's Overview are rows of the same order once the page has shown them, each reading as its sidebar row does: `Home` over that device's project count with the house mark, or the Project over `Overview` with the layout-dashboard mark; a revisit moves the one row to the front, a Home Overview leaves with its device, and an Overview leaves with its Project.
Committing one of them shows that screen at once, committing a Workspace surface from one shows the Workspace once its checkout is in front, and Recent Projects still restores a Project's last Workspace surface, never its Overview.
The overlay ("Recent Panels", ⌃Tab / ⌃⇧Tab in the desktop app, ⌥` / ⌥⇧` in a browser, where Chrome keeps ⌃Tab) returns to the actually previous surface on a single chord, and repeated chords toggle between the last two surfaces; holding the modifier while repeating the chord walks older visits rather than tab-strip or agent-list order.
A second cycle ("Recent Projects", ⌥Tab / ⌥⇧Tab) scopes to projects globally and restores each project's last used surface.
Committing a row brings its surface forward in its own project and checkout, switching the Workspace when needed, as one event; a display's View area shows if only Agents showed, and the keyboard lands in it.
On the web a surface is in use where the keyboard is: the focused checkout's active display while the keyboard is in its View area (including native browser pages), else its visible Herdr tab, and while a device is in front the tab that device's Herdr shows; a commit's intermediate frames are not visits.
Recent Panels and Recent Projects are one order over every connected device: a row not on the device in front carries that device's chip (`mini`, or `This Mac` with the laptop glyph), and committing it moves the rail, the sidebar and the center to that device and its surface together.
Holding the chord's modifier previews; releasing it commits; Escape keeps the original selection; a menu action commits immediately.
Reopen Closed Tab is disabled when the session-local recent-close stack is empty or a restore is already running, and restoration works regardless of which surface currently owns focus.
Restoration is one action with no confirmation: an in-flight pane shows inline progress, and a restore without a target pane shows a compact inline warning.
Missing cwd, an unavailable prior conversation, a missing file, and a retryable failure all use the same inline notice vocabulary, without a banner, card, or modal.
A definitive close refusal removes its reserved reopen entry, while an unconfirmed result keeps the entry and explains inline that Hide could not determine whether the item closed.
With no other project or tab available, navigation keeps the current selection without a modal; selecting an empty project shows its existing empty state.
Automatic pruning and concurrent-selection recovery use structured diagnostics without a modal.

The project identity is scoped by device, following the sidebar's Project > Workspace > Agents hierarchy; two checkouts of one repository share a project cycle, and panel history is one order over every project, narrowed to a project for its own last-surface lookup.
Both the recent-panel and project switchers show at most nine rows around the highlight.
Project rows show the last surface and checkout; panel rows show their project and checkout (collapsed to the checkout alone when both share a name) and their surface type.
A row's device chip follows its detail line, truncated before it would crowd the detail, with the agent mark, title and checkout intact; a device absent from the registration map is named by its actual remote ID rather than presented as local.
A panel row whose tab holds exactly one agent pane is titled by that agent's identity with its status mark; a tab with no agent or several keeps the Herdr tab label.
History is session-local and retains only existing projects and surfaces; a deleted highlight moves to the next surviving entry without reordering the held cycle, and if none survives, the cycle cancels and keeps the current selection.

## Device rail

Web owner: `web/src/components/device-rail.tsx`, `web/src/devices.ts`, the rail branch of `web/src/sidebar.tsx`.

The rail shows while at least one remote device is registered, connected or not (PRD home-device-rail D-09..D-11): a column on the sidebar's left with the Inbox on top, a divider, This Mac, each registered device in the core's order, and `기기 추가` at the bottom.
The stored sidebar width stays the content column's; the rail adds `--size-rail` to its left only while it shows, and the drag edge sits on the content column.
Each tile is a rounded square with its glyph (inbox, laptop, server) and its name under it, truncated; the selected tile has a bar at its left edge and one tile is selected at a time.
A device tile's badge is how many of its agents are in Needs You, with none at zero; the Inbox's is the sum over every connected device, this machine included.
A device that is not connected is dimmed with a `×` and no badge, since its last count is not current; selected, its sidebar shows only its name, `연결 안 됨` and `다시 연결`, which retries the connection in place, and never the tree it last reported.
Selecting a device tile sends `focus_device`, and the sidebar becomes that device's Needs You, Home and projects (see Sidebar hierarchy); no row there names the device.
Selecting the Inbox lists every connected device's agents under Needs You, Done, Working and Seen, each remote row with its device chip, and leaves the center where it was; a row on another device moves the rail, the sidebar and the center to that device's pane in one step.
Every tile is a button reached with Tab and chosen with Enter or Space, named for assistive technology by the device and its state (`mini, 연결 안 됨`, `This Mac, Needs You 2`; the Inbox as `Inbox 모든 기기`).
`기기 추가` opens Settings › Devices at its Add device form, as the footer's `기기 추가…` and the Add a project dialog's host list do.
Removing the device in front moves the front to This Mac; that device's agents and its `~/hide` stay on it.
Registering the first remote device, reachable or not, removes the footer's device button and shows the rail with This Mac selected and the center unchanged; removing the last one takes the rail away and gives the button back.

With no remote device there is no rail: the footer's left end is a laptop button whose popover lists `This Mac · 이 기기` with a check and `기기 추가…`.

A device's Workspace in front wears the device color, `--device-remote`: a band at the start of the Workspace toolbar with the server glyph and the device's name, truncated, and a border of the same color around its panes; this machine's Workspace has neither.

## Start panel

Web owner: `web/src/StartPanel.tsx`, `web/src/startTargets.ts`, `web/src/startDraft.ts`, `web/src/startAnswer.ts`, `web/src/agentPicker.ts`, `web/src/components/agent-picker.tsx`.

The start panel starts a Claude or Codex agent with a first instruction anywhere hide can reach (PRD home-device-rail D-17..D-22).
⌘N opens it in the desktop app, where it is also File › Start agent; in a browser tab ⌘N stays the browser's and `에이전트 시작…` in ⌘K opens it, on every screen.
It floats at the ⌘K palette's place and width with no backdrop, and the keyboard lands in its one-line text box, `무엇을 시킬까요?`.
Opened while Settings is up, it takes Settings' place: Settings closes and the target is the front device's Home.
Under the text are the target, the agent kind and the model menus, a `⏎` keycap and `시작`; Enter or `시작` sends one `agent_start_in_checkout` with a fresh request id, the text as the agent's first instruction, handed to the CLI as its own argument (ARCHITECTURE.md, the first prompt).
The target defaults to what is in front: the checkout of the Workspace in front (a worktree when that is it), a project's main checkout while its Overview is in front, and the front device's Home while a Home Overview, the Inbox or Settings is; a device's surface in front makes that device the target's.
The target menu lists the front device's Home and checkouts, then each other device's Home and checkouts prefixed with its name, with a separator between devices and a check on the chosen item; a device that is not connected is listed disabled with `연결 안 됨`.
The kind menu holds Claude and Codex with their provider marks; the model menu is `CLI 기본값` then the chosen kind's catalog, and changing the kind takes that kind's list and the model last chosen for it.
While the catalog is being read or cannot be read, the model menu shows the remembered model or `CLI 기본값`, is disabled, and gives the reason in its tooltip; starting is never held back by it.
Every start that names Claude or Codex, here or in a dialog, is remembered by the core with its model, `CLI 기본값` included, so the next open preselects that kind and its model with no `recent` mark; the target is not remembered and follows what is in front on every open.
Escape or a press outside closes the panel and keeps the text for the next open; a start that goes clears it.
The panel follows only the answer carrying its own request id, even after it closes: a refusal or a failed start shows its reason inside the panel and keeps the text, and a start that went brings the center to the new pane, on a device by moving the rail, the sidebar and the center together.
An agent that fails to start after its tab opened puts its text back in the draft with the reason, unless a new draft took its place, so the next ⌘N shows both; the reason stays until the text changes.
A start with no answer in 90 seconds, the core's own limit for one, says so inside the panel.
New worktree, Start from an issue and 맡기기 carry the same kind and model menus with the remembered choice preselected, and what they start becomes the next default; New worktree's kind menu starts with `Terminal only`, which is never remembered.
Settings › Issues has no default agent of its own.

## Weekly usage

Web owner: `web/src/components/weekly-usage.tsx`, `web/src/usage.ts`.
The core reads the numbers and names each row's state (`navigator.provider_usage`, [AI_PROVIDERS.md: weekly usage display](AI_PROVIDERS.md#weekly-usage-display)); the shell only draws them.

The sidebar footer reads the device button at its left while there is no rail, then one chip per provider and the Settings gear at its right; a chip is the provider mark and the rounded percent of the seven-day window.
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

Web owner: `web/src/search.ts`, `web/src/Palette.tsx`, `web/src/components/sidebar-header.tsx`.

Agent/workspace search and file search share the same focused query field and first-result selection behavior.
An agent result is titled by the identity every other surface uses and subtitled by the row's second line, falling back to the status word when the state chose no sentence; the pane id leaves the printed row but still matches the query and is read by accessibility, so a result can be found by title, sentence, or id.
On the web, the Search icon at the end of the sidebar's tab strip or its top line, hinted `Search ⌘K`, opens the same palette Command+K opens, and the query row carries an `Esc` keycap.
Results sit under headers in the form `<project> > AGENTS` (an agent under the first project whose checkouts hold its pane, `Home > AGENTS` for its device's Home, `AGENTS` when none does), `WORKSPACE > COMMANDS`, `COMMANDS`, `WORKSPACES > PROJECTS`, `WORKSPACES > CHECKOUTS`, and `DEVICES` while another device is registered (This Mac alone has no device row).
Search covers the agents, projects and checkouts of every connected device and the registered devices themselves; a result not on the device in front carries that device's chip after its title, and choosing it brings that device forward with it, a device result selecting its tile.
`에이전트 시작…` under `COMMANDS` is on every screen and opens the start panel.
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

Web owner: `web/src/PaneRelations.tsx`, `web/src/lineage.ts`, and the [Agent panes and the Agents explorer](#agent-panes-and-the-agents-explorer) subsection of Web Workspace above; the [agent workflow contract](../design/agent-workflow-review.md) is the authoritative lineage/ownership document.

The pane header keeps one identity row naming the pane and its status; a pane with children gains a second row for child chips that exists only when there are children.
An authoritative parent becomes a compact Return control in the first row, with icon-only fallback before current identity or actions are truncated.
A pane with children names the first direct child in the second row, and the remaining children fold into an adjacent `+N` relationship control opened by a bounded scrolling sheet; pending navigation disables child changes, and Retry remains attached to the inspected target that failed.
The relationship sheet inspects on row selection and navigates only through its explicit Open action.
A relationship Open or parent Return publishes one request-scoped pending state; the same target cannot dispatch again while that request is pending, and Retry starts a new request only after the prior one has settled.
Target retirement before dispatch, and a core-owned refusal, timeout, or remote-control failure, keep the current pane geometry and tab topology, show the scoped reason, offer Retry when the outcome is retryable, and offer Dismiss to clear only the notice.
The shell never derives success from an old focused layout or optimistic remote selection, never attributes an unrelated global error to the control, and never sends a second focus event as rollback; a canvas notice preserves pending and failed feedback after successful navigation removes the source header or sheet from view.
A root with no parent carries no Return control, following the rule that a control with nothing to do is not drawn.
A child chip names the child's checkout branch when it differs from the parent checkout, otherwise it keeps the child's identity label.
A child on another device adds the server-glyph device chip with that device's real display name.

Ownership is drawn as emphasis, not as a new color or container: the operator's own rows are bright, delegated rows are subdued, and nothing new is introduced, because a delegated row is simply never emphasized.
A child's question or completion reaches the operator through its ancestors: the ancestor row turns unread and its descendant badge changes, and the ancestor's own group does not move.
An uninstrumented mark (agent detected but its subagents not visible to Hide) is drawn only where an agent was detected, is a mark plus an accessible name and never a color alone, and its subagent count sits beside it as a badge; a count Hide cannot read is drawn as unknown and never as a zero, because a zero claims the agent is working alone.
An Overview agent row reuses the same agent identity and state presentation as the sidebar and relationship sheet; a missing row means the current live projection has no agent there, and an uninstrumented mark never means zero.
The header wash marks the pane Hide is showing, while the outer primary indicator marks the terminal that owns keyboard focus; moving keyboard focus into Overview keeps the shown wash and removes the terminal outline.
Unread weight is never reused to mean parent, child, delegated, or selected.

## Mobile companion

### Settings > Mobile

Settings has a Mobile tab after Devices.
Its switch, 폰에서 hide 열기, is off at first, and its description says hide turns tailscale serve on and removes only the entry it made.
While the switch is off hide runs no Tailscale command.
Turned on, it shows four steps in order: Tailscale installed on this Mac, logged in with the Mac's name, MagicDNS and HTTPS on for the tailnet, and Tailscale on the phone with the same account.
A passed step shows a check; only the first failing step shows a warning with its action (a download link, "Tailscale 앱에서 로그인", or the admin console's DNS page with a link), and the steps after it wait.
The phone step is guidance hide cannot check: it waits until the QR shows and then reads as done.
The tab rechecks every three seconds while it is open, so logging in or turning HTTPS on continues without reopening it.
Once every Mac step passes and hide has confirmed its serve entry, the tab shows the QR, 폰 카메라로 찍으세요, the ts.net address, the code's m:ss countdown and 새 코드.
An HTTPS entry hide did not make shows its target in one line and no QR; a failed serve command shows the failed step and its message in one line, and so does a Funnel that would publish the address, which also disconnects every phone when it is turned on later.
A removal that fails when the switch goes off keeps that line under the switch until hide finishes it.
Opening the tab, pressing 새 코드, or a phone pairing shows a new code, and the previous code stops working.
The phones group is titled 연결된 폰 · n / 4; each row shows the phone's name, when it was last seen, whether notifications are on or off, the days left before the seven-day revoke once it has been away a day, and 해지, which closes that phone at once.
푸시 알림 offers 끔 (the default), 앱이 닫혀 있을 때만 and 항상, and the choice survives a restart.

### The phone app

The QR opens a page with the hide icon, "<Mac>와 연결", a line on what the phone can do, 연결, and a note that the code expires in five minutes.
연결 opens the list and a one-time hint to keep hide on the Home Screen; an expired or spent code says so, a fifth phone is told the limit and to revoke one on the Mac, and a page opened with no code or credential says to scan the QR in Settings > Mobile.
The list's header shows hide, the Mac's name, how many other phones are connected, and a connection dot.
Agents sit in 내 확인 대기, 끝, 진행 중 and 확인함 with their counts, each row with its status mark, provider mark, task name, project and branch, the SSH device's chip, the elapsed time and the request or news line, and the list updates live.
With no agents the list is one line, 실행 중인 에이전트가 없어요.
The header's `+`, 에이전트 시작, opens the start sheet: a text box for what to do, the target (This Mac's Home first, then This Mac's checkouts, then each device's Home and checkouts, a device that is not connected disabled with 연결 안 됨), the kind, Claude or Codex, and the model, both preselected from the desktop's remembered choice, and 시작.
시작 starts the agent there; once it appears in the list the phone opens its detail.
Pressing 시작 again for the same text and choice after a lost answer sends the same request id, and hided starts one agent for it.
While hided is out of reach the sheet shows the unreachable line and keeps what was written; a refused start shows its reason inside the sheet and keeps the text, which is cleared only by a start that went.
When hided is out of reach the last list stays dimmed under "연결 안 됨 · 맥의 hide가 꺼져 있거나 폰의 Tailscale이 꺼져 있어요. 다시 시도 중", the app retries on its own, and it shows the same line when opened without a network.
A row opens its detail: ← 목록, the elapsed time and the row's head, with 대화 | 터미널 between ← 목록 and the elapsed time when the agent has a conversation to show, a Claude Code or Codex agent on this Mac whose session Herdr reports.
대화 is the agent's own conversation, its newest 30 messages with the newest at the bottom: the operator's messages as ❯ blocks on a grey ground, the agent's Markdown drawn at full width (headings, emphasis, lists, tables, and code wrapped without highlighting, a link or an image as its text), an interruption as 중단됨, and the time after each turn; pulling to the top loads 30 older messages at a time up to the first, and at 300 it says 최근 300개까지 볼 수 있어요.
What the agent writes next arrives on its own, and tool output and injected context are never shown.
터미널, and the whole detail of any other agent, is the pane's recent rows read-only in the terminal's colours with the newest at the bottom, each row wrapped at the phone's width so the detail never scrolls sideways; pulling to the top loads older rows until the pane has no more.
Every detail has the quick keys (Enter, Escape, 위 화살표, 아래 화살표, Ctrl-C by accessible name) and a one-line reply with 보내기; a reply is sent with Enter after it and clears on success, a failure keeps the text with the reason under it, and a reply over 2,000 characters is named before it is sent.
When hide cannot tell whether a reply reached the pane, the line says so and asks the operator to check the terminal before sending it again.
A closed pane shows "이 pane은 더 이상 열려 있지 않아요" and disables the reply bar; a disconnected SSH device shows that its device is not connected.
With push on, the list offers 알림 켜기; a Safari tab without push is told to open hide from the Home Screen first, and a refused permission shows "알림이 꺼져 있어요 · 설정 > 알림에서 hide를 켜세요".
A notification's title is the task name and its body 내 확인 대기 or 끝 with the project; tapping it opens that agent's detail.
A revoked phone shows "이 폰의 연결이 해지됐어요. 맥에서 QR을 다시 여세요." and drops its own push subscription.
The app follows the phone's light or dark setting, draws text a quarter larger than the desktop, and keeps every control at least 44 points tall.

## Keycaps, tooltips, and icon buttons

Every icon-only control has a tooltip and an accessible name carrying the same words as the tooltip.
A chorded tooltip reads the label followed by the shortcut chord; a chordless control shows only the label.
There is no native platform tooltip layered underneath the shared one; the shared tooltip is the only tooltip in the main shell.
Tooltip hover has a short reveal delay, and an exact modifier hold reveals shortcut hints faster than a hover tooltip does.
In the desktop app, holding ⌘ alone floats each tab's number at its top right in the agent tab strip in front, and holding ⌥ alone floats each Agents-list row's number at its top right: a keycap in the popover colors with a border, a small shadow and one mono digit, positioned over the tab or row rather than in it, so a title, an inline Rename field, a row's time, and its fold slot never move.
The number is the screen order at that moment, first to ninth, left to right for tabs and top to bottom for the rows the Agents list draws (a folded parent's descendants are not rows), and an item past the ninth carries none.
While Projects is on screen the same hold shows those Agents-list numbers, since ⌥n still selects by them, each once: on the agent's raised row, else on its row under the checkout that owns its pane.
The hint appears only after a short hold of the exact modifier; releasing it before then shows nothing, so a ⌘C never flashes numbers.
Releasing the modifier, adding another, pressing any key during the hold (including the numbered chord itself), losing the window, hiding the page, or opening a sheet, menu, dialog, palette, or cycle clears the numbers at once; the same modifiers still held after that show nothing until they are released and held again.
The keycaps and the hover tooltip never share space: a tooltip hangs beside its trigger and a keycap sits inside the trigger's own box.
A browser host has no numbered chords, so holding ⌘ or ⌥ there shows nothing.
Pane focus, active tab, tab order, zoom state, and disappearing anchors all update which controls can show a hint or tooltip; pointer exit, mouse down, scroll, key down, losing key window status, and anchor removal all dismiss an open tooltip.

Destructive buttons are named by their result (`Move to Trash`, `Close 3 panes and remove`, `Stop work and close`), never by a generic "Delete" or "OK" that hides the consequence; the non-destructive option is the default/cancel action.
Escape closes the innermost open layer and returns focus to whatever held it before that layer opened, including a terminal that was focused when a sheet, menu, or overlay opened over it.
A tooltip or hover card is not a layer: an Escape pressed while one shows still reaches the terminal or the screen it was meant for, and closes the tooltip on its way, whether the tooltip's own dismiss or the shell answers the press.
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
