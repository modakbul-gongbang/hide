import { MoonIcon } from "lucide-react";
import type { HTMLAttributes } from "react";
import { cn } from "../lib/utils";

const DOT = "●";
const RING = "○";
/** A sleeping agent (PRD agent-sleep B10); the glyph varies by face, the icon does not. */
const MOON = "☾";

/**
 * An agent's status mark, one size on every surface that lists agents
 * (docs/status-model.md, One meaning across surfaces). The core names the
 * mark; `●` and `○` are drawn as a dot and a ring of one diameter, because the
 * two glyphs render at different sizes in every face, and every other mark
 * (`?`, `!`, `×`, `✓`, `~`, `⊘`, a pending `…`) is its glyph in the same box,
 * except the sleeping `☾`, drawn as the moon icon for the same reason.
 * The colour comes from the caller's text tone, which the shapes take as
 * `currentColor`.
 */
export function StatusMark({ symbol, className, ...rest }: { symbol: string } & HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      aria-hidden="true"
      data-mark={symbol}
      className={cn("inline-flex size-(--size-agent-mark) shrink-0 items-center justify-center font-mono text-caption leading-none", className)}
      {...rest}
    >
      {symbol === DOT ? (
        <span className="size-(--size-status-mark) rounded-full bg-current" />
      ) : symbol === RING ? (
        <span className="size-(--size-status-mark) rounded-full border-(length:--size-hairline) border-current" />
      ) : symbol === MOON ? (
        <MoonIcon className="size-full" />
      ) : (
        symbol
      )}
    </span>
  );
}
