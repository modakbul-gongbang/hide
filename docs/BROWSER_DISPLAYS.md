# Browser displays

A web page in hide is a display in a View area, beside the files and diffs of the same Workspace (issue 155).
It is a tab like any other view: it splits, moves, closes, and comes back after a relaunch with the rest of the layout.
The page itself is drawn by the desktop app, a native Chromium view laid over the area's slot; a plain browser tab that serves the shell cannot draw one and says so.
There is no Herdr browser pane and no chromux profile behind it, and nothing is screencast.

## Who owns what

| Owner | Holds | Code |
| --- | --- | --- |
| The core | Browser View layout, requested address and load stamp, and the native page's reported loading state | `herdr-core/src/view_layout.rs`, `herdr-core/src/runtime/view_areas.rs`, `herdr-core/src/runtime/workspace_control.rs` |
| hided | The local `file:` event boundary, pane-scoped Browser CLI transport, and native routes into consented SSH devices | `hided/src/server.rs`, `hided/src/file_url.rs`, `hided/src/workspace_cli.rs`, `hided/src/browser_routes.rs`, `hided/src/browser_assets.rs` |
| The web shell | Where each page sits, since only it has the geometry, the toolbar, the overlay freeze, and the notice in a plain browser tab | `web/src/BrowserDisplay.tsx`, `web/src/browserViews.ts`, `web/src/host.ts` |
| The desktop app | The pages: one `WebContentsView` per display it was asked to show, their navigation, route requests, and their lifetime | `desktop/src/main/browser.ts`, `desktop/src/main/browserSync.ts`, `desktop/src/main/host.ts`, `desktop/src/preload/index.ts` |

The core never loads a page and the desktop app never decides which displays exist.
The core's Browser View inventory is sent as an empty array when the last page closes, so the native host removes that page even when no other Browser View remains.
A display's `load` stamp is how the core asks for a load: the host loads the display's address again whenever the stamp is newer than the one it last applied, so opening an address the Workspace already shows focuses that display and loads it again rather than adding a second one.
The stamp is not saved; a relaunched page loads its address once when it is first shown.

## Opening a page

- `hide browser open <url-or-path> [--reveal] [--wait] [--request-id <id>]` from a connected Herdr pane in the desktop app.
  The CLI resolves a relative file on the calling machine, gives loopback hosts `http` and other hosts `https`, and submits one pane-scoped action with no Workspace override.
  The core checks an HTML file against that pane's checkout on its own device and reads it outside the core lock before placing its Browser View.
  The action result confirms View placement, while `hide view status <view-id>` returns `pending`, `loading`, `loaded`, `failed`, `disconnected`, or `unsupported` for its native page.
  `disconnected` means a native page existed for that load but was removed or its desktop renderer disconnected; `unsupported` means no desktop renderer was available for a pending page.
  `--wait` polls that state for at most ten seconds and returns a failure if the page fails, disconnects, or remains pending; it does not reveal a hidden View.
  If a later status query fails, the refusal still carries the applied open request ID and View ID for reconciliation.
  The action receipt and page status carry the load stamp, so a concurrent reopen that supersedes the requested load returns `page_superseded` instead of the newer load's result.
  `--reveal` explicitly brings the caller's checkout and selected Browser View forward, including a connected SSH device.
  The CLI returns one JSON line, exits nonzero on refusal, and never starts Hide.
- Open in Browser in the Explorer's menu on an HTML file of a local or connected device checkout.
  The device host must have file access consent for a remote page to load.
- A page that asks for a new window gets another browser display in the same Workspace.
  A remote page's routed loopback address is translated back to its source device address before that request reaches the core.
- The address field in the display's toolbar loads what was typed into that display.

A display holds only an `http`, `https` or `file` address with something after the `//`, or `about:blank`, within 8 KiB; anything else is refused with its reason and nothing changes.
A remote `file:` address is loaded through a checkout-scoped route, never as a file on this Mac.

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

Pages run in persistent session partitions separated by Workspace and by web versus file preview, apart from the shell's own session, with a sandboxed renderer, context isolation, no Node, and no preload, so nothing in a page reaches the `hideHost` bridge or the daemon's token.
This separation keeps localhost cookies of a local page, an SSH device, and a remote HTML preview from crossing those boundaries even when their host names are equal.
A page gets no permission but writing the clipboard, because a prompt it would raise has nowhere to show; a download follows Chromium's default and is logged as `browser.download`.
A page may navigate to `http`, `https`, `file` and `about:blank`; a `mailto:` link goes to the default mail app, and anything else is refused and logged with its scheme only.

The shell tells the host, in one sync, every browser display of the Workspace in front, the rectangle its slot occupies now, and the core's retained Browser View inventory across all Workspaces.
A display the inventory no longer names is closed, which ends its renderer process even when its Workspace is in the background.
A retained display of a Workspace not in front, or not shown in its area, is hidden and keeps its page, so moving a page between areas or Workspaces never reloads it.
At most 12 pages live at once (`MAX_LIVE_VIEWS`); past that, the page shown least recently among the hidden ones is closed and loads its address again when it is next shown.
The host checks every field of a sync before it places anything and drops a message that fails whole; only the shell the daemon serves may send one.
Native page reports carry the display's current load stamp, so a late report from a previous reload cannot mark the new request loaded.
An evicted page reports its disappearance as disconnected until shown again.

## Overlays

A native view is drawn above the page's HTML, so the shell cannot draw over it.
When the palette, a menu, a dialog, a popover or a dragged tab meets a page's rectangle, the shell asks the host for a capture of the page, draws it in the page's place, and hides the page until the overlay is gone.
Tooltips are left alone, so a tooltip over a page is drawn under it.

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
It opens a page with `hide browser open`, checks the native view sits on its slot, types Hangul into the page, splits and resizes without a reload, freezes the pages under the palette, navigates and goes back, shows a failed load, opens an HTML file from the Explorer, refuses a file outside the checkout, ends a closed page's renderer, restores the page after a relaunch, and shows the notice in a plain browser tab.
The test window sits behind the operator's windows, so it launches with `--disable-backgrounding-occluded-windows`, and captures of it are taken by window id.
Keep screenshots and logs under local-only `agents/runs/`.
`desktop/e2e/remote-workspace.spec.ts` additionally uses an isolated SSH server and two private Herdr servers to prove remote CLI origin, HTTP and WebSocket forwarding, absolute loopback subrequests, local and remote cookie separation, remote popup address ownership, relative HTML assets, refusal of undeclared files and external requests, explicit reveal, background View cleanup, and route cleanup on close and forced candidate exit.
