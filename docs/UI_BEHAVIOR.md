# UI behavior

This document owns what Hide's UI does: the rules for how each screen and control behaves, independent of any platform's exact pixel values.
It replaces the behavioral content of the retired `DESIGN.md`.
Visual authority (what a control looks like) lives in the Pen library and `web/src/components/ui`/`web/src/components`, described in [DESIGN_WORKFLOW.md](DESIGN_WORKFLOW.md); numeric authority lives in `design/tokens.json`.
Each rule below names the code that owns it: a web owner in `web/src/` and, where the behavior is core-decided, a core owner in `herdr-core/src/`.

## Web Workspace

The web shell's Workspace screen follows the approved S6 proposal, its View areas follow the approved boards of PRD S7 (`agents/prd/workspace-views-layout/prd.md`), and its body follows PRD three-column-panel (`agents/prd/three-column-panel/prd.md`, issue 321): three docked columns, every control sitting once, on the container it changes.
Web owner: `web/src/WorkspaceScreen.tsx`, `web/src/ViewAreas.tsx`, `web/src/Tools.tsx`, `web/src/viewLayout.ts`, `web/src/viewDrag.ts`, `web/src/viewFocus.ts`.

The toolbar spans the Workspace's full width and holds the path back (`Home / Project / Workspace`, where `Home` opens the Overview), led by the device's colored band when the Workspace is not this Mac's; a tab in Home reads `Home / ~/hide`, since Home is no project.
Its right end holds three icons and nothing else, in this order: `Open server`, File Views and Tools.
`Open server` (a globe): one known listener opens directly as a page in File Views, and multiple listeners open a compact keyboard picker.
The picker shows each real bind address and port, preserves IPv4/IPv6 scope, and supports arrows, Home, End, Enter and Escape.
An empty, loading, disconnected, failed or stale catalog gives small feedback in the same popover; a remote Workspace explicitly has no local discovery.
Discovery never starts or stops a server, and an opened page retains the Browser View’s own connection failure and recovery controls.
The File Views and Tools icons are drawn pressed while their column shows, and each keeps one accessible name with its state as pressed or not.
Hovering or focusing an icon shows its name and chord in a tooltip, `File Views ⌘⇧B` and `Tools ⌘E`, with a rebound chord shown as bound; `Open server` has no chord.
While File Views is not on screen with views open, its icon carries a badge with their count, also given as its accessible description.
A right-click or the menu key on the toolbar offers Show or Hide File Views, Show or Hide Tools, Copy Workspace path, and Open Project Overview.
An empty Agent area offers New tab.
The area is empty only when the checkout has no tab: a checkout whose only tab holds delegated children keeps that tab off the strip and still draws it on the canvas, so the agent chosen from the sidebar there opens on its pane.

### The three columns

The Workspace body, under the toolbar, is three docked columns from left to right: Agent Views (the Agent area), File Views (the View areas), and Tools (the Explorer or History).
No column floats over another, carries a shadow or covers the agents; a column that is off takes no room, and Agent Views takes whatever File Views and Tools leave.
File Views and Tools are each on or off, stored per Workspace with the tool and both widths, and survive a restart; a Workspace seen for the first time shows Agent Views alone, File Views and Tools off, with the Explorer as its tool.
Each column's first row is its tab row, level with the others: the Agent tab strip, each top View area's tab strip with its own New tab (a new browser display in that area, see Browser displays), and the Explorer and History icon tabs, the shown one marked, named Explorer and History in their tooltips and accessible names.
No column has a title row, a close button or panel actions; the toolbar's icons and their chords are the only column controls.
The second row of File Views is level with the agents' first pane header: the active document's header over each View area, naming a file from its checkout and cutting a long path at its start so the file name stays.
With stacked View areas each area keeps its own tab strip.

⌘⇧B and the File Views icon toggle File Views only, and ⌘E and the Tools icon toggle Tools only: a column on screen turns off, and one that is not turns on.
Each sends the value the column should end at, so the same press arriving twice lands where it did once.
Turning File Views back on brings its areas, tabs, preview and active view back as they were; turning Tools on or off keeps its tool and leaves File Views and its views alone.
The desktop app's View menu names the two commands `Toggle File Views` and `Toggle Tools`, and a chord bound to either in Settings, Shortcuts before the change keeps toggling the same column.
The sidebar's Projects | Agents switch has no default chord and can be bound in Settings, Shortcuts; a bound chord shows in the tabs' hint.
`Toggle device rail` has no default chord either and is bound the same way; it shows or hides the device rail.

File Views is never empty: closing its last view turns it off in the same transition, Agent Views takes its width, and Tools stays as it was.
With no view open, ⌘⇧B or the File Views icon turns File Views on holding one New tab page (Open with File ⌘P, and Diff when the checkout has changes); closing that untouched tab turns File Views off again.
Tools alone is an ordinary state.
Opening a file, a diff or a page from the Explorer, History, ⌘P, a terminal link, Open in Browser or Open server turns File Views on with the opened tab in its active View area and leaves Tools as it was; revealing a file turns Tools on with the Explorer and leaves File Views as it was, and a terminal link that names a folder shows it in the Explorer the same way.
Choosing an agent or a tab from the sidebar, the palette, a tab cycle or another device focuses it in Agent Views and changes no column in a window wide enough for them all.

The column edges are dividers: Agent Views | File Views, and the edge left of Tools.
Each divider shows its grip on hover, keyboard focus and while dragging; a drag moves a guide with the pointer and lands once on release, and a focused divider moves one 32px step per arrow key, each divider a separator that reports its width.
Fast arrow presses retain every step before the preceding width has been confirmed, and reloading restores the final width.
The divider between File Views and Tools trades width between the two, so the agents keep theirs; the others change one column's width against Agent Views.
No column gets narrower than its minimum: Agent Views 480px, File Views 360px, Tools 260px; File Views starts at 640px and Tools at its usual tool width until first resized.
Turning File Views or Tools on or off, or landing a divider that moves Agent Views' edge, resizes the agents' terminals once; a drag in progress, an open, close or tab change inside File Views, a split or a drag of a view, and choosing an agent resize nothing.
Changing Workspace or column geometry during a column drag cancels its guide without saving a width, and releasing the old pointer cannot change the new Workspace.

Agent Views is always live: clicking a pane or a tab focuses it and typing goes to it, and chords such as ⌘F, ⌘T and ⌥W act where the keyboard is, a View area, Tools or a pane.
Hiding the column the keyboard is in hands the keyboard to File Views' active area while File Views shows, else to the focused pane, and Tab never walks into a column out of sight.
Inside File Views the View areas behave as they do anywhere else: tabs, splits, preview, dirty state and browser displays, each page inside its area's bounds.

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
An explicit local `--reveal` brings that Workspace and the opened View forward and turns File Views on, showing it in a window too narrow for every column; without it, File Views keeps its state and its icon's badge counts the new view.
A pane of an Agent tab that is not the Workspace's active tab, calling `hide file open`, `hide diff open`, `hide browser open` or `hide view select` without `--reveal`, adds or selects the View for its own tab's bookmark and leaves the front of every View area as the operator left it (see View bookmarks per Agent tab).
Called from the active tab, or with no pane at all (a checkout-bound caller, which has no tab), the View comes to the front as before and the active tab remembers it; with `--reveal` it comes to the front and both the active tab and the calling tab remember it.
`hide view split`, `move` and `close` change the shared layout, so they apply at once from any tab.

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

Agent Views has its own area tree, separate from File Views' View tree.
Each area has a tab bar, a New tab button and the active tab's live pane canvas; dividers separate areas.
Only the area holding the keyboard carries the foreground-colored selected-tab underline, across both Agent and View columns.
Clicking a tab or pane activates its area and sends the keyboard to that pane.
All shown tabs stay attached and awake, while only the active area's tab receives read and sleep-visit updates.
Pane headers, child chips, relationship controls and pane splits remain inside each canvas; find belongs to the focused pane.
⌘F on a full-screen Claude Code or Codex pane opens that agent's own search over its whole conversation in the pane, with the agent's own keys and count (Claude Code: type, Enter, `n`/`N`; Codex: type, Enter, Ctrl+P), and no find bar appears; any other pane, and an agent drawing inline, gets the find bar.

An Agent area's tabs share its bar the way a browser's tabs do (the Pen library's `Component / Adaptive Work Tab`), shrinking in three continuous stages so the bar is used to its end and the selected tab keeps its title longest.
While an equal share still holds the title minimum, each tab asks for the preferred width and all shrink alike with their titles.
Below that the selected tab keeps the title minimum with its title and close control, and the other tabs split the rest alike down to the icon identity width.
An unselected tab narrower than the title minimum draws its marks and a truncated title with narrow padding and no close control, and below the icon identity plus one control it draws its marks alone, centred.
Once every other tab is a mark, the selected tab gives up width from the title minimum, keeping its close control and a truncated title while it holds the icon identity plus two controls, and below that it becomes its marks with its close control; less than one control's width can then stay empty at the bar's end.
The strip scrolls only when even that selected mark and the other marks overflow.
The same width and tab count always draw the same strip whichever way the window was resized, a resize never paints an overflowing frame, and opening, closing or selecting a tab shares the bar again at once.
A tab being renamed keeps the preferred width at every density, and each tab's tooltip and accessible name still give its agent, full title and state.
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
Next and previous Agent area focus and Agent area grow and shrink are chordless commands of the shortcut registry (Settings › Shortcuts › Area commands, and the desktop Pane menu); they have no key until the operator binds one, and one that cannot run now does nothing.
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

File Views holds one or more View areas, each with its own tab bar above its own view, split left/right or up/down as often as the limits allow.
A split divides one area in two along one axis, and either half can split again along either axis, so every arrangement of side-by-side and stacked areas is a tree of halves.
Each layout keeps its own active area for opening and layout actions.
The recorded keyboard owner alone gives one Agent or View area the foreground-colored selected-tab underline.
Every other area's tab bar uses the card background, and its selected tab keeps a foreground title and secondary background without the underline.
Areas have no visible focus perimeter; their transparent content border reserves the same space as before.
The content stays readable and unfiltered; focus adds no content blur, opacity reduction, capture loop or geometry change.
The same rule applies at every File Views width, in the narrow single-area presentation and in both themes.
An area's tabs each ask for the preferred width and shrink alike down to the title minimum; a file's type mark does not tell files apart, so a View tab keeps its title and never turns to marks the way an Agent tab does.
An area whose tabs outrun its width even at that minimum scrolls its own strip so the shown view's tab stays in sight whenever the shown view changes or the area is resized.
Clicking a tab or a view, or moving focus to another area with a focus-area command, makes that area active.
The divider between two areas turns accent-colored on hover and keyboard focus, drags with a guide line, and lands once on release, so a drag never resizes a document or a terminal on every pointer move.
A focused divider moves with the arrow keys along its axis, one step and one change per press.
No area gets narrower or shorter than its minimum, a divider stops where either neighbour would, and each side of a split keeps between 15 and 85 percent of it.
An area whose last view leaves disappears, and its neighbour takes the space.
When the last view closes, File Views turns off and Tools stays as it was, with that state saved for the Workspace.

### View bookmarks per Agent tab

The View list, the layout, the columns' state and widths, and every document's text belong to the Workspace, so they are the same whichever Agent tab is in front.
Each Agent tab remembers, for every View area, the View that was in front while that tab was the active one, and gets it back when it is shown again.
Showing a tab that no Agent area of the Workspace showed a moment before makes each area that still holds that tab's bookmarked View show it, in the same frame that shows the tab.
A tab strip click, a sidebar agent or tab choice, the palette, a tab cycle, a pane in another checkout and a tab that Herdr's own focus moved to all do this; the tab's own bookmark applies to the Workspace it belongs to.
Nothing opens, closes or splits: an area whose bookmarked View has closed or moved to another area keeps what it shows, and a tab with no bookmark changes nothing.
A new tab, a delegated child moved to its own tab and every tab right after an update have none.
The area in use, the keyboard and the columns stay where they were; while File Views is off the bookmark still applies, so turning it on shows the tab's front.
Moving focus between Agent areas that already show their tabs, or between the panes of one tab, restores nothing, so two agents side by side never swap File Views under the operator.
When the front of a View area changes, the active tab remembers the new front.
The View that went behind when another tab came forward stays in the strip, one click away, and choosing it is that tab's new bookmark.
A bookmark points at a View, not at a file, so a preview View that another tab retargeted shows the new document when it returns; a pinned View is unaffected, and there is no preview View per tab.
Bookmarks are saved with the Workspace and survive a restart, and a tab that closed or vanished loses its bookmark.
Connected device Workspaces follow the same rules; they have one Agent area, so the visible tab changes when its Herdr's answer lands.
The phone never shows or changes a bookmark, and no badge, mark, notice or setting shows one.
Web owner: none, the shell draws each area's front as the snapshot names it.
Core owner: `herdr-core/src/runtime/view_bookmarks.rs` (`track_view_bookmarks`, `parked_caller`), `herdr-core/src/view_bookmarks.rs`, tests in `herdr-core/src/runtime/tests/view_bookmarks.rs` and `web/e2e/view-bookmarks.spec.ts`.

A Workspace holds at most 6 View areas, no area sits more than 3 splits deep, and at most 64 views are open at once.
A split past the area or depth limit is refused with its reason, and an open past the view limit says so and keeps every currently open view as it was.
Opening a file that is already shown moves to its view, so it is never refused.

### Opening, preview, and Open to the side

A single click on an Explorer file or a History row opens it in the active area's preview view (italic title), and the next single click replaces that preview in place, so browsing leaves one tab per area rather than a trail.
Each area has at most one preview, and a click never touches another area or a pinned view.
A double-click on the row or the tab, Keep open, or the first edit pins the preview where it is, including a double-click that lands while the file's first read is still running; because the first edit pins, a document that is dirty, saving, or whose save failed is never a preview in any area that shows it.
Opening a file that is already shown moves to its view instead of adding a tab, choosing the one used last when several views show it.
Opening a file while File Views is off turns it on first.
Diffs are placed by the same rules.

Open to the side (from the Explorer's file menu, a History row's menu, or ⌘↵ on a ⌘P result) is the only way to show one file twice.
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

A right-click on a View tab, or the menu key while the tab has focus, opens its menu with these items in order: Keep open, Split right, Split left, Split up, Split down, Move right, Move left, Move up, Move down, Copy path, Select in File Tree, the OS file manager's reveal, Close view.
Keep open appears only on a preview view.
Select in File Tree shows the Explorer with the file's row unfolded and selected and opens nothing; the reveal follows [the reveal rule](#explorer-file-management), and a page has neither.
Both are disabled with the reason while the file is unavailable or cannot be read yet.
A Move item appears only toward an area that exists in that direction, and moves the view into it without a split.
A Split item the Workspace cannot make stays listed, disabled, with its reason under it, the way every web menu draws an item its target cannot use.
The labels say where the view goes, never "Move to Group".
Close view closes the view and never the file on disk; the menu has no file deletion and never closes a pane or a tab.
Opening the menu moves no focus and changes nothing, and Escape closes it.
Next and previous View area focus and View area grow and shrink are chordless registry commands like the Agent area ones, and ⌘↵ on a ⌘P result opens that file to the side, so splits, moves, closes and resizes can all be done from the keyboard once the operator binds the area commands.

### View states

A view whose file is being read shows `Opening…`.
A restored view whose device or root is not ready says what it waits for (such as `Waiting for <device> to connect`) and reads its file by itself once that is ready.
A view whose file cannot be read shows why, with Close view and Retry, and its tab title reads as struck through; either action acts on that view alone and leaves the active area where it was.
Each state belongs to its view alone, so one missing file never blanks another view or area.
After a restart the app reopens the last Workspace as it was left: its areas and their sizes, each area's tabs in order with its preview, pinned views, and active view, the active area, whether File Views and Tools are on, their widths, and the tool; unsaved text returns from the browser's drafts, and Herdr's current tabs and panes are used as they are.
A first run, or a last Workspace that no longer exists, starts on the Overview.
A layout file that cannot be read is kept aside and the app starts on the Overview, where the operator picks a Workspace and continues with a new layout.
A Workspace stored with the side panel that came before the columns restarts with File Views on when its panel was open or expanded and off when it was closed, docked whether or not it was pinned, its tools, tool, views, tabs and areas as they were and both widths at their defaults; one stored before the side panel, with a layout instead of a panel state, restarts with File Views on unless it showed the agents alone.

### Browser displays

A View area's new tab offers only what can become a display, never a tool or a pane.
The View strip’s +, its New tab menu item, and the new tab chord while the area holds the keyboard open a distinct browser display in that area with no address and the address field focused.
Its body shows Open with File (⌘P), opening the file palette, and Diff only while the checkout has changes, opening a palette of changed files.
Picking either replaces that empty tab in place; entering a URL navigates the same tab, and closing an untouched tab creates no recovery entry.
The new-tab page offers no tools or panes; ⌘P remains the file palette everywhere.

A web page is a view like a file: it opens in the active area, has a tab, splits, moves, and closes like one, and comes back after a restart at the address it last showed.
A page with an address carries a globe mark and the page's title, else its host, else a local file's name; its tooltip and accessible name carry `Page`, the title, and the full address.
It opens from `hide browser open` in a connected Herdr pane or a shell inside a registered checkout, from Open in Browser on an HTML file in the Explorer's menu (listed after Open to the side), from a page that opens a tab (beside that page, which stays in view: in the area next to it, else in a new area to its right), and from the address field.
A sign-in popup a page opens is a small window of its own above hide, so the sign-in can report back to the page and close; ⌘W closes it.
A link that would open another app (Slack, Zoom) asks first with the app's name, and opens in that app only on Open.
On a connected SSH device, the native page uses that device's localhost or consented checkout resources; a route failure appears in the page's existing failure state.
Opening an address the Workspace already shows moves to that view and loads it again instead of adding a second one.
The view's own toolbar holds Back, Forward, Reload (Stop while the page loads) and the address, which shows a web address without its scheme until it is focused; focusing it selects the whole address, Return loads what was typed, and Escape puts the page's address back.
A page that cannot load says so in its place with the address and the reason, and Reload tries again; nothing else on screen changes.
While the palette, a menu or a dialog covers a page, the page is shown as a still picture of itself from its first frame, so the overlay draws over it and the page is never seen on top; a page with no picture yet is blank for a moment until one arrives, and an older picture turns to the page as it is now within a few frames; the page comes back live when the overlay closes.
A tooltip never turns a page into a picture; a tooltip, such as a tab's or a page toolbar button's, opens on the side of its trigger where the whole of it shows clear of every page, and keeps its usual side when no side is clear.
While the shell drags something (a tab, a divider, a column divider, an Explorer item), every page is shown as its still, so the guide or preview draws over it and a drop lands in the shell rather than the page; the pages come back live at release, and a drag inside a page is the page's own.
In a plain browser tab the view keeps its address on the toolbar row, level with a document header beside it, and below it reads `Pages open in the hide desktop app.`; a web address offers Open in browser, and nothing else is drawn in its place.
[BROWSER_DISPLAYS.md](BROWSER_DISPLAYS.md) owns which addresses a page may hold, the `file:` boundary, and the page's lifetime.

### Narrow windows

How many columns show depends on the Workspace body's width, not the window's, so hiding the sidebar with ⌘B can bring a column back.
The steps are where the column minimums and the 8px dividers between them fit: 1100px of columns (1116px of body with both dividers) and 840px (848px).
At the wide step every column that is on shows.
Between the two steps, Agent Views shows with one more column: File Views when both are on, Tools hidden first.
Calls made at a different width step do not override this fallback; entering a new step clears only the temporary column choice, and a call made in that step keeps its effect until the step changes.
Calling Tools there (⌘E, its icon, a reveal) puts Tools in File Views' place with its icon pressed and File Views' not, and opening a file or ⌘⇧B brings File Views back.
Under the lower step one column shows, Agent Views first; calling File Views or Tools, or opening a file, gives that column the whole body, with Agent Views kept at its size out of sight and taking no pointer or keyboard.
Choosing an agent from the sidebar, the palette or a tab cycle, or pressing the shown column's icon or chord again, gives the body back to Agent Views; that press sends nothing, so the column stays on.
A narrower body starts on Agent Views each time it drops under the lower step and each time another Workspace comes in front, unless that Workspace was just called, as a `--reveal` does.
No column ever floats over another.
When the View areas cannot all have their minimum, only the active area shows, with an area switcher to the others.
Widening the window brings back what the Workspace stores, whether each column is on, their widths, the tool and the area sizes, because none of these narrow arrangements is stored, and another Workspace's columns never change while one is narrow.

### Library masters

PRD three-column-panel D-13 requires a real `Screen / Workspace` board and column masters replacing the old `Component / Side panel` sheets through the design workflow.
The operator delegated final design judgment within the approved direction; delivery requires that review to compare the board with actual native captures in both themes and to confirm B35.
The View area masters in `design/hide-ui.lib.pen` are `Component / View tab`, `Component / View insertion line`, `Component / View split overlay`, `Component / View tab menu`, and `Component / View area message`.
Their sheets draw every state as refs: the View tab sheet draws preview, pinned, hover, active-in-the-active-area, active-in-another-area, dirty, unavailable, diff, a long title, and the floating drag copy; the placement sheet draws a reorder, a move into another area, a right and a down split, and an ineligible target; the tab menu sheet draws a preview's menu and a pinned view's menu with a disabled Split and its reason; the area states sheet draws each view's opening, waiting, and unavailable states.
The browser display's toolbar and its loading, load failed, and plain browser tab states are on `Component / Browser file diff toolbars`.

### Agent panes and the Agents explorer

Several View areas leave Agent Views as it was already drawn: the toolbar's icons, Tools, and the child chips below behave the same with one area or six.

A pane header reads, left to right: the Return mark of a child pane, the agent's status mark and provider mark under the sidebar row's rules (a plain shell has the neutral `>_` mark and no status mark), the title, the zoom control, the status caption, then the overflow control and ×.
While the tab is zoomed, the zoomed pane's header carries a zoom control that names how many panes it hides (`+1`) and unzooms the tab when pressed; an unzoomed header has none.
When a tab shows more than one pane, a thin subtle-foreground outline surrounds the pane whose terminal holds the keyboard, and stays while that pane's menu is open; a lone or zoomed pane has no outline, and moving the keyboard to a View area, the sidebar, or another app removes it while the header wash stays on the focused pane.
The focused pane's header reads in the foreground color and every other pane's header in the muted one, so the wash is not the only difference.
Web owner: `web/src/PaneView.tsx`, `web/src/PaneGrid.tsx`.

A pane whose agent delegated work shows every direct child on one row under its header, each chip a status mark, the provider mark, and a capped title; the row scrolls sideways instead of growing, and a pane with no children has no row.
Its library masters are `Component / Pane child chip` and `Component / Pane child row`.
A chip opens the existing child at once; while that move is in flight the chip shows a pending mark and repeats of it are ignored, and a failure shows the core's reason under the header with Retry (when the core says it can be retried) and Dismiss.
A child pane has a compact Return mark in its identity row, named with the parent in its tooltip and accessible name.
The pane menu (from its overflow control or a right-click on the header) lists the parent, the other siblings, and the children as explicit Open items, then Copy pane name and Close pane, which asks about the agents it spawned as Closing an agent that spawned others says; opening it moves no focus and marks nothing read.
A right-click in the terminal focuses that pane, like a click, and opens a longer menu: Copy (only over a selection), Paste, Select all, and Find; then Split right, Split down, and Zoom pane or Unzoom pane (disabled on a tab's only pane); then the pane menu's items; an item with a chord that does the same shows it, ⌘C and ⌘V included.
The right-click leaves the drag selection as it was, so Copy copies what the operator selected; the program in the pane never hears it.

A pane whose agent sleeps (PRD agent-sleep) shows its state in place of the terminal, which stays hidden until the agent is back because the shell under it is not what the operator was talking to: Sleeping with the last progress line and Wake agent; Waking… with how old the resumed conversation is; or `Couldn’t resume this conversation` with the core's plain reason, Retry, and Start new session.
The header caption reads `☾ sleeping · 22h`, `☾ waking…` or `could not resume`, and typed input to the pane goes nowhere.
Opening the pane's tab by a committed move (a row, a tab, a checkout, a relation) wakes it in the same pane with its conversation; a Recent Panels preview does not, and neither does a click inside the tab already on screen.
The pane menu offers Sleep agent on a local agent pane that is awake, disabled with the core's reason when the agent is working, waiting for the operator, of another kind, or has no conversation Herdr reported.
Web owner: `web/src/PaneView.tsx` (`SleepBody`), `web/src/sleep.ts`, `web/src/PaneRelations.tsx`.

The sidebar's Agents tab groups the device in front's current agents under Needs You, Done, Working, and Seen, and leaves an empty group out; no row names its device, since the whole list is that device's, and a device that is not connected lists nothing it only last reported (the Palette still names a remote row's device).
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

A sidebar agent row's first line is always its status mark, provider mark, stable task name, a branch chip only when a delegated row's checkout differs from its parent's and the row above does not already name that checkout, a device chip for a row on an SSH device, the badge, the elapsed time, and a parent's chevron; the elapsed time is always drawn and never gives way to a control, and the time is counted on the shell's one-second clock from the moment the core saw the agent change state, and an agent the core has no time for shows none rather than a made-up `0s`.
The device chip is the existing Badge treatment with a server glyph and the device's real display name, never a connection state inferred by the web shell.
Its second line exists only when the row has something to say, and from the moment it does: a question, approval or error keeps its request in the warning color (red for an error) until it is resolved, however often the row is read, and a row that changed since the operator last looked shows its sentence bright until it is read; either is one line, cut at its end.
A quiet sentence is never drawn on the row; the row's tooltip carries it with the full title.
In Agents a root row adds a fixed context line naming its project and checkout (`project › checkout`, the project alone for a plain folder), since the list does not otherwise say where the agent works; a row under a checkout in Projects has none, because the rows above it say it.
Hover, keyboard focus, selection and an open badge list change a fill, a ring and a chevron's opacity only; they never add a line or change a row's height, so the row below never moves.
A sidebar agent row is 28 high, or 44 with its second line: line one is 20 and line two 16, and the title is 12/400 in every state, a change brightening it rather than thickening it.
A row's height is a minimum, not a cap: a row grows to hold its lines rather than letting them run into the next row, and it still holds still under hover and focus.
No row draws a progress number or step the agent did not report.
The Project Overview's agent rows keep their own density: a quiet sentence is revealed on the selected or hovered row over up to two lines, with the whole of it in the tooltip.
While a close the core runs names an agent's pane, alone or as part of a subtree close, its sidebar row adds `closing…` after its name until the row goes.
Web owner: `web/src/agentRow.ts` (rules, reusable by any list of agents), `web/src/components/sidebar-agent-row.tsx` (the sidebar row), `web/src/components/agent-row.tsx` (the Overview row and the descendant badge both draw), `web/src/components/agent-children-popover.tsx`.

### Closing an agent that spawned others

Closing a pane or a tab asks about the agents spawned from it only when one of them runs outside what closes (PRD close-agent-subtree).
Those are the live descendants of every agent pane that closes, on this machine or a connected device, asleep or not, less the panes that close anyway; a device that is not connected contributes none.
With none, the close is the ordinary one: a quiet pane closes at once, a working or attention pane asks the Stop-work confirmation, and an unknown status shows the status notice.
A target whose own status is unknown still shows only the status notice.
The Stop-work confirmation has one line under its title and lists every pane that closes, named as the sidebar names it, with its status mark; a working, asking or unknown pane adds its status word, and a quiet one is dimmed with its state in the mark's tooltip and its accessible name.
While it is open the list is live: a pane that settles stays listed and dims, one that starts working or asking brightens, and its title, line, buttons and focus stay as they opened.
While a listed pane's status is unknown, `Stop work and close` is disabled, `Keep open` takes the keyboard if the button held it, and a `Check status` in the confirmation reads the status again without closing it.
`Stop work and close` sends the close the confirmation shows at the press.
Otherwise one sheet opens in place of the Stop-work confirmation, titled `이 에이전트와 자식 N개를 닫을까요?` (a tab: `이 탭과 자식 N개를 닫을까요?`), N counting only the descendants outside.
It has no sentence under the title, and lists the target first and then its descendants in tree order, indented by depth, each with the sidebar's status mark and name and a device chip when it runs on another device than the target; the list scrolls inside the sheet when it is long.
Closing a tab lists every agent in the tab as a target, each followed by its own descendants, so a working agent beside the parent is shown before it closes.
A line above the list counts, with the sidebar's marks and neutral text, the descendants that are working (`진행 중`), waiting for an answer to a question, approval or error (`답 대기`), holding an unread result (`확인 안 한 결과`) and unreadable (`상태 모름`), leaving out a kind with none; those rows are bright and add their status word in neutral text, and a quiet one (idle, read, asleep) is dimmed with no word, its state in its mark's tooltip.
Each row is focusable, and its accessible name is its name, its device when it differs, and its status word.
`이것만 닫기`'s tooltip and accessible description say what it leaves: the children keep running and come up into the operator's own list.
Colour in both sheets is the status marks' and the one destructive button's (`모두 닫기`, `Stop work and close`); `취소`, `Keep open` and `이것만 닫기` are neutral.
Its buttons are `취소`, `이것만 닫기` and `모두 닫기`, and `모두 닫기` holds the keyboard when it opens, so Enter closes the whole subtree; Escape or `취소` sends nothing and gives focus back.
While a descendant's status is unknown, `모두 닫기` is disabled, `취소` holds the keyboard, and a `상태 확인` in the sheet reads the status again without closing it; while a target's own status is unknown, `이것만 닫기` is disabled too.
`이것만 닫기` closes the target alone as the ordinary close, with no second question; its direct children become the operator's roots and their own children stay under them.
While the sheet is open its list is live, as in the removal dialogs: a descendant that appears shows up, one that goes drops out, and the title's N and the count line follow.
`모두 닫기` sends one event naming the target and exactly the descendants the sheet shows at the press: one that appears after the press is not closed, and one already gone counts as closed.
The core checks each listed pane again as it arrives; if one now needs a status check, an earlier close of one is still unresolved, or the close slots cannot take them all, nothing closes and the one-line notice says why and what to do.
A pane whose earlier close was refused is simply closed again, since asking again is the retry.
If every descendant goes while the sheet is open, it turns in place into the target's Stop-work confirmation, a quiet target dimmed, whose `Stop work and close` closes the target alone; a descendant that appears under an open Stop-work confirmation turns it into this sheet the same way.
Either one closes by itself only when its target pane or tab is gone or its device disconnects; otherwise it waits for the operator's answer and runs nothing on its own.
A sheet that turns into the other leaves the keyboard on the sheet itself, never on a close button an Enter meant for the old one would press.
The core closes the deepest descendants first, each pane only after every descendant below it is gone, and the target last, so no descendant ever surfaces as a root on the way; each row reads `closing…` until it goes.
A descendant whose close is refused, times out, or loses its device keeps itself and its ancestors, the target included, open while the other branches finish; the close failure notice shows and the detail goes to the diagnostic log.
Closing the same target again lists only what is left.
Each local pane or tab closed this way gets its own Reopen closed tab entry; a device's close leaves none, as before.
Every entry to a pane or tab close uses this: ⌘W or ⌥W, ⌘⇧W or ⌥⇧W, the desktop menu's commands, a tab's ×, the tab menu's `Close tab…`, the pane header's ×, the pane menu's `Close pane`, and the agent row menu's `Close tab…`; closing a View, the phone app and the Overview are unchanged.
Web owner: `web/src/close.ts` (which sheet, which panes and descendants, and their states), `web/src/Overlays.tsx` (both sheets), `web/src/components/subtree-list.tsx` (the list both the sheet and the removal dialogs draw); core owner: `herdr-core/src/runtime/tree_close.rs`.

### Terminal links

A terminal pane links what its program prints, whichever program it is: an http(s) URL, a path, and a link the program marked itself (OSC 8).
A plain-text path is a link only where it names something on this Mac, so a word that merely looks like a path draws nothing; the check runs when the pointer reaches a row, never per frame.
A relative path is looked for under the pane's folder and then under its checkout's root, `~/` names the home folder, and a location written after it (`:12`, `:12:5`, `#L12`, `(12,5)`) is where the file opens.
The original spelling is checked before treating punctuation, a Korean particle or a location as context: if `docs/a.md)에`, `docs/a.md)` or `src/a.ts:12:5` is itself a file name, that complete name wins.
Composite punctuation, particles and locations also preserve grammar-only literal names such as `(src/a.ts:12:5)` before dropping their leading or closing punctuation or interpreting the location.
Among the bounded interpretations, the longest existing path wins; a failed or budget-skipped longer check never establishes a shorter link.
In Korean prose such as `보드 보기 (docs/README.md)에 C안을 추가했습니다.`, only `docs/README.md` is underlined and clickable when that is the actual path.
Grammar is interpreted only after a closing parenthesis, square/curly bracket or quotation mark: 에, 에서, 에게, 께, 으로, 로, 와, 과, 을, 를, 은, 는, 이, 가, 의, 도, 만, 부터, 까지, optionally followed by one of 도, 만, 는 or 은.
An unsupported word, an unbounded particle chain, or Hangul attached without that closing boundary is never shortened.
Ranges follow the terminal's actual cells, including wide glyphs, combining sequences and wrapped rows.
A path or URL the terminal wrapped at its last column, or a TUI broke at its own margin and indented, is one link across its rows; a URL spans rows only where the terminal wrapped it, because it cannot be checked.
Under the pointer a link is underlined and the pointer becomes a hand; a click on it is the link's and never reaches the program, and a drag across it selects its text and opens nothing.
A click opens it in the front Workspace: a URL as a browser display, a file in View at its line (an ordinary tab, not a preview), a folder revealed in the Explorer.
A path in another registered checkout brings that checkout forward; with no Workspace in front a URL goes to the default browser.
⌘-click (Ctrl off macOS) hands the link to the operating system: a URL to the default browser, a path to its default application or, for a folder, a Finder window, within the limits below.
A path outside every registered checkout, such as `/tmp`, goes to the operating system on a plain click as well.
The operating system opens only a plain document (text, source, a PDF, an image, audio or video) or a folder whose name has no extension; anything else, such as a script, a program, a spreadsheet, an application bundle or an installer, is revealed in Finder, never opened.
A program's OSC 8 link takes the same routes with no confirmation dialog: `file://` and `vscode://file/` addresses are paths, any other scheme is not opened, and a refused link goes to the diagnostic log.
A pane on an SSH device, and a page outside the desktop app, links URLs only, because its paths name files this Mac cannot check.
A path with a space in it is a link only where the program marks it (OSC 8), since plain text gives no way to tell where it ends.
Web owner: `web/src/terminalLinks.ts` (detection), `web/src/terminalLinkProvider.ts` (the check and the routes), `web/src/actions.ts` (`openLink`, `openTerminalPath`), `web/src/editor/lineRequest.ts` (the line); desktop owner: `desktop/src/main/localPath.ts`.

## Web Project Sessions

A Project's Sessions is the Project Overview's Sessions tab (PRD S8), under the Overview's own path back, title and facts line.
Web owner: `web/src/ProjectSessions.tsx`, `web/src/sessions.ts`.

It lists the history of every Workspace the Project has, works for a Project with no Workspace, and never runs an agent or sends a session to a Workspace.
Above the list are the `All / Codex / Claude Code` choice, metadata and Human/Assistant content search, and the count (`N sessions` or `N of M sessions` while a filter narrows it); a read in flight adds `Reading…` beside the count and keeps the rows.
Each row shows the provider mark and name, the time, the first request or title in at most two lines with the full text in its tooltip, and the checkout it ran in.
A session with neither request nor title reads `Untitled session`, muted, and a time it never carried is left out.
A row's accessible name reads provider, first request, checkout, time, and availability, in that order.
Content matches add a short matching snippet, Human or Assistant role, and the message time, grouped once per session.
The provider choice scopes backend results before the session limit, and Korean substrings including two characters, identifiers, punctuation and query syntax characters are literal searches.
A content result opens the exact source message once; index updates and query snapshots never pull a manually scrolled conversation back to its match.
Metadata rows stay useful while content search prepares or indexes, and an incomplete, failed or stale search has a pending or recovery state rather than a completed no-match assertion.
`Copied history` selects Off, 30, 90 or 365 days, initially 90 days after the policy loads.
Off erases this Project’s local copied bodies and pauses indexing while metadata remains available; Rebuild index clears the copy and starts a fresh bounded pass under the selected retention.
Expired copies from inactive Projects are also removed, and originals remain intact.
Searching and these controls never schedule Memory analysis, provider calls, embeddings or injection.
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

Web owner: `web/src/ProjectOverview.tsx` (the Project Overview screen), `web/src/MainScreen.tsx` (the Overview of every project), `web/src/OverviewLenses.tsx` (the tiles, and the chips, popover and fold line the Agents graph shares), `web/src/GraphView.tsx` and `web/src/agentGraph.ts` (the Agents graph both scopes draw and its layout, routing and filter rules), `web/src/overviewLens.ts` (the buckets, the tile values and the cleanup rule), `web/src/IssuesView.tsx` (the Issues view both scopes draw, with its panel beside the board), `web/src/TaskBoards.tsx` (the Board, List and Dependencies modes and the issue card, which the Overview of every project calls Tasks), `web/src/IssuePanel.tsx` (the issue panel), `web/src/issueDetails.ts` (the issue reads the panel and the preview share), `web/src/MarkdownText.tsx` (an issue's body in Markdown), `web/src/IssueDialogs.tsx` (New issue and Start), `web/src/issueStart.ts` (the Start dialog's first name and prompt), `web/src/PullRequestsView.tsx` (the PRs view), `web/src/PrDialogs.tsx` (이슈 잇기 and 맡기기), `web/src/prDelegate.ts` (맡기기's first prompt), `web/src/projectBoard.ts` (the board rules, the PRs tab's groups included); a card's agent row is the Agents list's `web/src/components/agent-row.tsx`.
The web boards follow PRD task-agents-views (`agents/prd/task-agents-views/prd.md`), reworked issue-first on 2026-09-28: work starts from an issue, and a card reads issue, then agents, then pull request.
The project's page is laid out by PRD overview-lenses-tiles-agents (`agents/prd/overview-lenses-tiles-agents/prd.md`): tiles where the tab row was, and an Agents view, one graph of checkout boxes and delegation lines (PRD agents-graph-view, `agents/prd/agents-graph-view/prd.md`), in place of the agent inbox.
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

Every way into a project's Overview, the project row, its Overview row, ⌘K and ⌘⇧H, opens the Agents graph with the box of the checkout in front selected (outlined and scrolled into view), or main's box when the checkout in front is elsewhere or folded away, and with no filter.
Only Global Recent Panels (a separate command, unbound by default) brings an Overview back as it was left, its tile, mode, selected box, filter and opened folds; the view lives on the screen, not in stored settings.
The PRs view opens only from its tile, a PR chip, `이슈 없는 PR N`, the sidebar's PR card and Recent Panels.
The facts line's right end carries the chosen view's controls: the status chips, search and device choice for Agents, the filter and `Board · List · Dependencies` for Issues, and nothing for PRs.

### Agents: graph

The Agents view is one graph, laid out by PRD agents-graph-view (`agents/prd/agents-graph-view/prd.md`), which replaced the checkout and lineage modes: a checkout is a box, an agent is a row in it, and a delegation into another checkout is a line between two boxes.
Its web owners are `web/src/GraphView.tsx` (the renderer, with the filter controls), `web/src/agentGraph.ts` (the pure layout, routing and filter rules), `web/src/graphMotion.ts` (the glide) and `web/src/graphGeometry.ts` (the `--graph-*` sizes read from the tokens); the chips, popover and fold line it shares with the other tabs are in `web/src/OverviewLenses.tsx`.
There is no mode control: the facts line's right end carries the filter instead.

A box stands for each checkout that holds an agent; the agents working there are its rows, one line each, and a box is drawn quieter while every row in it rests.
A delegation inside one checkout has no line: the child row stands one step right under its parent, joined by a corner arrow.
Agents sharing one tab stand together on one pale tray, and a row alone in its tab has none.
A delegation into another checkout is a line from the parent row's right port to the child row's left port, so a box's column is its deepest delegating box's column plus one, and a box nothing delegated into, a worktree started directly included, is in the first column.
A cycle is cut where it closes, and the closing line is a dashed curve that may cross boxes.
Each first-column box and everything delegated from it form one horizontal band, a child box standing level with the row that delegated it, and bands never overlap.
A band stands where its most urgent row does, the boxes delegated below it included, so a band with a question stands above a larger main band and shows on the first screen; on a project's Overview the primary checkout's band leads only when ranks tie, and its box never folds.
Bands, boxes within a column and rows within a box go by attention and not by any control: the operator's turn, then working, then waiting on children, then resting, the most recently active first within each.
A line runs only through the gaps between boxes, turns with rounded corners, has a port dot at each end, and the lines of one parent gather into a single trunk in the gap and branch from it; a line that crosses more than one column keeps to a corridor no box covers.
A line takes its colour from the child it leads to: warning when the child asks, blue with dashes flowing from parent to child while it works, pale blue while it waits on its own children, grey otherwise.

A worktree box's head reads the kind glyph in its pull request's colour and the branch in mono, the purpose (else the pull request's title, else nothing), and a third line of the issue chip, the PR chip (the state colour, then CI as ✓, ✗ or ●, and `변경 요청` in warning on an open pull request that asked for changes), `↑N ↓N` and the changed files in warning when dirty; the primary checkout's head reads the house, main, its purpose and `에이전트 N`.
A worktree whose Git state has not been read shows `?` where the files go, never a false zero; before GitHub answers there is no PR chip and no PR colour, and the head stands on Git facts alone.
The head is one button: its click opens that checkout's Workspace, main included, `↵ Workspace` appears over the end of the branch line without moving the branch, and resting half a second on it opens the checkout card (the path, the base and `↑N ↓N`, the changed files, the last commit's age and the pull request with its checks).
The issue chip opens the Issues view with that issue's panel, the PR chip opens the pull request's row on the PRs view, unfolded, resting on either opens its own card, and a ⌘-click on a head, a row or a chip opens the pull request on GitHub, else the issue.
A merged worktree is dimmed with the purple merge glyph and a folder-less one reads `× 폴더 없음`; both carry the one word `정리` at the head's right, always visible, whose tooltip says what it removes and whose click opens the existing Delete worktree dialog, so cancelling it removes nothing.

A row reads the status mark, the provider mark, the title and the age on one line; only an agent that asks has a second line, its question in warning with the title in bold.
Progress lines, the result after ✓ and a parent's `일하는 중 N · 물음 N · 끝남 N` are not on the row: the popover and the tucked badge carry them.
A row's click is one event that opens the agent's pane; resting the pointer on a row or focusing it shows `↵ 패널` (`↵ 답하기` while it asks) where the age was, and Enter does the same.
Resting on a row keeps its delegation chain, its ancestors and descendants as drawn, and its lines bright and fades every other row and line, and the border of its tab's tray darkens; focusing the row does the same.
Resting half a second on a row opens a popover with everything the agent last said (the snapshot's `message`, the whole hook sentences, not a line cut to the row), where it stands (the checkout and the tab's name), the other agents in its tab, the agent that delegated it or `직접 시작`, and `↵ 패널에서 답하기`, the row's own click.

Boxes whose agents all rest fold away.
A box folds into `쉬는 체크아웃 N` when none of its agents asks, works, waits on children or has a finished root the operator has not looked at yet, so a box with an unread Done stays open until it is looked at; a worktree with no agent folds into `에이전트 없는 워크트리 N`, and a merged or folder-less worktree whose agents only rest, or that has none, into `정리할 것 N`, which is taken before the resting fold.
On a project's Overview the primary checkout's box never folds.
Each project has its own fold lines, identified as `empty:`, `cleanup:` or `resting:` and the project's id; a click unfolds the line in place, a line at zero is not drawn, and the folds stay open until the screen is left.
A selected box that is folded away leaves main's box carrying the selection, so a fold line always opens and closes by its own click.
The lines a folded box would have drawn are gone, and the row that delegated into it carries a tucked badge, the mark and count of the folded agents (`✓2`), counted on the nearest row still drawn; its tooltip says the words.
The Issues view's `이슈 없는 워크트리 N` opens this graph with `에이전트 없는 워크트리` unfolded.

The filter is at the facts line's right end: the status chips `내 차례 · 일하는 중 · 쉬는 중`, a search field, and, only when two or more devices run agents in scope, a device choice with this Mac first.
Chips are multiple choice and any lit one keeps its rows; `일하는 중` keeps working agents and agents waiting on their children together, pressing a lit chip turns it off, and with none lit the state does not filter.
The search keeps the agents whose title, branch, issue number or pull request number contains it, case ignored, and `#272` and `272` find the same number.
The chips are alternatives of one kind, and the three kinds (status, search, device) all have to match, so a row stays only when it passes each kind that is set.
A row or box that does not match is hidden, except that the chain of parents leading to a matching row stays, faded, so no line breaks; a filter draws every box it keeps and folds nothing, and turning one filter off redraws from the rest at once.
When nothing matches, the graph's place holds one line, `필터에 맞는 에이전트가 없습니다`, with `필터 해제`, which turns off the chips, the search and the device together.
Pressing a segment of the Agents tile's bar lights only the chip that segment belongs to (working and waiting on children are both `일하는 중`) and opens this graph.
Escape in the search field clears the search alone, leaving chips and device; in an empty field it leaves the Overview as before.
On a project's Overview the filter, the selected box and the opened folds live on the screen, so Recent Panels brings them back and every other way in starts with none.

The line `실행 중인 에이전트가 없습니다` stands only when there is no box to draw and no fold line, and no filter is set; a project with no agents but with worktrees shows its fold lines alone, and the primary checkout's box stands as its head alone.
When the device cannot answer, a project's Overview shows the reason above the graph and nothing where it was, and never says there are no agents; the last drawing is not left standing.

On the Overview of every project the graph is drawn once per project, under a header line with the project's name and its device when it is not this Mac, the projects ordered by attention (the one with an agent whose turn it is first, then the most recently active), and the filter applies to all of them.
A project whose boxes all rest is its header line and its fold lines.
No box is selected there, and a device that cannot answer is a notice above the graph for that device alone.

The graph moves only when what it draws changes.
Boxes, rows, trays and lines glide to their new places over 320 ms (`--graph-motion-ms`), a new box fades in and a new delegation's line draws itself from the parent to the child; a snapshot that leaves every position as it was starts no glide, no timer and no animation frame.
The only motion that never stops is the dashes flowing along a working line, which a timer steps three times a second (the `--graph-flow-*` tokens) instead of a CSS animation, and no timer runs while no line is working.
With `prefers-reduced-motion` every change is one jump, the entrances do not play, and a working line stays a still blue line; the app has no switch of its own.

Heads, rows, chips, `정리` and fold lines take focus; the arrow keys move between them by where they are drawn, Enter is the click, and Escape leaves the Overview as before.
On a narrow window only the graph scrolls, sideways, inside its own area, and the page does not; a long branch or a Korean title is cut to one line with its whole text in the tooltip or the popover.
Every icon button and chip has an accessible name, the same words as its tooltip.
Hover, focus, the popover and every filter change are local: they publish no snapshot, dispatch no core event, and start no Git or disk work.
The graph's elements carry `data-graph-box`, `data-graph-row`, `data-graph-edge` and `data-graph-fold`, each canvas `data-graph-canvas` with a `data-graph-revision` that counts the pictures applied and a `data-graph-flowing` that is present only while its dashes are stepping, and the filter `data-graph-chip`, `data-graph-search` and `data-graph-device`, which is what the browser tests read.

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
A worktree with no issue is one line at the foot of 진행 중, `이슈 없는 워크트리 N`, whose popover says it goes to Agents and names them, and whose click opens the Agents graph with its `에이전트 없는 워크트리` line unfolded.
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
The board is the Project Overview: the sidebar's project name or its Overview child (a plain folder's one row opens its checkout instead), the Overview's project row, ⌘K and the Workspace toolbar menu open it, and ⌘⇧H opens it for the checkout in front.
Escape, once no dialog or menu is open and no text field holds text, returns to the Workspace in front, or to the Home Overview when there is none.
The title row carries the path back (`Home / Project`, where Home is the project's device's), New agent and 새 이슈; directly under it is one line of facts, then the tiles, which show even while the project has no agent.
The facts line holds only facts about storage: for a Local Git project the worktree count, the disk every worktree and the shared Git directory occupy, main's distance behind origin only above zero, a warning cell only while the volume is short of room, and `N merged → 정리` only above zero; the open issues and pull requests are counted on the tiles, not here.
The disk number's tooltip lists build cache, dependencies, worktree source, the folders Hide does not know and the shared Git data, and pressing the number opens the disk cleanup sheet.
`N merged → 정리` opens the same sheet filtered to finished checkouts; the `정리할 것` fold on Agents and the box's own `정리` (the Delete worktree dialog) are unchanged.
The warning cell `여유 X GB · Y GB 비울 수 있음` stands only once the measurement is back and the volume has less than 10 GB free; Y is the build cache and dependencies of finished checkouts no agent is working in, and pressing the cell opens the sheet filtered to finished checkouts.
Opening a local Git project's Overview asks the core to measure its disk and re-read that project's worktree Git facts, pull requests and issues in the background.
The previous Git facts and pull requests stay visible while the read runs, and a small icon beside the facts line spins until both reads finish without moving the line.
The size reads `… GB` while measurement runs and is left out, with the reason only in the diagnostic log, when a part cannot be read.
Local checkout commits, branch switches, pulls, merges, fetches, rebases and staging changes refresh that repository's worktree facts after the Git directory becomes quiet; Hide does not fetch automatically, so `behind origin` follows the local remote ref.
An edit confined to a working-tree file is reflected when the Overview opens again.
Each issue card and List row shows its creation age in the same compact relative-time form used for activity, or no age when its source did not provide creation time; the backlog remains sorted by latest update.
The Home Overview keeps its tab row, `Tasks · Agents · Projects`: the Tasks board mixes the device's projects' issues, Agents is the same graph over those projects with the project's name on a header line above each project's boxes, and Projects is the device's registered projects; the device's Home folder is none of them.
Its Agents tab carries the count of agents it is the operator's turn with; there is no band under the header.
Its title row carries Add project in the desktop app and 새 이슈 (for the project in front, else the first one with a source), and its facts line the project count, the open issues once every source has answered, and, only when every project can give its part, the open pull-request and merged totals.
New agent opens the New worktree dialog on a Git project and the folder's Workspace otherwise.
Before the first snapshot the shell's own connecting state shows instead.
While hided or a device is unreachable the board keeps the last snapshot and the existing connection or device line is the only signal.
A card's agent row follows the Agents list's row rules above (`web/src/agentRow.ts`): the same first line, second line and branch chip, and the core's waiting-on-children ring.
A card row carries no chevron and no descendant badge.

## Explorer file management

Web owner: `web/src/ExplorerTree.tsx`, `web/src/explorer.ts`. Core owner: `herdr-core/src/changes.rs`.

The tree's context menu on a file row is Open to the side (and Open in Browser for an HTML file), a separator, the reveal, a separator, Rename, a separator, Move to Trash.
A folder row has New File and New Folder in place of the opens; the empty area below the rows stands for the root and offers only the two creations.
The item set the menu offers is a presentation decision a test can check directly (`explorerMenuItems` in `web/src/explorer.ts`), not something the platform decides implicitly.

The reveal is one action, `reveal_external`, on Explorer rows, History rows, View tabs and sidebar project and checkout rows (`web/src/revealExternal.ts`).
A History row's menu is Open to the side, a separator, then the reveal of the row's file.
It hands the file or folder to the desktop host, which shows it selected in its parent folder in the OS file manager (`shell.showItemInFolder`) and opens nothing.
Its label is the host's OS's name for that: `Reveal in Finder` on macOS, `Reveal in File Explorer` on Windows, `Open Containing Folder` on Linux, and `Show in File Manager` elsewhere; the host reports its OS through the preload bridge, and the shell never guesses it.
A plain browser tab has no file manager to hand anything to, so no menu lists the item there.
On another device's files the item is listed disabled with `Only for files and folders on this computer.`; a History row for a deleted file lists it disabled with `The file was deleted.`
The host refuses a path that is not absolute or no longer exists, and records the refusal or the reveal in its log without the path; nothing is shown on screen.

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
A folder being re-read, after a settled change, a change the watcher saw, or the refresh button, keeps its rows drawn until the new listing lands; it never empties for the round trip.
A device folder whose re-read is refused keeps those rows under its could-not-be-listed reason until Retry, or until the helper is ready again; a local subfolder whose re-read is refused keeps them until the next change reads it again.
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
Diff tabs retain their own viewer, with the same Wrap toggle.

The core names each open file's kind (text, markdown, image, pdf, binary), and the overlay picks the adapter from it; the toolbar is the same bar in every kind, with controls a kind cannot use taken away rather than left dead.
A PDF (recognised by its signature whatever its name) shows in a continuous, width-fitted, text-selectable, non-editable view.
Its toolbar keeps the breadcrumb and the two reveals, shows Find disabled with the reason that Find is unavailable for PDF, and hides Wrap, the Markdown mode group, and Unsaved, which a PDF can never earn.
A PDF that cannot be decoded, cannot be read, or is password-protected shows a `PDF unavailable` state with the reason under the same toolbar.
An image hides Wrap as well; a file that is not UTF-8 shows a `Preview only` state explaining the file type cannot be shown as text, and keeps Wrap disabled beside a disabled Find.

Markdown files alone show the centered Live/Source choice, and Live is the default.
Both are editors over the same draft: Live draws the formatting in place and hides the markup on every line the caret is not on (the way Obsidian's Live Preview does); Source is the monospaced editor with its line-number ruler and Wrap toggle.
The core owns mode and wrapping per open file tab, and wrapping per open diff tab; another tab has independent choices, returning to a tab restores them, and a new tab, close/reopen, or app restart starts Live with wrapping on.
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

Core owner: `herdr-core/src/sidebar.rs`, `herdr-core/src/project_context.rs`, `herdr-core/src/worktrees.rs`, `herdr-core/src/disk.rs`, `herdr-core/src/disk_layers.rs`, `herdr-core/src/worktree_cleanup.rs`, `herdr-core/src/runtime/projects.rs`. Web owner: `web/src/sidebar.tsx`, `web/src/projects.ts`.

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
A project row's menu is `Open Overview`, `New worktree…`, `New tab in main` (a new tab in the checkout the home glyph marks, brought to the front), then the reveal and `Copy path`, then `Pin` or `Unpin` and `Remove project…`, on every project row, registered or not.
A checkout row's menu is `Open` (the row's open, without unfolding its agents), `New tab here`, `Open pull request #n` while GitHub knows one, then `Set purpose…`, `Set as default checkout`, `Copy branch name`, `Copy path` and the reveal, then `Delete worktree…` in the destructive color on a linked worktree.
An agent row's menu, in Agents and under an opened checkout, is `Show` (the row's own open, with the ⌥n that selects the same row where the host has one), then `Copy title` and `Copy session id` (the conversation id Herdr recorded, disabled when it recorded none), then `Close tab…`, which closes the tab holding the agent's pane, wherever it is, through the tab close flow, including its question about the agents spawned from it; Herdr 0.9.1 can neither mark a pane seen nor stop an agent, so neither is offered.
`New tab in main` and `New tab here` show the registry's new-tab chord; the reveal follows [the reveal rule](#explorer-file-management): the desktop app's only, labelled by its OS, showing the folder selected in the OS file manager without opening anything.
On a device's rows the reveal and `Set as default checkout` are disabled with the reason; the rest act on that device as they do here.
`Delete worktree…` is never disabled on a linked worktree, on any device and before its Git state has been read; its confirmation says what would be lost and holds the choices, and reads `Reading the worktree's Git state…` until the row arrives.
Under the title and the path, one facts line says the folder is removed for good, how many panes close, and every agent those panes stop with its state; the core's warnings (uncommitted files with their count, a worktree inside it, the base branch, Git status unavailable, commits not merged, not pushed) follow as neutral badges.
`Keep worktree` is a neutral button, and only the widest action is in the destructive colour.
`Also delete branch <name>` deletes the branch with `git branch -d` when Git counts it merged, and otherwise with `git branch -D`, saying how many commits not on the base go with it or that Git could not tell; it is not offered for the base branch or a missing folder.
A folder that holds uncommitted files, a worktree inside it, or a status Git could not read shows a `Discard …` checkbox, and Delete stays disabled until it is ticked, because that loss cannot be undone; ticked, the folder is removed with `git worktree remove --force`.
When an agent in the worktree spawned agents that run outside it, the confirmation adds a short `N agents outside this worktree` heading over the same list, count line and states the close sheet draws, and its one Delete becomes two buttons named by their result on one row at the standard width: `Delete only`, the deletion as it is, and `Close N agents and delete`, the destructive one.
Both follow the dialog's own conditions, the Discard checkbox included, and neither holds the keyboard when the dialog opens; while one outside agent's status is unknown, the second is disabled and a `Check status` reads it again in place.
With the second, the core closes those agents deepest first as a subtree close does, and only once all are gone closes the worktree's panes and removes the folder; a close that is refused or times out starts no deletion and its reason shows in the dialog, and trying again continues from the agents that remain.
With the first, the agents outside keep running and the direct children become the operator's roots.
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
Every device's sidebar has the same two lines at its top (quick device-rail-badges, replacing PRD home-device-rail D-13 and D-14): the top line names the device in front, `This Mac` or a device's name with a smaller `Remote`, and ends in Add project (not on the Agents tab, and only in the desktop app) and Search, icons whose hints read `Add project` and `Search` with their chords; under it the `Projects | Agents` tab strip, Projects first and shown at launch, the choice kept for the session.
A device that is not connected shows the top line and its reconnect view with no tab strip.
Projects lists the device's Needs You, its Home row and its projects, with Done left to the Agents tab (the one-device sidebar used to raise Done above Home too); Agents lists the device's own agents under Needs You, Done, Working and Seen (docs/status-model.md), with `Needs You N · Done N · Working N` above the list, a zero count left out, and no row naming its device.
Choosing a tile keeps each device's lists apart: another device's agents never appear in this device's Agents, and ⌥n numbers the front device's Agents list.
Search opens the ⌘K palette and Add project the Add a project dialog (see Adding a project), on this machine or a selected SSH device alike; there is no Search field row and no bottom new-workspace button.
The Herdr status line sits under the top and above the list, and is not shown while a device is in front.
The web Projects list is the scope picker, starting at its first project.
One row carries the selected fill at a time, the row of the scope the center shows: the Home row while its Home Overview is in front, a Git project’s Overview child on its Overview, or the focused checkout and its open agent row only while a Workspace is in front.
An expanded Git project starts with an Overview row using the checkout row’s columns, single-line height, font and focus ring, with a layout-dashboard glyph and no badge, time or chevron.
Click, Enter or Space opens the same Overview as the project name without changing the fold; only the Overview child carries its selection fill, and folding the project hides the child too.
A plain folder, a project that is not a Git repository and holds one checkout, is one web row instead of a project row over an identical checkout row.
Its first line is the project's folder glyph, name and status badge, set in the checkout row's columns, and the badge stays while its agent rows are open, as a project's does; its second line and trailing chevron are the checkout's, and a plain folder has no commit age.
It has no project fold of its own and keeps the checkout row's right slots; the row opens the checkout and is marked while that checkout's Workspace or the project's Overview is in front, its menu lists the project's items and then the checkout's own `Open`, `Open pull request #n` and `Set purpose…` (the folder is the checkout, so its new tab, path and reveal items are the project's), and its Overview is reached from the Overview, ⌘K or the Workspace toolbar.
While a checkout's agent rows are closed, its status badge ends line one; opening them takes the badge away, since their own marks now speak, and changes nothing else on the row.
A checkout's second line is its purpose, after the parent checkout it was raised from when there is one, with the last-commit age ending it on the time column; it is drawn only while the checkout has a purpose or a raising parent, so a checkout with agents and neither is one line.
A checkout with neither, or one whose Git facts have not been read yet, is one line, with its age on that line.
Workspace disclosure persists across launches and hides only the nested agent rows, preserving selection, running panes, and raised attention rows.
The web shell draws the raised group at the top of the Projects list as `Needs You · N`, left out while empty: the Needs You agents whose pane a listed project's checkout or the Home owns, in the core's order, on the Agents list's own row with its place line, drawn whatever their project, checkout or parent has folded, never unfolded themselves, and opened as the Agents row opens.
A device's Projects list is its `Needs You · N` group, then the Home row, then Pinned and the projects, with no Done group (Done is in the Agents tab), and no row there names the device, since the whole list is that device's (PRD home-device-rail B6, with the rail rework of quick device-rail-badges).
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

Web owner: `web/src/DiskCleanupSheet.tsx`, `web/src/diskCleanup.ts`; core owner: `herdr-core/src/disk.rs`, `herdr-core/src/disk_layers.rs`, `herdr-core/src/worktree_cleanup.rs`.
Allocated-on-disk sums main, linked worktree folders, and the shared Git directory once; nested roots belong to the longest matching root, hard links share one inode allocation, and descendant symlinks are not followed.
Each checkout is measured under its own limit of one million entries and 30 seconds, so a checkout that exceeds it or cannot be read is the only row that has no size; the reason is in the diagnostic log.
An incomplete measurement has no total, and allocated blocks are not a promise of reclaimable space, so the result states only the volume's free space before and after.
Remote device projects are neither measured nor cleaned.

Only what the ignore rules hide can fall into a layer, so every path git sees is worktree source and is never counted in a layer cell.
An ignored folder holding a valid `CACHEDIR.TAG` is build cache, one Hide's ecosystem table names beside its marker file is build cache or dependencies, and every other ignored path is Other.
Measurement reads no git index, so a tracked file inside an ignored folder is found only when the cleanup runs, and that cell is then skipped.
The table covers Cargo, Node, Python, Gradle and Maven, Swift, Dart, Elixir, .NET and Composer, and applies only to ignored folders; a symlink, a folder holding another git repository and a folder holding tracked files are never removed.
Other has no checkbox and its folders are never removed, because Hide cannot tell whether what a tool made there can be rebuilt.

The sheet is one large dialog with no subtitle: a stacked usage bar whose segments say their layer and size on hover, the filter, and a table with a row per checkout and columns Build cache, Dependencies, Worktree, Other and a total.
A column head is its name and the column's total for the rows shown, with no second line.
Main leads, the rest follow by size, and checkouts under 1 GB fold into `작은 체크아웃 N · X GB`; a row that is still being measured is a skeleton with a disabled checkbox, and a row that could not be measured is dimmed with no size and cannot be chosen.
The filter is All, Finished, Resting and Working, each with its count: Finished is a linked checkout the Overview already calls done that nothing is using, Working is a checkout something is using, and Resting is the rest.
Checkboxes sit on each cell, on each row (its build cache and dependencies, never its worktree), on each column head and at the top left, and they reach only the rows the filter shows; a group that is partly chosen shows the middle bar, and a cell that cannot be chosen is skipped by every group.
Choosing a worktree cell shows the same row's cache cells as included: they cannot be chosen apart and are not counted twice, and they leave with the worktree folder.
A checkout is in use while an agent there is working, a terminal pane in it runs a process that is not a shell, or a server listens on a port opened from inside it; a pane that is merely open does not block a cache.
A worktree cell can be chosen only for a linked checkout that is merged into local main, clean, has no open pane, is not the checkout in front, is not locked and holds no nested git repository; otherwise it is disabled and its tooltip says which.
When Hide cannot read what is in use, the sheet says so above the table with a retry and every checkbox is disabled; no banner or alert appears.

The bottom line reads `N칸 · X` (`N칸 · 워크트리 M · X` when a worktree is chosen, and a part that is zero is left out, so a worktree alone reads `워크트리 M · X`) and `정리` runs at once for caches and dependencies alone.
With a worktree chosen a confirmation step titled `<branch> 폴더째 삭제` (`워크트리 N개 폴더째 삭제` for more than one) says only that the branch stays, lists the worktrees with their sizes and offers `워크트리 N개와 캐시 정리` and `돌아가기`, neither focused; going back deletes nothing and keeps the choice.
Cleanup deletes cache and dependency folders permanently, with no trash, and removes worktrees without force, keeping the branch.
Each cell is checked again when `정리` is pressed and again per folder: a cell that became in use, a worktree that changed and a folder that gained tracked files are skipped with their reason and the rest go on.
A folder is moved into the repository's Git directory (`hide-removed`) and disappears from the checkout at once; the cleanup thread then deletes it, and the sheet reads `비우는 중 · N/M`, keeps going when the sheet is closed and shows its progress or result when opened again.
Only one cleanup runs in a daemon, and pressing the same confirmation twice removes each folder once.
When the cleanup ends the result is the one line `여유 A → B GB` over each cell as removed, skipped or failed with its reason; `다시 검토` measures again and `닫기` closes the sheet.
The disk number, its tooltip and the warning cell then read the new measurement.
Every removed, skipped and failed cell leaves one diagnostic line with its kind, checkout, layer, bytes and reason code and no file contents.
Hover, filter and checkbox changes are local: they dispatch no core event and start no disk work, opening the sheet sends one review event and `정리` one confirm event.

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
A project with panes is not refused; under the title and the path one facts line names the panes that close, the running agents that stop (left out at zero), that only the registration goes (a row Herdr shows without one goes with its panes), and that files stay on disk; `Keep project` is a neutral button.
On confirmation the core closes every pane in the project's checkouts and waits for confirmation before removing the registration and its row; a timeout or refusal leaves the project registered with the reason in the error banner, and a repeated request continues from the panes that remain.
When an agent in the project spawned agents that run outside it, the confirmation lists them as Delete worktree does, with `Remove only` and `Close N agents and remove` (the destructive one) in place of its one button, neither holding the keyboard; the second closes those agents deepest first and removes the project only once all are gone, a refusal or timeout leaving the project registered with the reason in the error banner and a retry continuing from the agents that remain.
Removing the project that holds the focused checkout moves focus and pane selection to the next project.
An add that lands mid-removal cancels the removal and says so, rather than losing the project it just opened a pane in; a completed removal disappears from the snapshot and a repeated request is a quiet no-op.
A row Herdr shows without a registration offers `Remove project…` too: its confirmation counts its panes the same way and says the row goes with them, the confirmation closes those panes, and the row leaves once Herdr drops its workspace; a timeout leaves the row with the reason in the error banner, and repeating the removal closes the panes that remain.

## Home

Core owner: `herdr-core/src/runtime/home.rs`, `hide-host/src/home.rs` (see ARCHITECTURE.md, Home). Web owner: the Home row in `web/src/sidebar.tsx`, `web/src/devices.ts`.

Each device has one Home, `~/hide` in that account's home directory, for work that belongs to no project or to several.
Nothing is made on a device until its Home is first used: the first agent or tab started there makes `~/hide` with one link per project registered on that device and Hide's `AGENTS.md` with a `CLAUDE.md` link to it, and a device whose Home was never used has no `~/hide`.
Once a device has a Home, registering a project there adds its link and removing the registration removes only that link, never the project's folder; two projects with one folder name get distinct link names.
A link whose project folder moved away is dropped at the next sync with nothing on screen, only a diagnostic line.
A `~/hide` that Hide did not make is left untouched, and the start that wanted it says so where it was asked for, in the start panel or under the Home row for its `+`, with what to do.
A Home agent reads and edits the projects through their links, the change shows on that project's checkout row, and its row stays under the Home row.

## Recent navigation

Web owner: `web/src/recent.ts`, `web/src/areaCycle.ts`, `web/src/viewFocus.ts`, `web/src/keyboard.ts`, and `CycleOverlay` in `web/src/Overlays.tsx`.

Recent Agent pane or View tab cycling (⌃Tab / ⌃⇧Tab in the desktop app, ⌥` / ⌥⇧` in a browser) follows where the keyboard is.
In a View area it walks the tabs of that exact area, in recent-use order.
The View scope includes device, checkout and area ID, so an Agent area with the same ID or another checkout cannot widen it.
A document, diff, View tab bar or visible native browser page names its View area.
Everywhere outside a View area it walks the agent panes the keyboard has been in this session, one row per pane, across every device, project and checkout, in recent-use order (issue #301).
A terminal pane in a tab an Agent area of the Workspace in front draws (its normal tab or a delegated child's canvas), or that area's tab bar, puts the keyboard in the Agent area; from a tab bar the pane in use is the one the core focuses in that tab.
A pane joins the order the first time the keyboard is in it, and a commit's passing frames on the way to its pane are not visits; a pane that closes leaves the order.
A pane row is titled by the agent it runs, with the place as Recent Panels names it and `Terminal` beneath, and the agent's status mark; a pane running no agent, a View display, an Overview and a project are never rows.
From a pane running no agent, or from the sidebar, tools, search, Settings, a dialog, Main or Overview, the first chord lands on the most recent agent pane.
Outside the Agent area, opening the cycle neither invents a pane origin nor records the underlying pane as visited.
Main and Overview still open the Agent cycle when a shortcut left the previous Workspace's View owner recorded, because that View is no longer drawn.
A focused View area keeps its own scope even when it has zero or one tab or its target retired; it never falls through to Agent panes.
With no visited agent pane, or no other visited agent besides the pane in use, the Agent cycle is a no-op.
Holding the chord's actual modifier freezes the order and scope and previews in Recent Agent panes or Recent View tabs without moving the committed tab, layout or keyboard owner.
Repeated forward and backward chords walk that frozen order; Escape or losing the window cancels without a selection event.
Releasing the modifier on a View tab commits one in-place selection; on a pane it brings the pane's device, project, tab and pane forward with one event, as choosing it in the Agents list does, and the pane receives the keyboard.
Closed or moved View tabs leave the frozen candidates; removal of the origin area or checkout cancels, and no other area replaces it.
Closed panes and panes whose agent ended leave the frozen Agent candidates; a highlight that left moves to the next surviving pane, and with none left the cycle ends.

Global Recent Panels is a separate, named command, unbound by default and assignable in Settings alongside the Agent pane and View tab commands.
It walks every unified surface in recent-use order across projects, checkouts and connected devices: terminal tabs, View files, diffs and browser displays, plus every visited Home and Project Overview.
An Overview revisit moves its one row to the front; its saved tile, modes, selected lane and folds remain page-local, and it leaves the order with its Project or device.
A Global Recent Panels commit shows an Overview at once or brings one Workspace surface forward with one event; a View target also receives the keyboard.
Menu and palette invocations of either family select the next or previous valid target immediately, with no held modifier or second selection.
A bound Global Recent Panels chord keeps the same hold, release and cancellation behavior.
Recent Projects (⌥Tab / ⌥⇧Tab) retains its global project order and restores each Project's last Workspace surface, never its Overview.
Both global lists and the Agent pane cycle carry a device chip on a row outside the device in front and move the rail, sidebar and center together when committed.
The session uses one bounded recent-surface history; the View area cycle filters it and appends normal area tabs not yet visited.
The Agent pane cycle reads a second session-local order of pane visits, bounded by the panes that exist.
The shown tab of the recorded keyboard area is the current visit, including native browser pages; intermediate commit frames are not visits.

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

This section supersedes PRD home-device-rail rows D-10, D-11, D-27, B3, B4 and B12 (quick device-rail-badges): the Inbox, the rail that came and went with the first remote device, the Needs You-only badge and the footer device button no longer exist.
Quick device-rail-slack then narrowed the rail to `--size-rail` (48), ran it the sidebar's full height, took the names off the tiles and left Working to the Agents tab.

The rail is always shown, even when This Mac is the only device: the sidebar's full-height left column, with the top line and the `Projects | Agents` strip to its right, holding This Mac, each registered device in the core's order, and a `+` (`기기 추가`, a dashed tile) directly under the last device tile.
The stored sidebar width stays the content column's; the rail adds `--size-rail` to its left while it shows, and the drag edge sits on the content column.
Each tile is a 32 rounded square with no name under it: This Mac draws the laptop glyph and a device the monogram of its name, the first letter of each of its first two words, split at spaces, dots, dashes and underscores (`Mac mini` → `Mm`, `build-box` → `Bb`, `mini` → `M`); the tile's hint, shown to its right, is the name followed by `연결 안 됨` or each count it marks in full (`mini · Needs You 12 · Done 1`), since the pill stops at `9+`.
The selected tile is ringed (a 2px ring 2px off the tile), and one tile is selected at a time.
A tile carries at most one mark, notched into its top-right corner by a ring of the rail's fill, and it is the most urgent state: the Needs You count in a `--warning` pill, `9+` from 10, or, with no Needs You, a `--success` dot with no number while the device has unseen Done.
Working has no mark on the rail, since it is no reason to switch device; the Agents tab counts it.
A mark's digits are `--status-foreground`, white in Light and near-black in Dark.
The counts come from the device's own agents in the snapshot (`deviceAgents()` and `groupCounts()`), with no extra core or wire data.
A device that is not connected dims its glyph and wears a `×` at the bottom-right, with no mark, since its last counts are not current; selected, its sidebar shows only its name, `연결 안 됨` and `다시 연결`, which retries the connection in place, and never the tree it last reported.
Selecting a device tile sends `focus_device`, and the sidebar becomes that device's Projects | Agents; no row there names the device.
Every tile is a button reached with Tab and chosen with Enter or Space, named for assistive technology by the device, its connection and each count it marks (`mini, 연결 안 됨`, `This Mac, Needs You 2, Done 1`).
`기기 추가` opens Settings › Devices at its Add device form, as the hidden rail's `기기 추가…` and the Add a project dialog's host list do.
Removing the device in front moves the front to This Mac; that device's agents and its `~/hide` stay on it, and the rail stays.

A right-click on the rail offers `레일 숨기기`.
While the rail is hidden the sidebar's top-line device name becomes a `This Mac ⌄` menu that lists the devices (a check on the one in front), `기기 추가…` and `레일 표시`.
The View menu of the desktop app carries `Toggle device rail` (`toggle_device_rail`, no default chord, bindable in Settings › Shortcuts like the sidebar switch); a browser tab uses the `This Mac ⌄` menu.
The hidden state is the core's `ui_state.device_rail_visible`, kept beside `left_sidebar_visible`, so it survives a restart.

A device's Workspace in front wears the device color, `--device-remote`: a band at the start of the Workspace toolbar with the server glyph and the device's name, truncated, and a border of the same color around its panes; this machine's Workspace has neither.

## Start panel

Web owner: `web/src/StartPanel.tsx`, `web/src/startTargets.ts`, `web/src/startDraft.ts`, `web/src/startAnswer.ts`, `web/src/agentPicker.ts`, `web/src/components/agent-picker.tsx`.

The start panel starts a Claude or Codex agent with a first instruction anywhere hide can reach (PRD home-device-rail D-17..D-22).
⌘N opens it in the desktop app, where it is also File › Start agent; in a browser tab ⌘N stays the browser's and `에이전트 시작…` in ⌘K, found by typing, opens it, on every screen.
It floats at the ⌘P palette's place and width with no backdrop, and the keyboard lands in its one-line text box, `무엇을 시킬까요?`.
Opened while Settings is up, it takes Settings' place: Settings closes and the target is the front device's Home.
Under the text are the target, the agent kind and the model menus, a `⏎` keycap and `시작`; Enter or `시작` sends one `agent_start_in_checkout` with a fresh request id, the text as the agent's first instruction, handed to the CLI as its own argument (ARCHITECTURE.md, the first prompt).
The target defaults to what is in front: the checkout of the Workspace in front (a worktree when that is it), a project's main checkout while its Overview is in front, and the front device's Home while a Home Overview or Settings is; a device's surface in front makes that device the target's.
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

The sidebar footer reads one chip per provider and the Settings gear at its right; a chip is the provider mark and the rounded percent of the seven-day window.
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

Web owner: `web/src/SearchPalette.tsx` (⌘K), `web/src/search.ts`, `web/src/relations.ts`, `web/src/searchDetail.ts`, `web/src/searchGithub.ts`, `web/src/Palette.tsx` (⌘P and the diff palette), `web/src/components/sidebar-header.tsx`; core owner for the GitHub search: `herdr-core/src/runtime/issues.rs`.

⌘K goes to things; it runs no command except `에이전트 시작…`.
On the web, the Search icon at the end of the sidebar's tab strip or its top line, hinted `Search ⌘K`, opens the same palette Command+K opens, and the query row carries an `Esc` keycap.
Its own wide layout is a list on the left and the highlighted row's detail on the right, one and a half times the width ⌘P, the shortcut sheet and the start panel share; a dialog under 800 px, too narrow for both, draws the list alone.

### What a query finds

A query finds an agent, project, checkout, device, issue or pull request by name, and an issue or pull request by `#number` (`#273`, or `273`); an exact number match lists first, as its own row, issue and pull request separately.
Only digits in the query match a number by substring on other rows; the pane id no longer matches.
The rows are `Issues`, `Pull requests`, `Agents`, `Projects`, `Checkouts` and `Devices` (while another device is registered), then `Commands` holding `에이전트 시작…` and `GitHub`; a group stands where its best result ranked and keeps its results in rank order, so grouping never moves the best match off the first row.
Search covers every connected device; a result not on the device in front carries that device's chip after its title, and choosing it brings that device forward with it.
An agent row is the agent's own mark, its title, and its place and state under it; an issue or pull request row carries its number, state and, for a pull request, its CI.

### What an empty query shows

Nothing typed lists what is connected to the thing in front, drawn as a Project's Overview draws it: the issues the thing works on or closes on top, then one group per checkout with a head, the checkout's pull request, and the agent lineage under it.
The agent the keyboard was in is tagged `여기` and choosing it does nothing; a parent that works in another checkout is one `↑ 부모` line under the agent, and a child delegated to another checkout stands in that checkout's own group.
In front of an agent pane the thing is that agent; in the Workspace elsewhere it is the checkout in front; a connection the snapshot does not name has no row.

Under Related, `Recent` lists the checkouts last brought to the front (PRD cmdk-recent), newest first, at most five, as the same checkout row Related uses: the branch over the project, and the device's chip after the title while the checkout is not on the device in front.
It leaves out the checkout in front and every checkout Related already lists, and fills the five from the rest of the record.
The core keeps the record (`ui_state.recent_checkouts`, ten checkouts, newest first, saved with the rest of the UI state and so still there after a restart): a checkout is recorded whenever it comes to the front by any path (the sidebar, ⌥1-9, ⌘K, Herdr's own focus, a device's focus), moves to the top if it is already there, and the oldest leaves past ten.
A device's Home is never recorded.
Choosing a Recent row opens that checkout, bringing its device forward when it is on another one.
A checkout of a connected device is that device's live row; one a connected catalog no longer lists is not drawn, and the core drops the record of a project unregistered, a worktree removed through Hide or a device removed (a worktree deleted outside Hide only stops being drawn, and its record ages out of the ten).
A checkout of a device that is not connected stays as a dimmed row drawn from the names the record kept (the branch, the project and the device's name): arrows pass over it, its detail names the checkout and offers nothing, and Enter does nothing and says nothing.
Its row is the live one again when the device reconnects.
A typed query hides Recent and searches as before, and recency does not rank results.
Recent does not hold pull requests, issues or agents; ⌃Tab stays the agent pane cycle and Recent is the way back to a checkout. Unlike Recent Panels and Recent Projects, which the page holds for the session, it is the core's saved record and is still there after a restart.
On a screen with nothing in front, such as Settings, the palette shows Recent alone; with no record and nothing in front it is the input alone with the placeholder `이름이나 #번호를 입력하세요`.

### The detail

The highlighted row says what it is with the facts the snapshot carries and no others (a missing value has no line): a pull request's review, branch and the issues it closes, an issue's owner checkout and closing pull request, an agent's checkout and last words, and `N분 전 읽음` for the GitHub-backed ones.
Under `관계` it draws the same groups the empty list draws for the row, when it has more than itself; the footer line says what ↵ does.

### GitHub

Typing never calls GitHub.
Opening ⌘K asks once per app run for the local project in front when nothing has read it, and a checkout row shows a spinner while that read has no answer and a warning mark with the last value's age when it failed; the reason is in the log.
A query with a GitHub project on this Mac ends with `GitHub에서 "…" 검색`; choosing it sends one `github_search`, and the row shows a spinner, then the pull requests and issues GitHub holds under `GitHub` (an exact match to one already held is not listed twice), `GitHub에도 없음` (or `찾은 결과 없음 · 일부 저장소만 검색` when the core covered only some of the projects), or `GitHub 검색 실패 · 다시 시도`.
The query's words match like a search box, and a query is capped at 200 characters.
The same query already searching is not started again, and an answer for an older query is dropped.
Choosing a GitHub result opens it in the browser.

### Keys and results

Up and Down move the selection in display order, stopping at either end, while typing continues in the query field.
Return opens the highlighted result through the existing agent, checkout, Overview or Pull requests actions (an issue or pull request opens its Project's Overview on that issue or pull request, on the device that holds it); Escape closes the sheet.
The selected row scrolls into view.
Filtering preserves a surviving selection by identity; a retired selection moves to the first remaining result.
Empty results have no selection, and arrows or Return require no modal acknowledgement.
A stale result is checked against the live result set before execution.

### File search

⌘P lists what hided's index ranked for the typed query, with the same focused query field and first-result selection, and never executes results from a previous query or checkout while its asynchronous index is updating.
↵ opens the highlighted file; ⌘↵ opens it beside the active View area, and the footer hint says so.

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
The header wash marks the pane Hide is showing, while the neutral split-pane outline marks the terminal that owns keyboard focus; moving keyboard focus into Overview keeps the shown wash and removes the terminal outline.
Unread weight is never reused to mean parent, child, delegated, or selected.

## Settings: each machine's install kit

Settings > Devices shows This Mac and every device in the same form: under each machine's connection and helper lines, one line per part of Hide's kit (the `hide` command, the Claude Code hook, the Codex hook, hcoord), with a mark, the part, and where it is when installed or its state and reason when not (PRD device-parity B7).
hcoord's place is `~/.hide/hcoord/bin/hcoord`; an `hcoord` on `PATH` that is not Hide's, and an old `~/.hcoord` left beside `~/.hide/hcoord`, are named in a dimmed line under the installed hcoord row's location, and a move from the old `~/.hcoord` that failed reads failed with the reason and that the next launch or Reinstall tries again (PRD hide-home-layout B12, B14).
A `~/.hide/hcoord` that holds a ledger but no `bin/hcoord` reads outdated, not removed, and is finished on the next launch: it is a move whose install did not complete, and hcoord's home is never taken by removal.
Installed is ✓, not on this machine is –, outdated, not installed or removed is !, and failed is ✕; the state is also read out, since the mark is hidden from assistive technology.
Reinstall sits on a machine's row only while one of its parts needs it, repairs only those parts, and reads Reinstalling… while the machine's kit work runs (B8); nothing else on the screen reacts, and the detail of every install goes to the diagnostic log (B18).
A machine whose kit does not run says why in that place instead of its parts: a daemon outside the installed app, a device not allowed yet, a device that must be allowed again, or a platform this build does not carry (B11, B17, B21).
A device not read yet reads that its kit is checked when it connects; the tab reads every machine once when it opens.
The add form has one Add button and lists, once, what the kit puts on the device and where (B12); a device registered earlier without the helper offers Allow and install on its row, with the same list.
Removing a device asks once, names in one line what comes off that device (with its helper folder, `~/.hide/host-helper` by default) and that hcoord and the records in `~/.hide` stay, or, when its helper is not connected, that the kit stays there; no button is focused when the confirmation opens (B22).
When another registered device reaches the same account on that machine, such as a second Herdr server there, the line says the kit stays for it instead.

Settings > Agents lists the hook parts of every machine, This Mac first and then each device in the Devices order, with Reinstall on a part that needs it and nowhere else (B27).

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

## Keyboard shortcuts per system

Web owner: `web/src/shortcuts.ts` (the registry, the rule and its exceptions), `web/src/keys.ts` (terminal keys), `web/src/shortcutLabels.ts` (every chord a screen prints), `web/src/keyboard.ts` (the window listener); desktop owner: `desktop/src/main/menu.ts` (the menu's accelerators); test owners: `web/src/shortcuts.test.ts`, `web/src/keys.test.ts`, `desktop/src/main/menu.test.ts`, and `web/e2e/chords.ts`, through which every web e2e presses its chords as the runner's system does.

The rest of this document writes chords as macOS has them; this section is how Windows and Linux press each one.
Which system applies is the one the operator types on: the desktop app's, or the browser's for a browser tab, whatever system the daemon runs on.

### The rule

On Windows and Linux every ⌘ is Ctrl+Shift, as in Windows Terminal, GNOME Terminal, WezTerm and kitty, so a plain Ctrl key reaches the shell and the agent CLIs (Ctrl+C, Ctrl+R, Ctrl+W, Ctrl+K, Ctrl+D and the rest); the only plain Ctrl keys the app takes are text size and, in a browser tab, the ones Chrome keeps for itself.
A ⇧ or ⌥ that a macOS chord adds to ⌘ makes it Alt+Shift there, the layer Windows Terminal puts its panes on (Alt+Shift+D splits a pane there too): ⌘T is Ctrl+Shift+T and ⇧⌘T is Alt+Shift+T, so no chord needs more than three keys.
The chords the rule puts on that layer take no key a shell or an agent CLI documents: readline reads Alt+Shift+B as Alt+B (`do-lowercase-version`), so Alt+B, Alt+D and Alt+T still reach the shell and Claude Code.
None of them is a key Windows, GNOME or Chrome lists as its own, except Chrome's Alt+Shift+T (below).
Windows can be set to switch the input language on Alt+Shift, and a Linux layout can be too (`grp:alt_shift_toggle`); a chord still runs there, because chords match the physical key, but whether the layout switches as well is unconfirmed on a real Windows machine.
A chord without ⌘ is the same keys on every system: ⌃Tab is Ctrl+Tab and ⌥1 is Alt+1, so those Alt chords take keys a shell reads as Meta, as they do on a Mac whose terminal sends Option as Meta.
Chords are written in words joined with `+`, in Windows' order Ctrl, Alt, Shift, as Windows Terminal writes them, so every chord the rule makes begins with `Ctrl+Shift+` or `Alt+Shift+`.
AltGr types a character on Windows and Linux layouts (Windows reports it as Ctrl+Alt), and a key pressed with it is never a chord.
A stored chord set keeps macOS chords, so a set a Mac saved means the same keys through the rule on Windows and Linux, except that ⇧⌘X, ⌥⌘X and ⌥⇧X are all Alt+Shift+X there, so a set binding two of them, or binding one onto a default that is another, is refused whole and the defaults run, and a chord recorded there as Alt+Shift+X is stored as ⇧⌘X; on those systems a desktop pane chord needs Ctrl+Shift or Alt+Shift, a browser chord Ctrl or Alt, and the Windows or Super key is refused because the system keeps it.

### Exceptions

Each exception has a dominant convention on Windows and Linux, or a chord the system keeps, behind it.

- Text size is Ctrl and =, - or 0, as in Windows Terminal, GNOME Terminal, WezTerm, VS Code and every browser.
- The recent project cycle is Ctrl+Shift+` (previous Ctrl+Alt+Shift+`): Alt+Tab is the system's window switcher on Windows and Linux and never reaches an app, so the cycle moves to ` under the rule, the key macOS keeps for its own window cycle.
  Going back adds Alt to the Ctrl+Shift the cycle already holds, the one four-key desktop chord: a held cycle commits when its Ctrl is released, so the rule's Alt+Shift+` could not step back inside it, and GNOME keeps Alt+Shift+` for switching between an app's windows.
- In a browser tab, Reopen closed tab is Ctrl+Alt+Shift+T: Chrome keeps Alt+Shift+T to focus its toolbar, which keyboard users rely on, and the macOS browser chord ⌥⇧T is those same keys there.
- Not yet handled: a browser tab's area cycle is Alt+` and Alt+Shift+` on Windows and Linux (Chrome keeps Ctrl+Tab, as it keeps ⌃Tab on macOS), and stock GNOME and KDE take both to switch between an app's windows, so on those desktops the cycle reaches the page only once the desktop's keys are changed or the cycle is bound to another chord in Settings, Shortcuts.
- Move to Trash in the Explorer is Delete, as in Windows Explorer and the Linux file managers.
- Inside a text field, a palette, a board or a document, no terminal holds the keyboard, so the system's own command key applies: ⌘↵ is Ctrl+Enter (send a form, open a ⌘P result to the side, open a pull request on GitHub), ⌘-click is Ctrl-click (a link, a pull request, an issue), and Ctrl+S saves the document in front as well as Ctrl+Shift+S.
- Tab digits follow the rule (Ctrl+Shift+1-9, as in WezTerm): Windows Terminal, GNOME Terminal and the browsers each use a different modifier, so no convention outweighs the rule.

### In a terminal

| Action | macOS | Windows and Linux |
| --- | --- | --- |
| Copy the selection | `⌘C` | `Ctrl+Shift+C` (the terminal's even with nothing selected, so Chrome's element inspector never opens), or `Ctrl+C` while text is selected |
| Interrupt the program | `⌃C` | `Ctrl+C` with nothing selected |
| Paste | `⌘V` | `Ctrl+Shift+V` |
| Delete to the line start, go to the line start or end | `⌘⌫`, `⌘←`, `⌘→` (sent as ^U, ^A, ^E) | `Ctrl+U`, `Home` and `End`, which the terminal sends as typed |
| A newline the agent keeps in its prompt | `⇧↩` | `Shift+Enter` |

Ctrl+C copies only while text is selected and clears the selection as it copies, as Windows Terminal does, so the next Ctrl+C interrupts; with nothing selected it reaches the program.
Every other plain Ctrl key reaches the program, apart from text size (Ctrl+=, Ctrl+- and Ctrl+0).
Settings, Shortcuts refuses a binding on the terminal's copy or paste chord.

### Every command

The hold hint follows the same table: holding Ctrl+Shift alone in the desktop app on Windows and Linux floats the tab numbers, and holding Alt alone the Agents-list numbers.
A command marked none has no chord until the operator binds one in Settings, Shortcuts, where it can be bound; the browser tab's moved chords are the ones Chrome keeps for itself on that system.

| Command | Desktop app, macOS | Desktop app, Windows and Linux | Browser tab, macOS | Browser tab, Windows and Linux |
| --- | --- | --- | --- | --- |
| New tab | `⌘T` | `Ctrl+Shift+T` | `⌥T` | `Alt+T` |
| Close focused view or pane | `⌘W` | `Ctrl+Shift+W` | `⌥W` | `Alt+W` |
| Reopen closed tab (exception) | `⇧⌘T` | `Alt+Shift+T` | `⌥⇧T` | `Ctrl+Alt+Shift+T` |
| Select tab 1-9 | `⌘1 … ⌘9` | `Ctrl+Shift+1 … Ctrl+Shift+9` | none | none |
| Add project | `⇧⌘N` | `Alt+Shift+N` | none | none |
| Start agent | `⌘N` | `Ctrl+Shift+N` | none | none |
| Next recent Agent pane or View tab | `⌃⇥` | `Ctrl+Tab` | `` ⌥` `` | `` Alt+` `` |
| Previous recent Agent pane or View tab | `⌃⇧⇥` | `Ctrl+Shift+Tab` | `` ⌥⇧` `` | `` Alt+Shift+` `` |
| Next global recent panel | none | none | none | none |
| Previous global recent panel | none | none | none | none |
| Next recent project (exception) | `⌥⇥` | `` Ctrl+Shift+` `` | `⌥⇥` | `` Ctrl+Shift+` `` |
| Previous recent project (exception) | `⌥⇧⇥` | `` Ctrl+Alt+Shift+` `` | `⌥⇧⇥` | `` Ctrl+Alt+Shift+` `` |
| Select agent 1-9 | `⌥1 … ⌥9` | `Alt+1 … Alt+9` | none | none |
| Search | `⌘K` | `Ctrl+Shift+K` | `⌘K` | `Ctrl+Shift+K` |
| Open file | `⌘P` | `Ctrl+Shift+P` | `⌘P` | `Ctrl+Shift+P` |
| Project home | `⇧⌘H` | `Alt+Shift+H` | `⇧⌘H` | `Alt+Shift+H` |
| Toggle left sidebar | `⌘B` | `Ctrl+Shift+B` | `⌘B` | `Ctrl+Shift+B` |
| Toggle sidebar view | none | none | none | none |
| Toggle device rail | none | none | none | none |
| Toggle Tools | `⌘E` | `Ctrl+Shift+E` | `⌘E` | `Ctrl+Shift+E` |
| Toggle File Views | `⇧⌘B` | `Alt+Shift+B` | `⇧⌘B` | `Alt+Shift+B` |
| Find in pane | `⌘F` | `Ctrl+Shift+F` | `⌘F` | `Ctrl+Shift+F` |
| Save file | `⌘S` | `Ctrl+Shift+S` | `⌘S` | `Ctrl+Shift+S` |
| Keep open | `⇧⌘K` | `Alt+Shift+K` | `⇧⌘K` | `Alt+Shift+K` |
| Split right | `⌘D` | `Ctrl+Shift+D` | `⌘D` | `Ctrl+Shift+D` |
| Split down | `⇧⌘D` | `Alt+Shift+D` | `⇧⌘D` | `Alt+Shift+D` |
| Zoom pane | `⌥⌘↩` | `Alt+Shift+Enter` | `⌥⌘↩` | `Alt+Shift+Enter` |
| Close pane | `⇧⌘W` | `Alt+Shift+W` | `⌥⇧W` | `Alt+Shift+W` |
| Larger text (exception) | `⌘=` | `Ctrl+=` | `⌘=` | `Ctrl+=` |
| Smaller text (exception) | `⌘-` | `Ctrl+-` | `⌘-` | `Ctrl+-` |
| Reset text size (exception) | `⌘0` | `Ctrl+0` | `⌘0` | `Ctrl+0` |
| Focus next Agent area | none | none | none | none |
| Focus previous Agent area | none | none | none | none |
| Grow Agent area | none | none | none | none |
| Shrink Agent area | none | none | none | none |
| Focus next View area | none | none | none | none |
| Focus previous View area | none | none | none | none |
| Grow View area | none | none | none | none |
| Shrink View area | none | none | none | none |
| Move to Trash (exception) | `⌘⌫` | `Delete` | `⌘⌫` | `Delete` |
| Settings | `⌘,` | `Ctrl+Shift+,` | `⌥,` | `Ctrl+Shift+,` |
| Keyboard shortcuts | `⌘/` | `Ctrl+Shift+/` | `⌘/` | `Ctrl+Shift+/` |

## Keycaps, tooltips, and icon buttons

Every icon-only control has a tooltip and an accessible name carrying the same words as the tooltip.
A chorded tooltip reads the label followed by the shortcut chord; a chordless control shows only the label.
There is no native platform tooltip layered underneath the shared one; the shared tooltip is the only tooltip in the main shell.
Tooltip hover has a short reveal delay, and an exact modifier hold reveals shortcut hints faster than a hover tooltip does.
In the desktop app, holding ⌘ alone (Ctrl+Shift on Windows and Linux) floats each tab's number at its top right in the agent tab strip in front, and holding ⌥ alone floats each Agents-list row's number at its top right: a keycap in the popover colors with a border, a small shadow and one mono digit, positioned over the tab or row rather than in it, so a title, an inline Rename field, a row's time, and its fold slot never move.
The number is the screen order at that moment, first to ninth, left to right for tabs and top to bottom for the rows the Agents list draws (a folded parent's descendants are not rows), and an item past the ninth carries none.
While Projects is on screen the same hold shows those Agents-list numbers, since ⌥n still selects by them, each once: on the agent's raised row, else on its row under the checkout that owns its pane.
The hint appears only after a short hold of the exact modifier; releasing it before then shows nothing, so a ⌘C never flashes numbers.
Releasing the modifier, adding another, pressing any key during the hold (including the numbered chord itself), losing the window, hiding the page, or opening a sheet, menu, dialog, palette, or cycle clears the numbers at once; the same modifiers still held after that show nothing until they are released and held again.
The keycaps and the hover tooltip never share space: a tooltip hangs beside its trigger and a keycap sits inside the trigger's own box.
In the desktop app a tooltip avoids the browser pages on screen: it opens on its usual side, else the opposite one, else across, whichever first fits the window clear of every page, because a page is drawn over the shell (Browser displays).
A browser host has no numbered chords, so holding ⌘ or ⌥ (Ctrl+Shift or Alt on Windows and Linux) there shows nothing.
Pane focus, active tab, tab order, zoom state, and disappearing anchors all update which controls can show a hint or tooltip; pointer exit, mouse down, scroll, key down, losing key window status, and anchor removal all dismiss an open tooltip.

Destructive buttons are named by their result (`Move to Trash`, `Close 3 panes and remove`, `Stop work and close`), never by a generic "Delete" or "OK" that hides the consequence; the non-destructive option is the default/cancel action, except in the sheet that closes an agent with the agents it spawned.
There the operator chose `모두 닫기` as the Enter default (PRD close-agent-subtree D-07, D-18), and a descendant whose status is unknown gives the default back to `취소`; the removal dialogs that ask the same question keep no default.
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
