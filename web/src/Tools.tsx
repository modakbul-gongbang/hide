import { useId, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import { MemoryTab, useMemoryProject } from "./MemoryTab";
import { useWorkCounts } from "./workCount";
import { WorkPanel } from "./WorkPanel";
import { frontCheckout } from "./snapshot";
import { useShellStore } from "./store";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import type { Tool } from "./workspace";

// The Tools column (PRD S6 D-05, B10; PRD three-column-panel D-01, D-04): the
// Work (Sessions and PRs), Explorer, History or Memory (PRD project-memory-v2 D-17), one at a time, at the Workspace body's right
// edge. The tool is the front Workspace's, so switching it here leaves every
// other Workspace's alone. The column has no title row and no close: its
// first row is its named tool tabs, level with the other
// columns' tab rows, and the toolbar's Tools icon and ⌘E turn it off. The
// names are docs/UI_BEHAVIOR.md's current ones.

const TOOL_NAMES = { work: "work.title", explorer: "panes.tools.explorer", changes: "panes.tools.history", memory: "memory.title" } as const satisfies Record<Tool, MessageKey>;
const TOOLS: readonly Tool[] = ["work", "explorer", "changes", "memory"];

export function Tools({ tool, actions }: { tool: Tool; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const id = useId();
  useMemoryProject(actions);
  return (
    <aside className="flex h-full min-h-0 min-w-0 flex-1 flex-col text-foreground" aria-label={t("panes.tools.aria")} data-workspace-tools={tool}>
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center border-b border-border" data-column-row="tools">
        <ToolTabs tool={tool} actions={actions} id={id} />
      </div>
      <section role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${tool}`} className="flex min-h-0 flex-1 flex-col" aria-label={t(TOOL_NAMES[tool])} data-tool={tool} data-right-panel={tool}>
        {/* The tree reads the selected device's checkout through its helper;
            History reads the front checkout's Git through its own host. */}
        {tool === "work" ? <WorkPanel actions={actions} /> : tool === "explorer" ? <ExplorerTree actions={actions} /> : tool === "memory" ? <MemoryTab actions={actions} /> : <HistoryList actions={actions} />}
      </section>
    </aside>
  );
}

/** Named tabs share one keyboard stop; arrows select in displayed order. */
function ToolTabs({ tool, actions, id }: { tool: Tool; actions: Actions; id: string }) {
  const { t } = useInterfaceTranslation();
  // The Memory tab's dot: its Project has something to ask (B28).
  const asking = useShellStore((state) => {
    const project = state.memory?.project;
    return Boolean(project?.enabled && project.questions.length > 0 && project.workspace_id === frontCheckout(state.rest)?.workspace_id);
  });
  // The Work tab's number: the Needs You of Sessions and PRs added, once each (PRD work-tab-pr-lens B4).
  const work = useWorkCounts(null).total;
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : null;
    if (step === null && event.key !== "Home" && event.key !== "End") return;
    const index = TOOLS.indexOf(tool);
    const next = event.key === "Home" ? TOOLS[0] : event.key === "End" ? TOOLS.at(-1) : TOOLS[(index + (step ?? 0) + TOOLS.length) % TOOLS.length];
    if (!next) return;
    event.preventDefault();
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-tool-tab="${next}"]`)?.focus();
    actions.showTool(next);
  };
  return (
    <div role="tablist" aria-label={t("panes.tools.aria")} onKeyDown={move} className="flex h-full min-w-0 items-stretch" data-tool-tabs="true">
      {TOOLS.map((each) => {
        const selected = each === tool;
        return (
            <button
              key={each}
              id={`${id}-${each}`}
              type="button"
              role="tab"
              aria-controls={`${id}-panel`}
              tabIndex={selected ? 0 : -1}
              aria-selected={selected}
              aria-label={each === "work" && work > 0 ? `${t(TOOL_NAMES[each])} · ${t("work.count", { count: work })}` : t(TOOL_NAMES[each])}
              data-tool-tab={each}
              className={`relative flex min-w-0 items-center justify-center gap-xxs px-sm text-caption outline-none hover:bg-accent focus-visible:bg-accent ${selected ? "text-foreground" : "text-subtle-foreground"}`}
              onClick={() => actions.showTool(each)}
            >
              <span className={each === "work" ? "shrink-0" : "truncate"}>{t(TOOL_NAMES[each])}</span>
              {each === "work" && work > 0 ? <span className="shrink-0 tabular-nums text-warning" data-work-count={work}>{work}</span> : null}
              {each === "memory" && asking ? <span role="img" aria-label={t("memory.questionsWaiting")} className="size-(--size-status-mark) shrink-0 rounded-full bg-warning" data-tool-dot="memory" /> : null}
              {selected ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-primary" /> : null}
            </button>
        );
      })}
    </div>
  );
}
