import { Switch as SwitchPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "../../lib/utils";

function Switch({ className, ...props }: ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        "peer inline-flex h-(--size-checkbox) w-(--size-control) shrink-0 items-center rounded-xl border border-transparent outline-none transition-colors focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-(--opacity-disabled) data-[state=checked]:bg-primary data-[state=unchecked]:bg-input",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block size-(--size-icon) rounded-xl bg-background ring-0 transition-transform data-[state=checked]:translate-x-(--size-icon-sm) data-[state=unchecked]:translate-x-(--spacing-none)"
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
