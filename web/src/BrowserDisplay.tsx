import { ArrowLeftIcon, ArrowRightIcon, RotateCwIcon, XIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { AreaEmpty } from "./AreaEmpty";
import { addressShown, addressUrl, hostKey, notePageState, parseWorkspaceKey, registerBrowserSlot, stateReport, syncBrowserFront, useBrowserStore, withoutClosed } from "./browserViews";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Hint } from "./components/ui/tooltip";
import { browserBridge, type BrowserCommand } from "./host";
import type { ViewDisplaySnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { areasOf, locateDisplay, workspaceKey, type ViewWorkspace } from "./viewLayout";
import { workspaceViewOf } from "./workspace";

// A browser display (issue 155): its toolbar and the place its page shows.
// In the desktop app the page is a native view the host lays over the slot
// below the toolbar; a plain browser tab cannot draw one there, and says so
// with a way to open the address in a tab of its own.

export function BrowserDisplay({ display, workspace, actions }: { display: ViewDisplaySnapshot; workspace: ViewWorkspace; actions: Actions }) {
  const bridge = browserBridge();
  const url = display.url ?? "";
  if (!bridge) {
    const web = /^https?:/i.test(url);
    return (
      <AreaEmpty state="browser-host" text={`Pages open in the hide desktop app. ${url}`}>
        {web ? (
          <Button variant="secondary" data-browser-open-external={display.id} onClick={() => window.open(url, "_blank", "noopener,noreferrer")}>
            Open in browser
          </Button>
        ) : null}
      </AreaEmpty>
    );
  }
  return <HostedPage display={display} workspace={workspaceKey(workspace)} actions={actions} />;
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
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" data-browser-display={display.id}>
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
function AddressField({ address, onSubmit }: { address: string; onSubmit: (url: string) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <Input
      mono
      className="h-(--size-control-sm) flex-1 text-caption"
      aria-label="Page address"
      spellCheck={false}
      autoComplete="off"
      value={draft ?? addressShown(address)}
      data-browser-address="true"
      onFocus={(event) => {
        const field = event.currentTarget;
        setDraft(address);
        requestAnimationFrame(() => field.select());
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
  const front = view ? workspaceKey({ device_id: view.device_id, path: view.path }) : null;
  const layout = view?.layout ?? null;
  useEffect(() => {
    syncBrowserFront(front ? parseWorkspaceKey(front) : null, layout);
  }, [front, layout]);

  const latest = useRef(actions);
  latest.current = actions;
  // The last report per page still waiting for the core's echo, so it is not sent twice.
  const sent = useRef<Readonly<Record<string, { url: string; title: string }>>>({});
  const record = () => {
    const current = workspaceViewOf(useShellStore.getState().rest);
    if (!current?.layout) return;
    const workspace = { device_id: current.device_id, path: current.path };
    const key = workspaceKey(workspace);
    const pages = useBrowserStore.getState().pages;
    const shown = new Set<string>();
    for (const area of areasOf(current.layout.root)) {
      for (const display of area.displays) {
        if (display.kind === "browser") shown.add(display.id);
        const page = display.kind === "browser" ? pages[hostKey(key, display.id)] : undefined;
        const next = page ? stateReport(display, page, sent.current[hostKey(key, display.id)] ?? null) : null;
        if (!next) continue;
        sent.current = { ...sent.current, [hostKey(key, display.id)]: next };
        latest.current.reportBrowserState(workspace, display.id, next.url, next.title);
      }
    }
    sent.current = withoutClosed(sent.current, key, shown);
  };
  // A page of a Workspace that was not in front reports once it is.
  useEffect(record, [front, layout]);

  useEffect(() => {
    const bridge = browserBridge();
    if (!bridge) return undefined;
    return bridge.onEvent((event) => {
      if (event.kind === "state") {
        notePageState(event);
        record();
      } else if (event.kind === "open") {
        const workspace = parseWorkspaceKey(event.workspace);
        if (workspace) latest.current.openBrowser(event.url, workspace);
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
