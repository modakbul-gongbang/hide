import { ArrowLeftIcon, ArrowRightIcon, RotateCwIcon, XIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AreaEmpty } from "./AreaEmpty";
import { addressShown, addressUrl, hostKey, notePageState, parseWorkspaceKey, registerBrowserSlot, syncBrowserFront, useBrowserStore } from "./browserViews";
import { NewTabBody } from "./components/new-tab-body";
import { changedFiles } from "./newTab";
import { useUiStore } from "./ui";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Hint } from "./components/ui/tooltip";
import { browserBridge, type BrowserCommand } from "./host";
import type { ViewDisplaySnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { locateDisplay, workspaceKey, type ViewWorkspace } from "./viewLayout";
import { workspaceViewOf } from "./workspace";
import { commandLabel } from "./shortcutLabels";

// A browser display (issue 155): its toolbar and the place its page shows.
// In the desktop app the page is a native view the host lays over the slot
// below the toolbar; a plain browser tab cannot draw one there, and says so
// with a way to open the address in a tab of its own. Either way the toolbar
// row carries the address, so the row lines up with a document header beside
// it (issue 170).

export function BrowserDisplay({ display, workspace, actions }: { display: ViewDisplaySnapshot; workspace: ViewWorkspace; actions: Actions }) {
  const bridge = browserBridge();
  const url = display.url ?? "";
  if (!url) return <NewTab display={display} actions={actions} />;
  if (!bridge) {
    const web = /^https?:/i.test(url);
    return (
      <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-browser-display={display.id}>
        <div className="flex h-(--size-tab-strip) shrink-0 items-center border-b border-border px-md" data-browser-toolbar={display.id}>
          <span className="min-w-0 flex-1 truncate font-mono text-caption text-subtle-foreground" data-browser-address="true">
            {addressShown(url)}
          </span>
        </div>
        <AreaEmpty state="browser-host" text="Pages open in the hide desktop app.">
          {web ? (
            <Button variant="secondary" data-browser-open-external={display.id} onClick={() => window.open(url, "_blank", "noopener,noreferrer")}>
              Open in browser
            </Button>
          ) : null}
        </AreaEmpty>
      </div>
    );
  }
  return <HostedPage display={display} workspace={workspaceKey(workspace)} actions={actions} />;
}

function NewTab({ display, actions }: { display: ViewDisplaySnapshot; actions: Actions }) {
  const root = useShellStore((s) => s.rest?.navigator?.changes_root_path ?? null);
  const changes = useShellStore((s) => s.changes);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-browser-display={display.id}>
      <div className="flex h-(--size-tab-strip) shrink-0 items-center gap-xs border-b border-border px-sm" data-browser-toolbar={display.id}>
        <Hint label="Back"><Button variant="ghost" size="icon-sm" disabled><ArrowLeftIcon /></Button></Hint>
        <Hint label="Forward"><Button variant="ghost" size="icon-sm" disabled><ArrowRightIcon /></Button></Hint>
        <Hint label="Reload"><Button variant="ghost" size="icon-sm" disabled><RotateCwIcon /></Button></Hint>
        <AddressField address="" autoFocus onSubmit={(url) => actions.navigateBrowser(display.id, url)} />
      </div>
      <NewTabBody hasChanges={changedFiles(changes, root).length > 0} fileChord={commandLabel("open_file")} onFile={() => actions.openFilePalette()} onDiff={() => useUiStore.getState().openOverlay("diff_palette")} />
    </div>
  );
}

function HostedPage({ display, workspace, actions }: { display: ViewDisplaySnapshot; workspace: string; actions: Actions }) {
  const page = useBrowserStore((s) => s.pages[hostKey(workspace, display.id)] ?? null);
  const still = useBrowserStore((s) => s.stills[display.id]);
  const slot = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = slot.current;
    return element ? registerBrowserSlot(display.id, element) : undefined;
  }, [display.id]);
  const command = (name: BrowserCommand) => browserBridge()?.command(workspace, display.id, name);
  const address = page?.url || display.url || "";
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-browser-display={display.id}>
      <div className="flex h-(--size-tab-strip) shrink-0 items-center gap-xs border-b border-border px-sm" data-browser-toolbar={display.id}>
        <Hint label="Back">
          <Button variant="ghost" size="icon-sm" disabled={!page?.canGoBack} onClick={() => command("back")} data-browser-command="back">
            <ArrowLeftIcon />
          </Button>
        </Hint>
        <Hint label="Forward">
          <Button variant="ghost" size="icon-sm" disabled={!page?.canGoForward} onClick={() => command("forward")} data-browser-command="forward">
            <ArrowRightIcon />
          </Button>
        </Hint>
        {page?.loading ? (
          <Hint label="Stop loading">
            <Button variant="ghost" size="icon-sm" onClick={() => command("stop")} data-browser-command="stop">
              <XIcon />
            </Button>
          </Hint>
        ) : (
          <Hint label="Reload">
            <Button variant="ghost" size="icon-sm" onClick={() => command("reload")} data-browser-command="reload">
              <RotateCwIcon />
            </Button>
          </Hint>
        )}
        <AddressField address={address} onSubmit={(url) => actions.navigateBrowser(display.id, url)} />
      </div>
      <div ref={slot} className="relative min-h-0 min-w-0 flex-1" data-browser-slot={display.id}>
        {page?.failure ? (
          <AreaEmpty state="browser-failed" text={`${address} could not be shown: ${page.failure}`}>
            <Button variant="secondary" onClick={() => command("reload")} data-browser-retry={display.id}>
              Reload
            </Button>
          </AreaEmpty>
        ) : still ? (
          <img src={still} alt="" aria-hidden="true" draggable={false} className="absolute inset-0 size-full select-none object-fill" data-browser-still={display.id} />
        ) : null}
      </div>
    </div>
  );
}

/**
 * The page's address, shown without a web scheme until it is edited so a
 * narrow area still shows the host; focusing it shows the whole address
 * selected. Return loads what was typed, Escape puts the page's address back.
 */
function AddressField({ address, onSubmit, autoFocus = false }: { address: string; onSubmit: (url: string) => void; autoFocus?: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  // Focus swaps the shown address for the whole one, which drops any
  // selection; selecting in the commit that swaps it means the first key
  // after focus replaces the address rather than landing after it.
  const selectOnCommit = useRef(false);
  useLayoutEffect(() => {
    if (!selectOnCommit.current) return;
    selectOnCommit.current = false;
    field.current?.select();
  });
  return (
    <Input
      ref={field}
      mono
      className="h-(--size-control-sm) flex-1 text-caption"
      aria-label="Page address"
      placeholder="Enter a URL"
      autoFocus={autoFocus}
      spellCheck={false}
      autoComplete="off"
      value={draft ?? addressShown(address)}
      data-browser-address="true"
      onFocus={() => {
        selectOnCommit.current = true;
        setDraft(address);
      }}
      onChange={(event) => setDraft(event.currentTarget.value)}
      onBlur={() => setDraft(null)}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Enter") {
          event.preventDefault();
          const url = addressUrl(draft ?? address);
          setDraft(null);
          if (url) onSubmit(url);
          event.currentTarget.blur();
        } else if (event.key === "Escape" && draft !== null && draft !== address) {
          event.preventDefault();
          event.stopPropagation();
          setDraft(address);
        }
      }}
    />
  );
}

/**
 * The page-side half of the host's browser views, mounted once: it tells the
 * host which browser displays the front Workspace holds, records what each
 * page says in the core (its address and title), turns a page's new window
 * into another browser display, and makes a clicked page's area the active one.
 */
export function BrowserHost({ actions }: { actions: Actions }) {
  const view = useShellStore((s) => workspaceViewOf(s.rest));
  const inventory = useShellStore((s) => s.rest?.browser_views);
  const front = view ? workspaceKey({ device_id: view.device_id, path: view.path }) : null;
  const layout = view?.layout ?? null;
  useEffect(() => {
    syncBrowserFront(front ? parseWorkspaceKey(front) : null, layout, inventory ?? []);
  }, [front, layout, inventory]);

  const latest = useRef(actions);
  latest.current = actions;
  useEffect(() => {
    const bridge = browserBridge();
    if (!bridge) return undefined;
    return bridge.onEvent((event) => {
      if (event.kind === "state") {
        notePageState(event);
        const workspace = parseWorkspaceKey(event.workspace);
        if (workspace) latest.current.reportBrowserState(workspace, event.id, event.state.url, event.state.title, event.load, event.state.loading, event.state.failure, true);
      } else if (event.kind === "gone") {
        const workspace = parseWorkspaceKey(event.workspace);
        if (workspace) latest.current.reportBrowserState(workspace, event.id, event.url, "", event.load, false, null, false);
      } else if (event.kind === "open") {
        const workspace = parseWorkspaceKey(event.workspace);
        if (workspace) latest.current.openBrowser(event.url, workspace, undefined, event.id);
      } else if (event.kind === "focus") {
        const current = workspaceViewOf(useShellStore.getState().rest);
        if (!current?.layout || workspaceKey({ device_id: current.device_id, path: current.path }) !== event.workspace) return;
        const located = locateDisplay(current.layout.root, event.id);
        if (located && (current.layout.active_area !== located.area.id || located.area.active !== event.id)) latest.current.focusView(event.id);
      }
    });
    // `record` reads everything through the stores and refs.
  }, []);
  return null;
}
