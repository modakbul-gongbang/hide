import { CornerDownRightIcon, GitMergeIcon, SearchIcon, XIcon } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  backPath,
  buildGraph,
  chainOf,
  forwardPath,
  graphDevices,
  graphFilterActive,
  graphTargets,
  NO_GRAPH_FILTER,
  routeFrom,
  STATUS_CHIPS,
  THIS_DEVICE,
  type EdgeKind,
  type FoldKind,
  type GraphBox,
  type GraphEdge,
  type GraphFilter,
  type GraphGeometry,
  type GraphRow,
  type ProjectGraph,
  type StatusChip,
} from "./agentGraph";
import { AgentMark } from "./AgentMark";
import { badgeParts, badgeWords, lineTone, markTone, rowAccessibleName, rowLine } from "./agentRow";
import { CHECKOUT_KIND_ICON } from "./components/checkout-icon";
import { CheckoutCardHint } from "./components/pr-card";
import { StatusMark } from "./components/status-mark";
import { BadgeMarks } from "./components/status-badge";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import { readGraphGeometry, readMotionMs } from "./graphGeometry";
import { Tween, type Targets } from "./graphMotion";
import { cn } from "./lib/utils";
import { FoldLine, gitHubClick, IssueChip, AgentMessagePopover, PullRequestChip, type LensHandlers } from "./OverviewLenses";
import type { LensAgent } from "./overviewLens";
import type { BoardProject } from "./projectBoard";
import { checkoutPresentation, distanceText, filesText, laneCheckoutCard, shownPullRequest } from "./projects";

// The Agents tab's graph (PRD agents-graph-view): a checkout is a box, an agent
// a row in it, a delegation into another checkout a line. `agentGraph.ts`
// computes every number; React renders the structure here, and `graphMotion.ts`
// writes the positions straight into the DOM so a glide never re-renders and a
// snapshot that changes nothing draws nothing (B33, B39). Hover, focus and the
// half-second popover are local and publish nothing (B38).

const FOLD_LABEL: Record<FoldKind, string> = { empty: "에이전트 없는 워크트리", cleanup: "정리할 것", resting: "쉬는 체크아웃" };

const EDGE_WORDS: Record<EdgeKind, string> = { ask: "묻는 중", flow: "일하는 중", wait: "하위를 기다리는 중", rest: "쉬는 중" };

const CLEANUP_HELP = {
  merged: "머지됐거나 폴더가 없는 워크트리와 거기서 쉬는 에이전트를 지운다. 확인 대화상자가 먼저 뜬다.",
  missing: "폴더가 없는 워크트리의 기록을 지운다.",
} as const;

// --- painting ------------------------------------------------------------------

type EdgeParts = { back: boolean; paths: SVGPathElement[]; out: SVGCircleElement | null; into: SVGCircleElement | null };

/** The elements whose places `paintGraph` writes, found once per render. */
type PaintIndex = {
  canvas: HTMLElement;
  boxes: Map<string, HTMLElement>;
  rows: Map<string, HTMLElement>;
  trays: Map<string, HTMLElement>;
  edges: Map<string, EdgeParts>;
};

function indexCanvas(canvas: HTMLElement, edges: readonly GraphEdge[]): PaintIndex {
  const byAttribute = (attribute: string) => new Map([...canvas.querySelectorAll<HTMLElement>(`[${attribute}]`)].map((element) => [element.getAttribute(attribute) ?? "", element]));
  const drawn = new Map<string, EdgeParts>();
  for (const edge of edges) {
    const group = canvas.querySelector(`[data-graph-edge="${CSS.escape(edge.id)}"]`);
    if (!group) continue;
    drawn.set(edge.id, {
      back: edge.back,
      paths: [...group.querySelectorAll<SVGPathElement>("path")],
      out: group.querySelector<SVGCircleElement>('[data-graph-port="out"]'),
      into: group.querySelector<SVGCircleElement>('[data-graph-port="in"]'),
    });
  }
  return { canvas, boxes: byAttribute("data-graph-box"), rows: byAttribute("data-graph-row"), trays: byAttribute("data-graph-tray"), edges: drawn };
}

/** Writes one picture of the graph into the DOM: a transform and a size per element, a path per line. */
function paintGraph(index: PaintIndex, values: Targets, g: GraphGeometry): void {
  const at = (key: string) => values.get(key) ?? 0;
  index.canvas.style.width = `${at("gw")}px`;
  index.canvas.style.height = `${at("gh")}px`;
  for (const [id, element] of index.boxes) {
    element.style.transform = `translate(${at(`bx:${id}`)}px, ${at(`by:${id}`)}px)`;
    element.style.height = `${at(`bh:${id}`)}px`;
  }
  for (const [id, element] of index.rows) element.style.transform = `translateY(${at(`rt:${id}`)}px)`;
  for (const [key, element] of index.trays) {
    element.style.transform = `translateY(${at(`tt:${key}`) - g.trayInsetY}px)`;
    element.style.height = `${at(`th:${key}`) + 2 * g.trayInsetY}px`;
  }
  for (const [id, parts] of index.edges) {
    const route = routeFrom(values, id);
    const d = parts.back ? backPath(route, g.columnGap) : forwardPath(route);
    for (const path of parts.paths) path.setAttribute("d", d);
    parts.out?.setAttribute("cx", String(route.sx));
    parts.out?.setAttribute("cy", String(route.sy));
    parts.into?.setAttribute("cx", String(route.tx));
    parts.into?.setAttribute("cy", String(route.ty));
  }
}

/** A box or line new to the picture enters; the class ends with its animation. */
function enter(index: PaintIndex, key: string): void {
  let target: Element | null | undefined = null;
  let className = "graph-enter";
  if (key.startsWith("bx:")) target = index.boxes.get(key.slice(3));
  else if (key.startsWith("e:") && key.endsWith(":sx")) {
    target = index.edges.get(key.slice(2, -3))?.paths[0];
    className = "graph-draw";
  }
  if (!target) return;
  target.classList.add(className);
  target.addEventListener("animationend", () => target.classList.remove(className), { once: true });
}

// --- keyboard ----------------------------------------------------------------

/**
 * Arrow keys between the graph's heads, rows, chips and fold lines (B35): the
 * nearest one in the pressed direction by where it is drawn, so a box, a
 * column and a band all move the way they look. Enter is the button's click.
 */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
  const direction = { ArrowRight: [1, 0], ArrowLeft: [-1, 0], ArrowDown: [0, 1], ArrowUp: [0, -1] }[event.key];
  if (!direction || event.metaKey || event.ctrlKey || event.altKey) return;
  const from = (event.target as HTMLElement).closest<HTMLElement>("[data-graph-focus]");
  if (!from) return;
  const [dx, dy] = direction as [number, number];
  const origin = from.getBoundingClientRect();
  const ox = origin.left + origin.width / 2;
  const oy = origin.top + origin.height / 2;
  let best: { element: HTMLElement; score: number } | null = null;
  for (const element of event.currentTarget.querySelectorAll<HTMLElement>("[data-graph-focus]")) {
    if (element === from) continue;
    const rect = element.getBoundingClientRect();
    const x = rect.left + rect.width / 2 - ox;
    const y = rect.top + rect.height / 2 - oy;
    const along = x * dx + y * dy;
    if (along <= 1) continue;
    const across = Math.abs(dx !== 0 ? y : x);
    const score = along + across * 2;
    if (!best || score < best.score) best = { element, score };
  }
  if (!best) return;
  event.preventDefault();
  best.element.focus();
  best.element.scrollIntoView({ block: "nearest", inline: "nearest" });
}

// --- the graph -----------------------------------------------------------------

export type AgentGraphProps = {
  projects: readonly BoardProject[];
  agents: readonly LensAgent[];
  scope: "project" | "all";
  /** The box a way in selected, outlined and brought into view (B1). */
  selectedBox: string | null;
  filter: GraphFilter;
  onFilter: (filter: GraphFilter) => void;
  /** The folds the operator opened, by `foldId`. */
  folds: readonly string[];
  handlers: LensHandlers;
  now: number;
};

export function AgentGraph({ projects, agents, scope, selectedBox, filter, onFilter, folds, handlers, now }: AgentGraphProps) {
  // The sizes are tokens, read once: the layout is numbers (D-31).
  const [geometry] = useState(() => readGraphGeometry());
  const [motionMs] = useState(() => readMotionMs());
  const board = useMemo(() => buildGraph(projects, agents, { scope, geometry, openFolds: folds, selectedBox, filter }), [projects, agents, scope, geometry, folds, selectedBox, filter]);
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!selectedBox) return;
    root.current?.querySelector(`[data-graph-box="${CSS.escape(selectedBox)}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [selectedBox]);
  if (board.empty) {
    return (
      <div className="flex flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-graph-empty="true">
        <p>실행 중인 에이전트가 없습니다</p>
      </div>
    );
  }
  if (board.filterEmpty) {
    return (
      <div className="flex items-center justify-center gap-sm p-xl text-caption text-muted-foreground" data-graph-filter-empty="true">
        <p>필터에 맞는 에이전트가 없습니다</p>
        <Button variant="ghost" size="sm" onClick={() => onFilter(NO_GRAPH_FILTER)} data-graph-filter-clear="true">
          필터 해제
        </Button>
      </div>
    );
  }
  return (
    <div ref={root} className="flex min-w-0 flex-col gap-lg pb-xl" data-graph={scope} onKeyDown={moveFocus}>
      {board.sections.map((section) => (
        <GraphSection key={section.project.id} section={section} scope={scope} geometry={geometry} motionMs={motionMs} selectedBox={selectedBox} handlers={handlers} now={now} />
      ))}
    </div>
  );
}

function GraphSection({ section, scope, geometry, motionMs, selectedBox, handlers, now }: { section: ProjectGraph; scope: "project" | "all"; geometry: GraphGeometry; motionMs: number; selectedBox: string | null; handlers: LensHandlers; now: number }) {
  const canvas = useRef<HTMLDivElement>(null);
  const tween = useRef<Tween | null>(null);
  const index = useRef<PaintIndex | null>(null);
  const settled = useRef(false);
  const painted = useRef<HTMLElement | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const chain = useMemo(() => (hover ? chainOf(section, hover) : null), [section, hover]);
  const targets = useMemo(() => graphTargets(section), [section]);
  const hasCanvas = section.boxes.length > 0;

  useLayoutEffect(() => {
    const element = canvas.current;
    if (!element) return;
    index.current = indexCanvas(element, section.edges);
    tween.current ??= new Tween({
      durationMs: motionMs,
      reduced: () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      paint: (values) => {
        if (index.current) paintGraph(index.current, values, geometry);
      },
    });
    const { changed, added } = tween.current.retarget(targets);
    // A canvas that came back after the graph had gone is a new element holding nothing yet.
    if (!changed && painted.current !== element) tween.current.repaint();
    painted.current = element;
    element.dataset.graphRevision = String(tween.current.revision);
    if (changed && settled.current) for (const key of added) enter(index.current, key);
    settled.current = true;
  }, [section, targets, geometry, motionMs, hasCanvas]);
  useLayoutEffect(() => () => tween.current?.dispose(), []);

  const names = useMemo(() => new Map([...section.rows.values()].map((row) => [row.paneId, row.value.agent.identity_label])), [section]);
  const label = (kind: FoldKind) => FOLD_LABEL[kind];
  return (
    <section className="flex min-w-0 flex-col gap-sm" data-graph-section={section.project.id} aria-label={scope === "all" ? section.project.label : undefined}>
      {scope === "all" ? (
        <h2 className="flex min-w-0 items-center gap-sm px-lg text-body font-medium text-foreground" data-graph-project={section.project.id}>
          <span className="min-w-0 truncate">{section.project.label}</span>
          {section.device ? <Badge variant="secondary">{section.device}</Badge> : null}
        </h2>
      ) : null}
      {hasCanvas ? (
        <div className="min-w-0 overflow-x-auto px-lg">
          <div ref={canvas} className="relative" data-graph-canvas={section.project.id} data-graph-revision="0">
            <svg className="pointer-events-none absolute left-0 top-0 size-full overflow-visible">
              {section.edges.map((edge) => {
                const faded = edge.dim || (chain !== null && !(chain.has(edge.from) && chain.has(edge.to)));
                return (
                  <g key={edge.id} className={cn("graph-edge", faded && "opacity-(--opacity-dimmed)")} data-graph-edge={edge.id} data-edge-kind={edge.kind} data-edge-back={edge.back ? "true" : undefined}>
                    <title>{`${names.get(edge.from) ?? ""} → ${names.get(edge.to) ?? ""} · ${EDGE_WORDS[edge.kind]}`}</title>
                    <path className="graph-edge-base" fill="none" stroke="currentColor" pathLength={1} />
                    {edge.kind === "flow" && !edge.back ? <path className="graph-edge-flow" fill="none" /> : null}
                    <circle className="graph-port" data-graph-port="out" fill="currentColor" />
                    <circle className="graph-port" data-graph-port="in" fill="currentColor" />
                  </g>
                );
              })}
            </svg>
            {section.boxes.map((box) => (
              <BoxView key={box.id} box={box} selected={box.id === selectedBox} chain={chain} hover={hover} names={names} onHover={setHover} handlers={handlers} now={now} />
            ))}
          </div>
        </div>
      ) : null}
      {section.folds.length > 0 ? (
        <div className="flex flex-col gap-sm px-lg">
          {section.folds.map((fold) => (
            <FoldLine key={fold.id} fold={fold.id} kind={fold.kind} label={label(fold.kind)} count={fold.count} names={fold.names} open={fold.open} onToggle={() => handlers.toggleFold(fold.id)} />
          ))}
        </div>
      ) : null}
    </section>
  );
}

// --- a box ---------------------------------------------------------------------

function BoxView({ box, selected, chain, hover, names, onHover, handlers, now }: { box: GraphBox; selected: boolean; chain: Set<string> | null; hover: string | null; names: ReadonlyMap<string, string>; onHover: (paneId: string | null) => void; handlers: LensHandlers; now: number }) {
  const peers = useMemo(() => new Map(box.rows.map((row) => [row.paneId, row.tray ? box.rows.filter((other) => other.tray === row.tray && other.paneId !== row.paneId).map((other) => other.value.agent.identity_label) : []])), [box]);
  return (
    <div
      className={cn(
        "absolute left-0 top-0 w-(--graph-box-width) rounded-md border bg-card",
        selected ? "border-primary" : "border-border",
        box.resting && "opacity-(--opacity-secondary)",
        box.dim && "opacity-(--opacity-dimmed)",
      )}
      data-graph-box={box.id}
      data-graph-col={box.col}
      data-graph-primary={box.primary ? "true" : undefined}
      data-selected={selected ? "true" : undefined}
    >
      <BoxHead box={box} handlers={handlers} now={now} />
      <div className="absolute inset-x-0 top-(--graph-head-height)">
        {box.trays.map((tray) => (
          <div
            key={tray.key}
            aria-hidden="true"
            className={cn("pointer-events-none absolute inset-x-(--graph-tray-inset-x) top-0 rounded-sm border bg-muted", hover !== null && tray.paneIds.includes(hover) ? "border-muted-foreground" : "border-border")}
            data-graph-tray={tray.key}
          />
        ))}
        {box.rows.map((row) => (
          <RowView key={row.paneId} row={row} faded={chain !== null && !chain.has(row.paneId)} peers={peers.get(row.paneId) ?? []} parent={row.parent ? (names.get(row.parent) ?? null) : null} onHover={onHover} handlers={handlers} />
        ))}
      </div>
    </div>
  );
}

/**
 * A box's head (B12-B16): the kind glyph in its pull request's colour and the
 * branch, the purpose (else the pull request's title), then the issue chip,
 * the PR chip, `↑N ↓N` and the changed files; the primary checkout's says how
 * many agents it has. A merged worktree is dimmed with the merge glyph, a
 * folder-less one is `× 폴더 없음`, and both offer `정리`. The head is one
 * button to the Workspace; `↵ Workspace` appears over the end of the branch
 * line without moving it, and resting on the head opens the checkout card.
 */
function BoxHead({ box, handlers, now }: { box: GraphBox; handlers: LensHandlers; now: number }) {
  const { project, checkout, cleanup } = box;
  const view = checkoutPresentation(project, checkout, now);
  const pr = view.pullRequest;
  const Glyph = cleanup === "missing" ? XIcon : cleanup === "merged" && !pr ? GitMergeIcon : CHECKOUT_KIND_ICON[view.kind];
  const glyphTone = cleanup === "missing" ? "text-destructive" : cleanup === "merged" && !pr ? "text-pr-merged" : view.kindTone;
  const name = checkout.branch ?? checkout.label;
  const purpose = checkout.purpose?.text ?? pr?.title ?? null;
  const worktree = checkout.worktree;
  const unread = project.is_git === true && !worktree && checkout.exists;
  const ahead = checkout.ahead ?? 0;
  const behind = worktree?.behind_upstream ?? 0;
  const files = worktree?.changed_file_count ?? 0;
  const open = () => handlers.openCheckout(project, checkout);
  return (
    <div className={cn("group/head absolute inset-x-0 top-0 flex h-(--graph-head-height) min-w-0 flex-col justify-center px-sm", cleanup && "opacity-(--opacity-secondary)")} data-graph-head={checkout.id}>
      <CheckoutCardHint card={laneCheckoutCard(project, checkout, now)} description={view.detail} onOpenPullRequest={(url) => handlers.openGitHub(url, project.device_id)} onOpenWorkspace={open}>
        <button
          type="button"
          aria-label={`Workspace ${name}${purpose ? ` · ${purpose}` : ""}`}
          data-graph-focus="head"
          data-graph-head-open={checkout.id}
          className="absolute inset-0 rounded-t-md outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={(event) => {
            if (!gitHubClick(event, pr?.url ?? box.task?.url, project.device_id, handlers)) open();
          }}
        />
      </CheckoutCardHint>
      <span className="pointer-events-none relative flex h-(--graph-head-line) min-w-0 items-center gap-xs">
        <Glyph aria-hidden="true" className={cn("size-(--size-checkout-icon) shrink-0", glyphTone)} />
        <span className="min-w-0 flex-1 truncate font-mono text-body text-foreground">{name}</span>
        <span className="absolute inset-y-0 right-0 hidden items-center bg-accent pl-xs text-caption text-foreground group-hover/head:flex group-focus-within/head:flex" data-graph-head-hint="true">
          ↵ Workspace
        </span>
      </span>
      <span className="pointer-events-none relative h-(--graph-head-line) truncate text-caption text-muted-foreground">{purpose}</span>
      <span className="pointer-events-none relative flex h-(--graph-head-line) min-w-0 items-center gap-sm font-mono text-caption text-muted-foreground">
        {box.primary ? (
          <span data-graph-head-agents={box.rows.filter((row) => !row.dim).length}>에이전트 {box.rows.filter((row) => !row.dim).length}</span>
        ) : cleanup === "missing" ? (
          <span className="text-destructive">폴더 없음</span>
        ) : (
          <>
            {box.task ? <IssueChip project={project} task={box.task} handlers={handlers} now={now} /> : null}
            <PullRequestChip project={project} checkout={checkout} onOpen={(url) => handlers.openGitHub(url, project.device_id)} onRow={(number) => handlers.openPullRequestRow(project, number)} now={now} />
            {ahead > 0 || behind > 0 ? <span data-graph-head-distance={`${ahead}:${behind}`}>{distanceText(ahead, behind)}</span> : null}
            {unread ? (
              <Hint label="Git 상태를 아직 읽지 못함">
                <span className="pointer-events-auto relative z-10" data-graph-head-files="unread">
                  ?
                </span>
              </Hint>
            ) : files > 0 ? (
              <span className={cn(worktree?.dirty && "text-warning")} data-graph-head-files={files}>
                {filesText(files)}
              </span>
            ) : null}
          </>
        )}
        <span className="flex-1" />
        {cleanup ? (
          <Hint label={CLEANUP_HELP[cleanup]}>
            <button
              type="button"
              data-graph-cleanup={checkout.id}
              data-graph-focus="cleanup"
              className="pointer-events-auto relative z-10 shrink-0 rounded-xs px-xs font-sans text-caption text-subtle-foreground outline-none hover:bg-secondary hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
              onClick={(event) => {
                event.stopPropagation();
                handlers.cleanup(project, checkout);
              }}
            >
              정리
            </button>
          </Hint>
        ) : null}
      </span>
    </div>
  );
}

// --- a row ---------------------------------------------------------------------

/**
 * One agent (B17-B19): its mark, provider, title and age on one line, and
 * only while it asks, its question in the warning colour on a second. The row
 * is one button that opens the agent's pane; hovering or focusing it keeps its
 * delegation chain bright and shows `↵ 패널` where the age was, and resting on
 * it opens everything the agent last said with where it stands.
 */
function RowView({ row, faded, peers, parent, onHover, handlers }: { row: GraphRow; faded: boolean; peers: readonly string[]; parent: string | null; onHover: (paneId: string | null) => void; handlers: LensHandlers }) {
  const { agent, project, checkout, task, device } = row.value;
  const asking = row.line !== null;
  const said = rowLine(agent);
  const place = checkout.branch ?? checkout.label;
  const open = () => handlers.openAgent(agent.pane_id);
  const parts = badgeParts(row.tucked ?? undefined);
  return (
    <div
      className={cn("group/row absolute inset-x-0 top-0", faded && "opacity-(--opacity-dimmed)", row.dim && "opacity-(--opacity-dimmed)")}
      style={{ height: row.height }}
      data-graph-row={row.paneId}
      data-bucket={row.value.bucket}
      data-attention={row.attention}
      data-depth={row.depth}
      onPointerEnter={() => onHover(row.paneId)}
      onPointerLeave={() => onHover(null)}
      onFocus={() => onHover(row.paneId)}
      onBlur={() => onHover(null)}
    >
      <AgentMessagePopover
        agent={agent}
        place={place}
        fallback={said?.text ?? agent.identity_label}
        tone={said ? lineTone(said, agent.demand) : "text-muted-foreground"}
        onOpen={open}
        context={{ checkout: place, tab: row.tab?.label ?? null, peers, parent }}
      >
        <button
          type="button"
          aria-label={rowAccessibleName(agent, device)}
          data-graph-focus="row"
          data-graph-open={agent.pane_id}
          className="absolute inset-0 outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={(event) => {
            if (!gitHubClick(event, shownPullRequest(checkout)?.url ?? task?.url, project.device_id, handlers)) open();
          }}
        />
      </AgentMessagePopover>
      <span className="pointer-events-none relative flex h-(--graph-row-height) min-w-0 items-center gap-xs pr-sm" style={{ paddingLeft: `calc(var(--spacing-sm) + ${row.depth} * var(--size-lineage-indent))` }}>
        {row.depth > 0 ? <CornerDownRightIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" data-graph-indent="true" /> : null}
        <StatusMark symbol={agent.symbol} className={markTone(agent)} />
        <AgentMark kind={agent.agent_kind} />
        <span className={cn("min-w-0 flex-1 truncate text-body text-foreground", asking && "font-semibold")}>{agent.identity_label}</span>
        {parts.length > 0 ? (
          <Hint label={`접힌 하위 에이전트 ${badgeWords(row.tucked ?? undefined)}`}>
            <span className="pointer-events-auto relative z-10 inline-flex shrink-0 items-center gap-xs font-mono text-caption" data-graph-tucked={parts.map((part) => `${part.state}:${part.count}`).join(" ")}>
              <BadgeMarks parts={parts} />
            </span>
          </Hint>
        ) : null}
        <span className="shrink-0 font-mono text-caption text-muted-foreground group-focus-within/row:hidden group-hover/row:hidden">{agent.elapsed}</span>
        <span className="hidden shrink-0 text-caption text-foreground group-focus-within/row:inline group-hover/row:inline" data-graph-row-hint="true">
          {asking ? "↵ 답하기" : "↵ 패널"}
        </span>
      </span>
      {asking ? (
        <span className={cn("pointer-events-none relative block truncate pr-sm text-caption", said ? lineTone(said, agent.demand) : "text-warning")} style={{ paddingLeft: `calc(var(--spacing-sm) + ${row.depth} * var(--size-lineage-indent) + var(--size-agent-mark))` }} data-graph-row-line={agent.pane_id}>
          {row.line}
        </span>
      ) : null}
    </div>
  );
}

// --- the filter ----------------------------------------------------------------

/**
 * The status chips, the search and, when two or more devices are in scope,
 * the device choice (B24-B26): a lit chip keeps the rows of its state, any of
 * them; the search keeps the rows whose title, branch, issue or pull request
 * matches; all of them together narrow the graph.
 */
export function GraphFilterControls({ agents, filter, onChange }: { agents: readonly LensAgent[]; filter: GraphFilter; onChange: (filter: GraphFilter) => void }) {
  const devices = useMemo(() => graphDevices(agents), [agents]);
  return (
    <span className="flex min-w-0 flex-wrap items-center gap-sm" data-graph-filter={graphFilterActive(filter) ? "active" : "none"}>
      <ToggleGroup type="multiple" value={[...filter.chips]} onValueChange={(chips) => onChange({ ...filter, chips: chips as StatusChip[] })} aria-label="상태" data-graph-chips="true">
        {STATUS_CHIPS.map(({ chip, label }) => (
          <ToggleGroupItem key={chip} value={chip} data-graph-chip={chip}>
            {label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <span className="relative flex items-center">
        <SearchIcon aria-hidden="true" className="pointer-events-none absolute left-xs size-(--size-icon-sm) text-muted-foreground" />
        <Input
          value={filter.query}
          onChange={(event) => onChange({ ...filter, query: event.target.value })}
          onKeyDown={(event) => {
            // The first Escape clears only the search; on an empty field it is the screen's (B29).
            if (event.key === "Escape" && filter.query !== "") onChange({ ...filter, query: "" });
          }}
          placeholder="제목 · 브랜치 · #번호"
          aria-label="에이전트 검색"
          className="h-(--size-control-sm) w-(--graph-search-width) pl-lg"
          data-graph-search="true"
        />
      </span>
      {devices.length >= 2 ? (
        <ToggleGroup type="single" value={filter.device ?? ""} onValueChange={(device) => onChange({ ...filter, device: device || null })} aria-label="기기" data-graph-devices="true">
          {devices.map((device) => (
            <ToggleGroupItem key={device} value={device} data-graph-device={device === THIS_DEVICE ? "this" : device}>
              {device}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      ) : null}
    </span>
  );
}
