import { useMemo } from "react";
import { useInterfaceTranslation } from "../i18n/client";
import { DependencyGraphView } from "../TaskBoards";
import { useUiStore } from "../ui";
import { NoMatch } from "./FactoryBoard";
import { TaskCardView } from "./FactoryCard";
import type { FactoryView } from "./model";
import { factoryGraph, type GraphNode } from "./view";

/**
 * The graph (PRD software-factory-ui D-08, B17): each Factory's Tasks laid out
 * by what they wait on, full width, with every arrow a longer path implies
 * left out. Finished work is dimmed and old completions fold away; the Tasks
 * with no relation sit below. A node opens its Task page.
 */
export function FactoryGraph({ factories, filtered }: { factories: FactoryView[]; filtered: boolean }) {
  const { t } = useInterfaceTranslation();
  const graphs = useMemo(() => factories.map((factory) => ({ factory, graph: factoryGraph(factory) })), [factories]);
  const many = factories.length > 1;
  const nodes = graphs.reduce((sum, { graph }) => sum + graph.layers.flat().length + graph.unrelated.length, 0);
  const draw = (node: GraphNode) => (
    <div key={node.id} className="factory-graph-node" data-dependency-node={node.id}>
      <TaskCardView factory={node.factory} card={node.card} showProject={many} dim={node.card.state === "done"} />
    </div>
  );
  if (nodes === 0) {
    return filtered ? (
      <div className="px-lg">
        <NoMatch onClear={() => useUiStore.getState().setFactoryPlace({ factory: null })} />
      </div>
    ) : (
      <p className="px-lg py-md text-body text-muted-foreground" data-factory-graph-empty="intake">{t("factory.intake")}</p>
    );
  }
  return (
    <div className="flex w-fit min-w-full flex-col gap-lg px-lg pb-xl" data-factory-graph="true">
      <p className="text-caption text-muted-foreground">{t("factory.graph.legend")}</p>
      {graphs.map(({ factory, graph }) => (
        <section key={factory.id} className="flex flex-col gap-md" aria-label={factory.project_name} data-factory-graph-of={factory.id}>
          {many ? <h2 className="text-subhead font-semibold">{factory.project_name}</h2> : null}
          {graph.layers.length > 0 ? <DependencyGraphView graph={graph} draw={draw} idOf={(node) => node.id} /> : null}
          {graph.unrelated.length > 0 ? (
            <div className="flex flex-col gap-sm border-t border-border pt-md" data-factory-graph-unrelated="true">
              <h3 className="text-caption text-subtle-foreground">{t("factory.graph.unrelated")}</h3>
              <div className="flex flex-wrap items-start gap-md">{graph.unrelated.map(draw)}</div>
            </div>
          ) : null}
        </section>
      ))}
    </div>
  );
}
