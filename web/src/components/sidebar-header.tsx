import { PlusIcon, SearchIcon } from "lucide-react";
import type { ReactNode } from "react";
import { SIDEBAR_MODES, type SidebarMode } from "../ui";
import { Button } from "./ui/button";
import { Hint } from "./ui/tooltip";

const MODE_LABEL: Record<SidebarMode, string> = { projects: "Projects", agents: "Agents" };

/**
 * The top of the sidebar (PRD home-device-rail D-13, D-14). With the device
 * rail showing it is one line, the name of what is in front (`This Mac`,
 * `mini Remote`, `Inbox 모든 기기`) with Add project and Search at its right
 * end, and the rail's own selection decides the lists below. With no remote
 * device there is no rail and no name: the Home row is the fixed destination
 * above the Projects | Agents tab strip, which keeps Search and, on Projects
 * only, Add project at its right end. Drawn from what the caller reads out of
 * the stores, so a test renders it without them.
 */
export function SidebarHeader({
  rail,
  title,
  addProject,
  mode,
  home,
  switchChord,
  searchChord,
  newWorkspaceChord,
  onMode,
  onSearch,
  onNewWorkspace,
}: {
  /** The device rail is showing: the line names what is in front and the tabs are gone. */
  rail: boolean;
  /** What is in front, and the smaller word after it; used with the rail. */
  title: { name: string; note: string | null };
  /** Add project is offered on this line (with the rail, not while the Inbox is in front). */
  addProject: boolean;
  mode: SidebarMode;
  /** The Home row, fixed above the tab strip when there is no rail. */
  home: ReactNode;
  /** The bound `toggle_sidebar_view` chord; it has none until the operator binds one. */
  switchChord: string | null;
  searchChord: string | null;
  newWorkspaceChord: string | null;
  onMode: (mode: SidebarMode) => void;
  onSearch: () => void;
  /** Add project; null where the host cannot pick a folder (a browser tab). */
  onNewWorkspace: (() => void) | null;
}) {
  const add = onNewWorkspace ? (
    <Hint label="Add project" shortcut={newWorkspaceChord}>
      <Button variant="ghost" size="icon-sm" data-sidebar-new-workspace="true" onClick={onNewWorkspace}>
        <PlusIcon />
      </Button>
    </Hint>
  ) : null;
  const search = (
    <Hint label="Search" shortcut={searchChord}>
      <Button variant="ghost" size="icon-sm" data-sidebar-search="true" onClick={onSearch}>
        <SearchIcon />
      </Button>
    </Hint>
  );
  if (rail) {
    return (
      <div className="flex h-(--size-tab-strip) shrink-0 items-center gap-sm border-b border-border pr-xs pl-md" data-sidebar-title="true">
        <span className="flex min-w-0 flex-1 items-baseline gap-sm">
          <span className="min-w-0 truncate text-subhead font-semibold text-foreground" data-sidebar-title-name="true">
            {title.name}
          </span>
          {title.note ? <span className="shrink-0 text-caption text-muted-foreground">{title.note}</span> : null}
        </span>
        {addProject ? add : null}
        {search}
      </div>
    );
  }
  return (
    <>
      <ul className="shrink-0 border-b border-border p-xs" aria-label="Destinations" data-sidebar-destinations="true">
        {home}
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
        {mode === "projects" ? add : null}
        {search}
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
