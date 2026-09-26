import { SearchIcon } from "lucide-react";
import { hostKind } from "../host";
import { displayCommand } from "../shortcuts";
import { Kbd } from "./ui/kbd";

/**
 * The sidebar's Search field (issue 154): a button drawn as a field, with the
 * chord that also opens it, so a click reaches the same ⌘K palette the
 * shortcut does. Typing happens in the palette, never here.
 */
export function SearchField({ onOpen }: { onOpen: () => void }) {
  return (
    <div className="shrink-0 px-md pb-xs">
      <button
        type="button"
        data-sidebar-search="true"
        className="flex h-(--size-control) w-full items-center gap-sm rounded-sm border border-input bg-background px-sm text-left text-body text-muted-foreground outline-none transition-colors hover:text-subtle-foreground focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring"
        onClick={onOpen}
      >
        <SearchIcon className="size-(--size-icon) shrink-0" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate">Search</span>
        <Kbd>{displayCommand("search", hostKind())}</Kbd>
      </button>
    </div>
  );
}
