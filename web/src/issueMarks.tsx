import { CircleDotIcon, FileTextIcon } from "lucide-react";
import type { CSSProperties } from "react";
import { cn } from "./lib/utils";
import type { IssueLabel, Task } from "./snapshot";

// The marks an issue carries wherever it is listed: its source's glyph and
// its labels in the colours the source gives them.

/** The mark a task's id carries: an issue's circle-dot, a local issue's page (D-06). */
export function TaskGlyph({ task, className }: { task: Task; className?: string }) {
  const Glyph = task.source === "github" ? CircleDotIcon : FileTextIcon;
  return <Glyph aria-hidden="true" className={cn("size-(--size-icon-sm) shrink-0", className)} />;
}

/** A label's dot in the colour its source gives it, grey without one. */
export function IssueLabelDot({ color }: { color: string | null }) {
  // The colour is the source's data, not a design value, so it reaches the dot through a custom property.
  const tint = color ? { "--issue-label": `#${color}` } : undefined;
  return <span aria-hidden="true" className={cn("size-(--issue-label-dot) shrink-0 rounded-full", color ? "bg-(--issue-label)" : "bg-muted-foreground")} style={tint as CSSProperties} />;
}

/** One label as its source colours it: a dot in the label's colour and its name. */
export function IssueLabelView({ label }: { label: IssueLabel }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-xxs text-caption text-muted-foreground" data-issue-label={label.name}>
      <IssueLabelDot color={label.color} />
      <span className="truncate">{label.name}</span>
    </span>
  );
}
