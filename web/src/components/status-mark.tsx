import { MoonIcon } from "lucide-react";
import type { HTMLAttributes } from "react";
import { cn } from "../lib/utils";

const DOT = "●";
const RING = "○";
/** A blocked agent: a filled triangle, a shape no other demand shares. */
const TRIANGLE = "▲";
/** A stopped turn with nothing left to wake the agent: a ring with its left half filled. */
const HALF = "◐";
/** A sleeping agent (PRD agent-sleep B10); the glyph varies by face, the icon does not. */
const MOON = "☾";

/**
 * An agent's status mark, one size on every surface that lists agents
 * (docs/status-model.md, One meaning across surfaces). The core names the
 * mark; `●` and `○` are drawn as a dot and a ring of one diameter, because the
 * two glyphs render at different sizes in every face, and the blocked `▲` and
 * stopped `◐` are drawn as shapes in the same box for the same reason. Every
 * other mark (`?`, `!`, `✓`, `~`, `⊘`, a pending `…`) is its glyph in that box,
 * except the sleeping `☾`, drawn as the moon icon.
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
      ) : symbol === TRIANGLE ? (
        <svg viewBox="0 0 12 12" className="size-full" fill="currentColor" fillRule="evenodd">
          {/* The exclamation is cut out, so the row's own background shows through it. */}
          <path d="M6 1.2 11.4 10.6H0.6ZM5.4 4.8V7.6H6.6V4.8ZM5.4 8.4V9.4H6.6V8.4Z" />
        </svg>
      ) : symbol === HALF ? (
        <svg viewBox="0 0 12 12" className="size-full" fill="none" stroke="currentColor" strokeWidth="1">
          <circle cx="6" cy="6" r="3" />
          <path d="M6 3A3 3 0 0 0 6 9Z" fill="currentColor" stroke="none" />
        </svg>
      ) : symbol === MOON ? (
        <MoonIcon className="size-full" />
      ) : (
        symbol
      )}
    </span>
  );
}
