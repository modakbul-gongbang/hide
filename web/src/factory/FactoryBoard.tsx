import { useState } from "react";
import { ChevronDownIcon, ChevronRightIcon, XIcon } from "lucide-react";
import type { Actions } from "../actions";
import { useRemaining } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { useInterfaceTranslation } from "../i18n/client";
import { useUiStore, type FactoryPlace } from "../ui";
import { taskRef } from "./commands";
import { TaskCardView } from "./FactoryCard";
import { COLUMN_LABEL } from "./labels";
import type { CardView, FactoryView, InboxItem } from "./model";
import { Refusal } from "./MyTurn";
import { useFactoryRequest } from "./request";
import { boardColumns, cancelledCards } from "./view";

function cardInboxItem(inbox: InboxItem[], factory: string, card: CardView) {
  const preferred = card.state === "merge_waiting" ? "merge" : card.state === "stopped" ? "stopped" : card.state === "paused" ? "paused" : null;
  let fallback: InboxItem | undefined;
  for (const item of inbox) {
    if (item.factory !== factory || item.task !== card.task || item.kind === "notice") continue;
    if (preferred === null || item.kind === preferred) return item;
    fallback ??= item;
  }
  // Action and cap questions replace the engine's synthetic stopped item.
  return fallback;
}

/**
 * Four movement columns, with person waits first and old completions folded.
 * Card commands share the inbox request path; the board owns no engine state.
 */
export function FactoryBoard({ factories, place, actions, inbox }: { factories: FactoryView[]; place: FactoryPlace; actions: Actions; inbox: InboxItem[] }) {
  const { t } = useInterfaceTranslation();
  const columns = boardColumns(factories, place.column);
  const many = factories.length > 1;
  const cards = columns.reduce((sum, column) => sum + column.groups.reduce((count, group) => count + group.cards.length + group.folded.length, 0), 0);
  // Archived cards have left the board, so a Factory holding only those is as empty as a new one.
  const all = factories.reduce((sum, view) => sum + view.columns.reduce((count, column) => count + column.cards.filter((card) => !card.archived).length, 0), 0);
  const set = (patch: Partial<FactoryPlace>) => useUiStore.getState().setFactoryPlace(patch);
  return (
    <div className="factory-board-container flex min-h-0 flex-1 flex-col gap-sm px-lg pb-lg" data-factory-board={place.cancelled ? "cancelled" : "board"}>
      <div className="flex shrink-0 items-center gap-sm">
        {place.column ? (
          <Button variant="secondary" size="sm" data-factory-column-filter={place.column} onClick={() => set({ column: null })} aria-label={t("factory.board.clearColumn")}>
            {t(COLUMN_LABEL[place.column])}
            <XIcon />
          </Button>
        ) : null}
        <span className="flex-1" />
        <Button variant={place.cancelled ? "secondary" : "ghost"} size="sm" aria-pressed={place.cancelled} data-factory-cancelled-filter="true" onClick={() => set({ cancelled: !place.cancelled })}>
          {t("factory.board.cancelled")}
        </Button>
      </div>
      {all === 0 && !place.cancelled ? <p className="py-md text-body text-muted-foreground" data-factory-board-empty="intake">{t("factory.intake")}</p> : null}
      {place.cancelled ? (
        <CancelledList factories={factories} actions={actions} many={many} />
      ) : cards === 0 && all > 0 ? (
        <NoMatch onClear={() => set({ column: null, factory: null })} />
      ) : (
        <div className="factory-board-grid min-h-0 flex-1 gap-md" data-filtered={place.column !== null}>
          {columns.map((column) => (
            <section key={column.column} className="flex min-w-0 flex-col gap-sm" aria-label={t(COLUMN_LABEL[column.column])} data-factory-column={column.column}>
              <h2 className="text-caption text-subtle-foreground">
                {t(COLUMN_LABEL[column.column])} {column.groups.reduce((sum, group) => sum + group.cards.length, 0)}
              </h2>
              {(column.column === "stuck" ? ["person", "other"] as const : [null]).map((waiting) => {
                const groups = column.groups.map((group) => ({ ...group, cards: group.cards.filter((card) => waiting === null || card.waiting_group === waiting) }));
                const count = groups.reduce((sum, group) => sum + group.cards.length, 0);
                if (waiting !== null && count === 0) return null;
                return <div key={waiting ?? "all"} className="flex flex-col gap-sm" data-factory-waiting-group={waiting ?? undefined}>
                  {waiting !== null ? <h3 className="text-caption text-muted-foreground">{t(waiting === "person" ? "factory.board.person" : "factory.board.other")} {count}</h3> : null}
                  {groups.map((group) => <div key={group.factory.id} className="flex min-w-0 flex-col gap-sm">
                    {many && group.cards.length + group.folded.length > 0 ? <span className="truncate text-caption text-muted-foreground">{group.factory.project_name}</span> : null}
                    {group.cards.map((card) => <TaskCardView key={card.task} factory={group.factory} card={card} showProject={false} actions={actions} item={cardInboxItem(inbox, group.factory.id, card)} />)}
                    {waiting === null && group.folded.length > 0 ? <FoldedDone factory={group.factory} cards={group.folded} /> : null}
                  </div>)}
                </div>;
              })}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

/** A filter that leaves no card says so and offers to clear it (B21). */
export function NoMatch({ onClear }: { onClear: () => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <p className="flex items-center gap-sm py-md text-body text-muted-foreground" data-factory-no-match="true">
      {t("factory.board.noMatch")}
      <Button variant="link" size="sm" onClick={onClear}>
        {t("factory.board.clearFilter")}
      </Button>
    </p>
  );
}

function FoldedDone({ factory, cards }: { factory: FactoryView; cards: CardView[] }) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon;
  return (
    <div className="flex flex-col gap-sm" data-factory-folded={cards.length}>
      <button type="button" aria-expanded={open} className="flex items-center gap-xxs rounded-sm text-caption text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" onClick={() => setOpen(!open)}>
        <Chevron aria-hidden="true" className="size-(--size-icon-sm)" />
        {t("factory.board.folded", { count: cards.length })}
      </button>
      {open ? cards.map((card) => <TaskCardView key={card.task} factory={factory} card={card} showProject={false} dim />) : null}
    </div>
  );
}

function CancelledList({ factories, actions, many }: { factories: FactoryView[]; actions: Actions; many: boolean }) {
  const { t } = useInterfaceTranslation();
  const rows = cancelledCards(factories);
  if (rows.length === 0) return <p className="py-md text-body text-muted-foreground" data-factory-cancelled-empty="true">{t("factory.board.noCancelled")}</p>;
  return (
    <div className="flex max-w-(--size-pr-popover) flex-col gap-sm">
      {rows.map(({ factory, card }) => (
        <div key={`${factory.id}/${card.task}`} className="flex flex-col gap-xxs" data-factory-cancelled={card.task}>
          <TaskCardView factory={factory} card={card} showProject={many} dim />
          <Revive factory={factory} card={card} actions={actions} />
        </div>
      ))}
    </div>
  );
}

/** Revive, while the keep period lasts; after it the Task's worktree is gone and so is the button (B16). */
export function Revive({ factory, card, actions }: { factory: FactoryView | string; card: CardView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const left = useRemaining(card.revive_until);
  if (card.revive_until === null || left === "past" || left === null) return null;
  const id = typeof factory === "string" ? factory : factory.id;
  return (
    <div className="flex flex-col gap-xxs">
      <span className="flex items-center gap-xs text-caption text-muted-foreground">
        {t("factory.board.cancelledLine")}
        <Button variant="link" size="sm" data-factory-revive={card.task} disabled={request.state.phase === "sending"} onClick={() => request.send({ verb: "revive", task: taskRef(id, card.task) })}>
          {t("factory.action.revive")}
        </Button>
        <span>{t("factory.board.reviveLeft", { time: left.text })}</span>
      </span>
      <Refusal state={request.state} />
    </div>
  );
}
