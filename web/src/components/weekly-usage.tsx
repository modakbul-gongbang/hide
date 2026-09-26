// The weekly usage chips at the foot of the sidebar and the Weekly Usage
// popover above them, the web form of the native `SidebarUtilityBar` usage
// button and `HideUsagePopover` (docs/UI_BEHAVIOR.md "Weekly usage"). The core
// reads the numbers (herdr-core/src/usage.rs); the page draws them and tells
// the core when it is looking, which is what paces the reads. No usage state
// becomes a banner: a chip without a reading is muted, and the popover row
// says why.

import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { AgentMark } from "../AgentMark";
import type { ProviderUsage } from "../generated/hided-ws";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { percentLabel, resetCountdown, usagePercent, usageSummary, usageTone, usageValueLabel, type UsageReading, type UsageTone } from "../usage";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { Hint } from "./ui/tooltip";

const NO_ROWS: ProviderUsage[] = [];

const TEXT_TONE: Record<UsageTone, string> = {
  success: "text-success",
  warning: "text-warning",
  danger: "text-destructive",
  muted: "text-muted-foreground",
};

const FILL_TONE: Record<UsageTone, string> = {
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-destructive",
  muted: "bg-muted-foreground",
};

/**
 * Tells the core whether this page is on screen: while one is, the core reads
 * each provider every five minutes. It is said again on every live
 * connection, because a daemon that restarted starts out not looking.
 */
export function useUsageWindowHint(actions: Actions) {
  const live = useShellStore((s) => s.connection === "live");
  useEffect(() => {
    if (!live) return;
    const report = () => actions.observeUsage({ usage_window_visible: document.visibilityState === "visible" });
    report();
    document.addEventListener("visibilitychange", report);
    return () => document.removeEventListener("visibilitychange", report);
  }, [actions, live]);
}

export function WeeklyUsage({ actions }: { actions: Actions }) {
  const rows = useShellStore((s) => s.rest?.navigator?.provider_usage ?? NO_ROWS);
  const [open, setOpen] = useState(false);
  // The popover's hint lasts exactly as long as it is open, however it ends:
  // opening it reads again any provider last read more than a minute ago.
  useEffect(() => {
    if (!open) return;
    actions.observeUsage({ usage_popover_open: true });
    return () => actions.observeUsage({ usage_popover_open: false });
  }, [actions, open]);
  if (rows.length === 0) return null;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Hint label="Weekly usage">
        <PopoverTrigger
          aria-label={usageSummary(rows)}
          data-usage-trigger="true"
          className="group/usage flex shrink-0 items-center gap-xs rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {rows.map((row) => (
            <UsageChip key={row.provider} row={row} />
          ))}
        </PopoverTrigger>
      </Hint>
      <PopoverContent side="top" className="flex w-(--size-usage-popover) flex-col gap-md p-lg" data-usage-popover="true">
        <div className="flex items-baseline gap-sm">
          <span className="min-w-0 flex-1 text-subhead font-semibold text-foreground">Weekly Usage</span>
          <span className="shrink-0 font-mono text-micro font-semibold text-muted-foreground">7 days</span>
        </div>
        <UsageRows rows={rows} />
      </PopoverContent>
    </Popover>
  );
}

function UsageChip({ row }: { row: ProviderUsage }) {
  const percent = usagePercent(row);
  return (
    <span
      data-usage-chip={row.provider}
      data-usage-state={row.state}
      className={cn(
        "flex h-(--size-control-sm) items-center gap-xxs rounded-sm bg-secondary px-xs font-mono text-micro font-semibold group-hover/usage:bg-accent group-data-[state=open]/usage:bg-accent",
        TEXT_TONE[usageTone(percent)],
      )}
    >
      <UsageMark provider={row.provider} muted={percent === null} />
      {percent === null ? null : <span>{percentLabel(percent)}</span>}
    </span>
  );
}

function UsageMark({ provider, muted }: { provider: string; muted: boolean }) {
  return <AgentMark kind={provider} className={muted ? "opacity-(--opacity-dimmed) grayscale" : ""} />;
}

/** Mounted only while the popover is open, so the countdown's minute clock runs only then. */
function UsageRows({ rows }: { rows: readonly ProviderUsage[] }) {
  const now = useMinuteClock();
  return (
    <>
      {rows.map((row) => (
        <div key={row.provider} className="flex flex-col gap-sm">
          <UsageLine provider={row.provider} label={row.label} reading={row} message={row.message} bucket={false} now={now} />
          {row.buckets.map((bucket) => (
            <UsageLine key={bucket.label} provider={row.provider} label={bucket.label} reading={bucket} message={bucket.message} bucket now={now} />
          ))}
        </div>
      ))}
    </>
  );
}

function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

function UsageLine({
  provider,
  label,
  reading,
  message,
  bucket,
  now,
}: {
  provider: string;
  label: string;
  reading: UsageReading & { resets_at_unix_seconds: number | null };
  message: string | null;
  bucket: boolean;
  now: number;
}) {
  const percent = usagePercent(reading);
  const tone = usageTone(percent);
  // A line without a reading says why in place of its bar. A provider line
  // with an older reading keeps its bar and says how old it is; a bucket's
  // copy of that note is its provider's, so a bucket repeats only its own.
  const note = percent === null || !bucket ? message : null;
  return (
    <div data-usage-row={provider} data-usage-bucket={bucket ? label : undefined} data-usage-state={reading.state} className="flex flex-col gap-xs">
      <div className="flex min-w-0 items-center gap-sm">
        {bucket ? (
          <span aria-hidden="true" className="w-sm shrink-0 font-mono text-micro text-muted-foreground">
            └
          </span>
        ) : null}
        <UsageMark provider={provider} muted={percent === null} />
        <span className={cn("min-w-0 truncate font-medium text-subtle-foreground", bucket ? "text-caption" : "text-body")}>{label}</span>
        {reading.resets_at_unix_seconds === null ? null : (
          <span className="shrink-0 whitespace-nowrap font-mono text-micro text-muted-foreground" data-usage-reset="true">
            · {resetCountdown(reading.resets_at_unix_seconds, now)}
          </span>
        )}
        <span className="flex-1" />
        <span className={cn("shrink-0 font-mono text-caption font-semibold", TEXT_TONE[tone])} data-usage-value="true">
          {usageValueLabel(reading)}
        </span>
      </div>
      {percent === null ? null : (
        <div aria-hidden="true" className="h-xs overflow-hidden rounded-full bg-border">
          <div className={cn("h-full rounded-full", FILL_TONE[tone])} style={{ width: `${Math.min(Math.max(percent, 0), 100)}%` }} />
        </div>
      )}
      {note ? <p className="text-caption text-muted-foreground">{note}</p> : null}
    </div>
  );
}
