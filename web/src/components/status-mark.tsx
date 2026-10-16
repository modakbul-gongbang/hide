import type { HTMLAttributes } from "react";
import { MoonIcon } from "lucide-react";
import { cn } from "../lib/utils";
import type { RowMark } from "../snapshot";

/**
 * An agent's status mark: whose move is next, one size on every surface that
 * lists agents (docs/status-model.md, One meaning across surfaces). The core
 * names the mark; this draws it. The four shapes differ without their
 * colours: the operator's turn is a `!` in a warning badge, in progress is a
 * static three-quarter ring (dashed while it waits on proven work, D-25), to read is a small dot (green only when the label
 * read the turn as finished), and quiet keeps its slot empty so titles line up.
 * An agent Hide put to sleep keeps a grey moon, the one shape outside the four
 * (status-marks D-21). Each shape carries its own colour; a caller's class dims
 * a read turn.
 */
export function StatusMark({ mark, className, ...rest }: { mark: RowMark } & HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      aria-hidden="true"
      data-mark={mark}
      className={cn("inline-flex size-(--size-agent-mark) shrink-0 items-center justify-center", className)}
      {...rest}
    >
      {mark === "turn" ? (
        <svg viewBox="0 0 12 12" className="size-full">
          <circle cx="6" cy="6" r="6" className="fill-warning" />
          <path d="M5.25 2.6h1.5l-.25 4.6h-1zM5.25 8.2h1.5v1.4h-1.5z" className="fill-status-foreground" />
        </svg>
      ) : mark === "going" || mark === "waiting" ? (
        <svg viewBox="0 0 12 12" className="size-full">
          {/* Three quarters from three o'clock round to twelve; it does not turn (D-02). Waiting cuts it into five dashes (D-25). */}
          <path d="M10.05 6A4.05 4.05 0 1 1 6 1.95" fill="none" strokeWidth="1.9" className="stroke-agent-working" {...(mark === "waiting" ? { pathLength: 19, strokeDasharray: "2.2 2" } : {})} />
        </svg>
      ) : mark === "done" || mark === "unread" ? (
        <span className={cn("size-(--size-status-mark) rounded-full", mark === "done" ? "bg-success" : "bg-foreground")} />
      ) : mark === "asleep" ? (
        <MoonIcon className="size-full text-muted-foreground" />
      ) : null}
    </span>
  );
}
