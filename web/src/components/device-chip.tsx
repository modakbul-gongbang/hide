import { ServerIcon } from "lucide-react";
import { Badge } from "./ui/badge";

/** R1: a remote location is always the server glyph and Hide's device name. */
export function DeviceChip({ label, className = "" }: { label: string; className?: string }) {
  return (
    <Badge variant="outline" className={`min-w-0 shrink gap-xxs ${className}`} data-device-chip={label}>
      <ServerIcon aria-hidden="true" className="size-(--size-icon-xs) shrink-0" />
      <span className="truncate">{label}</span>
    </Badge>
  );
}
