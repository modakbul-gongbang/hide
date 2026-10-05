import { GlobeIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import type { Checkout } from "./snapshot";
import { useShellStore } from "./store";
import { useInterfaceTranslation } from "./i18n/client";
import type { WorkspaceView } from "./workspace";

/** Listener ports are already attributed below each pane's cwd by the core. */
export function runningServers(checkout: Checkout): { host: string; port: number }[] {
  const endpoints = checkout.tabs.flatMap((tab) => tab.panes.flatMap((pane) => pane.servers ?? []));
  return [...new Map(endpoints.map((endpoint) => [serverAddress(endpoint), endpoint])).values()].sort((a, b) => a.port - b.port || a.host.localeCompare(b.host));
}

function serverAddress(server: {host: string; port: number}): string {
  return `${server.host.includes(":") ? `[${server.host}]` : server.host}:${server.port}`;
}

export function RunningServers({ checkout, view, actions }: { checkout: Checkout; view: WorkspaceView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const live = useShellStore((s) => s.connection === "live");
  const discovery = useShellStore((s) => s.rest?.status?.server_discovery);
  const [open, setOpen] = useState(false);
  const [gone, setGone] = useState(false);
  const ports = runningServers(checkout);
  const unavailable = view.device_id !== "local" ? t("documents.serversUnavailable") : !live ? t("documents.serversWaiting") : discovery?.loading ? t("documents.serversReading") : discovery?.failure;
  const choose = (server: {host: string; port: number}) => {
    // A picker can outlive a port sample or Workspace. Read the current
    // catalog, route explicitly, and never substitute another Workspace.
    const state = useShellStore.getState();
    const current = state.rest?.navigator?.workspaces?.filter((project) => project.device_id === view.device_id).flatMap((project) => project.checkouts).find((row) => row.id === checkout.id && row.path === view.path);
    if (!current || !runningServers(current).some((row) => serverAddress(row) === serverAddress(server)) || state.connection !== "live") {
      setGone(true);
      setOpen(true);
      return;
    }
    actions.openBrowser(`http://${serverAddress(server)}`, { device_id: view.device_id, path: view.path });
    setOpen(false);
    setGone(false);
  };
  return (
    <Popover open={open} onOpenChange={(next) => {
      if (next && !unavailable && ports.length === 1 && ports[0] !== undefined) { choose(ports[0]); return; }
      setOpen(next);
      if (next) setGone(false);
    }}>
      <Hint label={t("documents.openServer")}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={t("documents.openServer")} data-open-server="true"><GlobeIcon /></Button>
        </PopoverTrigger>
      </Hint>
      <PopoverContent align="end" aria-label={t("documents.runningServers")} className="flex flex-col gap-xs" onOpenAutoFocus={(event) => {
        const first = document.querySelector<HTMLButtonElement>("[data-server-port]");
        if (first) { event.preventDefault(); first.focus(); }
      }} onKeyDown={(event) => {
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-server-port]")];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        if (buttons[next]) { event.preventDefault(); buttons[next]?.focus(); }
      }}>
        <p className="text-caption font-medium">{t("documents.runningServers")}</p>
        {gone || unavailable ? <p role="status" className="text-caption text-muted-foreground">{gone ? t("documents.serverGone") : unavailable}</p> : ports.length === 0 ? <p role="status" className="text-caption text-muted-foreground">{t("documents.noServers")}</p> : ports.map((server) => (
          <Button key={serverAddress(server)} variant="ghost" className="justify-start" data-server-port={server.port} onClick={() => choose(server)}>
            <GlobeIcon /><span className="truncate font-mono text-caption">{serverAddress(server)}</span>
          </Button>
        ))}
      </PopoverContent>
    </Popover>
  );
}
