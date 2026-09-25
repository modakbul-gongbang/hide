import type { ComponentProps } from "react";
import { cn } from "../../lib/utils";

/** A text field. `mono` is for machine text (paths, aliases, branches); prose such as a purpose or a label keeps the interface face, which keeps Korean readable. */
function Input({ className, type, mono = false, ...props }: ComponentProps<"input"> & { mono?: boolean }) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "h-(--size-control) w-full min-w-0 rounded-sm border border-input bg-background px-sm text-body text-foreground outline-none transition-colors selection:bg-primary selection:text-primary-foreground placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-(--opacity-disabled) aria-invalid:border-destructive aria-invalid:ring-destructive",
        mono && "font-mono",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
