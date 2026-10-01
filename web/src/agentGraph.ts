// The Agents tab's graph (PRD agents-graph-view) as pure functions over the
// snapshot's agent rows: a checkout is a box, an agent a row inside it, and a
// delegation into another checkout a line between two boxes. Every
// coordinate is a number computed from row counts and the geometry the
// caller passes in, so nothing here measures the page (D-31); the renderer
// draws what this returns and only interpolates between two answers (D-20).
// A value not read yet is null, never a zero (design 10).

import type { BadgeCounts } from "./agentRow";
import { rowLine } from "./agentRow";
import { cleanupOf, type AgentBucket, type Cleanup, type LensAgent } from "./overviewLens";
import type { BoardProject } from "./projectBoard";
import type { AgentRow, Checkout, Tab, Task, Workspace } from "./snapshot";
import { primaryCheckout } from "./workspaceManage";

// --- geometry -----------------------------------------------------------------

/** The sizes the layout is built from, read from the tokens by the renderer and given explicitly by a test. */
export type GraphGeometry = {
  boxWidth: number;
  boxBorder: number;
  headHeight: number;
  rowHeight: number;
  askingRowHeight: number;
  boxPadBottom: number;
  /** The gap between two columns, where the lines run. */
  columnGap: number;
  /** The gap between two boxes of one column. */
  boxGap: number;
  /** The margin around the whole graph. */
  pad: number;
  /** A port's distance below its row's top, the middle of a one-line row. */
  portOffset: number;
  /** Space kept clear of a column gap's edges, where no trunk runs. */
  trunkInset: number;
  /** The most two trunks of one gap stand apart. */
  trunkStep: number;
  /** How far a corridor keeps from the box above and below it. */
  corridorClear: number;
  /** A same-tab tray's inset from the box's sides and from its first and last row. */
  trayInsetX: number;
  trayInsetY: number;
};

// --- filter -------------------------------------------------------------------

/** The status chips (D-09): the operator's turn, working (a parent waiting on children included, D-35), resting. */
export type StatusChip = "turn" | "working" | "resting";

export const STATUS_CHIPS: readonly { chip: StatusChip; label: string }[] = [
  { chip: "turn", label: "내 차례" },
  { chip: "working", label: "일하는 중" },
  { chip: "resting", label: "쉬는 중" },
];

/** What narrows the graph (D-36): any lit chip (OR), and the search and the device (AND with the chips). */
export type GraphFilter = { chips: readonly StatusChip[]; query: string; device: string | null };

export const NO_GRAPH_FILTER: GraphFilter = { chips: [], query: "", device: null };

export function graphFilterActive(filter: GraphFilter): boolean {
  return filter.chips.length > 0 || filter.query.trim() !== "" || filter.device !== null;
}

/** The chip a bucket belongs to; a tile bar's segment lights the same one (D-35). */
export function chipOfBucket(bucket: AgentBucket): StatusChip {
  return bucket === "turn" ? "turn" : bucket === "resting" ? "resting" : "working";
}

/** A number with its hash sign and without it name one number, so a search is read without the sign. */
function searchTerm(query: string): string {
  return query.trim().toLowerCase().replace(/^#/, "");
}

/** The digits an issue or pull request is found by: a hashed id, a local `L-1` and a bare number all carry theirs. */
function numberOf(id: string | null | undefined): string | null {
  const digits = id?.match(/\d+/)?.[0];
  return digits ?? null;
}

export function matchesFilter(value: LensAgent, filter: GraphFilter): boolean {
  if (filter.chips.length > 0 && !filter.chips.includes(chipOfBucket(value.bucket))) return false;
  if (filter.device !== null && (value.device ?? THIS_DEVICE) !== filter.device) return false;
  const term = searchTerm(filter.query);
  if (term === "") return true;
  const pr = value.checkout.pull_request;
  const haystack = [value.agent.identity_label, value.checkout.branch ?? value.checkout.label, numberOf(value.task?.id), pr ? String(pr.number) : null];
  return haystack.some((part) => part?.toLowerCase().includes(term));
}

/** The device filter's name for this machine; a remote device is named by its label. */
export const THIS_DEVICE = "이 Mac";

/** The devices a scope's agents run on, this machine first; the control stands only when there are two or more (B24). */
export function graphDevices(agents: readonly LensAgent[]): string[] {
  const names = new Set(agents.map((value) => value.device ?? THIS_DEVICE));
  return [...names].sort((a, b) => Number(b === THIS_DEVICE) - Number(a === THIS_DEVICE) || a.localeCompare(b));
}

// --- the agent as the graph reads it -----------------------------------------

/** Where an agent stands for ordering and folding (B5): 0 the operator's attention, 1 working, 2 waiting on children, 3 resting. */
export type Attention = 0 | 1 | 2 | 3;

const REQUESTS = new Set(["question", "approval", "error"]);

/** The agent holds a question, an approval or an error of its own, delegated or not (B17). */
export function isAsking(agent: AgentRow): boolean {
  return REQUESTS.has(agent.demand ?? "none");
}

/** A quiet agent whose descendants are still working or asking, so it is not done (docs/status-model.md). */
export function waitsOnChildren(agent: AgentRow): boolean {
  if (agent.waiting_on_descendants) return true;
  const counts = agent.descendant_counts;
  return counts !== undefined && counts.working + counts.question + counts.approval + counts.error > 0;
}

export function attentionOf(value: LensAgent): Attention {
  const { agent, bucket } = value;
  if (isAsking(agent) || bucket === "turn") return 0;
  if (bucket === "working") return 1;
  return waitsOnChildren(agent) ? 2 : 3;
}

/** A line's colour follows the child it leads to (D-04, B9). */
export type EdgeKind = "ask" | "flow" | "wait" | "rest";

export function edgeKindOf(child: AgentRow): EdgeKind {
  if (isAsking(child)) return "ask";
  if (child.activity === "working") return "flow";
  return waitsOnChildren(child) ? "wait" : "rest";
}

/** The second line a row draws, a question in warning, only while the agent asks (D-27). */
export function askingLine(agent: AgentRow): string | null {
  return isAsking(agent) ? (rowLine(agent)?.text ?? null) : null;
}

const MARK_STATE: Record<string, keyof BadgeCounts> = { "×": "error", "!": "approval", "?": "question", "●": "working", "✓": "done", "○": "idle" };

function byAttentionThenActivity(a: LensAgent, b: LensAgent): number {
  return attentionOf(a) - attentionOf(b) || (b.agent.last_activity ?? "").localeCompare(a.agent.last_activity ?? "");
}

function latest(values: readonly LensAgent[]): string {
  return values.reduce((best, value) => ((value.agent.last_activity ?? "") > best ? (value.agent.last_activity ?? "") : best), "");
}

// --- the model ----------------------------------------------------------------

/** One agent's row in a box. */
export type GraphRow = {
  value: LensAgent;
  paneId: string;
  /** The box this row stands in. */
  box: string;
  /** Indent steps: a delegation inside one checkout is a step right under its parent (B6). */
  depth: number;
  /** The row's top inside the box, from the end of the head. */
  top: number;
  height: number;
  attention: Attention;
  /** The question line, only on an asking row. */
  line: string | null;
  /** The shared tab's key when two or more rows of this box stand in it (B7). */
  tray: string | null;
  /** The tab the agent's pane is in, for its popover. */
  tab: { label: string | null } | null;
  /** The row's parent when it is drawn, for the hover chain. */
  parent: string | null;
  /** What the folded boxes below this row hold, as the sidebar's badge counts them (B22). */
  tucked: BadgeCounts | null;
  /** Kept only to link a matching row to its parent chain, so drawn faded (B27). */
  dim: boolean;
};

export type GraphTray = { key: string; top: number; bottom: number; paneIds: string[] };

export type GraphBox = {
  id: string;
  project: Workspace;
  checkout: Checkout;
  primary: boolean;
  cleanup: Cleanup | null;
  task: Task | null;
  rows: GraphRow[];
  trays: GraphTray[];
  col: number;
  x: number;
  y: number;
  height: number;
  /** Every row rests: the box is drawn quieter (B21). */
  resting: boolean;
  dim: boolean;
};

/** A line's route as seven numbers, so a move between two layouts is seven tweens (D-20). */
export type EdgeRoute = { sx: number; sy: number; t1x: number; cy: number; t2x: number; ty: number; tx: number };

export type GraphEdge = {
  id: string;
  from: string;
  to: string;
  fromBox: string;
  toBox: string;
  kind: EdgeKind;
  /** Closes a cycle: drawn dashed and free to cross boxes (D-03, B10). */
  back: boolean;
  dim: boolean;
  route: EdgeRoute;
};

export type FoldKind = "empty" | "cleanup" | "resting";

/** A folded line (B21, B23): its words come from the renderer, its count and names from here. */
export type GraphFold = { kind: FoldKind; id: string; count: number; names: string[]; open: boolean };

export type ProjectGraph = {
  project: Workspace;
  device: string | null;
  boxes: GraphBox[];
  edges: GraphEdge[];
  folds: GraphFold[];
  /** The drawn box carrying the selection outline, if any. */
  selected: string | null;
  /** Every drawn row by pane id. */
  rows: Map<string, GraphRow>;
  width: number;
  height: number;
  /** The project's place in the order of projects (B30). */
  attention: Attention | 4;
  recency: string;
};

export type GraphBoard = {
  sections: ProjectGraph[];
  /** Nothing to draw and no filter to blame: `실행 중인 에이전트가 없습니다` (B31). */
  empty: boolean;
  /** A filter is on and nothing matches (B28). */
  filterEmpty: boolean;
};

export type GraphOptions = {
  scope: "project" | "all";
  geometry: GraphGeometry;
  /** The folds the operator opened, by `foldId`. */
  openFolds: readonly string[];
  /** The box a way in selected; a box that is not drawn leaves the project's primary box selected (B1). */
  selectedBox: string | null;
  filter: GraphFilter;
};

export function foldId(kind: FoldKind, projectId: string): string {
  return `${kind}:${projectId}`;
}

function push<K, V>(map: Map<K, V[]>, key: K, value: V): void {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

type Candidate = { checkout: Checkout; primary: boolean; cleanup: Cleanup | null; members: LensAgent[]; rank: Attention | 4 };

type Info = { box: GraphBox; best: Attention | 4; recent: string };

/** The graph of every project in scope: a project's graph apart from the others, in attention order (D-11). */
export function buildGraph(projects: readonly BoardProject[], agents: readonly LensAgent[], options: GraphOptions): GraphBoard {
  const filtering = graphFilterActive(options.filter);
  const ofProject = new Map<Workspace, LensAgent[]>();
  for (const value of agents) push(ofProject, value.project, value);
  const sections = projects
    .map((project) => projectGraph(project, ofProject.get(project.workspace) ?? [], options, filtering))
    .filter((section): section is ProjectGraph => section !== null);
  if (options.scope === "all") sections.sort((a, b) => a.attention - b.attention || b.recency.localeCompare(a.recency));
  return { sections, empty: sections.length === 0 && !filtering, filterEmpty: sections.length === 0 && filtering };
}

function projectGraph({ workspace, device }: BoardProject, members: LensAgent[], options: GraphOptions, filtering: boolean): ProjectGraph | null {
  const { geometry: g } = options;
  const primaryId = primaryCheckout(workspace)?.id ?? null;
  const tasks = new Map((workspace.tasks?.tasks ?? []).map((task) => [task.key, task]));
  const byPane = new Map(members.map((value) => [value.agent.pane_id, value]));

  // A filter keeps the matching rows and the chain of parents that leads to them (B27).
  let matched: Set<string> | null = null;
  let kept: Set<string> | null = null;
  if (filtering) {
    matched = new Set(members.filter((value) => matchesFilter(value, options.filter)).map((value) => value.agent.pane_id));
    kept = new Set(matched);
    for (const id of matched) {
      for (let parent = byPane.get(id)?.agent.lineage_parent_pane_id; parent && byPane.has(parent) && !kept.has(parent); parent = byPane.get(parent)?.agent.lineage_parent_pane_id) kept.add(parent);
    }
  }

  const byCheckout = new Map<string, LensAgent[]>();
  for (const value of members) push(byCheckout, value.checkout.id, value);

  const candidates = new Map<string, Candidate>();
  const folded: Record<FoldKind, Candidate[]> = { empty: [], cleanup: [], resting: [] };
  const foldOf = new Map<string, FoldKind>();
  const drawn: Candidate[] = [];
  for (const checkout of workspace.checkouts) {
    const primary = checkout.id === primaryId;
    const cleanup = cleanupOf(workspace, checkout);
    const own = (byCheckout.get(checkout.id) ?? []).slice().sort(byAttentionThenActivity);
    const rank = own.length === 0 ? 4 : attentionOf(own[0]!);
    const candidate: Candidate = { checkout, primary, cleanup, members: kept ? own.filter((value) => kept.has(value.agent.pane_id)) : own, rank };
    candidates.set(checkout.id, candidate);
    if (filtering) {
      // A filter draws every checkout that holds a kept row and folds nothing: a `쉬는 중` chip would otherwise show an empty graph.
      if (candidate.members.length > 0) drawn.push(candidate);
      continue;
    }
    const resting = own.every((value) => attentionOf(value) === 3);
    const alwaysDrawn = primary && options.scope === "project";
    const kind: FoldKind | null = alwaysDrawn ? null : cleanup && resting ? "cleanup" : own.length === 0 ? "empty" : resting ? "resting" : null;
    if (kind === null) drawn.push(candidate);
    else {
      folded[kind].push(candidate);
      foldOf.set(checkout.id, kind);
    }
  }

  const opened = new Set(options.openFolds);
  const foldList: GraphFold[] = (["empty", "cleanup", "resting"] as const).flatMap((kind) => {
    const list = folded[kind];
    if (list.length === 0) return [];
    const id = foldId(kind, workspace.id);
    return [{ kind, id, count: list.length, names: list.map((entry) => entry.checkout.branch ?? entry.checkout.label), open: opened.has(id) }];
  });
  for (const kind of ["empty", "cleanup", "resting"] as const) if (opened.has(foldId(kind, workspace.id))) drawn.push(...folded[kind]);
  const shownIds = new Set(drawn.map((entry) => entry.checkout.id));

  // Rows, trays and each box's own height.
  const rowOf = new Map<string, GraphRow>();
  const infos: Info[] = drawn.map((entry) => {
    const { rows, trays, inner } = rowsOf(entry, matched, g);
    for (const row of rows) rowOf.set(row.paneId, row);
    const box: GraphBox = {
      id: entry.checkout.id,
      project: workspace,
      checkout: entry.checkout,
      primary: entry.primary,
      cleanup: entry.cleanup,
      task: entry.checkout.task_key ? (tasks.get(entry.checkout.task_key) ?? null) : null,
      rows,
      trays,
      col: 0,
      x: 0,
      y: 0,
      height: 2 * g.boxBorder + g.headHeight + inner + g.boxPadBottom,
      resting: rows.every((row) => row.attention === 3),
      dim: rows.length > 0 && rows.every((row) => row.dim),
    };
    return { box, best: rows.length === 0 ? 4 : (Math.min(...rows.map((row) => row.attention)) as Attention), recent: latest(entry.members) };
  });

  // A folded box's agents are counted on the nearest drawn ancestor, as the sidebar badges its folded descendants (B22).
  if (!filtering) {
    for (const value of members) {
      if (shownIds.has(value.checkout.id)) continue;
      let ancestor = value.agent.lineage_parent_pane_id ? byPane.get(value.agent.lineage_parent_pane_id) : undefined;
      const seen = new Set<string>();
      while (ancestor && !rowOf.has(ancestor.agent.pane_id) && !seen.has(ancestor.agent.pane_id)) {
        seen.add(ancestor.agent.pane_id);
        ancestor = ancestor.agent.lineage_parent_pane_id ? byPane.get(ancestor.agent.lineage_parent_pane_id) : undefined;
      }
      const row = ancestor ? rowOf.get(ancestor.agent.pane_id) : undefined;
      const state = MARK_STATE[value.agent.symbol];
      if (!row || !state) continue;
      row.tucked = { ...row.tucked, [state]: (row.tucked?.[state] ?? 0) + 1 };
    }
  }

  const edges = edgesOf(rowOf, matched);
  place(infos, edges, rowOf, options);
  routeEdges(edges, infos, rowOf, g);

  const boxes = infos.map((info) => info.box);
  let width = 0;
  let height = 0;
  for (const box of boxes) {
    width = Math.max(width, box.x + g.boxWidth);
    height = Math.max(height, box.y + box.height);
  }
  if (boxes.length === 0 && foldList.length === 0) return null;
  const everyone = members.map((value) => attentionOf(value));
  return {
    project: workspace,
    device,
    boxes,
    edges,
    folds: filtering ? [] : foldList,
    selected: options.selectedBox === null ? null : shownIds.has(options.selectedBox) ? options.selectedBox : (drawn.find((entry) => entry.primary)?.checkout.id ?? null),
    rows: rowOf,
    width: boxes.length === 0 ? 0 : width + g.pad,
    height: boxes.length === 0 ? 0 : height + g.pad,
    attention: everyone.length === 0 ? 4 : (Math.min(...everyone) as Attention),
    recency: latest(members),
  };
}

/** One box's rows: agents sharing a tab stand together on one tray and a delegation inside the box is indented under its parent (B6, B7). */
function rowsOf(entry: Candidate, matched: ReadonlySet<string> | null, g: GraphGeometry): { rows: GraphRow[]; trays: GraphTray[]; inner: number } {
  const { checkout, members } = entry;
  const inBox = new Set(members.map((value) => value.agent.pane_id));
  const tabOf = new Map<string, Tab>();
  for (const tab of checkout.tabs) for (const pane of tab.panes) if (!tabOf.has(pane.id)) tabOf.set(pane.id, tab);
  const children = new Map<string, LensAgent[]>();
  const roots: LensAgent[] = [];
  for (const value of members) {
    const parent = value.agent.lineage_parent_pane_id;
    if (parent && parent !== value.agent.pane_id && inBox.has(parent)) push(children, parent, value);
    else roots.push(value);
  }
  const emitted = new Set<string>();
  const out: { value: LensAgent; depth: number; trayKey: string | null }[] = [];
  const emit = (agents: readonly LensAgent[], depth: number) => {
    const clusters = new Map<string, LensAgent[]>();
    for (const value of agents) {
      if (emitted.has(value.agent.pane_id)) continue;
      const tab = tabOf.get(value.agent.pane_id);
      push(clusters, tab?.id ? `${checkout.id}/${tab.id}` : `solo/${value.agent.pane_id}`, value);
    }
    for (const [key, group] of clusters) {
      for (const value of group) {
        emitted.add(value.agent.pane_id);
        out.push({ value, depth, trayKey: group.length > 1 ? key : null });
      }
      for (const value of group) emit(children.get(value.agent.pane_id) ?? [], depth + 1);
    }
  };
  emit(roots, 0);
  // A cycle inside one checkout has no root; its members are drawn last as roots.
  emit(members.filter((value) => !emitted.has(value.agent.pane_id)), 0);

  let top = 0;
  const rows: GraphRow[] = [];
  const trays = new Map<string, GraphTray>();
  for (const item of out) {
    const { value } = item;
    const line = askingLine(value.agent);
    const height = line ? g.askingRowHeight : g.rowHeight;
    const parentId = value.agent.lineage_parent_pane_id ?? null;
    const tab = tabOf.get(value.agent.pane_id);
    const row: GraphRow = {
      value,
      paneId: value.agent.pane_id,
      box: checkout.id,
      depth: item.depth,
      top,
      height,
      attention: attentionOf(value),
      line,
      tray: item.trayKey,
      tab: tab ? { label: tab.label } : null,
      parent: parentId,
      tucked: null,
      dim: matched ? !matched.has(value.agent.pane_id) : false,
    };
    rows.push(row);
    top += height;
    if (item.trayKey) {
      const tray = trays.get(item.trayKey) ?? { key: item.trayKey, top: row.top, bottom: 0, paneIds: [] };
      tray.bottom = top;
      tray.paneIds.push(row.paneId);
      trays.set(item.trayKey, tray);
    }
  }
  return { rows, trays: [...trays.values()], inner: top };
}

/** A delegation into another drawn checkout is a line; one inside a checkout is an indent (B6, B8). */
function edgesOf(rowOf: ReadonlyMap<string, GraphRow>, matched: ReadonlySet<string> | null): GraphEdge[] {
  const edges: GraphEdge[] = [];
  for (const row of rowOf.values()) {
    const parent = row.parent ? rowOf.get(row.parent) : undefined;
    if (!parent || parent.box === row.box) continue;
    edges.push({
      id: `${parent.paneId}>${row.paneId}`,
      from: parent.paneId,
      to: row.paneId,
      fromBox: parent.box,
      toBox: row.box,
      kind: edgeKindOf(row.value.agent),
      back: false,
      dim: matched ? !(matched.has(parent.paneId) && matched.has(row.paneId)) : false,
      route: { sx: 0, sy: 0, t1x: 0, cy: 0, t2x: 0, ty: 0, tx: 0 },
    });
  }
  return edges;
}

/**
 * Columns, bands and positions (D-03, D-08). A box stands one column right of
 * its deepest delegating box, a cycle cut where it closes and its closing line
 * marked `back`. Each first-column box and everything it delegated form one
 * horizontal band, a child box standing level with the row that delegated it
 * so the line stays short; bands do not overlap.
 */
function place(infos: Info[], edges: GraphEdge[], rowOf: ReadonlyMap<string, GraphRow>, options: GraphOptions): void {
  const g = options.geometry;
  const byId = new Map(infos.map((info) => [info.box.id, info]));
  const incoming = new Map<string, string[]>();
  for (const edge of edges) {
    const list = incoming.get(edge.toBox) ?? [];
    if (!list.includes(edge.fromBox)) list.push(edge.fromBox);
    incoming.set(edge.toBox, list);
  }
  const column = new Map<string, number>();
  const visiting = new Set<string>();
  const columnOf = (id: string): number => {
    const known = column.get(id);
    if (known !== undefined) return known;
    if (visiting.has(id)) return -1;
    visiting.add(id);
    let col = 0;
    for (const from of incoming.get(id) ?? []) col = Math.max(col, columnOf(from) + 1);
    visiting.delete(id);
    column.set(id, col);
    return col;
  };
  for (const info of infos) info.box.col = Math.max(0, columnOf(info.box.id));
  for (const edge of edges) edge.back = (byId.get(edge.toBox)?.box.col ?? 0) <= (byId.get(edge.fromBox)?.box.col ?? 0);

  const maxCol = Math.max(0, ...infos.map((info) => info.box.col));
  const cursor: number[] = Array.from({ length: maxCol + 1 }, () => g.pad);
  const portY = new Map<string, number>();
  const placed = new Set<string>();
  const anchorOf = (info: Info): number => {
    let best = Infinity;
    for (const row of info.box.rows) {
      const parent = row.parent ? rowOf.get(row.parent) : undefined;
      const above = parent && parent.box !== info.box.id ? portY.get(parent.paneId) : undefined;
      if (above !== undefined) best = Math.min(best, above - (g.boxBorder + g.headHeight + row.top + g.portOffset));
    }
    return best;
  };
  const stand = (info: Info) => {
    const { box } = info;
    const anchor = anchorOf(info);
    box.x = g.pad + box.col * (g.boxWidth + g.columnGap);
    box.y = Math.max(cursor[box.col]!, Number.isFinite(anchor) ? anchor : cursor[box.col]!);
    for (const row of box.rows) portY.set(row.paneId, box.y + g.boxBorder + g.headHeight + row.top + g.portOffset);
    cursor[box.col] = box.y + box.height + g.boxGap;
    placed.add(box.id);
  };
  const roots = infos
    .filter((info) => info.box.col === 0)
    .sort((a, b) => (options.scope === "project" ? Number(b.box.primary) - Number(a.box.primary) : 0) || a.best - b.best || Number(b.box.primary) - Number(a.box.primary) || b.recent.localeCompare(a.recent));
  for (const root of roots) {
    cursor.fill(Math.max(...cursor));
    stand(root);
    for (let col = 1; col <= maxCol; col++) {
      infos
        .filter((info) => info.box.col === col && !placed.has(info.box.id) && Number.isFinite(anchorOf(info)))
        .map((info) => ({ info, anchor: anchorOf(info) }))
        .sort((a, b) => a.anchor - b.anchor || a.info.best - b.info.best)
        .forEach(({ info }) => stand(info));
    }
  }
  for (const info of infos) if (!placed.has(info.box.id)) stand(info);
}

/**
 * Every forward line runs `S, (t1x, sy), (t1x, cy), (t2x, cy), (t2x, ty), T`:
 * its trunks stand in the gaps between columns, and when the target is more
 * than one column away the run across the columns between uses a corridor
 * that no box of those columns covers (B8). A line to the next column is the
 * three-segment line with `cy = ty`. A trunk lane is shared by one parent's
 * lines, and the higher the parent the further right its lane, so a lower
 * parent's way out never crosses a higher one's trunk.
 */
function routeEdges(edges: GraphEdge[], infos: readonly Info[], rowOf: ReadonlyMap<string, GraphRow>, g: GraphGeometry): void {
  const box = new Map(infos.map((info) => [info.box.id, info.box]));
  const portOf = (row: GraphRow): number => (box.get(row.box)?.y ?? 0) + g.boxBorder + g.headHeight + row.top + g.portOffset;
  const forward = edges.filter((edge) => !edge.back);
  const gapRight = (gap: number) => g.pad + gap * (g.boxWidth + g.columnGap) + g.boxWidth;
  const users = new Map<number, Map<string, number>>();
  const use = (gap: number, key: string, y: number) => {
    const list = users.get(gap) ?? new Map<string, number>();
    list.set(key, y);
    users.set(gap, list);
  };
  for (const edge of forward) {
    const a = box.get(edge.fromBox)!.col;
    const b = box.get(edge.toBox)!.col;
    const sy = portOf(rowOf.get(edge.from)!);
    use(a, `dep:${edge.from}`, sy);
    if (b - a >= 2) use(b - 1, `arr:${edge.from}`, sy);
  }
  const lane = new Map<string, number>();
  for (const [gap, list] of users) {
    const ordered = [...list].sort((p, q) => p[1] - q[1] || p[0].localeCompare(q[0]));
    const n = ordered.length;
    const step = n > 1 ? Math.max(0, Math.min(g.trunkStep, (g.columnGap - 2 * g.trunkInset) / (n - 1))) : 0;
    ordered.forEach(([key], index) => lane.set(`${gap}|${key}`, gapRight(gap) + g.columnGap / 2 + ((n - 1) / 2 - index) * step));
  }

  const bottom = Math.max(0, ...infos.map((info) => info.box.y + info.box.height));
  for (const edge of edges) {
    const from = box.get(edge.fromBox)!;
    const to = box.get(edge.toBox)!;
    const sx = from.x + g.boxWidth;
    const sy = portOf(rowOf.get(edge.from)!);
    const tx = to.x;
    const ty = portOf(rowOf.get(edge.to)!);
    if (edge.back) {
      edge.route = { sx, sy, t1x: sx, cy: sy, t2x: tx, ty, tx };
      continue;
    }
    const t1x = lane.get(`${from.col}|dep:${edge.from}`)!;
    const long = to.col - from.col >= 2;
    const t2x = long ? lane.get(`${to.col - 1}|arr:${edge.from}`)! : t1x;
    const cy = long ? corridor(infos, from.col, to.col, sy, ty, bottom, g) : ty;
    edge.route = { sx, sy, t1x, cy, t2x, ty, tx };
  }
}

/**
 * The height at which a line crosses the columns strictly between `a` and `b`:
 * the target's own height when no box there covers it, else the source's,
 * else the free gap between boxes (or above or below them all) nearest the
 * target.
 */
function corridor(infos: readonly Info[], a: number, b: number, sy: number, ty: number, bottom: number, g: GraphGeometry): number {
  const blocking = infos.filter((info) => info.box.col > a && info.box.col < b).map((info) => [info.box.y, info.box.y + info.box.height] as const);
  const free = (y: number) => blocking.every(([top, end]) => y < top - g.corridorClear || y > end + g.corridorClear);
  if (free(ty)) return ty;
  if (free(sy)) return sy;
  const edges = blocking.slice().sort((p, q) => p[0] - q[0]);
  const candidates = [g.pad / 2, bottom + g.pad / 2];
  const merged: [number, number][] = [];
  for (const [top, end] of edges) {
    const last = merged[merged.length - 1];
    if (last && top <= last[1] + 2 * g.corridorClear) last[1] = Math.max(last[1], end);
    else merged.push([top, end]);
  }
  for (let i = 0; i + 1 < merged.length; i++) candidates.push((merged[i]![1] + merged[i + 1]![0]) / 2);
  const open = candidates.filter(free);
  return open.sort((p, q) => Math.abs(p - ty) - Math.abs(q - ty) || Math.abs(p - sy) - Math.abs(q - sy))[0] ?? bottom + g.pad / 2;
}

// --- the line as a path -------------------------------------------------------

/** The corner radius of a line's bends. */
export const EDGE_RADIUS = 10;

/** The route's polyline, repeated and collinear points dropped. */
export function routePoints(route: EdgeRoute): [number, number][] {
  const { sx, sy, t1x, cy, t2x, ty, tx } = route;
  const raw: [number, number][] = [[sx, sy], [t1x, sy], [t1x, cy], [t2x, cy], [t2x, ty], [tx, ty]];
  const points: [number, number][] = [];
  for (const point of raw) {
    const last = points[points.length - 1];
    if (last && Math.abs(last[0] - point[0]) < 0.01 && Math.abs(last[1] - point[1]) < 0.01) continue;
    points.push(point);
  }
  // Remove a middle point lying on the line between its neighbours.
  for (let i = points.length - 2; i >= 1; i--) {
    const [px, py] = points[i - 1]!;
    const [x, y] = points[i]!;
    const [nx, ny] = points[i + 1]!;
    if ((Math.abs(px - x) < 0.01 && Math.abs(x - nx) < 0.01) || (Math.abs(py - y) < 0.01 && Math.abs(y - ny) < 0.01)) points.splice(i, 1);
  }
  return points;
}

/** A forward line's path: straight runs with rounded bends. */
export function forwardPath(route: EdgeRoute, radius: number = EDGE_RADIUS): string {
  const points = routePoints(route);
  const first = points[0]!;
  let d = `M ${first[0]} ${first[1]}`;
  for (let i = 1; i < points.length - 1; i++) {
    const [px, py] = points[i - 1]!;
    const [x, y] = points[i]!;
    const [nx, ny] = points[i + 1]!;
    const before = Math.hypot(x - px, y - py);
    const after = Math.hypot(nx - x, ny - y);
    const r = Math.min(radius, before / 2, after / 2);
    const inX = x + ((px - x) / before) * r;
    const inY = y + ((py - y) / before) * r;
    const outX = x + ((nx - x) / after) * r;
    const outY = y + ((ny - y) / after) * r;
    d += ` L ${inX} ${inY} Q ${x} ${y} ${outX} ${outY}`;
  }
  const last = points[points.length - 1]!;
  return `${d} L ${last[0]} ${last[1]}`;
}

/** A closing line's path: a curve out of the parent's port into the child's, free to cross boxes. */
export function backPath(route: EdgeRoute, reach: number): string {
  return `M ${route.sx} ${route.sy} C ${route.sx + reach} ${route.sy}, ${route.tx - reach} ${route.ty}, ${route.tx} ${route.ty}`;
}

// --- the hover chain ----------------------------------------------------------

/** An agent with its ancestors and descendants as drawn, the chain a hover keeps bright (B19). */
export function chainOf(section: ProjectGraph, paneId: string): Set<string> {
  const chain = new Set<string>([paneId]);
  for (let row = section.rows.get(paneId); row?.parent && section.rows.has(row.parent) && !chain.has(row.parent); row = section.rows.get(row.parent)) chain.add(row.parent);
  const children = new Map<string, string[]>();
  for (const row of section.rows.values()) if (row.parent) push(children, row.parent, row.paneId);
  const down = (id: string) => {
    for (const child of children.get(id) ?? []) {
      if (chain.has(child)) continue;
      chain.add(child);
      down(child);
    }
  };
  down(paneId);
  return chain;
}

// --- motion targets -----------------------------------------------------------

/** Every number the renderer animates, by name (D-20): equal maps mean nothing moved. */
export function graphTargets(section: ProjectGraph): Map<string, number> {
  const targets = new Map<string, number>();
  for (const box of section.boxes) {
    targets.set(`bx:${box.id}`, box.x);
    targets.set(`by:${box.id}`, box.y);
    targets.set(`bh:${box.id}`, box.height);
    for (const row of box.rows) targets.set(`rt:${row.paneId}`, row.top);
    for (const tray of box.trays) {
      targets.set(`tt:${tray.key}`, tray.top);
      targets.set(`th:${tray.key}`, tray.bottom - tray.top);
    }
  }
  for (const edge of section.edges) for (const [name, value] of Object.entries(edge.route)) targets.set(`e:${edge.id}:${name}`, value);
  targets.set("gw", section.width);
  targets.set("gh", section.height);
  return targets;
}

/** A route read back out of the animated numbers. */
export function routeFrom(values: ReadonlyMap<string, number>, id: string): EdgeRoute {
  const read = (name: keyof EdgeRoute) => values.get(`e:${id}:${name}`) ?? 0;
  return { sx: read("sx"), sy: read("sy"), t1x: read("t1x"), cy: read("cy"), t2x: read("t2x"), ty: read("ty"), tx: read("tx") };
}

/** The box a way in selects (D-22): the checkout in front when it is this project's, else the primary one. */
export function entryBox(workspace: Workspace | null | undefined, frontCheckoutId: string | null | undefined): string | null {
  if (!workspace) return null;
  const front = frontCheckoutId ? workspace.checkouts.find((checkout) => checkout.id === frontCheckoutId) : undefined;
  return front?.id ?? primaryCheckout(workspace)?.id ?? workspace.checkouts[0]?.id ?? null;
}
