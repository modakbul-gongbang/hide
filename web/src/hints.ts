// The modifier hold hint, as a pure state machine (PRD
// electron-digit-shortcuts-hints D-03, D-04; engineering principle 13: the
// hold is modeled as state, not as a second look at the last key event).
//
// A port of the removed native app's `HideHintState`: one hold owns one
// modifier set and one deadline. The set changes on every modifier key
// event; a non-empty set that is not yet revealed starts a deadline, and
// reaching it reveals. A change to the set while revealed keeps the reveal
// only while the set is still non-empty, so adding ⇧ to a held ⌘ hides the
// ⌘ keycaps (the family test below no longer matches) and releasing
// everything hides them too. Time is injected, so the release-before-delay
// case (⌘C never flashes a hint, B8) is a unit test and not a wall clock.

import { familyModifiers, type Chord, type Command, type NumberedFamily, NUMBERED_FAMILIES } from "./shortcuts";
import type { HostKind } from "./host";

/** The modifiers held, as a chord without a key. */
export type Modifiers = Required<Omit<Chord, "code">>;

export type HintState = {
  modifiers: Modifiers;
  /** When the current hold reveals, or null while nothing is pending. */
  deadline: number | null;
  revealed: boolean;
};

/** The native app's delay before a hold reveals its keycaps (`HideTheme.Hint.delay`). */
export const HINT_DELAY_MS = 150;

export const NO_MODIFIERS: Modifiers = { meta: false, alt: false, shift: false, ctrl: false };

export function idleHint(): HintState {
  return { modifiers: NO_MODIFIERS, deadline: null, revealed: false };
}

type ModifierEventLike = Pick<KeyboardEvent, "metaKey" | "altKey" | "shiftKey" | "ctrlKey">;

export function modifiersOf(event: ModifierEventLike): Modifiers {
  return { meta: event.metaKey, alt: event.altKey, shift: event.shiftKey, ctrl: event.ctrlKey };
}

export function modifiersEqual(a: Modifiers, b: Modifiers): boolean {
  return a.meta === b.meta && a.alt === b.alt && a.shift === b.shift && a.ctrl === b.ctrl;
}

function anyHeld(modifiers: Modifiers): boolean {
  return modifiers.meta || modifiers.alt || modifiers.shift || modifiers.ctrl;
}

/** The held set changed at `now`. */
export function holdModifiers(state: HintState, modifiers: Modifiers, now: number): HintState {
  if (modifiersEqual(state.modifiers, modifiers)) return state;
  const held = anyHeld(modifiers);
  const revealed = held && state.revealed;
  // An empty set is the one shared idle value, so an ended hold is recognizable by reference.
  return { modifiers: held ? modifiers : NO_MODIFIERS, revealed, deadline: !held || revealed ? null : now + HINT_DELAY_MS };
}

/** Time moved to `now`: a hold whose deadline passed reveals. */
export function advanceHint(state: HintState, now: number): HintState {
  if (state.deadline === null || now < state.deadline) return state;
  return { ...state, revealed: anyHeld(state.modifiers), deadline: null };
}

/**
 * The hold ends without a modifier event: the window lost focus, a layer
 * opened, or a key was pressed during the hold (B6). The next modifier
 * event starts over, so keeping ⌘ down after ⌘2 shows nothing until ⌘ is
 * released and pressed again.
 */
export function clearHint(): HintState {
  return idleHint();
}

/**
 * The family the hold reveals on `host`: the one whose chords in `registry`
 * are exactly the held modifiers plus a digit, or null while nothing is
 * revealed or the host has no such chords (a browser, B3).
 */
export function revealedFamily(state: HintState, registry: readonly Command[], host: HostKind): NumberedFamily | null {
  if (!state.revealed) return null;
  for (const spec of NUMBERED_FAMILIES) {
    const modifiers = familyModifiers(spec.family, registry, host);
    if (modifiers && modifiersEqual({ meta: !!modifiers.meta, alt: !!modifiers.alt, shift: !!modifiers.shift, ctrl: !!modifiers.ctrl }, state.modifiers)) return spec.family;
  }
  return null;
}
