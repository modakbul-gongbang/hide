import type { Actions } from "./actions";
import { Dialog, DialogBody, DialogContent, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Kbd } from "./components/ui/kbd";
import { Hint } from "./components/ui/tooltip";
import { hostKind } from "./host";
import { displayChord, hostChord, resolvedRegistry, storedBindings, type Command } from "./shortcuts";
import { useShellStore } from "./store";

// The sheet is generated from the registry (PRD S2 B11): every mapping of the
// running host by group, a "moved for Chrome" note on the chords Chrome
// reserves (browser only), and a passthrough note on the one chord the
// terminal keeps.

const GROUPS: Command["group"][] = ["Tabs", "Navigate", "Panels", "Panes", "Help"];

export function ShortcutSheet({ actions }: { actions: Actions }) {
  // The sheet reads the same effective registry the window listener runs, so
  // a rebound pane chord is what it lists (PRD S5 B9).
  const host = hostKind();
  const stored = useShellStore((s) => storedBindings(s.rest?.ui_state, host));
  const { registry, diagnostic } = resolvedRegistry(stored, host);
  // The gate in App.tsx only mounts this component while the overlay is
  // "shortcuts", so the dialog is always open here; closing it (Escape, a
  // click outside, or the trigger elsewhere) toggles that overlay off.
  return (
    <Dialog open onOpenChange={(next) => { if (!next) actions.openShortcuts(); }}>
      <DialogContent data-shortcut-sheet="true" className="w-(--size-search-sheet-w)">
        <DialogHeader className="flex-row items-baseline justify-between">
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <span className="text-caption text-muted-foreground">{host === "electron" ? "desktop app" : "browser"}</span>
        </DialogHeader>
        <DialogBody>
          {diagnostic ? (
            <p className="mb-md text-caption text-warning" data-shortcut-diagnostic="true">
              {diagnostic}
            </p>
          ) : null}
          {GROUPS.map((group) => (
            <section key={group} className="mb-md">
              <h3 className="mb-xs text-caption uppercase text-muted-foreground">{group}</h3>
              <ul>
                {registry.filter((command) => command.group === group).map((command) => (
                  <li key={command.id} className="flex items-center gap-md py-xxs" data-shortcut={command.id}>
                    <span className="min-w-0 flex-1 truncate">{command.title}</span>
                    {host === "browser" && command.moved ? (
                      <Hint label={`Chrome reserves ${command.movedFrom}`}>
                        <span className="text-caption text-warning">moved for Chrome ({command.movedFrom})</span>
                      </Hint>
                    ) : null}
                    {command.passthrough ? <span className="text-caption text-muted-foreground">{command.passthrough}</span> : null}
                    <Kbd>{hostChord(command, host) ? displayChord(hostChord(command, host)!) : "-"}</Kbd>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
