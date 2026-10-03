// The keys a spec presses, as the machine it runs on presses them: macOS's
// own chords on the macOS job and the Ctrl+Shift and Alt+Shift set on Linux, read from the
// shell's registry (`src/shortcuts.ts`), so each system's job exercises that
// system's chords and a spec never spells a chord the shell would not answer.
// The browser under test runs on the same machine as the runner, so both
// read the same system.

import type { HostKind } from "../src/host";
import { defaultChord, displayChord, fieldChord, keySystemOf, modChord, type Chord, type CommandId } from "../src/shortcuts";

export const SYSTEM = keySystemOf(process.platform);

/** A chord's modifiers as Playwright names them. */
function modifiers(chord: Chord): string[] {
  return [chord.ctrl && "Control", chord.alt && "Alt", chord.shift && "Shift", chord.meta && "Meta"].filter((key): key is string => !!key);
}

/** A chord as Playwright presses it: its modifiers, then the physical key by its code. */
function keys(chord: Chord): string {
  return [...modifiers(chord), chord.code].join("+");
}

function bound(id: CommandId, host: HostKind): Chord {
  const chord = defaultChord(id, host, SYSTEM);
  if (!chord) throw new Error(`${id} has no ${host} chord on ${SYSTEM}`);
  return chord;
}

/** The default chord of `id` on `host` for this system; a command without one is a spec error. */
export function chord(id: CommandId, host: HostKind = "browser"): string {
  return keys(bound(id, host));
}

/** `id`'s default chord split for a cycle held open: hold `modifiers`, press `key`, release. */
export function held(id: CommandId, host: HostKind = "browser"): { modifiers: string[]; key: string } {
  const chord = bound(id, host);
  return { modifiers: modifiers(chord), key: chord.code };
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
  return label(bound(id, host));
}
