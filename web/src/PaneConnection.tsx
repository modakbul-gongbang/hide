import { TriangleAlertIcon } from "lucide-react";
import { useRef, useState } from "react";
import type { Actions } from "./actions";
import { badgeVariants } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { useInterfaceTranslation } from "./i18n/client";
import { connectionCopy, notConnected, offersReopen, offersSharedServerOff, reopenFailureKey, reopenPending, sharedServerOutcome } from "./paneConnectionRules";
import { remoteContext } from "./remote";
import type { PaneRow } from "./snapshot";
import { useShellStore } from "./store";

// The "Not connected" chip in a pane header and the popover that fixes it
// (PRD settings-cleanup D-11, D-12, B26 to B31). The core judges the
// connection (`PaneChildren.connection`) and owns what Reopen is doing, so
// the popover only shows the snapshot: the pending and failed states, and the
// result of turning off Codex's shared server, are read back, never assumed.
// A pane that is connected, or that has nothing to judge, draws nothing.

export function PaneConnectionChip({ pane, actions, local }: { pane: PaneRow; actions: Actions; local: boolean }) {
  const connection = notConnected(pane.children?.connection);
  if (!connection) return null;
  return <ConnectionPopover paneId={pane.id} connection={connection} actions={actions} local={local} />;
}

function ConnectionPopover({
  paneId,
  connection,
  actions,
  local,
}: {
  paneId: string;
  connection: NonNullable<ReturnType<typeof notConnected>>;
  actions: Actions;
  local: boolean;
}) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const [askedOff, setAskedOff] = useState(false);
  // One press is one event: a second press before the next snapshot would
  // otherwise send again, because the core's pending state has not arrived.
  const sentAgainst = useRef<unknown>(null);
  const rest = useShellStore((s) => s.rest);
  const device = local ? rest?.navigator?.devices?.find((row) => row.id === "local") : remoteContext(rest)?.device;
  const machineId = local ? "local" : (device?.id ?? null);
  const copy = connectionCopy(connection.reason);
  const pending = reopenPending(connection);
  const failure = reopenFailureKey(connection);
  const outcome = sharedServerOutcome(device?.kit?.codex_daemon_off, askedOff);
  const once = (send: () => void) => {
    if (sentAgainst.current === rest) return;
    sentAgainst.current = rest;
    send();
  };
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setAskedOff(false);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          className={`${badgeVariants({ variant: "outline" })} cursor-pointer gap-xxs text-warning outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring`}
          data-pane-connection={connection.reason}
          data-pane-connection-pane={paneId}
        >
          <TriangleAlertIcon aria-hidden="true" />
          {t("panes.connection.chip")}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        role="dialog"
        aria-label={t(copy.title)}
        className="flex flex-col gap-sm"
        data-pane-connection-popover={connection.reason}
      >
        <strong className="text-subhead text-foreground">{t(copy.title)}</strong>
        <p className="text-body text-muted-foreground">{t(copy.reason)}</p>
        <div className="flex items-center gap-sm">
          {offersReopen(connection) && copy.reopen ? (
            <Button size="sm" aria-busy={pending} disabled={pending} data-pane-reopen={paneId} onClick={() => once(() => actions.reopenPane(paneId))}>
              {pending ? t("panes.connection.reopening") : t(copy.reopen)}
            </Button>
          ) : null}
          <Button size="sm" variant="ghost" data-pane-connection-dismiss={paneId} onClick={() => setOpen(false)}>
            {t("panes.connection.notNow")}
          </Button>
        </div>
        {failure ? (
          <p className="text-caption text-destructive" role="alert" data-pane-reopen-failed={connection.reopen?.state === "failed" ? connection.reopen.reason : undefined}>
            {t(failure)}
          </p>
        ) : null}
        {offersSharedServerOff(connection) && machineId ? (
          <div className="flex flex-col gap-xxs border-t border-border pt-sm">
            <Button
              variant="link"
              size="sm"
              className="h-auto justify-start p-none text-caption"
              disabled={outcome?.phase === "pending"}
              data-codex-shared-server-off={machineId}
              onClick={() =>
                once(() => {
                  setAskedOff(true);
                  actions.turnOffCodexSharedServer(machineId);
                })
              }
            >
              {t("panes.connection.sharedServer.link")}
            </Button>
            <span className="text-caption text-muted-foreground">
              {local ? t("panes.connection.sharedServer.scopeLocal") : t("panes.connection.sharedServer.scopeDevice", { device: device?.label ?? "" })}
            </span>
          </div>
        ) : null}
        {/* The answer outlives the reason it fixed: once the server is off, this pane reads "started before Hide was set up". */}
        {outcome ? (
          <span
            className={`text-caption ${outcome.phase === "failed" ? "text-destructive" : "text-muted-foreground"}`}
            role={outcome.phase === "failed" ? "alert" : "status"}
            data-codex-shared-server-outcome={outcome.phase}
          >
            {outcome.phase === "pending" ? t("panes.connection.sharedServer.pending") : outcome.phase === "done" ? t("panes.connection.sharedServer.done") : t(outcome.key)}
          </span>
        ) : null}
      </PopoverContent>
    </Popover>
  );
}
