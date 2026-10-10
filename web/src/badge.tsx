import { windowStrip } from "./connection";
import { useInterfaceTranslation } from "./i18n/client";
import { coreMachineName, moveMachines } from "./screenMachine";
import { useShellStore } from "./store";
import { cn } from "./lib/utils";

export function ConnectionBadge() {
  const { t } = useInterfaceTranslation();
  const connection = useShellStore((s) => s.connection);
  const refused = useShellStore((s) => s.refused);
  const move = useShellStore((s) => s.coreMove);
  const link = useShellStore((s) => s.coreLink);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const strip = windowStrip({ connection, refused, move, link, machines: move ? moveMachines(devices, move, t) : null, coreMachine: coreMachineName(devices, t) }, t);
  if (!strip) return null;
  return (
    <div
      role="status"
      className="flex items-center gap-xs border-b border-border bg-card px-md py-xs text-caption text-subtle-foreground"
      data-connection={strip.kind}
    >
      {strip.mark ? (
        <span aria-hidden="true" className={cn("shrink-0", strip.mark === "warn" ? "font-semibold text-warning" : "text-muted-foreground")}>
          {strip.mark === "warn" ? "!" : "…"}
        </span>
      ) : null}
      <span className="min-w-0 truncate">{strip.text}</span>
    </div>
  );
}
