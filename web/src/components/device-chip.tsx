import { LaptopIcon, ServerIcon } from "lucide-react";
import { Badge } from "./ui/badge";

/** R1: a remote location is always the server glyph and Hide's device name; this Mac's own chip wears the laptop glyph. */
export function DeviceChip({ label, local = false, className = "" }: { label: string; local?: boolean; className?: string }) {
  const Icon = local ? LaptopIcon : ServerIcon;
  return (
    <Badge variant="outline" className={`min-w-0 shrink gap-xxs ${className}`} data-device-chip={label}>
      <Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="truncate">{label}</span>
    </Badge>
  );
}
