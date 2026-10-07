// What the Factory screens draw, arranged from the engine's summary
// (PRD software-factory-ui B7-B17). Every number, name, order and state is the
// engine's: these helpers only pick the Factories the project filter keeps
// and lay out what the engine gave, never decide a state of their own.

import { layerDependencies, transitiveReduction, type LayeredGraph } from "../projectBoard";
import type { CardView, Column, FactorySummary, FactoryView, Flow, InboxItem } from "./model";

/** The Factories the project filter keeps; a closed Factory leaves the screen. */
export function shownFactories(summary: FactorySummary, factory: string | null): FactoryView[] {
  return summary.factories.filter((view) => !view.closed && (factory === null || view.id === factory));
}

/** The inbox in the engine's order, kept to the filtered Factory. */
export function shownInbox(summary: FactorySummary, factory: string | null): InboxItem[] {
  return factory === null ? summary.inbox : summary.inbox.filter((item) => item.factory === factory);
}

/** An inbox item's identity across summaries: a question by its id, a merge or stop by its Task. */
export function inboxKey(item: Pick<InboxItem, "factory" | "task" | "question" | "group">): string {
  return `${item.factory}/${item.task}/${item.question ?? item.group}`;
}

/** The flow bar's four counts over the shown Factories. */
export function shownFlow(factories: readonly FactoryView[]): Flow {
  return factories.reduce<Flow>(
    (sum, view) => ({ drafting: sum.drafting + view.flow.drafting, waiting: sum.waiting + view.flow.waiting, running: sum.running + view.flow.running, done_today: sum.done_today + view.flow.done_today }),
    { drafting: 0, waiting: 0, running: 0, done_today: 0 },
  );
}

/**
 * The flow bar's last outside read (B20): the oldest among the shown
 * Factories that read GitHub, and whether any of them failed three reads in
 * a row. A local Factory reads nothing outside and has no time here.
 */
export function outsideRead(factories: readonly FactoryView[]): { at: number | null; stale: boolean } | null {
  const reading = factories.filter((view) => view.source === "github");
  if (reading.length === 0) return null;
  const times = reading.flatMap((view) => (view.outside_read_at === null ? [] : [view.outside_read_at]));
  return { at: times.length > 0 ? Math.min(...times) : null, stale: reading.some((view) => view.stale) };
}

/** One board column: each shown Factory's cards in the engine's order, Factory after Factory. */
export type BoardColumn = { column: Column; groups: { factory: FactoryView; cards: CardView[]; folded: CardView[] }[] };

/**
 * The board (B15): the four columns the engine names, filtered to one when
 * the flow bar asked for it. A completion older than the fold age goes into
 * the column's folded group, and an archived one leaves the board; both stay
 * on their Task page.
 */
export function boardColumns(factories: readonly FactoryView[], filter: Column | null): BoardColumn[] {
  const columns = new Map<Column, BoardColumn>();
  for (const factory of factories) {
    for (const view of factory.columns) {
      if (filter !== null && view.column !== filter) continue;
      const column = columns.get(view.column) ?? { column: view.column, groups: [] };
      columns.set(view.column, column);
      const kept = view.cards.filter((card) => !card.archived);
      column.groups.push({ factory, cards: kept.filter((card) => !card.folded), folded: kept.filter((card) => card.folded) });
    }
  }
  return [...columns.values()];
}

/** The cancelled Tasks the board's `취소됨` filter lists while they can be revived (B16). */
export function cancelledCards(factories: readonly FactoryView[]): { factory: FactoryView; card: CardView }[] {
  return factories.flatMap((factory) => factory.cancelled.map((card) => ({ factory, card })));
}

/** Every card of a Factory the engine keeps on its board, by Task id. */
export function factoryCards(factory: FactoryView): Map<string, CardView> {
  const cards = new Map<string, CardView>();
  for (const column of factory.columns) for (const card of column.cards) cards.set(card.task, card);
  for (const card of factory.cancelled) cards.set(card.task, card);
  return cards;
}

/** A graph node: a Task's card, named across Factories. */
export type GraphNode = { id: string; factory: FactoryView; card: CardView };

/**
 * One Factory's graph (B17, D-08): every dependency the engine keeps, laid
 * out like the Issues view's Dependencies mode, with the arrows a longer path
 * implies left out. Folded completions leave the drawing; their Tasks keep
 * their edges in the data.
 */
export function factoryGraph(factory: FactoryView): LayeredGraph<GraphNode> {
  const cards = factoryCards(factory);
  const nodes = new Map<string, GraphNode>();
  for (const [id, card] of cards) {
    if (card.folded || card.archived || card.state === "cancelled") continue;
    nodes.set(id, { id: `${factory.id}/${id}`, factory, card });
  }
  const blockers = new Map<string, GraphNode[]>();
  for (const [from, to] of factory.dependencies) {
    const before = nodes.get(from);
    if (!before || !nodes.has(to)) continue;
    blockers.set(to, [...(blockers.get(to) ?? []), before]);
  }
  const graph = layerDependencies([...nodes.values()], (node) => node.id, (node) => blockers.get(node.card.task) ?? []);
  return { ...graph, edges: transitiveReduction(graph.edges) };
}

/** The Task page's chain (B18): the predecessors, the Task, and the Tasks waiting on it, by Task id. */
export function taskChain(factory: FactoryView, task: string): { before: CardView[]; after: CardView[] } {
  const cards = factoryCards(factory);
  const pick = (ids: string[]) => ids.flatMap((id) => (cards.has(id) ? [cards.get(id)!] : []));
  return {
    before: pick(factory.dependencies.filter(([, to]) => to === task).map(([from]) => from)),
    after: pick(factory.dependencies.filter(([from]) => from === task).map(([, to]) => to)),
  };
}

/** The panes that are Factory workers, which the Overview leaves out of its requests and count (B13). */
export function workerPanes(summary: FactorySummary | null | undefined): ReadonlySet<string> {
  const panes = new Set<string>();
  for (const factory of summary?.factories ?? []) {
    for (const column of factory.columns) for (const card of column.cards) if (card.worker_pane) panes.add(card.worker_pane);
  }
  return panes;
}

/** The mark the engine's store leaves where it shortened a text (`[cut N bytes]`), read as a cue rather than shown. */
const CUT_MARK = /\n?\[cut \d+ bytes\]$/;

export function withoutCutMark(text: string): { text: string; cut: boolean } {
  return CUT_MARK.test(text) ? { text: text.replace(CUT_MARK, ""), cut: true } : { text, cut: false };
}
