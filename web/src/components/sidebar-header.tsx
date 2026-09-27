import { HouseIcon, PlusIcon, SearchIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../lib/utils";
import { SIDEBAR_MODES, type SidebarMode } from "../ui";
import { FoldLane } from "./sidebar-agent-row";
import { Button } from "./ui/button";
import { Hint } from "./ui/tooltip";

const MODE_LABEL: Record<SidebarMode, string> = { projects: "Projects", agents: "Agents" };

/**
 * The top of the sidebar (PRD sidebar-shell D-02..D-07): the global
 * destinations, today one Overview row, fixed above the lists, then the
 * Projects | Agents tab strip with Search and, on Projects only, New
 * workspace at its right end. Drawn from what the caller reads out of the
 * stores, so a test renders it without them.
 */
export function SidebarHeader({
  mode,
  overviewSelected,
  projectCount,
  switchChord,
  searchChord,
  newWorkspaceChord,
  onOverview,
  onMode,
  onSearch,
  onNewWorkspace,
}: {
  mode: SidebarMode;
  /** The Overview screen is in front. */
  overviewSelected: boolean;
  /** Every project on every device, or null before the first snapshot. */
  projectCount: number | null;
  /** The bound `toggle_sidebar_view` chord; it has none until the operator binds one. */
  switchChord: string | null;
  searchChord: string | null;
  newWorkspaceChord: string | null;
  onOverview: () => void;
  onMode: (mode: SidebarMode) => void;
  onSearch: () => void;
  onNewWorkspace: () => void;
}) {
  return (
    <>
      <ul className="shrink-0 border-b border-border p-xs" aria-label="Destinations" data-sidebar-destinations="true">
        <li>
          <button
            type="button"
            data-overview-destination="true"
            aria-current={overviewSelected ? "page" : undefined}
            className={cn(
              "flex min-h-(--size-project-row) w-full items-center gap-sm rounded-sm pr-xs pl-sm text-left outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
              overviewSelected ? "bg-secondary" : "hover:bg-accent",
            )}
            onClick={onOverview}
          >
            <HouseIcon aria-hidden="true" className="size-(--size-checkout-icon) shrink-0 text-subtle-foreground" />
            <span className="min-w-0 flex-1 truncate text-subhead font-semibold text-foreground">Overview</span>
            <span className="flex shrink-0 items-center gap-xs">
              {projectCount === null ? null : (
                <span className="text-body text-muted-foreground" data-overview-count="true">
                  {projectCount} {projectCount === 1 ? "project" : "projects"}
                </span>
              )}
              <FoldLane />
            </span>
          </button>
        </li>
      </ul>
      <div className="flex h-(--size-tab-strip) shrink-0 items-center gap-sm border-b border-border pr-xs pl-md text-caption" data-sidebar-strip="true">
        {SIDEBAR_MODES.map((candidate) => (
          <ModeHint key={candidate} label={MODE_LABEL[candidate]} chord={switchChord}>
            <button
              type="button"
              data-sidebar-mode={candidate}
              aria-pressed={mode === candidate}
              className={mode === candidate ? "text-foreground" : "text-muted-foreground hover:text-subtle-foreground"}
              onClick={() => onMode(candidate)}
            >
              {MODE_LABEL[candidate]}
            </button>
          </ModeHint>
        ))}
        <span className="flex-1" />
        {mode === "projects" ? (
          <Hint label="New workspace" shortcut={newWorkspaceChord}>
            <Button variant="ghost" size="icon-sm" data-sidebar-new-workspace="true" onClick={onNewWorkspace}>
              <PlusIcon />
            </Button>
          </Hint>
        ) : null}
        <Hint label="Search" shortcut={searchChord}>
          <Button variant="ghost" size="icon-sm" data-sidebar-search="true" onClick={onSearch}>
            <SearchIcon />
          </Button>
        </Hint>
      </div>
    </>
  );
}

/** A tab's hint exists only to show the switch chord, so a tab without one has none. */
function ModeHint({ label, chord, children }: { label: string; chord: string | null; children: ReactNode }) {
  if (!chord) return children;
  return (
    <Hint label={label} shortcut={chord}>
      {children}
    </Hint>
  );
}
