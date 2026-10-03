// The keys a spec presses, as the machine it runs on presses them: macOS's
// own chords on the macOS job and the Ctrl+Shift set on Linux, read from the
// shell's registry (`src/shortcuts.ts`), so each system's job exercises that
// system's chords and a spec never spells a chord the shell would not answer.
// The browser under test runs on the same machine as the runner, so both
// read the same system.

import type { HostKind } from "../src/host";
import { defaultChord, displayChord, fieldChord, keySystemOf, modChord, type Chord, type CommandId } from "../src/shortcuts";

export const SYSTEM = keySystemOf(process.platform);

/** A chord as Playwright presses it: its modifiers, then the physical key by its code. */
function keys(chord: Chord): string {
  return [chord.ctrl && "Control", chord.alt && "Alt", chord.shift && "Shift", chord.meta && "Meta", chord.code].filter(Boolean).join("+");
}

/** The default chord of `id` on `host` for this system; a command without one is a spec error. */
export function chord(id: CommandId, host: HostKind = "browser"): string {
  const bound = defaultChord(id, host, SYSTEM);
  if (!bound) throw new Error(`${id} has no ${host} chord on ${SYSTEM}`);
  return keys(bound);
}

/** A macOS ⌘ chord through the rule: `mod("KeyC")` is Meta+KeyC on macOS and Control+Shift+KeyC elsewhere. */
export function mod(code: string, extra: { shift?: boolean; alt?: boolean } = {}): string {
  return keys(modChord({ code, meta: true, ...extra }, SYSTEM));
}

/** The system's command key inside a text field or palette: Meta+Enter on macOS, Control+Enter elsewhere. */
export function field(code: string): string {
  return keys(fieldChord(code, SYSTEM));
}

/** The modifiers an app chord holds on this system, for a hold: Meta, or Control and Shift. */
export const MOD_KEYS: readonly string[] = SYSTEM === "mac" ? ["Meta"] : ["Control", "Shift"];

/** How the shell writes `chord` on this system ("⌥G", "Alt+G"). */
export function label(chord: Chord): string {
  return displayChord(chord, SYSTEM);
}

/** How the shell writes `id`'s default chord on `host` for this system. */
export function commandLabel(id: CommandId, host: HostKind = "browser"): string {
  const bound = defaultChord(id, host, SYSTEM);
  if (!bound) throw new Error(`${id} has no ${host} chord on ${SYSTEM}`);
  return label(bound);
}
