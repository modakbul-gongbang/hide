# Browser displays

A web page in hide is a display in a View area, beside the files and diffs of the same Workspace (issue 155).
It is a tab like any other view: it splits, moves, closes, and comes back after a relaunch with the rest of the layout.
The page itself is drawn by the desktop app, a native Chromium view laid over the area's slot; a plain browser tab that serves the shell cannot draw one and says so.
There is no Herdr browser pane and no chromux profile behind it, and nothing is screencast.

## Who owns what

| Owner | Holds | Code |
| --- | --- | --- |
| The core | Browser View layout, requested address and load stamp, and the native page's reported loading state | `herdr-core/src/view_layout.rs`, `herdr-core/src/runtime/view_areas.rs`, `herdr-core/src/runtime/workspace_control.rs` |
| hided | The local `file:` event boundary, checkout-scoped Browser commands and gateway discovery, and native routes into consented SSH devices | `hided/src/server.rs`, `hided/src/file_url.rs`, `hided/src/workspace_cli.rs`, `hided/src/browser_control.rs`, `hided/src/browser_routes.rs`, `hided/src/browser_assets.rs` |
| The web shell | Where each page sits, since only it has the geometry, the toolbar, the overlay freeze, and the notice in a plain browser tab | `web/src/BrowserDisplay.tsx`, `web/src/browserViews.ts`, `web/src/host.ts` |
| The desktop app | The pages: one `WebContentsView` per display it was asked to show, their navigation, route requests, and their lifetime | `desktop/src/main/browser.ts`, `desktop/src/main/browserSync.ts`, `desktop/src/main/host.ts`, `desktop/src/preload/index.ts` |

The core never loads a page and the desktop app never decides which displays exist.
The core's Browser View inventory is sent as an empty array when the last page closes, so the native host removes that page even when no other Browser View remains.
A display's `load` stamp is how the core asks for a load: the host loads the display's address again whenever the stamp is newer than the one it last applied, so opening an address the Workspace already shows focuses that display and loads it again rather than adding a second one.
The stamp is not saved; a relaunched page loads its address once when it is first shown.
The retained inventory also carries the authoritative `area_id`, so native control never guesses a page's area from the front Workspace.
Only checkouts in the current connected catalog contribute to that inventory; a removed or disconnected checkout's saved layout grants no native page authority.

## Opening a page

- `hide browser open <url-or-path> [--reveal] [--wait] [--request-id <id>]` from a shell the daemon can bind to a checkout: a connected Herdr pane, or a local process whose working directory is inside a registered checkout (`docs/ARCHITECTURE.md`, hided and the WebSocket boundary).
  The CLI resolves a relative file on the calling machine, gives loopback hosts `http` and other hosts `https`, and submits one action scoped to the caller's checkout with no Workspace override.
  The core checks an HTML file against that checkout on its own device and reads it outside the core lock before placing its Browser View.
  The action result confirms View placement, while `hide view status <view-id>` returns `pending`, `loading`, `loaded`, `failed`, `disconnected`, or `unsupported` for its native page.
  `disconnected` means a native page existed for that load but was removed or its desktop renderer disconnected; `unsupported` means no desktop renderer was available for a pending page.
  `--wait` polls that state for at most ten seconds and returns a failure if the page fails, disconnects, or remains pending; it does not reveal a hidden View.
  If a later status query fails, the refusal still carries the applied open request ID and View ID for reconciliation.
  The action receipt and page status carry the load stamp, so a concurrent reopen that supersedes the requested load returns `page_superseded` instead of the newer load's result.
  `--reveal` explicitly brings the caller's checkout and selected Browser View forward, including a connected SSH device.
  The CLI returns one JSON line, exits nonzero on refusal, and never starts Hide.
  Its successful result includes `cdp_http_url` and `browser_ws_url` when the native gateway can issue a capability for that display.
  If discovery fails after placement, the result retains its applied View receipt and carries `browser_control.state: unavailable` with a retry command; discovery does not undo or repeat the open.
- Open in Browser in the Explorer's menu on an HTML file of a local or connected device checkout.
  The device host must have file access consent for a remote page to load.
- A page that opens a tab, a `target=_blank` link (an image a page wraps in one), a shift-click or `window.open` without window features, gets another browser display in the same Workspace, beside that page so the page that asked stays in view.
  The shell names the page (`beside_display`), and the core places the new display in the View area next to the page's area in Open to the side's order (right, left, down, up), else, the page's area being the only one, in a new area to its right; with the page gone it opens where any page would.
  An address the Workspace already shows is focused and loaded again there instead.
  A sized popup, `window.open` with window features the way a sign-in button opens one, is not a display; see Popups below.
  A remote page's routed loopback address is translated back to its source device address before that request reaches the core.
- The address field in the display's toolbar loads what was typed into that display.

An empty address is a new-tab page rendered by the shell, with a focused address field and Open choices for File (⌘P) and Diff when the checkout has changes.
File and Diff use the existing palettes and replace the empty display, preserving its ID and position; a file read that fails leaves it intact, and one whose original tab was closed or navigated meanwhile cannot overwrite another tab.
Each + or View tab menu New tab action creates its own empty display in that area, without creating a native page.
Closing an untouched empty display adds nothing to Recent Closed or draft recovery.

A nonempty display holds only an `http`, `https` or `file` address with something after the `//`, or `about:blank`, within 8 KiB; anything else is refused with its reason and nothing changes.
A remote `file:` address is loaded through a checkout-scoped route, never as a file on this Mac.

## Scoped browser control

`hide browser connect [--display <id>]` uses the caller's existing Workspace credential and prints a JSON result with `cdp_http_url` and `browser_ws_url`.
It accepts no Workspace override and requires a connected desktop renderer.
An explicit display must be a browser in that credential's checkout; an omitted display uses that checkout's active View area for new targets.
Discovery does not reveal the Workspace, activate the app, or start a daemon.

The desktop registers one private numeric-loopback gateway address and random control token with hided through daemon-authenticated `/browser-control`.
The registration is tied to the app PID and process birth, has a four-host cap, and is removed on release or discarded when a later request finds the app gone.
An ambiguous multi-window registration refuses discovery instead of choosing another window.
At most eight discovery and browser action requests are in flight, and discovery has an eight-second deadline with a 16 KiB response cap, no proxy and no redirects.
The CLI receives only the public capability URLs; it never receives the desktop registration token or the shell's daemon token.

CDP target creation uses authenticated `/browser-control/action` and the existing Workspace prepare/read/commit contract.
The core inserts a distinct browser display into the capability's named area even when another area is active or the same address is already open.
An unknown area refuses before placement.
CDP exposes only HTTP(S) and blank pages; native `file:` displays, filesystem paths and other schemes return `browser_address_unsupported` for attachment or creation.
Every HTTP(S) or blank native page generation blocks `file:` requests before its first load, independently of debugger attachment, and retains that restriction until the generation ends.
The native guard cancels file main-frame and child-frame navigation, redirects, resources and popup requests, including addresses assembled by `Runtime.evaluate`; it never attempts to recognize JavaScript source text.
Manual local-file displays retain the file policy below and are never CDP targets.
An explicit manual navigation to a validated local file replaces the restricted WebContents with the existing separate file partition instead of relaxing an in-flight generation.
Downloads and filesystem writes are disabled once a generation has held a debugger lease, including after disconnect.
Each native generation logs its first file request, navigation and popup refusal separately, and its first controlled-download refusal once; repeated attempts remain canceled without more diagnostic output.
Close and select bind the authenticated area into the core action and recheck the display's area and browser kind under the commit lock after retry lookup.
A page moved after the query is refused without changing its selection or closing it; shell renderers and terminal displays cannot be selected or closed by this route.
The app has one retry identity per launch registration, so retransmitting a completed close returns its receipt after the page is gone.
The core also publishes positive area scopes separately from its browser inventory, including empty areas of connected catalog checkouts.
Each scope has an incarnation that remains stable during ordinary layout changes and changes after authority is revoked and granted again.
The shell forwards these scopes through the existing native sync; a missing scope grants no CDP authority.
Capabilities pin that incarnation, so a coalesced snapshot that hides the intermediate revocation cannot revive an old connection URL.
The core records loss of authority when a session or checkout catalog is updated, before another worker update or snapshot read can hide it.
Unregistering a checkout, disconnecting its device or removing its area permanently revokes existing capabilities, so registering it again requires a fresh connection URL.
Closing an area's final page retains its area scope and allows a fresh target there without reviving an expired checkout capability.

The gateway creates a new private control token and random capability path for each app launch; restarting the app or replacing its daemon connection invalidates every old URL and debugger session.
It binds only numeric loopback, checks Host and Origin, and serves discovery only under the issued capability path.
It does not enable Electron's process-wide debugging port, and its target provider contains only the BrowserViews inventory, never the shell renderer or another area's pages.
The tab's attachment mark follows an actual debugger lease; disconnect detaches the client while preserving the display, and closing the display ends its sessions.
Browser-wide cookie, cache, permission, download, context and shutdown commands are refused; page-origin cookie reads are scoped to the current page.
Flattened descendant iframe sessions and one level of legacy session envelopes are supported; deeper legacy envelopes are refused before a mutation.
Limits are fixed in the host: 64 capabilities, 32 sockets, eight clients, 16 HTTP requests, 32 pending commands per client, 64 sessions per client, 4 MiB messages, 8 MiB buffered output and 600 commands per minute per client.
A command has a ten-second deadline; each client keeps at most 128 mutation receipts, accepts a replay for at most nine minutes, and retries an ambiguous core action at most three times with its original request identity.

### Client invocations

Use the complete `browser_ws_url` from `hide browser connect` as `browser_ws_url` below.
It is a capability: preserve its path, keep it out of shared logs, and request a fresh URL after its authority ends.
The supported profiles are agent-browser 0.38.2, chrome-devtools-mcp 1.10.1, Playwright 1.63.0 and the Browser Use 0.13.10 low-level session API.
Each client connects to an existing HTTP(S) or blank display; an area capability also permits a new display in that same area.

```sh
agent-browser --cdp "$browser_ws_url" open http://127.0.0.1:3000
agent-browser --cdp "$browser_ws_url" snapshot -i
```

```sh
chrome-devtools-mcp --wsEndpoint "$browser_ws_url" --no-usage-statistics --no-performance-crux
```

MCP's literal `--browser-url` mode resolves discovery at `/json/version` and loses the capability prefix; use its official `--wsEndpoint` option.
Anonymous root discovery remains unavailable.

```js
import { chromium } from "playwright";

const browser = await chromium.connectOverCDP(browser_ws_url, { noDefaults: true });
try {
  const context = browser.contexts()[0];
  const page = context.pages()[0];
  console.log(await page.title());
} finally {
  await browser.close(); // Disconnects this external client; preserves the display.
}
```

Playwright uses the existing context with `noDefaults: true`; creating another context or applying its default global download configuration is unsupported.
`Target.attachToBrowserTarget` supplies an opaque protocol parent for Playwright's independent page sessions, with only scoped discovery, version, and explicit child attach/detach commands.
It never attaches a native browser debugger or creates a context; virtual parents count toward the same 64-session limit and release their admitted children when detached.
Once the core confirms a selected-display or direct-page close, the gateway revokes that authority and drains the successful reply before the normal WebSocket close handshake, even when native retirement arrives later.
Page, tab and iframe target metadata carries one opaque context identifier per launch so clients can associate those targets with the existing context; it does not grant native context authority.
The gateway attests each leased debugger's native main frame before forwarding client commands and translates protocol frame references to that display's public page target ID.
Iframe target IDs retain their native frame identity only after attachment from that owned debugger; their session IDs remain opaque, and parent-frame references use the public page identity.
This translation applies to protocol frame metadata and commands, never to values returned by page evaluation.

```sh
BU_CDP_URL="$browser_ws_url" python - <<'PY'
import asyncio
import os
from browser_use import BrowserSession

async def main():
    session = BrowserSession(
        cdp_url=os.environ["BU_CDP_URL"], is_local=False, keep_alive=True,
        use_cloud=False, enable_default_extensions=False, accept_downloads=False,
        permissions=[], captcha_solver=False, auto_download_pdfs=False,
    )
    try:
        await session.connect()
        print(await session.get_tabs())
        page = await session.get_current_page()
        if page is None:
            raise RuntimeError("No eligible browser display is attached")
        print(await page.evaluate("() => document.title"))
    finally:
        await session.stop()

asyncio.run(main())
PY
```

This Browser Use example explicitly passes `BU_CDP_URL` to the Python API and uses `connect`, page queries and `stop`.
Default `Agent` or `start` watchdogs request browser-wide download behavior and are unsupported; this profile does not establish Browser Use CLI support.
Native file displays and unrestricted browser-context administration are unsupported for every client.

## Agent page commands

`hide browser snapshot|click|fill|type|press|hover|drag|scroll|wait|screenshot|eval|console|network <display> ...` read and drive one browser display of the caller's checkout, by its View id.
`hide browser help` prints the agent guide for them, the commands, the ref format, checking with `--diff` and `changed`, and each failure reason with its recovery; it is the source of the `hide browser` agent skill, and the session guidance names the commands and points at it (`hide-agent-hooks/src/workspace_context.rs`).
A snapshot is text and exits zero; an action prints one JSON line with what it did, `changed` and `next`; every failure prints `{"ok":false,"reason","display","next_action"}` and exits non-zero.

Each command is one connection and one debugger lease: the CLI asks hided for a relay to the display ([ARCHITECTURE.md](ARCHITECTURE.md#hided-and-the-websocket-boundary)), attaches to its page, does its work and disconnects, which releases the lease and turns the tab's attachment mark off.
A display another debugger holds, an external client or another `hide browser` command, fails `display_busy` at once; nothing queues.
A display that is not HTTP(S) or blank fails `display_unsupported`, a missing one `display_missing`, and one that closes during the command `display_closed`.
Nothing outlives the command in hided or on disk, except the file `screenshot` writes: refs are `data-ct-ref` attributes of the page's elements, so they survive a re-snapshot of the same document and start again at `@1` after a navigation, and the `--diff` baselines and the no-change streak live in a non-enumerable property of each frame's document.
Every CDP step has an eight-second deadline, below the gateway's ten-second command deadline, which would otherwise end the whole lease, and a whole command ends with `page_unresponsive` after two minutes.

Cross-origin frames are auto-attached, up to 24 per page, and each frame's document carries a tag of four random characters; a snapshot shows it as `# OOPIF <tag> origin=<origin>` with refs `@<tag>:N`, and its field values and link targets reduced to their origin.
A frame has its own renderer, so one that answers nothing within a step (its `Page.enable`, a read of its document or its own frame lookup) does not fail the command: the snapshot notes it as `# OOPIF unresponsive origin=<origin> - no answer in time; ...` right after the top document, with the origin its `location.origin` would have, reads the rest of the page, and gives it no tag, refs or baseline of its own.
Only an unanswered call counts as silence: a closed connection, including the relay's idle close, still fails the command.
The note is part of the top document's `--diff` baseline, so a diff names a frame once, when it goes silent or answers again; the lines a frame showed before it went silent cannot be read back and are not reported as removed.
Silence is what one read observed, not a state of the frame: every frame stays known, and a read of the page (every snapshot, the capture before an action and each one after it, every poll of a `wait`) asks again a frame that gave no answer before, whether it missed its `Page.enable`, its tag, its look for its own frames or an earlier read, but only within the command's budget for silence.
That budget is `MISS_LIMIT` (two) unanswered rounds per frame in one command: the round that found it silent and one more in case it was only slow.
After that the frame is noted and not asked again for the rest of that command, so a frame that never answers costs at most two steps (its rounds run together with the other frames', one step each) and not one on every read; a read that would have asked it gets the silence it already showed (`Page::eval` and `Page::eval_all` answer a frame the command has given up on without sending), so the frame stays unread and named, never counted as a change or as absent.
The next command starts with a clean budget.
Each such round sends its calls together, `Page.enable` for the frames not yet enabled, then their tags, then the reads and the tag checks that follow them and the baseline swaps, and reads the answers under one deadline, so the frames a page holds cost one step between them per round, and a round larger than the gateway's 32 commands in flight goes in rounds of that size.
Looking inside several frames for their own frames is one such round too, not one frame after another.
The cap counts what the gateway still holds, not what this client waits for: a command this client gave up on at its eight-second step stays pending in the gateway until its ten-second deadline, so `Cdp` keeps every command it sent and has not seen answered until that deadline (plus a quarter second, since the gateway starts its clock on arrival) and a round that would pass 32 waits for room, or, when room would come only after the call's own deadline, sends nothing and reads as unanswered; the gateway never closes the connection with 1013 because of frames that are silent.
The gateway handles each command as it arrives, not in sequence, and each frame has its own renderer, so a frame's answer is due one step after it was sent, as it was when frames were read one at a time.
A frame that goes silent while its baseline is swapped leaves the snapshot for a note, so a diff shows the note and not every line of the frame as new.
A silent frame is never evidence of absence: a ref whose frame tag no readable frame carries, while a frame that gave no document may be the one, is neither stale nor gone, but fails an action with `page_unresponsive`, whose `next_action` says to take a fresh snapshot (a ref from an earlier snapshot is the likelier cause, and the remedy is the same whether or not the silent frame was ever the ref's), and leaves `wait` to its next poll, so `wait --gone @tag:N` never succeeds on a frame that has only not answered yet.
`wait` asks the top document and every frame together at each poll, inside the budget above; a wait that times out says that frames were not read and names their origins.
A wait's own timeout is the deadline of every read of its polls: a read waits no longer than the wait has left (and no longer than a step), so `wait` reports `timeout` at its deadline, not a step after it, and a read cut short by that deadline is a wait that ran out of time, not `page_unresponsive`.
An action's change check that could not read a frame says so, with the frame's origin, instead of reporting that nothing changed, and adds no stall to the streak.
The change check compares like with like: a frame that one of the two reads (before the action, after it) could not read is left out of both texts, so a frame that answered in one read and not the other is never a content change, and it is named: with no other change the answer says the frame did not answer and adds no stall, and with a change it ends with an `# unread:` line naming the frames that could not be compared.
A frame whose own frames could not be looked for (its look for child frames got no answer) is as unread as one that gave no document: a ref that no readable frame carries may be a child of it, so it is not stale or gone either.
The frame that holds the keyboard focus is found by asking every frame together. When none reports the focus and a frame did not answer, `type` and `press` fail with `page_unresponsive` and send nothing, since the focus may be in the frame that did not answer; the top document is the answer only when every frame answered and none holds the focus.
An action on such a ref goes to that frame's own session in its own coordinates, so a ref from a frame that navigated fails `ref_stale` instead of touching another element.
Same-origin frames and open shadow roots are read and acted on through the top document.
`changed` compares a snapshot of every frame taken before the action with one after it, read again after 0.7 and 1.2 seconds when nothing changed yet; clickable detection is capped in document order so a scroll alone never reads as a change.

Input and screenshots go only to a display on screen, so the operator sees what an action does.
A display that is not its area's selected View fails them at once with `display_hidden` and names `hide view select <display> --reveal`; hided reads that from the core's view list when it opens the relay.
That is hided's own rule, not Chromium's: a View the host hides still answers.
A selected View of a Workspace that is not in front is not refused: the host hides it, yet the page still reports its document visible and Chromium answers input and screenshots within the step, so no input times out there and none waits to be delivered when the Workspace returns, and the operator does not see the action drawn.
The page's own visibility is not the test either: a selected View in a window another app covers reads `hidden` and still takes input and screenshots.
An input the page does not answer within the step, because a script holds it, fails `page_unresponsive` and is not undone: the event was sent and takes effect when the script yields, so the failure's `next_action` says it may already have happened and to look with `snapshot --diff` before repeating it, as it does when the relay ends the connection before the answer.
A call that times out and then finds the connection closed or a dialog open reports that cause, not silence.
A press that held the page this way ran its handler to the end, and no release or click arrives later, since the command sends none after the failure and its session ending delivers none.
A drag whose move or drop the page does not answer sends no release either, for the same reason: a release a held page would take when its script yields lands long after the command reported its failure, and it does not switch drag interception off, which ends with the session.
Reading commands work on a hidden display.
No command calls `Page.bringToFront`, moves the operator's mouse, changes the View in front or takes keyboard focus.
A JavaScript dialog is never answered for the operator: an action that opens one reports its type and message, and while it is open the next command's `Page.enable` on the top document's session gets no answer within the step deadline and fails `dialog_open`; Electron shows the dialog as a sheet on the window, where the operator answers it.
A step that gets no answer after the command received a dialog event from any frame also fails `dialog_open`.
A dialog that a cross-origin frame opened before the command started cannot be told from a frame whose script never yields, since Chromium sends no event again to a new client, so that frame is noted as unresponsive and the rest of the page is read.
Such a dialog does not hold the top document: a click on a top-document button was answered and ran its handler while a frame's `confirm` was open, which is why the frame is noted and not the whole page refused.
`console` relies on Chromium replaying the messages the current document logged to a session that enables `Runtime`, and shows the newest 50; `network` reads the document's resource timing, the newest 100 rows, and has no request or response headers or bodies.
`wait --timeout` is at most 60 seconds and `--verify` at most 10.
There is no file upload and no download: `DOM.setFileInputFiles` stays refused by the gateway and downloads stay disabled for a page that has held a debugger lease.
`Input.dispatchDragEvent` with a `data.files` list that is not empty is refused for the same reason, since dropping files hands the page local files as an upload does; a drag that carries no files passes.
A native HTML5 drag whose intercepted data names files fails `drag_carries_files` before any replay, so the gateway's refusal is never the first the agent hears of it; its `next_action` says not to retry, since a pointer drag would start the native drag where the gateway sees neither it nor its files.

Each action draws on the operator's view of the page: an arrow cursor in the accent color glides to the target, a click ripples, a drag leaves a line, filled fields flash, a pressed key is named beside its field and a scroll shows an arrow.
It is one `<hide-agent-overlay>` element, a direct child of the document element with a closed shadow root, `aria-hidden` and `pointer-events: none`, so it never enters a snapshot and lets the operator's clicks through; it removes itself two seconds after the last action.
Drawing it has a one-second deadline and never fails a command; reading commands draw nothing.

The page-side code is chromux's (MIT, `modakbul-gongbang/chromux` `93f770f`), kept as embedded assets that carry its notice: `snapshot.js`, `dom.js` (deep query, target checks, fill and the wait probes), `render.js` (`--diff`, `--grep`, `changed`, evaluated in an isolated world) and `overlay.js`.

## The file boundary

A client names a local file through a `file:` URL, so hided checks it like any path a client sends, on `browser_open`, `browser_state` and the `view_layout` `navigate` action.
It decodes the path, refuses one that names another host or does not decode (`invalid_path`), runs it through the same checkout boundary the Explorer uses (`outside_checkout` for a file outside every registered checkout), and writes the checked path back as the one spelling the shell and the CLI also produce.
A refusal is a `path_refused` frame to that client and never reaches the core.
For a native `browser_state` report, a registered checkout's pinned physical root can be mapped back to its registered spelling before that same boundary checks the file.
This preserves a completed load's stamp when the checkout was registered through an alias, while a replaced root, a symlink escape or an unrelated file is still refused; it does not extend file-open authority.

A device file keeps its remote path through the UI boundary.
The core confirms that the current View belongs to the connected device and checkout, and hided reads the HTML and its declared relative assets through that device's consented `hide-host` channel.
The channel pins the checkout root, refuses traversal and links outside that root, and caps each response at 16 MiB.
The route serves the opened HTML plus at most 128 declared relative stylesheets, images, scripts, and CSS image or font URLs.
It refuses undeclared checkout files, including same-directory secrets, and sends a restrictive content security policy that prevents a remote HTML preview from contacting another origin.
The native page receives a random loopback route for its own View and load stamp; the route URL does not replace the remote address stored in the core or shown in the toolbar.
Remote `localhost`, `localhost.`, IPv4 `127/8`, IPv6 loopback, and IPv4-mapped IPv6 loopback HTTP, HTTPS, and WebSocket traffic instead uses a dedicated SSH local forward to that device's loopback port.
The native route binds a local loopback IP and uses its actual local port.
For `localhost`, the forward reserves both IPv4 and IPv6 loopback at that port before publishing the route, so either resolver choice reaches the SSH device.
HTTPS keeps the source hostname or numeric loopback IP in the browser URL for certificate checks.
If this Mac cannot bind a numeric source IP for an HTTPS forward, the route fails explicitly.
The wildcard spelling `0.0.0.0` uses `127.0.0.1` as its safe local destination and a certificate for the wildcard name may fail validation.
The remote source address remains in the Browser toolbar.
Absolute loopback subrequests from a forwarded page use that View's forward when their scheme and source port match; other loopback requests are refused instead of reaching this Mac.
If the SSH route fails, the page shows the failure; it never tries the same port on this Mac.
Each native route is bounded, belongs to the desktop process that requested it, and closes when its View closes, its device disconnects, or that process exits.
Closing a route cancels its pending SSH channel open and its active transfer tasks before releasing both local listeners.
An individual SSH channel open has a 15-second limit, including the alternate localhost address attempt.
Creating a route reserves its View and load briefly, then connects to SSH outside the route registry lock.
Other Views can close while that connection is pending, and a canceled or superseded reservation cannot publish a late forward.
The complete SSH connection and authentication attempt has a 15-second limit.
Closing a pending View cancels its SSH connection, and at most four route builds may be in flight even when Views are repeatedly opened and closed.
Cancellation during SSH key exchange also shuts down the TCP socket before a session handle exists, so the connection cannot outlive its build permit.

A page the operator loaded can still follow its own links.
A `file:` page that moves to a file outside the checkouts keeps showing it in its view, but its report is refused at the boundary, so the core keeps the last address it accepted and a relaunch opens that one.

## The page and its limits

Web pages in every Workspace share one persistent session for logins, cookies, origin storage and zoom.
Remote loopback pages use a persistent session per device, and HTML file previews keep a separate session per Workspace; none uses the shell's session.
This keeps localhost cookies of this Mac, each SSH device, and remote HTML previews apart even when their host names are equal.
Pages run in a sandboxed renderer with context isolation, no Node and no preload, so nothing in a page reaches the `hideHost` bridge or the daemon's token.
Existing Workspace-specific web sessions are not copied into the shared session, so an upgrade may require one new login per site.
A page gets no permission but writing the clipboard, because a prompt it would raise has nowhere to show; a download follows Chromium's default and is logged as `browser.download`.
A page may navigate to `http`, `https`, `file` and `about:blank`.
A link to another app's scheme (`slack:`, `zoommtg:`) from a page on screen leaves only after the operator agrees, the way Chrome asks first: whether it arrives as a navigation, a server redirect, a frame or a new window, a sheet on the window names the app macOS would open it with, the asking origin and the link, and Open hands the link to that app.
Cancel is the default button, so a stray Return from a question the operator did not expect opens nothing, and after a Cancel the same page or popup asks nothing more until it navigates again.
A frame's link reaches Chromium's external protocol handler rather than a navigation event, so the session's `openExternal` permission request asks the same question, and Chromium's own opener never runs.
A `mailto:` link from a page on screen goes to the default mail app without asking.
Refused and logged with their scheme only: a scheme no app claims; a scheme Chromium answers itself (`data:`, `blob:`, `javascript:`, every `chrome` one and the like); a scheme macOS hands to a file share, a shell, a script or a remote session (`smb:`, `afp:`, `ftp:`, `ssh:`, `telnet:`, `vnc:`, `news:`, `applescript:`, `shortcuts:`, every `x-apple` one and the like), so one click on an unexpected question never mounts a share or runs anything (`appScheme` in `browserSync.ts`); a link from a hidden page or an HTML file preview; and a link that arrives while a question is open.
The link never reaches the log, since an app link often carries a sign-in code.

The shell tells the host, in one sync, every browser display of the Workspace in front, the rectangle its slot occupies now, and the core's retained Browser View inventory across all Workspaces.
A display the inventory no longer names is closed, which ends its renderer process even when its Workspace is in the background.
A retained display of a Workspace not in front, or not shown in its area, is hidden and keeps its page, so moving a page between areas or Workspaces never reloads it.
At most 12 pages live at once (`MAX_LIVE_VIEWS`); past that, the page shown least recently among the hidden ones is closed and loads its address again when it is next shown.
The host checks every field of a sync before it places anything and drops a message that fails whole; only the shell the daemon serves may send one.
Native page reports carry the display's current load stamp, so a late report from a previous reload cannot mark the new request loaded.
An evicted page reports its disappearance as disconnected until shown again.

## Keyboard ownership and cycling

Only a visible native page can report keyboard focus to the shell; the shell resolves its complete Workspace identity and current View area.
The page's `before-input-event` matches the shell's effective Electron shortcut registry for focused-area, global panel and project cycle commands before the page or menu sees them.
One host slot holds the initiating page, actual release modifier and cycle identifier, including input received after the overlay hides that page.
The first native cycle chord gives keyboard response to the shell in the same focused window, even when the overlay leaves the initiating page visible.
Held repeats, release and Escape received by the shell follow the same host IPC route as the first chord, so a quick release cannot arrive before cycle initialization or also run through the DOM listener.
After hiding that held page, the host reasserts shell keyboard response, because hiding a WebContentsView removes its native keyboard response.
The shell retains the initiating page's logical ownership until release commits or Escape restores its input destination.
When a first start is rejected or has zero or one eligible item, or a release commits nothing because the cycle came back to its origin, or the hold ends because its area shrank to one tab or the daemon went away, it restores the initiating visible page; a release that commits instead follows the chosen page.
The trusted bridge carries only cycle keydown/keyup and cancellation; the shell keeps the frozen scope and preview and owns the single commit.
The shell also reports the matching identifier when release arrived in its renderer, so a late completion cannot erase a newer hold.
Release or Escape received by another native page still ends the frozen initiating cycle.
Window blur, page renderer failure and destruction of the held page cancel the host slot, and a hidden or unfocused page cannot begin a cycle.
After a blur cancels a hold, the host owes the initiating page the keyboard and pays it once the window is back and that page is shown, as Escape does.
The window is back at its own `focus` event, not when it reads as key: on macOS that event is the window becoming main again, and Electron restores the focus it stored on `blur` (the shell, which the hold had given the keyboard) just before it, so a page paid while the window is key but not yet back loses the keyboard to that restore.
The shell's sync that uncovers the page can land after the window returns, so the debt waits for the page instead of expiring at the first focus event.
Another page taking the keyboard, a new hold, leaving the page's Workspace, or the page closing ends the debt.
The host logs the window's blur, focus and visibility (macOS reports it hidden while nothing of it is drawn, and the shell then sends no sync), each page's focus, blur and shown state with its display id and whether it is the owed page, and the shell's focus beside `browser.window_return`, so a failed return shows whether the keyboard was never paid or was taken after it, and by what.
None of these carries an address or page content, and each fires at the rate the operator focuses, covers or switches a window or page.
After the core confirms a selected display, the existing keyboard-follow request focuses its document or diff, or schedules one trusted native-page focus command after visible-slot sync.
The host focuses only a visible page in its already-focused candidate window; it never brings a window forward for this command.
Cycle menu items are immediate command clicks without accelerators, so one physical key cannot also dispatch a menu selection.
The keyboard area has a strong tab accent and a content boundary outside the native slot; other selected tabs remain readable, with no page blur or recurring capture for focus styling.

## Popups

A sized popup a page on screen opens (`window.open` with window features: Chromium's `new-window` disposition with features, `isPopup` in `browserSync.ts`) is a real window above hide that keeps its opener, so a sign-in popup can post its result to the page and close itself; a display would have no opener, and the page would wait forever.
A shift-click on a link is `new-window` too but has no features, so it is a tab like `target=_blank`.
The host builds the window from its own options, and of its geometry and chrome takes only the size the page asked for, held between 320 x 240 and the work area and centred over hide's window (`popupBounds`); no window feature makes it frameless, always on top, unclosable, modal or off screen.
Electron gives it the opener's web preferences with Node off and the sandbox and isolation forced, so a feature can at most change the popup's own page (turn its script off, say), never what it can reach.
It takes the keyboard only when the page that opened it held the keyboard; otherwise it is ordered in without activating the app, so a page cannot pull focus from what the operator is typing.
It shares its page's session and request rules, holds no bridge, follows the same navigation rules as its page, and its own popups belong to the same page.
Electron closes it with its opener, so it ends when its page's display closes, is evicted, or the window closes.
At most 4 are open at once (`MAX_POPUPS`); a hidden page asking for one, or any page asking for a fifth, is refused and logged.
While a popup holds the keyboard, Close (⌘W) and Close pane close the popup and the text-size commands zoom it; no other app command reaches the shell behind it.

## Zoom

While a page holds the keyboard, the text-size commands zoom the page instead of sizing text, the way ⌘= / ⌘- / ⌘0 do in Chrome.
The desktop app routes them by command, not by key, so a rebound text-size chord zooms the page too: an app-menu command for Larger, Smaller or Reset text that arrives while a page holds the keyboard steps that page through Chrome's zoom levels (25% to 500%) or back to 100%, and the page keeps the keyboard (`BrowserViews.zoomFocused`).
Chrome's second zoom-in chord, ⌘+ (⌘⇧= on a US keyboard), reaches no menu item, so the host zooms the focused page on that key before the page or the menu sees it, unless the operator bound that chord to a command.
With the shell holding the keyboard the same commands size the focused terminal or document text as before.
Chromium keeps one zoom level per host within a page's session partition, so web pages of the same host in different Workspaces zoom together.
The shell's own zoom, which places every page on its slot, is a separate session and never moves with a page's zoom.

A trackpad pinch magnifies the page up to 3x without reflowing it, as in Chrome.
Electron turns pinch off by default, and the limit lives in the page's renderer, so the host sets it again on every committed navigation, including one that moves the page to a new renderer.

## Overlays

A native view is drawn above the page's HTML, so the shell cannot draw over it.
When the palette, a menu, a dialog, a popover, a focused-area, Global Recent Panels or Recent Projects list meets a page's rectangle, the shell hides the page in the same sync that first sees the overlay, so no frame shows the page over it, and draws the page's idle still in its place (below); with no still of the page's address and slot size yet, the place stays blank until one arrives.
In that same flush, ahead of the sync that hides the page, the shell asks the host for a fresh capture; the host answers only for a page it still shows (`browser.capture_refused` otherwise), and IPC from one renderer is handled in order, so the capture is taken of the page as it is now and replaces the still and the cached one when it arrives.
A capture that fails or comes back empty leaves the place blank or the older still, draws no message, and is logged (`browser.capture_failed`, `browser.capture_empty`).
The page comes back live when the overlay is gone, and its still goes a frame later, so nothing flashes between the two.
A shell drag (a tab, a divider, a column divider, an Explorer item) marks the document root with `data-view-drag` or `data-agent-drag` while it runs (`web/src/shellDrag.ts`), and every page of the front Workspace freezes the same way until the mark is gone, so the drag's guide or preview draws over the pages and its drop never reaches one.
A new shell drag sets that mark rather than adding its guide to the overlay selector; a drag inside a page never touches the shell and is the page's own.
Radix layers and the Recent cycle mount as children of the body, where the sync watches for them.
While a layer that can move with its anchor (a popover, a menu) is drawn over a page the sync follows it every frame.
No column is a layer: File Views, where pages sit, and Tools are docked side by side (PRD three-column-panel D-11), so a page is never under Tools and needs no still for it; a column turned off or out of a narrow body's one column unmounts its slots, which hides their pages without closing them.

The idle still is one capture per shown page of the front Workspace, kept with the page's address and its slot's size; a still of another address or size is never drawn.
It is taken once a page has been quiet for half a second (`STILL_QUIET_MS`) after its load finished, its address changed, its slot changed size, or it came into view, and only while the page is shown, uncovered, loaded and not failed; nothing else asks, so an idle page is not captured again, and there is no timer per frame or per tick.
At most one capture is in flight per page, the stills are dropped with their display, when their page leaves the screen, and when the front Workspace changes, so the cache never holds more stills than pages on screen.
Each capture is one `capturePage` and PNG encode in the main process.

Tooltips never freeze a page, since they open and close too often.
A tooltip measures itself as it mounts, before Radix places it, and opens on the first side that fits the window and meets no shown page, trying its own side, the opposite one, then the two across (`web/src/tooltipSide.ts`), the shell's rectangles of the pages the last sync showed being the ones it avoids (`visiblePageRects`).
When every side that fits meets a page it keeps its own side and is drawn under the page; with no page shown, as in a plain browser tab, it keeps its own side.

## States

The toolbar holds Back, Forward, Reload (Stop while the page loads) and the address, shown without a web scheme until it is focused, so a narrow area still shows the host.
A page that cannot load says so in its slot with the address and Chromium's reason, and Reload loads its address again.
A page whose renderer stopped says the same with the reason.
In a plain browser tab a browser display shows its address on the toolbar row and reads `Pages open in the hide desktop app.` below it, and a web address gets Open in browser, which opens it in a new tab.

## Verification

```sh
bash scripts/verify-cargo.sh test
pnpm --dir web test
pnpm --dir desktop test
pnpm --dir desktop e2e
```

`desktop/e2e/browser.spec.ts` drives a desktop app it launched itself against a private hided and an isolated Herdr, never the operator's.
It opens a page with `hide browser open`, checks the native view sits on its slot, posts a sign-in popup's result back to its opener, holds a hostile popup's window features to a framed, closable window on the work area, caps popups and closes them with their page, asks before a navigation, a redirect and a frame hand a link to another app (with macOS's app lookup, sheet and opener stood in for), opens it only on Open and asks nothing more after a Cancel until the page navigates, opens a shift-clicked link as a display beside its page, which stays in view, refuses a popup and an app link from a hidden page, zooms a focused page with the text-size commands and ⌘+ without moving any text size, pinches a page before and after it moves to another renderer, types Hangul into the page, splits and resizes without a reload, freezes the pages under the palette, the Recent Panels list and a divider drag, hides a page with the palette's first frames while the host's capture is held back and turns its still to what the page shows now, hides a page when a narrow window calls Tools into its one column and puts it back on its slot when File Views is called again, opens a page tab's wrapped hint beside the tab rather than over the page, navigates and goes back, shows a failed load, opens an HTML file from the Explorer, refuses a file outside the checkout, ends a closed page's renderer, restores the page after a relaunch, and shows the notice in a plain browser tab.
The test window never activates the app or takes the keyboard, because the e2e fixture launches every app with `--hide-show-inactive` and `--disable-backgrounding-occluded-windows` (see [BUILD.md](BUILD.md#the-desktop-app)); it is shown behind the operator's windows, keeps painting there, and captures of it are taken by window id.
The zoom test needs the key window: it is tagged `@needs-focus` and brings its window to the front itself.
Keep screenshots and logs under local-only `agents/runs/`.
`desktop/e2e/browser-cdp.spec.ts` exercises the scoped native gateway against its own candidate, including pre-attachment native file-request cancellation and persistent download cancellation after client disconnect.
Its iframe proof obtains the cross-site iframe's frame-owner ID directly from the candidate WebContents' native DOM, independently of the scoped gateway, and requires both the scoped child frame ID and attached target ID to equal it.
Chromium's `Page.getFrameTree` includes local children only, so an out-of-process child's identity cannot be inferred from the parent session's frame tree.
The proof retains exact parent identity, native frame parentage, distinct renderer processes and child-session runtime execution, then retires the child and requires stale commands to fail while the parent remains usable.
A same-renderer cross-site frame fails that OOPIF proof instead of substituting an in-process interaction or forcing launch flags.
JavaScript and HTTP redirect file attempts must leave native file content unreadable and permit the authorized HTTP page to remain usable; a redirect must reject its navigation caller.
Each JavaScript attempt must return its completion marker without an exception, so an empty result or unrelated script failure cannot satisfy the denial.
The redirect proof independently requires the fixture's single permitted redirect response to finish with status 302 and the forbidden file location, so an unrelated transport failure cannot satisfy the denial.
Chromium may preempt these attempts before the host receives them, so each diagnostic category is capped at one rather than required for those attempts.
The separate app-owned pre-first-load, post-disconnect and native iframe file attempts must each prove guard-caused cancellation with exactly one refusal diagnostic per generation and no file content.
Each run stores its iframe provenance in a new private evidence directory, preserving earlier results.
Its download witness confines the candidate app and owned page session to a private sink before every attempt, observes production cancellation on every native callback, and requires one refusal diagnostic and an empty sink.
`web/e2e/browser-agent-assets.spec.ts` runs the agent page assets in plain Chromium on chromux's own cases: masking, state suffixes, stable refs, `--diff`, `--grep`, clickable divs, covering overlays, frame and shadow reach, fill, the refusals of hidden, covered and stale targets, `click --text`, and `changed` ignoring a scroll.
`desktop/e2e/browser-agent.spec.ts` runs `hide browser` from an isolated pane against its own candidate: snapshot, fill and click in a page and its cross-site frame, `--diff`, the overlay, console, network, eval and screenshot, a dialog that holds the display until it is answered, a cross-site frame that never answers, a selected View of a Workspace behind another that still takes input and screenshots, a press that holds the page past a step, and busy, hidden, file and missing displays.
`hided/src/browser_page/fake_gateway.rs` is the scripted gateway the frame, drag release and wait rules of `page.rs` and `actions.rs` are tested against, with a step and a hold for unanswered commands they take as values rather than the production eight and ten seconds; `recording` models the gateway's pending cap and the requests it saw, so the silence budget and the cap are asserted by counts (how many `Page.enable` a frame got, the most commands pending at once), never by elapsed time; a frame there can answer a kind of request only once every frame has asked, which proves requests go out together by order, not by time, and `desktop/src/main/browserCdp.test.ts` tests the drag refusal.
The spec also replays a native HTML5 drag through the gateway and sees the page receive its data and no files.
Playwright dismisses a dialog no listener handles, so that test listens and answers it the way the operator would.
`desktop/e2e/remote-browser-agent.spec.ts` runs the same snapshot and click from a device pane over the isolated SSH server, and the Workspace commands' `renderer_unavailable` once the desktop is gone.
`desktop/e2e/remote-workspace.spec.ts` additionally uses an isolated SSH server, whose sessions get the private HOME `desktop/e2e/device-home.ts` proves because connecting installs Hide's kit there, and two private Herdr servers to prove remote CLI origin, HTTP and WebSocket forwarding, absolute loopback subrequests, local and remote cookie separation, remote popup address ownership, relative HTML assets, refusal of undeclared files and external requests, explicit reveal, background View cleanup, route cleanup on close and forced candidate exit, and a return route that comes back after the device's helper connection ends and reconnects, with every remote command run through the `hide` Hide installed and linked on the device.

## Running Workspace servers

The Workspace toolbar’s globe, `Open server`, left of the File Views and Tools icons, reuses the existing cwd-attributed pane listener projection.
The projection retains each bind address as well as its port, mapping wildcard listeners to loopback of the same address family.
The current device, checkout ID, path and endpoint are checked again when a choice is made; identical paths on a remote Project never authorize a local listener.
One endpoint uses the existing explicit Workspace `browser_open` route directly, and multiple endpoints use the shared popover picker.
Discovery identifies TCP listeners, so successful discovery does not prove an HTTP page loads; native verification must observe the actual Browser View loading the chosen IPv4/IPv6 URL and its connection-error recovery.
The action starts no server and adds no duplicate context footer.
