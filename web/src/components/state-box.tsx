import type { HTMLAttributes } from "react";
import { cn } from "../lib/utils";

/**
 * The box a state stands in, edged in that state's colour (PRD
 * pr-card-and-session-record-redesign D-11): the PR card's decision box and
 * the session record's 지금 box are this one part, so the operator reads
 * both the same way. What goes inside is the caller's.
 */
export type StateBoxTone = "merge" | "failed" | "attention" | "working" | "done" | "merged" | "quiet";

const EDGE: Record<StateBoxTone, string> = {
  merge: "border-pr-mergeable",
  failed: "border-pr-failed",
  attention: "border-warning",
  working: "border-agent-working",
  done: "border-success",
  merged: "border-pr-merged",
  quiet: "border-border",
};

export function StateBox({ tone, className, ...rest }: { tone: StateBoxTone } & HTMLAttributes<HTMLElement>) {
  return <section className={cn("flex flex-col gap-sm rounded-md border bg-card px-md py-sm", EDGE[tone], className)} data-state-box={tone} {...rest} />;
}
