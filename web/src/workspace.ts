// The Workspace screen's rules (PRD S6 B5-B11, B17; PRD three-column-panel
// D-01, D-07, D-12): which strip entries are Agent tabs, what an Agent tab is
// called and marked with, and how the body is divided into the Agent Views,
// File Views and Tools columns. The core owns every stored value here
// (`rest.workspace_view`, the strip); these functions only read them, so
// they are testable without a page. The View areas' own rules are
// `viewLayout.ts`.

import type { Checkout, SnapshotRest, StripTab, ViewLayoutSnapshot } from "./snapshot";
import { agentAdapter } from "./agentAdapters";

/** The Tools column's one tool, separate from Memory's `sessions` reader. */
export type Tool = "agent_sessions" | "explorer" | "changes";

/** The front Workspace's presentation as the core published it. */
export type WorkspaceView = {
  device_id: string;
  path: string;
  /** Whether the File Views column is on; the core turns it off with its last view. */
  views: boolean;
  /** Whether the Tools column is on. */
  tools: boolean;
  /** The one tool the Tools column holds, kept while the column is off. */
  tool: Tool;
  /** The File Views column's width in CSS pixels, or null for the default until it is resized. */
  views_width: number | null;
  /** The Tools column's width, the same way. */
  tools_width: number | null;
  /** The core's latest width acknowledgement, absent before any named request. Never saved. */
  width_request_id?: string | null;
  /** The number of this Workspace's last File Views call, 0 for none since the core started (`readCalls`). */
  views_called: number;
  /** How many File Views calls the core has numbered, in any Workspace. */
  views_calls: number;
  /** The Workspace the operator last chose, now or before a restart, so the page opens on it (D-11). */
  resumed?: boolean;
  /** The View areas (S7); absent only from a core that predates them. */
  layout?: ViewLayoutSnapshot;
  agent_layout?: import("./agentLayout").AgentLayout;
};

/** The two columns beside Agent Views, left to right. */
export type SideColumn = "views" | "tools";
/** Every column of the body, left to right. */
export type Column = "agents" | SideColumn;

/** The pixel sizes the columns are laid out with, read from their tokens. */
export type ColumnSizes = {
  agentMin: number;
  viewsMin: number;
  toolsMin: number;
  /** File Views' width until the operator resizes it. */
  viewsIdeal: number;
  /** Tools' width until the operator resizes it. */
  toolsIdeal: number;
  /** The divider between two columns. */
  divider: number;
};

/**
 * Which column a narrow body shows, a fact of this page alone and never sent
 * (D-07): `side` is the one column beside Agent Views between the two steps
 * when both are on, `single` the one column below the narrow step.
 */
export type ColumnSlots = { side: SideColumn; single: Column };

export const DEFAULT_SLOTS: ColumnSlots = { side: "views", single: "agents" };

/** How many columns the body can hold, from the column minimums (D-07). */
export type BodyStep = "wide" | "mid" | "narrow";

/**
 * Calls only override the measured width step where they were made (B25-B27).
 * The first measurement preserves a call that brought this Workspace forward.
 */
export function slotsForStep(slots: ColumnSlots, previous: BodyStep | null, step: BodyStep): ColumnSlots {
  return previous !== null && previous !== step ? DEFAULT_SLOTS : slots;
}

/**
 * The body as it is drawn: each shown column's width, Agent Views taking
 * what the others leave. A column that is off, or on but hidden by the
 * body's width, has no width here.
 */
export type ColumnFrame = {
  step: BodyStep;
  agents: number | null;
  views: number | null;
  tools: number | null;
};

/** The body widths where a third and a second column stop fitting at their minimums, with the dividers between them. */
export function bodySteps(sizes: ColumnSizes): { wide: number; mid: number } {
  return { wide: sizes.agentMin + sizes.viewsMin + sizes.toolsMin + 2 * sizes.divider, mid: sizes.agentMin + sizes.viewsMin + sizes.divider };
}

export function bodyStep(body: number, sizes: ColumnSizes): BodyStep {
  const { wide, mid } = bodySteps(sizes);
  return body >= wide ? "wide" : body >= mid ? "mid" : "narrow";
}

/**
 * The columns the body draws (D-07, D-12), from what is on, the stored
 * widths and the body's width alone; nothing here is sent or stored.
 *
 * - `views`: File Views is on and has something to show (a view, or a file
 *   of this checkout opening into it).
 * - At `wide` every column that is on shows; at `mid` one column beside
 *   Agent Views, Tools giving way first unless `slots.side` asks for it; at
 *   `narrow` one column, Agent Views unless `slots.single` names another
 *   that is on.
 * - Widths: the stored width or the default, no column under its minimum,
 *   and Agent Views keeps at least its own; it takes the rest.
 */
export function columnFrame(input: { views: boolean; tools: boolean; viewsWidth: number | null; toolsWidth: number | null; body: number; sizes: ColumnSizes; slots: ColumnSlots }): ColumnFrame {
  const { body, sizes, slots } = input;
  const step = bodyStep(body, sizes);
  // Before the body is measured nothing beside Agent Views is placed, so no terminal fits to a guess.
  if (body <= 0) return { step, agents: 0, views: null, tools: null };
  let showViews = input.views;
  let showTools = input.tools;
  if (step === "mid" && showViews && showTools) {
    if (slots.side === "tools") showViews = false;
    else showTools = false;
  }
  if (step === "narrow") {
    const single = slots.single === "views" && input.views ? "views" : slots.single === "tools" && input.tools ? "tools" : "agents";
    return { step, agents: single === "agents" ? body : null, views: single === "views" ? body : null, tools: single === "tools" ? body : null };
  }
  const dividers = (showViews ? sizes.divider : 0) + (showTools ? sizes.divider : 0);
  const room = body - dividers - sizes.agentMin;
  let tools: number | null = null;
  let views: number | null = null;
  if (showTools) {
    const reserve = showViews ? sizes.viewsMin : 0;
    tools = clamp(input.toolsWidth ?? sizes.toolsIdeal, sizes.toolsMin, room - reserve);
  }
  if (showViews) views = clamp(input.viewsWidth ?? sizes.viewsIdeal, sizes.viewsMin, room - (tools ?? 0));
  return { step, agents: body - dividers - (views ?? 0) - (tools ?? 0), views, tools };
}

/** `value` within `min..max`, the minimum winning when the two cross. */
function clamp(value: number, min: number, max: number): number {
  return Math.round(Math.max(min, Math.min(value, max)));
}

/**
 * Where a divider dragged to `x` pixels from the body's left edge lands, as
 * the widths to store: the divider left of File Views sets its width; the
 * divider left of Tools trades width with File Views when File Views shows,
 * so Agent Views keeps its width and no terminal resizes, and otherwise
 * sets Tools' width. Every column stays at or above its minimum.
 */
export function dividerLanding(input: { divider: SideColumn; x: number; body: number; frame: ColumnFrame; sizes: ColumnSizes }): { views_width?: number; tools_width?: number } {
  const { divider, x, body, frame, sizes } = input;
  const views = frame.views ?? 0;
  const tools = frame.tools ?? 0;
  const dividers = (frame.views !== null ? sizes.divider : 0) + (frame.tools !== null ? sizes.divider : 0);
  if (divider === "views") {
    // The File Views column's right edge stays where Tools begins.
    const right = body - tools - (frame.tools !== null ? sizes.divider : 0);
    const width = clamp(right - x - sizes.divider, sizes.viewsMin, body - dividers - tools - sizes.agentMin);
    return { views_width: width };
  }
  if (frame.views !== null) {
    const pair = views + tools;
    const toolsWidth = clamp(body - x - sizes.divider, sizes.toolsMin, pair - sizes.viewsMin);
    return { views_width: pair - toolsWidth, tools_width: toolsWidth };
  }
  return { tools_width: clamp(body - x - sizes.divider, sizes.toolsMin, body - dividers - sizes.agentMin) };
}

export function workspaceViewOf(rest: SnapshotRest | null): WorkspaceView | null {
  return (rest?.workspace_view as WorkspaceView | undefined) ?? null;
}

/** Herdr tabs: the Agent area's strip, in the core's order. */
export function agentEntries(checkout: Checkout): StripTab[] {
  return checkout.strip.filter((entry) => entry.kind === "herdr");
}

/** The agent kinds whose own mark Hide ships; any other agent is drawn with the neutral mark (D-09). */
export type Provider = "claude" | "codex";

export function knownProvider(kind: string | null | undefined): Provider | null {
  // The generated SidebarMark enum selects the artwork; this type names the assets.
  return (agentAdapter(kind ?? "")?.sidebar_mark as Provider | null | undefined) ?? null;
}

/** What the page has read of the core's File Views calls. */
export type CallsSeen = {
  /** The core's call count at the last read, or null before the first. */
  last: number | null;
  /** The Workspace in front at the last read, by device and path. */
  front: string | null;
};

export const NO_CALLS_SEEN: CallsSeen = { last: null, front: null };

type FrontView = Pick<WorkspaceView, "device_id" | "path" | "views_called" | "views_calls">;

/**
 * The page's reading of the front Workspace's calls: `reset` when another
 * Workspace came in front (a narrow body starts on Agent Views there), and
 * `call` when that Workspace was called since the last read, the call that
 * brought it in front included. A call older than the last read is history,
 * so an old call never moves a Workspace chosen later; a core that started
 * again counts from zero, so its lower count is only read.
 */
export function readCalls(seen: CallsSeen, view: FrontView | null | undefined): { seen: CallsSeen; reset: boolean; call: boolean } {
  if (!view) return { seen: seen.front === null ? seen : { ...seen, front: null }, reset: false, call: false };
  const front = `${view.device_id}\u0000${view.path}`;
  const reset = front !== seen.front;
  const call = seen.last !== null && view.views_calls >= seen.last && view.views_called > seen.last;
  const next = seen.last === view.views_calls && !reset ? seen : { last: view.views_calls, front };
  return { seen: next, reset, call };
}
