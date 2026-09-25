import { badgeText } from "./connection";
import { useShellStore } from "./store";

export function ConnectionBadge() {
  const connection = useShellStore((s) => s.connection);
  const refused = useShellStore((s) => s.refused);
  const text = badgeText(connection, refused);
  if (!text) return null;
  return (
    <div
      role="status"
      className="border-b border-border bg-card px-md py-xs text-caption text-subtle-foreground"
      data-connection={connection}
    >
      {text}
    </div>
  );
}
