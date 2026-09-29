// The kind and model every start surface chooses (PRD home-device-rail D-18,
// D-20): the remembered choice the core keeps (`ui_state.agent_start`), the
// model list the provider catalog answers, and the one observation the
// catalog needs while any picker shows. Pure, so the rules read and test on
// their own; `components/agent-picker.tsx` draws them.

import type { AgentStartChoice, AiProvider, BackgroundAi } from "./snapshot";

/** The kinds hide starts with a model choice; `terminal` is a tab alone and has no model. */
export type ProviderKind = "claude" | "codex";
export const PROVIDER_KINDS: readonly ProviderKind[] = ["claude", "codex"];

export type AgentKind = ProviderKind | "terminal";

/** One picker's value: the kind and the catalog id of its model, or null for the CLI's own default. */
export type AgentSelection = { kind: AgentKind; model: string | null };

const NOTHING_REMEMBERED: AgentStartChoice = { kind: null, models: {} };

/** The kind a picker opens on: the last one a start named, Claude before any. */
export function rememberedSelection(start: AgentStartChoice | undefined): AgentSelection {
  const remembered = start ?? NOTHING_REMEMBERED;
  const kind = remembered.kind ?? "claude";
  return { kind, model: remembered.models[kind] ?? null };
}

/** The selection after the kind menu changes: that kind's remembered model, never the other kind's. */
export function selectKind(kind: AgentKind, start: AgentStartChoice | undefined): AgentSelection {
  if (kind === "terminal") return { kind, model: null };
  return { kind, model: (start ?? NOTHING_REMEMBERED).models[kind] ?? null };
}

/**
 * The model list of one kind: the catalog's `ready` list, the last one seen
 * while the catalog is read again, or the reason there is none. `loading` is
 * every state where the catalog has not answered yet.
 */
export type ModelCatalog =
  | { state: "ready"; models: readonly string[] }
  | { state: "loading" }
  | { state: "unavailable"; reason: string };

/** The lists seen so far, kept for the page's life so a second open shows one at once. */
const lastLists = new Map<ProviderKind, readonly string[]>();

export function catalogFor(ai: BackgroundAi | undefined, kind: ProviderKind): ModelCatalog {
  const provider: AiProvider | undefined = ai?.providers.find((row) => row.id === kind);
  if (provider && provider.models.length > 0) {
    lastLists.set(kind, provider.models);
    return { state: "ready", models: provider.models };
  }
  const kept = lastLists.get(kind);
  if (kept) return { state: "ready", models: kept };
  if (!provider || provider.state === "unread") return { state: "loading" };
  return { state: "unavailable", reason: provider.models_unavailable_reason ?? provider.message ?? "모델 목록이 비어 있습니다." };
}

/** Forgets the kept lists; tests start from a page that has seen none. */
export function forgetCatalogs() {
  lastLists.clear();
}

/**
 * The model a start sends: the selection's own when the list still has it,
 * the CLI default otherwise. While the list is not known the remembered
 * model is sent as it is: hide never guesses, and the CLI's refusal is what
 * the pane then shows (B32).
 */
export function modelToSend(selection: AgentSelection, catalog: ModelCatalog): string | null {
  if (selection.kind === "terminal" || selection.model === null) return null;
  if (catalog.state === "ready" && !catalog.models.includes(selection.model)) return null;
  return selection.model;
}

/** The model a start surface sends for `selection`, read against the catalog as it stands now. */
export function sendableModel(selection: AgentSelection, ai: BackgroundAi | undefined): string | null {
  if (selection.kind === "terminal") return null;
  return modelToSend(selection, catalogFor(ai, selection.kind));
}

/**
 * One observation of the catalog for every picker showing: the first opens
 * it, the last closes it, so two pickers do not switch each other off.
 */
export function createCatalogObserver(send: (observing: boolean) => void) {
  let holders = 0;
  return {
    acquire(): () => void {
      holders += 1;
      if (holders === 1) send(true);
      let released = false;
      return () => {
        if (released) return;
        released = true;
        holders -= 1;
        if (holders === 0) send(false);
      };
    },
    holders: () => holders,
  };
}
