import type { Actions } from "./actions";
import { hostKind } from "./host";
import { displayChord, hostChord, hostRegistry, type Command } from "./shortcuts";
import { useShellStore } from "./store";

// The sheet is generated from the registry (PRD S2 B11): every mapping of the
// running host by group, a "moved for Chrome" note on the chords Chrome
// reserves (browser only), and a passthrough note on the one chord the
// terminal keeps.

const GROUPS: Command["group"][] = ["Tabs", "Navigate", "Panels", "Panes", "Help"];

export function ShortcutSheet({ actions }: { actions: Actions }) {
  // The sheet reads the same effective registry the window listener runs, so
  // a rebound pane chord is what it lists (PRD S5 B9).
  const stored = useShellStore((s) => s.rest?.ui_state?.browser_shortcut_bindings);
  const host = hostKind();
  const { registry, diagnostic } = hostRegistry(stored, host);
  return (
    <div className="absolute inset-0 z-40 flex items-start justify-center p-xl" role="presentation" onClick={() => actions.openShortcuts()}>
      <div className="absolute inset-0 bg-background opacity-[var(--opacity-secondary)]" />
      <div
        role="dialog"
        aria-label="Keyboard shortcuts"
        data-shortcut-sheet="true"
        className="relative max-h-full w-[var(--size-search-sheet-w)] overflow-auto rounded-lg border border-divider bg-balloon p-lg text-body text-primary shadow-lg"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="mb-md flex items-baseline justify-between">
          <h2 className="text-title">Keyboard shortcuts</h2>
          <span className="text-caption text-muted">{host === "electron" ? "desktop app" : "browser"}</span>
        </div>
        {diagnostic ? (
          <p className="mb-md text-caption text-warning" data-shortcut-diagnostic="true">
            {diagnostic}
          </p>
        ) : null}
        {GROUPS.map((group) => (
          <section key={group} className="mb-md">
            <h3 className="mb-xs text-caption uppercase text-muted">{group}</h3>
            <ul>
              {registry.filter((command) => command.group === group).map((command) => (
                <li key={command.id} className="flex items-center gap-md py-xxs" data-shortcut={command.id}>
                  <span className="min-w-0 flex-1 truncate">{command.title}</span>
                  {host === "browser" && command.moved ? (
                    <span className="text-caption text-warning" title={`Chrome reserves ${command.movedFrom}`}>
                      moved for Chrome ({command.movedFrom})
                    </span>
                  ) : null}
                  {command.passthrough ? <span className="text-caption text-muted">{command.passthrough}</span> : null}
                  <kbd className="rounded-xs bg-elevated px-xs font-mono text-caption leading-[var(--size-keycap-height)] text-secondary">
                    {hostChord(command, host) ? displayChord(hostChord(command, host)!) : "-"}
                  </kbd>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
