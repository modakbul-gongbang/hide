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

## The file boundary

A client names a local file through a `file:` URL, so hided checks it like any path a client sends, on `browser_open`, `browser_state` and the `view_layout` `navigate` action.
It decodes the path, refuses one that names another host or does not decode (`invalid_path`), runs it through the same checkout boundary the Explorer uses (`outside_checkout` for a file outside every registered checkout), and writes the checked path back as the one spelling the shell and the CLI also produce.
A refusal is a `path_refused` frame to that client and never reaches the core.

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
After a blur cancels a hold, the window's next focus gives the keyboard back to the initiating page if it is still shown, as Escape does.
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
When the palette, a menu, a dialog, a popover, a focused-area, Global Recent Panels or Recent Projects list, or the Tools overlay of a narrow window meets a page's rectangle, the shell hides the page in the same sync that first sees the overlay, so no frame shows the page over it, and draws the page's idle still in its place (below); with no still of the page's address and slot size yet, the place stays blank until one arrives.
In that same flush, ahead of the sync that hides the page, the shell asks the host for a fresh capture; the host answers only for a page it still shows (`browser.capture_refused` otherwise), and IPC from one renderer is handled in order, so the capture is taken of the page as it is now and replaces the still and the cached one when it arrives.
A capture that fails or comes back empty leaves the place blank or the older still, draws no message, and is logged (`browser.capture_failed`, `browser.capture_empty`).
The page comes back live when the overlay is gone, and its still goes a frame later, so nothing flashes between the two.
A shell drag (a tab, a divider, the side panel's edge, an Explorer item) marks the document root with `data-view-drag` or `data-agent-drag` while it runs (`web/src/shellDrag.ts`), and every page of the front Workspace freezes the same way until the mark is gone, so the drag's guide or preview draws over the pages and its drop never reaches one.
A new shell drag sets that mark rather than adding its guide to the overlay selector; a drag inside a page never touches the shell and is the page's own.
Radix layers and the Recent cycle mount as children of the body, where the sync watches for them; the Tools overlay mounts deep in the tree, so it tells the sync itself when it opens and closes (`noteShellLayer`).
While a layer that can move with its anchor (a popover, a menu) is drawn over a page the sync follows it every frame; the Tools overlay stays where it opened and does not keep the sync running.

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
It opens a page with `hide browser open`, checks the native view sits on its slot, posts a sign-in popup's result back to its opener, holds a hostile popup's window features to a framed, closable window on the work area, caps popups and closes them with their page, asks before a navigation, a redirect and a frame hand a link to another app (with macOS's app lookup, sheet and opener stood in for), opens it only on Open and asks nothing more after a Cancel until the page navigates, opens a shift-clicked link as a display beside its page, which stays in view, refuses a popup and an app link from a hidden page, zooms a focused page with the text-size commands and ⌘+ without moving any text size, pinches a page before and after it moves to another renderer, types Hangul into the page, splits and resizes without a reload, freezes the pages under the palette, the Recent Panels list and a divider drag, hides a page with the palette's first frames while the host's capture is held back and turns its still to what the page shows now, freezes a page under a narrow window's Tools overlay and gives the keyboard back to its toggle, opens a page tab's wrapped hint beside the tab rather than over the page, navigates and goes back, shows a failed load, opens an HTML file from the Explorer, refuses a file outside the checkout, ends a closed page's renderer, restores the page after a relaunch, and shows the notice in a plain browser tab.
The test window never activates the app or takes the keyboard, because the e2e fixture launches every app with `--hide-show-inactive` and `--disable-backgrounding-occluded-windows` (see [BUILD.md](BUILD.md#the-desktop-app)); it is shown behind the operator's windows, keeps painting there, and captures of it are taken by window id.
The zoom test needs the key window: it is tagged `@needs-focus` and brings its window to the front itself.
Keep screenshots and logs under local-only `agents/runs/`.
`desktop/e2e/remote-workspace.spec.ts` additionally uses an isolated SSH server, whose sessions get the private HOME `desktop/e2e/device-home.ts` proves because connecting installs Hide's kit there, and two private Herdr servers to prove remote CLI origin, HTTP and WebSocket forwarding, absolute loopback subrequests, local and remote cookie separation, remote popup address ownership, relative HTML assets, refusal of undeclared files and external requests, explicit reveal, background View cleanup, route cleanup on close and forced candidate exit, and a return route that comes back after the device's helper connection ends and reconnects, with every remote command run through the `hide` Hide installed and linked on the device.

## Running Workspace servers

The Workspace toolbar’s globe reuses the existing cwd-attributed pane listener projection.
The projection retains each bind address as well as its port, mapping wildcard listeners to loopback of the same address family.
The current device, checkout ID, path and endpoint are checked again when a choice is made; identical paths on a remote Project never authorize a local listener.
One endpoint uses the existing explicit Workspace `browser_open` route directly, and multiple endpoints use the shared popover picker.
Discovery identifies TCP listeners, so successful discovery does not prove an HTTP page loads; native verification must observe the actual Browser View loading the chosen IPv4/IPv6 URL and its connection-error recovery.
The action starts no server and adds no duplicate context footer.
