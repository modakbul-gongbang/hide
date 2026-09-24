import { useMemo } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/controls";
import { RemotePaneCanvas } from "./PaneGrid";
import { remoteView } from "./remote";
import { canRetryDevice, deviceLine } from "./settings";
import { focusedRemoteDevice } from "./snapshot";
import { useShellStore } from "./store";
import { RemoteTabBar } from "./TabBar";

// The center of the shell while an SSH device is selected (PRD S5 B19): that
// host's visible tab, drawn from the session the core projected over SSH, or
// the connection's own state when there is no session to draw. Nothing here
// reads this machine's tabs, so a remote screen cannot act on a local pane.

export function RemoteSurface({ actions }: { actions: Actions }) {
  const device = useShellStore((s) => focusedRemoteDevice(s.rest));
  const status = useShellStore((s) => (device ? (s.rest?.status?.remote?.find((row) => row.target_id === device.id) ?? null) : null));
  const session = status?.session ?? null;
  const view = useMemo(() => remoteView(session), [session]);
  if (!device) return null;
  const connected = status?.state === "connected";
  if (!view) {
    const line = deviceLine(device, status ?? undefined);
    return (
      <div className="flex min-h-0 flex-1 flex-col items-start justify-center gap-sm p-xl text-body text-secondary" data-remote-device-surface={device.id} data-remote-state={status?.state ?? "none"}>
        <h2 className="text-title font-semibold text-primary">
          {device.label} <span className="font-mono text-caption text-muted">{device.ssh_alias}</span>
        </h2>
        <p>
          {connected
            ? `${device.label} is connected, but no Herdr workspace is open there.`
            : `${device.label} is ${line.text}${status?.message ? `: ${status.message}` : "."}`}
        </p>
        <div className="flex gap-sm">
          {canRetryDevice(device, status ?? undefined) ? (
            <Button onClick={() => actions.retryDevice(device.id)} data-remote-retry={device.id}>
              Retry
            </Button>
          ) : null}
          <Button appearance="quiet" onClick={() => actions.focusDevice("local")} data-use-local-device="true">
            Show this machine
          </Button>
        </div>
      </div>
    );
  }
  return (
    <>
      {connected ? null : (
        // A lost connection keeps the last session on screen, but the host
        // takes no command until it is back (`remote.control.not_connected`).
        <div role="status" className="flex items-center gap-md border-b border-divider bg-panel px-md py-xs text-caption text-warning" data-remote-stale={device.id}>
          <span className="min-w-0 flex-1 truncate" title={status?.message ?? undefined}>
            {device.label} is not connected{status?.message ? `: ${status.message}` : ""}. Showing the last state it reported; nothing is sent until it reconnects.
          </span>
          {canRetryDevice(device, status ?? undefined) ? (
            <Button onClick={() => actions.retryDevice(device.id)} data-remote-retry={device.id}>
              Retry
            </Button>
          ) : null}
        </div>
      )}
      <RemoteTabBar view={view} actions={actions} />
      <RemotePaneCanvas view={view} connected={connected} dispatch={actions.dispatch} onClosePane={(paneId) => actions.closePane(paneId)} />
    </>
  );
}
