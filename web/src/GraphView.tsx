import { useEscapeLayer } from "./components/ui/layer";
import { ArrowLeftIcon, ArrowRightIcon, CornerDownRightIcon, GitMergeIcon, SearchIcon, XIcon } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  backPath,
  buildGraph,
  chainOf,
  foldHolding,
  forwardPath,
  graphDevices,
  graphFilterActive,
  graphTargets,
  NO_GRAPH_FILTER,
  routeFrom,
  STATUS_CHIPS,
  THIS_DEVICE,
  type CrossChip,
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
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { BadgeMarks } from "./components/status-badge";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import type { MessageKey } from "./i18n/catalogs";
import { useInterfaceTranslation } from "./i18n/client";
import { readFlowTiming, readGraphGeometry, readMotionMs, type FlowTiming } from "./graphGeometry";
import { Flow, Tween, type Targets } from "./graphMotion";
import { cn } from "./lib/utils";
import { FoldLine, gitHubClick, IssueChip, AgentMessagePopover, PullRequestChip, type LensHandlers } from "./OverviewLenses";
import type { LensAgent } from "./overviewLens";
import type { BoardProject } from "./projectBoard";
import { checkoutPresentation, distanceText, laneCheckoutCard, shownPullRequest } from "./projects";

// The Agents tab's graph (PRD agents-graph-view): a checkout is a box, an agent
// a row in it, a delegation into another checkout a line. `agentGraph.ts`
// computes every number; React renders the structure here, and `graphMotion.ts`
// writes the positions straight into the DOM so a glide never re-renders and a
// snapshot that changes nothing draws nothing (B33, B39). Hover, focus and the
// half-second popover are local and publish nothing (B38).

const FOLD_LABEL: Record<FoldKind, MessageKey> = { empty: "graph.fold.empty", cleanup: "graph.fold.cleanup", resting: "graph.fold.resting" };

const EDGE_WORDS: Record<EdgeKind, MessageKey> = { ask: "graph.edge.ask", flow: "requests.verb.working", wait: "graph.edge.wait", rest: "requests.verb.idle" };

const CLEANUP_HELP: Record<"merged" | "missing", MessageKey> = {
  merged: "graph.cleanup.merged",
  missing: "graph.cleanup.missing",
};

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
    const flow = canvas.querySelector<SVGPathElement>(`[data-graph-flow="${CSS.escape(edge.id)}"]`);
    drawn.set(edge.id, {
      back: edge.back,
      paths: [...group.querySelectorAll<SVGPathElement>("path"), ...(flow ? [flow] : [])],
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
  /** Every agent of every project on every device, where a chip finds the other end of a delegation into another project. */
  everyone: readonly LensAgent[];
  /** All projects only: a chip whose box this page draws selects it here, its fold opened and a filter that hides it cleared (issue 718). */
  onSelectBox?: (box: string, reveal: GraphReveal) => void;
  handlers: LensHandlers;
  now: number;
};

/** What a chip's box needs before it can be seen: its fold opened, the filter turned off. */
export type GraphReveal = { fold: string | null; clearFilter: boolean };

export function AgentGraph({ projects, agents, scope, selectedBox, filter, onFilter, folds, everyone, onSelectBox, handlers, now }: AgentGraphProps) {
  const { t } = useInterfaceTranslation();
  // The sizes are tokens, read once: the layout is numbers (D-31).
  const [geometry] = useState(() => readGraphGeometry());
  const [motionMs] = useState(() => readMotionMs());
  const [flowTiming] = useState(() => readFlowTiming());
  const board = useMemo(() => buildGraph(projects, agents, { scope, geometry, openFolds: folds, selectedBox, filter, everyone }), [projects, agents, scope, geometry, folds, selectedBox, filter, everyone]);
  const root = useRef<HTMLDivElement>(null);
  const selected = board.sections.find((section) => section.selected !== null)?.selected ?? null;
  useLayoutEffect(() => {
    if (!selected) return;
    root.current?.querySelector(`[data-graph-box="${CSS.escape(selected)}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [selected]);
  // A chip's click on this page, counted so a second click on the same chip brings its box back into view.
  const [jump, setJump] = useState<{ box: string; count: number } | null>(null);
  useLayoutEffect(() => {
    if (!jump) return;
    const box = root.current?.querySelector(`[data-graph-box="${CSS.escape(jump.box)}"]`);
    box?.closest("[data-graph-section]")?.scrollIntoView({ block: "start" });
    box?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [jump]);
  // A chip goes to the other end's box: here when this page draws its project, else on that project's Overview (issue 718).
  const onCross = (chip: CrossChip) => {
    if (onSelectBox && projects.some(({ workspace }) => workspace.id === chip.project.id)) {
      const drawn = board.sections.some((section) => section.rows.has(chip.paneIds[0]!));
      onSelectBox(chip.box, { fold: drawn ? null : foldHolding(chip.project, everyone, chip.box, "all"), clearFilter: !drawn && graphFilterActive(filter) });
      setJump((last) => ({ box: chip.box, count: (last?.count ?? 0) + 1 }));
    } else handlers.openProjectBox(chip.project, chip.box, foldHolding(chip.project, everyone, chip.box, "project"));
  };
  if (board.empty) {
    return (
      <div className="flex flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-graph-empty="true">
        <p>{t("requests.noAgents")}</p>
      </div>
    );
  }
  if (board.filterEmpty) {
    return (
      <div className="flex items-center justify-center gap-sm p-xl text-caption text-muted-foreground" data-graph-filter-empty="true">
        <p>{t("graph.noMatch")}</p>
        <Button variant="ghost" size="sm" onClick={() => onFilter(NO_GRAPH_FILTER)} data-graph-filter-clear="true">
          {t("board.clearFilter")}
        </Button>
      </div>
    );
  }
  return (
    <div ref={root} className="flex min-w-0 flex-col gap-lg pb-xl" data-graph={scope} onKeyDown={moveFocus}>
      {board.sections.map((section) => (
        <GraphSection key={section.project.id} section={section} scope={scope} geometry={geometry} motionMs={motionMs} flowTiming={flowTiming} handlers={handlers} onCross={onCross} now={now} />
      ))}
    </div>
  );
}

function GraphSection({ section, scope, geometry, motionMs, flowTiming, handlers, onCross, now }: { section: ProjectGraph; scope: "project" | "all"; geometry: GraphGeometry; motionMs: number; flowTiming: FlowTiming; handlers: LensHandlers; onCross: (chip: CrossChip) => void; now: number }) {
  const { t } = useInterfaceTranslation();
  const canvas = useRef<HTMLDivElement>(null);
  const tween = useRef<Tween | null>(null);
  const flow = useRef<Flow | null>(null);
  const index = useRef<PaintIndex | null>(null);
  const settled = useRef(false);
  const painted = useRef<HTMLElement | null>(null);
  const paintedEdges = useRef("");
  const [hover, setHover] = useState<string | null>(null);
  const chain = useMemo(() => (hover ? chainOf(section, hover) : null), [section, hover]);
  const targets = useMemo(() => graphTargets(section), [section]);
  const hasCanvas = section.boxes.length > 0;
  // A line that changes state changes which paths are drawn without moving a number, so the paths need painting on their own.
  const edgeSignature = useMemo(() => section.edges.map((edge) => `${edge.id}:${edge.kind}:${edge.back}`).join("|"), [section.edges]);

  useLayoutEffect(() => {
    const element = canvas.current;
    if (!element) {
      // The canvas is gone, and the dashes on it stop with it.
      flow.current?.set([]);
      index.current = null;
      return;
    }
    index.current = indexCanvas(element, section.edges);
    tween.current ??= new Tween({
      durationMs: motionMs,
      reduced: () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      paint: (values) => {
        if (!index.current) return;
        paintGraph(index.current, values, geometry);
        index.current.canvas.dataset.graphFrames = String(tween.current?.frames ?? 0);
      },
    });
    const { changed, added } = tween.current.retarget(targets);
    // A canvas that came back after the graph had gone is a new element holding nothing yet.
    if (!changed && (painted.current !== element || paintedEdges.current !== edgeSignature)) tween.current.repaint();
    painted.current = element;
    paintedEdges.current = edgeSignature;
    element.dataset.graphRevision = String(tween.current.revision);
    element.dataset.graphFrames = String(tween.current.frames);
    flow.current ??= new Flow({
      ...flowTiming,
      reduced: () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      // The canvas says whether its dashes are stepping, so a check reads the state the stepper owns and not a sample of its output.
      running: (running) => {
        if (!canvas.current) return;
        if (running) canvas.current.dataset.graphFlowing = "true";
        else delete canvas.current.dataset.graphFlowing;
      },
    });
    flow.current.set([...element.querySelectorAll<SVGPathElement>(".graph-edge-flow")]);
    if (changed && settled.current) for (const key of added) enter(index.current, key);
    settled.current = true;
  }, [section, targets, edgeSignature, geometry, motionMs, flowTiming, hasCanvas]);
  useLayoutEffect(() => {
    // A working line's dashes stop under reduced motion and start again when the setting is turned off.
    const setting = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => flow.current?.resume();
    setting.addEventListener("change", changed);
    return () => {
      setting.removeEventListener("change", changed);
      tween.current?.dispose();
      flow.current?.dispose();
    };
  }, []);

  const names = useMemo(() => new Map([...section.rows.values()].map((row) => [row.paneId, row.value.agent.identity_label])), [section]);
  const lines = useMemo(() => new Map(section.edges.map((edge) => [edge.to, t(EDGE_WORDS[edge.kind])])), [section, t]);
  const label = (kind: FoldKind) => t(FOLD_LABEL[kind]);
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
          <div ref={canvas} className="relative" data-graph-canvas={section.project.id} data-graph-revision="0" data-graph-frames="0">
            <svg className="pointer-events-none absolute left-0 top-0 size-full overflow-visible">
              {section.edges.map((edge) => {
                const faded = edge.dim || (chain !== null && !(chain.has(edge.from) && chain.has(edge.to)));
                return (
                  <g key={edge.id} className={cn("graph-edge", faded && "opacity-(--opacity-dimmed)")} data-graph-edge={edge.id} data-edge-kind={edge.kind} data-edge-back={edge.back ? "true" : undefined}>
                    <title>{`${names.get(edge.from) ?? ""} → ${names.get(edge.to) ?? ""} · ${t(EDGE_WORDS[edge.kind])}`}</title>
                    <path className="graph-edge-base" fill="none" stroke="currentColor" pathLength={1} />
                    <circle className="graph-port" data-graph-port="out" fill="currentColor" />
                    <circle className="graph-port" data-graph-port="in" fill="currentColor" />
                  </g>
                );
              })}
            </svg>
            {/* The flowing dashes are the only thing that moves while nothing changes, so they stand in a layer of their own: a frame repaints this layer, not the lines and ports under it. */}
            <svg aria-hidden="true" className="graph-flow-layer pointer-events-none absolute left-0 top-0 size-full overflow-visible">
              {section.edges
                .filter((edge) => edge.kind === "flow" && !edge.back)
                .map((edge) => {
                  const faded = edge.dim || (chain !== null && !(chain.has(edge.from) && chain.has(edge.to)));
                  return <path key={edge.id} className={cn("graph-edge-flow", faded && "opacity-(--opacity-dimmed)")} data-graph-flow={edge.id} fill="none" />;
                })}
            </svg>
            {section.boxes.map((box) => (
              <BoxView key={box.id} box={box} selected={box.id === section.selected} chain={chain} hover={hover} names={names} lines={lines} onHover={setHover} handlers={handlers} onCross={onCross} now={now} />
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

function BoxView({ box, selected, chain, hover, names, lines, onHover, handlers, onCross, now }: { box: GraphBox; selected: boolean; chain: Set<string> | null; hover: string | null; names: ReadonlyMap<string, string>; lines: ReadonlyMap<string, string>; onHover: (paneId: string | null) => void; handlers: LensHandlers; onCross: (chip: CrossChip) => void; now: number }) {
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
          <RowView key={row.paneId} row={row} faded={chain !== null && !chain.has(row.paneId)} peers={peers.get(row.paneId) ?? []} parent={parentName(row, names)} line={lines.get(row.paneId) ?? null} onHover={onHover} handlers={handlers} onCross={onCross} />
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
 * folder-less one is `× Folder missing`, and both offer `Clean up`. The head is one
 * button to the Workspace; `↵ Workspace` appears over the end of the branch
 * line without moving it, and resting on the head opens the checkout card.
 */
function BoxHead({ box, handlers, now }: { box: GraphBox; handlers: LensHandlers; now: number }) {
  const { t } = useInterfaceTranslation();
  const { project, checkout, cleanup } = box;
  const view = checkoutPresentation(project, checkout, now, t);
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
      <CheckoutCardHint card={laneCheckoutCard(project, checkout, now, t)} description={view.detail} onOpenPullRequest={(url) => handlers.openGitHub(url, project.device_id)} onOpenWorkspace={open}>
        <button
          type="button"
          aria-label={purpose ? t("graph.workspaceWithPurpose", { workspace: name, purpose }) : t("board.workspaceName", { branch: name })}
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
          {t("graph.workspaceHint")}
        </span>
      </span>
      <span className="pointer-events-none relative h-(--graph-head-line) truncate text-caption text-muted-foreground">{purpose}</span>
      <span className="pointer-events-none relative flex h-(--graph-head-line) min-w-0 items-center gap-sm font-mono text-caption text-muted-foreground">
        {box.primary ? (
          <span data-graph-head-agents={box.rows.filter((row) => !row.dim).length}>{t("graph.agents", { count: box.rows.filter((row) => !row.dim).length })}</span>
        ) : cleanup === "missing" ? (
          <span className="text-destructive">{t("graph.folderMissing")}</span>
        ) : (
          <>
            {box.task ? <IssueChip project={project} task={box.task} handlers={handlers} now={now} /> : null}
            <PullRequestChip project={project} checkout={checkout} onOpen={(url) => handlers.openGitHub(url, project.device_id)} onRow={(number) => handlers.openPullRequestRow(project, number)} now={now} />
            {ahead > 0 || behind > 0 ? <span data-graph-head-distance={`${ahead}:${behind}`}>{distanceText(ahead, behind)}</span> : null}
            {unread ? (
              <Hint label={t("graph.gitUnread")}>
                <span className="pointer-events-auto relative z-10" data-graph-head-files="unread">
                  ?
                </span>
              </Hint>
            ) : files > 0 ? (
              <span className={cn(worktree?.dirty && "text-warning")} data-graph-head-files={files}>
                {t("issue.changedFiles", { count: files })}
              </span>
            ) : null}
          </>
        )}
        <span className="flex-1" />
        {cleanup ? (
          <Hint label={t(CLEANUP_HELP[cleanup])}>
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
              {t("prList.cleanup")}
            </button>
          </Hint>
        ) : null}
      </span>
    </div>
  );
}

// --- a row ---------------------------------------------------------------------

/** The agent that delegated a row, drawn in this project or named by the chip of a parent in another one. */
function parentName(row: GraphRow, names: ReadonlyMap<string, string>): string | null {
  if (!row.parent) return null;
  return names.get(row.parent) ?? row.cross.find((chip) => chip.direction === "in")?.names[0] ?? null;
}

/**
 * One agent (B17-B19): its mark, provider, title and age on one line, and
 * only while it asks, its question in the warning colour on a second. The row
 * is one button that opens the agent's pane; hovering or focusing it keeps its
 * delegation chain bright and shows `↵ Panel` where the age was, and resting on
 * it opens everything the agent last said with where it stands.
 */
function RowView({ row, faded, peers, parent, line, onHover, handlers, onCross }: { row: GraphRow; faded: boolean; peers: readonly string[]; parent: string | null; line: string | null; onHover: (paneId: string | null) => void; handlers: LensHandlers; onCross: (chip: CrossChip) => void }) {
  const { t } = useInterfaceTranslation();
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
        tone={said ? lineTone(said, agent) : "text-muted-foreground"}
        onOpen={open}
        context={{ checkout: place, tab: row.tab?.label ?? null, peers, parent, line }}
      >
        <button
          type="button"
          aria-label={rowAccessibleName(t, agent, device)}
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
        {/* The title takes what is left, so it gives way before a chip's project name, the one thing a chip says (issue 718). */}
        <span className={cn("min-w-0 flex-1 truncate text-body text-foreground", asking && "font-semibold")} data-graph-row-title="true">
          {agent.identity_label}
        </span>
        {row.cross.map((chip) => (
          <CrossProjectChip key={`${chip.direction}:${chip.project.id}`} chip={chip} onClick={() => onCross(chip)} />
        ))}
        {parts.length > 0 ? (
          <Hint label={t("graph.tuckedDescendants", { states: badgeWords(row.tucked ?? undefined, t) })}>
            <span className="pointer-events-auto relative z-10 inline-flex shrink-0 items-center gap-xs font-mono text-caption" data-graph-tucked={parts.map((part) => `${part.state}:${part.count}`).join(" ")}>
              <BadgeMarks parts={parts} />
            </span>
          </Hint>
        ) : null}
        <Elapsed since={agent.changed_at_unix_ms} className="shrink-0 font-mono text-caption text-muted-foreground group-focus-within/row:hidden group-hover/row:hidden" />
        <span className="hidden shrink-0 text-caption text-foreground group-focus-within/row:inline group-hover/row:inline" data-graph-row-hint="true">
          {t(asking ? "graph.answerHint" : "graph.panelHint")}
        </span>
      </span>
      {asking ? (
        <span className={cn("pointer-events-none relative block truncate pr-sm text-caption", said ? lineTone(said, agent) : "text-warning")} style={{ paddingLeft: `calc(var(--spacing-sm) + ${row.depth} * var(--size-lineage-indent) + var(--size-agent-mark))` }} data-graph-row-line={agent.pane_id}>
          {row.line}
        </span>
      ) : null}
    </div>
  );
}

/**
 * A delegation into or from another project (issue 718): `→ sasu 2` on the
 * parent's row, `← herdr-ide` on the child's, with the other end's device when
 * it is not this row's. The tooltip and the accessible name name the agents
 * at the other end, and the click goes to the box that holds them.
 */
function CrossProjectChip({ chip, onClick }: { chip: CrossChip; onClick: () => void }) {
  const { t } = useInterfaceTranslation();
  const device = chip.device === null ? null : chip.device === THIS_DEVICE ? t("common.thisMac") : chip.device;
  const place = device ? `${chip.project.label} · ${device}` : chip.project.label;
  const label = t(chip.direction === "out" ? "graph.cross.out" : "graph.cross.in", { project: place, agents: chip.names.join(", ") });
  const Arrow = chip.direction === "out" ? ArrowRightIcon : ArrowLeftIcon;
  return (
    <Hint label={label}>
      <button
        type="button"
        aria-label={label}
        data-graph-focus="chip"
        data-graph-cross={chip.direction}
        data-graph-cross-project={chip.project.id}
        className="pointer-events-auto relative z-10 inline-flex h-(--graph-row-line) min-w-0 max-w-(--size-pane-child-chip-max) items-center gap-xxs rounded-xs border border-border px-xs font-mono text-caption text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
      >
        <Arrow aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        <span className="min-w-0 truncate font-sans" data-graph-cross-name="true">{chip.project.label}</span>
        {device ? <span className="min-w-0 truncate font-sans text-subtle-foreground">{device}</span> : null}
        {chip.paneIds.length > 1 ? <span className="shrink-0">{chip.paneIds.length}</span> : null}
      </button>
    </Hint>
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
  const { t } = useInterfaceTranslation();
  const devices = useMemo(() => graphDevices(agents), [agents]);
  useEscapeLayer(filter.query !== "", () => onChange({ ...filter, query: "" }));
  return (
    <span className="flex min-w-0 flex-wrap items-center gap-sm" data-graph-filter={graphFilterActive(filter) ? "active" : "none"}>
      <ToggleGroup type="multiple" value={[...filter.chips]} onValueChange={(chips) => onChange({ ...filter, chips: chips as StatusChip[] })} aria-label={t("graph.filter.status")} data-graph-chips="true">
        {STATUS_CHIPS.map(({ chip, labelKey }) => (
          <ToggleGroupItem key={chip} value={chip} data-graph-chip={chip}>
            {t(labelKey)}
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
          placeholder={t("graph.filter.query")}
          aria-label={t("graph.filter.search")}
          className="h-(--size-control-sm) w-(--graph-search-width) pl-lg"
          data-graph-search="true"
        />
      </span>
      {devices.length >= 2 ? (
        <ToggleGroup type="single" value={filter.device ?? ""} onValueChange={(device) => onChange({ ...filter, device: device || null })} aria-label={t("graph.filter.device")} data-graph-devices="true">
          {devices.map((device) => (
            <ToggleGroupItem key={device} value={device} data-graph-device={device === THIS_DEVICE ? "this" : device}>
              {device === THIS_DEVICE ? t("common.thisMac") : device}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      ) : null}
    </span>
  );
}
