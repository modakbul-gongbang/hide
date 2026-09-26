# Browser displays

A web page in hide is a display in a View area, beside the files and diffs of the same Workspace (issue 155).
It is a tab like any other view: it splits, moves, closes, and comes back after a relaunch with the rest of the layout.
The page itself is drawn by the desktop app, a native Chromium view laid over the area's slot; a plain browser tab that serves the shell cannot draw one and says so.
There is no Herdr browser pane and no chromux profile behind it, and nothing is screencast.

## Who owns what

| Owner | Holds | Code |
| --- | --- | --- |
| The core | Which browser displays exist, where they sit in the tree, the address each was asked to load and a load stamp, and the address and title the page last reported | `herdr-core/src/view_layout.rs` (`DisplayKind::Browser`, `browser_address`, `browser_title`), `herdr-core/src/runtime/view_areas.rs` (`open_browser`, `place_browser`, `navigate_browser`, `record_browser_state`) |
| hided | The `file:` address boundary, and `hide browser open` | `hided/src/server.rs` (`browser_url`), `hided/src/file_url.rs`, `hided/src/browser_cli.rs` |
| The web shell | Where each page sits, since only it has the geometry, the toolbar, the overlay freeze, and the notice in a plain browser tab | `web/src/BrowserDisplay.tsx`, `web/src/browserViews.ts`, `web/src/host.ts` |
| The desktop app | The pages: one `WebContentsView` per display it was asked to show, their navigation, and their lifetime | `desktop/src/main/browser.ts`, `desktop/src/main/browserSync.ts`, `desktop/src/preload/index.ts` |

The core never loads a page and the desktop app never decides which displays exist.
A display's `load` stamp is how the core asks for a load: the host loads the display's address again whenever the stamp is newer than the one it last applied, so opening an address the Workspace already shows focuses that display and loads it again rather than adding a second one.
The stamp is not saved; a relaunched page loads its address once when it is first shown.

## Opening a page

- `hide browser open <url-or-path> [--pane <id>]` from a terminal.
  An argument with a scheme is used as written, an existing file or folder becomes its `file:` URL, a loopback host such as `localhost:3000` gets `http`, and any other host gets `https`.
  The page opens in the Workspace of the pane that asked (`--pane`, else `HERDR_PANE_ID`), else in the Workspace in front.
  The CLI prints one JSON line, the core's receipt (`ok`, `display_id`, `device_id`, `path`) or the refusal, and exits 2 on anything but `ok`, like every other `hide` command.
  It only attaches: with no running hide it fails rather than starting one.
- Open in Browser in the Explorer's menu on an HTML file of this Mac's checkout.
  On a device's file it stays listed, disabled, with its reason, because pages load on this Mac.
- A page that asks for a new window gets another browser display in the same Workspace.
- The address field in the display's toolbar loads what was typed into that display.

A display holds only an `http`, `https` or `file` address with something after the `//`, or `about:blank`, within 8 KiB; anything else is refused with its reason and nothing changes.
A `file:` address is refused on a device's Workspace, since the page would load a file of this Mac.

## The file boundary

A client names a local file through a `file:` URL, so hided checks it like any path a client sends, on `browser_open`, `browser_state` and the `view_layout` `navigate` action.
It decodes the path, refuses one that names another host or does not decode (`invalid_path`), runs it through the same checkout boundary the Explorer uses (`outside_checkout` for a file outside every registered checkout), and writes the checked path back as the one spelling the shell and the CLI also produce.
A refusal is a `path_refused` frame to that client and never reaches the core.

A page the operator loaded can still follow its own links.
A `file:` page that moves to a file outside the checkouts keeps showing it in its view, but its report is refused at the boundary, so the core keeps the last address it accepted and a relaunch opens that one.

## The page and its limits

Pages run in one persistent session partition, `persist:hide-browser`, apart from the shell's own session, with a sandboxed renderer, context isolation, no Node, and no preload, so nothing in a page reaches the `hideHost` bridge or the daemon's token.
A page gets no permission but writing the clipboard, because a prompt it would raise has nowhere to show; a download follows Chromium's default and is logged as `browser.download`.
A page may navigate to `http`, `https`, `file` and `about:blank`; a `mailto:` link goes to the default mail app, and anything else is refused and logged with its scheme only.

The shell tells the host, in one sync, every browser display of the Workspace in front and the rectangle its slot occupies now.
A display the sync no longer names is closed, which ends its renderer process; a display of a Workspace not in front, or not shown in its area, is hidden and keeps its page, so moving a page between areas or Workspaces never reloads it.
At most 12 pages live at once (`MAX_LIVE_VIEWS`); past that, the page shown least recently among the hidden ones is closed and loads its address again when it is next shown.
The host checks every field of a sync before it places anything and drops a message that fails whole; only the shell the daemon serves may send one.

## Overlays

A native view is drawn above the page's HTML, so the shell cannot draw over it.
When the palette, a menu, a dialog, a popover or a dragged tab meets a page's rectangle, the shell asks the host for a capture of the page, draws it in the page's place, and hides the page until the overlay is gone.
Tooltips are left alone, so a tooltip over a page is drawn under it.

## States

The toolbar holds Back, Forward, Reload (Stop while the page loads) and the address, shown without a web scheme until it is focused, so a narrow area still shows the host.
A page that cannot load says so in its slot with the address and Chromium's reason, and Reload loads its address again.
A page whose renderer stopped says the same with the reason.
In a plain browser tab a browser display reads `Pages open in the hide desktop app.` with its address, and a web address gets Open in browser, which opens it in a new tab.

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
