import { useState } from "react";
import type { Actions } from "./actions";
import { IssuePanel, type EditRequest } from "./IssuePanel";
import { filterActive, filterBoard, NO_FILTER, type BoardScope, type IssueFilter, type TaskCard, type TasksBoard } from "./projectBoard";
import { DependenciesView, FocusedTask, TasksListView, TasksView, type BoardHandlers } from "./TaskBoards";
import type { TasksMode } from "./ui";

// The Issues view as both scopes draw it (PRD overview-lenses-issues): the
// board in its mode, filtered, and the issue panel beside it once a card is
// chosen (D-08, D-43). The board stays where it is, in the width the panel
// leaves; the page decides where the panel's issue is kept, so a Project's
// Overview brings it back with the rest of its lens.

/** The card whose panel is open, looked up on the whole board so a filter never closes it. */
export function panelCard(board: TasksBoard, key: string | null): TaskCard | null {
  return key === null ? null : (board.cards.find((value) => value.task.key === key) ?? null);
}

/** What only the page knows: where a checkout, a start, a new issue and the Agents lens are. */
export type IssuesPage = Pick<BoardHandlers, "openCheckout" | "startIssue" | "newIssue" | "showCheckouts">;

export function IssuesView({
  board,
  scope,
  mode,
  filter,
  onFilterChange,
  panel,
  onPanel,
  focusTask,
  focusedPaneId,
  doneOpen,
  onToggleDone,
  actions,
  page,
}: {
  board: TasksBoard;
  scope: BoardScope;
  mode: TasksMode;
  filter: IssueFilter;
  onFilterChange: (filter: IssueFilter) => void;
  /** The issue whose panel is open, by task key. */
  panel: string | null;
  onPanel: (key: string | null) => void;
  focusTask: string | null;
  focusedPaneId: string | null;
  doneOpen: boolean;
  onToggleDone: () => void;
  actions: Actions;
  page: IssuesPage;
}) {
  const [edit, setEdit] = useState<EditRequest | null>(null);
  const shown = filterBoard(board, filter);
  const open = panelCard(board, panel);
  const handlers: BoardHandlers = {
    ...page,
    openPanel: (card) => {
      setEdit(null);
      onPanel(card.task.key);
    },
    editIssue: (card) => {
      setEdit({ key: card.task.key, at: Date.now() });
      onPanel(card.task.key);
    },
    openGitHub: (url, deviceId) => actions.openPullRequest(url, deviceId, true),
  };
  const boardPage = { panel: open ? panel : null, focusedPaneId };
  const view =
    mode === "dependencies" ? (
      <DependenciesView board={shown} page={boardPage} actions={actions} handlers={handlers} />
    ) : mode === "list" ? (
      <TasksListView board={shown} page={boardPage} actions={actions} handlers={handlers} />
    ) : (
      <TasksView board={shown} scope={scope} page={boardPage} actions={actions} handlers={handlers} doneOpen={doneOpen} onToggleDone={onToggleDone} filtered={filterActive(filter)} onClearFilter={() => onFilterChange(NO_FILTER)} />
    );
  // One tree whether or not the panel is open: the board is never remounted,
  // so its scroll, its unfolded lines and the card the keyboard is on stay.
  return (
    <FocusedTask.Provider value={focusTask}>
      <div className={open ? "flex min-h-0 flex-1 items-stretch gap-md pb-lg pr-lg" : "contents"} data-issues-split={open ? "true" : undefined}>
        <div className={open ? "min-h-0 min-w-0 flex-1 overflow-auto" : "contents"}>{view}</div>
        {open ? (
          <IssuePanel
            card={open}
            actions={actions}
            handlers={handlers}
            focusedPaneId={focusedPaneId}
            editRequest={edit}
            onClose={() => {
              setEdit(null);
              onPanel(null);
              // The keyboard goes back to the card the panel was for.
              document.querySelector<HTMLElement>(`[data-issue-card="${CSS.escape(open.task.key)}"]`)?.focus();
            }}
          />
        ) : null}
      </div>
    </FocusedTask.Provider>
  );
}
