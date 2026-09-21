import type { HTMLAttributes } from "react";

export function Badge({ className = "", ...props }: HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={`inline-flex h-[var(--size-badge-height)] items-center rounded-sm bg-elevated px-xs text-micro text-secondary ${className}`}
      {...props}
    />
  );
}
