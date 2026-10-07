import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArrowRightIcon, BanIcon, BellIcon, CheckIcon, CircleAlertIcon, CirclePauseIcon, CornerDownLeftIcon, GitMergeIcon, LoaderCircleIcon, MessageSquareIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed, useRemaining } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Kbd } from "../components/ui/kbd";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import { useOverviewCount } from "../Overview";
import { useUiStore } from "../ui";
import { taskRef, type FactoryCommand } from "./commands";
import { ACTION_LABEL, GROUP_LABEL, KIND_LABEL, itemWhy, refusalText, resultText } from "./labels";
import type { FactorySummary, InboxItem } from "./model";
import { useFactoryRequest, type RequestState } from "./request";
import { inboxKey, shownFactories, shownInbox } from "./view";

/** A choice in an expanded item: a suggestion, a listed choice, a merge or stop verb, or the person's own words. */
type Choice = { value: string; own: boolean };

/** The verbs a merge or stop item lists; a question's choices are words the engine answers with. */
const VERB_ITEMS = new Set(["merge", "stopped"]);

/** An item's choices in the engine's order, the suggestion first; a question also takes the person's own words. */
export function itemChoices(item: InboxItem): Choice[] {
  const listed = [item.suggestion, ...item.choices, ...(item.default_action ? [item.default_action] : [])].filter((value) => value.trim() !== "");
  const unique = listed.filter((value, index) => listed.indexOf(value) === index).map((value) => ({ value, own: false }));
  return VERB_ITEMS.has(item.kind) || item.kind === "notice" ? unique : [...unique, { value: "", own: true }];
}

/** The command a choice sends: a verb for a merge or stop item, else an answer to the item's question. */
export function choiceCommand(item: InboxItem, choice: Choice, text: string): FactoryCommand | null {
  const task = taskRef(item.factory, item.task);
  if (VERB_ITEMS.has(item.kind)) {
    if (choice.value === "merge") return { verb: "merge", task };
    if (choice.value === "retry") return { verb: "retry", task };
    if (choice.value === "cancel") return { verb: "cancel", task };
    if (choice.value === "request-changes") return text.trim() ? { verb: "request_changes", task, comment: text.trim() } : null;
    return null;
  }
  if (choice.own) return text.trim() ? { verb: "answer", task, question: item.question, choice: null, text: text.trim() } : null;
  return { verb: "answer", task, question: item.question, choice: choice.value === item.suggestion ? "suggestion" : choice.value, text: null };
}

/**
 * 내 차례 (PRD software-factory-ui D-03, D-04, B8-B14): every Factory's
 * person-facing items in one column, in the engine's order and under its
 * groups. The top item opens expanded with the suggestion chosen, so ⏎ once
 * answers; ↑↓ move and open, a number picks another choice. An item the
 * engine takes leaves the list and the next one opens; a refusal keeps it,
 * with the engine's next action in its place.
 */
export function MyTurn({ summary, factory, actions }: { summary: FactorySummary; factory: string | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const items = shownInbox(summary, factory);
  const focus = useUiStore((s) => (s.screen?.kind === "factory" ? s.screen.place.focus : null));
  const [open, setOpen] = useState<string | null>(focus);
  const lastIndex = useRef(0);
  const list = useRef<HTMLDivElement>(null);
  const keys = items.map(inboxKey);
  const found = open !== null ? keys.indexOf(open) : -1;
  // The open item, or the one that took the place of an item the engine took.
  const index = found >= 0 ? found : Math.min(lastIndex.current, keys.length - 1);
  if (index >= 0) lastIndex.current = index;
  const openKey = index >= 0 ? keys[index]! : null;
  useEffect(() => {
    if (focus === null) return;
    setOpen(focus);
    useUiStore.getState().setFactoryPlace({ focus: null });
  }, [focus]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>("[data-factory-item-open='true'] [data-factory-send]")?.focus({ preventScroll: true });
    list.current?.querySelector<HTMLElement>("[data-factory-item-open='true']")?.scrollIntoView({ block: "nearest" });
  }, [openKey]);
  const move = (step: number) => {
    if (keys.length === 0) return;
    setOpen(keys[Math.max(0, Math.min(keys.length - 1, index + step))]!);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.target instanceof HTMLInputElement) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      move(event.key === "ArrowDown" ? 1 : -1);
    }
  };
  const tasks = shownFactories(summary, factory).reduce((sum, view) => sum + view.columns.reduce((count, column) => count + column.cards.length, 0) + view.cancelled.length, 0);
  return (
    <div className="flex min-h-full flex-col px-lg" data-factory-turn="true">
      <div ref={list} role="list" aria-label={t("factory.tab.turn")} className="flex flex-1 flex-col gap-xxs pb-lg" onKeyDown={onKeyDown}>
        {items.length === 0 ? (
          <p className="py-md text-body text-muted-foreground" data-factory-turn-empty={tasks === 0 ? "intake" : "done"}>
            {tasks === 0 ? t("factory.intake") : t("factory.turn.empty")}
          </p>
        ) : null}
        {items.map((item, at) => {
          const key = keys[at]!;
          const head = at === 0 || items[at - 1]!.group !== item.group;
          const count = items.filter((other) => other.group === item.group).length;
          return (
            <div key={key} role="listitem" className="flex flex-col">
              {head ? (
                <h2 className="pt-md pb-xs text-caption text-subtle-foreground" data-factory-group={item.group}>
                  {t(GROUP_LABEL[item.group])} {count}
                </h2>
              ) : null}
              {key === openKey ? (
                <OpenItem item={item} actions={actions} />
              ) : (
                <ClosedItem item={item} onOpen={() => setOpen(key)} />
              )}
            </div>
          );
        })}
      </div>
      <OutsideRequests actions={actions} />
    </div>
  );
}

function KindIcon({ item, className }: { item: InboxItem; className?: string }) {
  const Icon = item.kind === "merge" ? GitMergeIcon : item.group === "stopped" ? CirclePauseIcon : item.group === "notice" ? BellIcon : MessageSquareIcon;
  return <Icon aria-hidden="true" className={cn("size-(--size-icon) shrink-0", className)} />;
}

/** The item's time cue: days a blocking question has waited, the time left before its deadline, else how long it has waited. */
function TimeCue({ item }: { item: InboxItem }) {
  const { t } = useInterfaceTranslation();
  const left = useRemaining(item.deadline);
  if (item.kind === "blocking" && item.waiting_days >= 1) {
    return <span className="shrink-0 text-caption text-warning" data-factory-time="waiting">{t("factory.turn.waitingDays", { count: item.waiting_days })}</span>;
  }
  if (left === "past") return <span className="shrink-0 text-caption text-muted-foreground" data-factory-time="past">{t("factory.turn.deadlinePassed")}</span>;
  if (left) return <span className="shrink-0 text-caption text-muted-foreground" data-factory-time="left">{t("factory.turn.left", { time: left.text })}</span>;
  return <Elapsed since={item.waiting_since} className="shrink-0 text-caption text-muted-foreground" data-factory-time="since" />;
}

function Place({ item }: { item: InboxItem }) {
  return (
    <>
      <span className="shrink-0 font-mono text-caption text-muted-foreground" data-factory-item-id={item.display_id}>
        {item.display_id}
      </span>
      <span className="max-w-1/5 shrink-0 truncate text-caption text-muted-foreground">{item.project}</span>
    </>
  );
}

function ClosedItem({ item, onOpen }: { item: InboxItem; onOpen: () => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <button
      type="button"
      className="flex min-w-0 items-center gap-sm rounded-sm px-md py-xs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
      aria-label={`${t(KIND_LABEL[item.kind])}: ${item.title}`}
      data-factory-item={inboxKey(item)}
      data-factory-item-open="false"
      onClick={onOpen}
    >
      <KindIcon item={item} className="text-subtle-foreground" />
      <span className="min-w-0 shrink truncate text-body">{item.title}</span>
      <span className="min-w-0 flex-1 truncate text-caption text-muted-foreground">{itemWhy(item, t)}</span>
      <Place item={item} />
      <TimeCue item={item} />
    </button>
  );
}

function OpenItem({ item, actions }: { item: InboxItem; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const choices = itemChoices(item);
  const [picked, setPicked] = useState(0);
  const [text, setText] = useState("");
  const request = useFactoryRequest(actions);
  const field = useRef<HTMLInputElement>(null);
  const choice = choices[Math.min(picked, choices.length - 1)] ?? null;
  const needsText = choice !== null && (choice.own || choice.value === "request-changes");
  useEffect(() => {
    if (needsText) field.current?.focus();
  }, [needsText]);
  const command = choice ? choiceCommand(item, choice, text) : null;
  const sending = request.state.phase === "sending";
  // A taken answer stays taken until the summary drops the item, so it is not sent twice.
  const send = () => {
    if (command && !sending && request.state.phase !== "taken") request.send(command);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === "Enter") {
      if (event.nativeEvent.isComposing || event.keyCode === 229) return;
      // A focused button other than a choice (자세히, 보내기) keeps its own Enter.
      if (event.target instanceof HTMLButtonElement && event.target.getAttribute("role") !== "radio") return;
      event.preventDefault();
      send();
      return;
    }
    if (event.target instanceof HTMLInputElement) return;
    const number = Number(event.key);
    if (Number.isInteger(number) && number >= 1 && number <= choices.length) {
      event.preventDefault();
      setPicked(number - 1);
    }
  };
  const label = choice ? choiceLabel(item, choice, text, t) : "";
  return (
    <div
      className={cn("flex flex-col gap-sm rounded-md border px-md py-sm", item.group === "notice" ? "border-border" : "border-warning")}
      data-factory-item={inboxKey(item)}
      data-factory-item-open="true"
      onKeyDown={onKeyDown}
    >
      <div className="flex min-w-0 items-start gap-sm">
        <KindIcon item={item} className="mt-xxs text-warning" />
        <div className="flex min-w-0 flex-1 flex-col gap-xxs">
          <span className="text-body font-semibold [overflow-wrap:anywhere]">{item.title}</span>
          <span className="text-body text-subtle-foreground [overflow-wrap:anywhere]" data-factory-item-why="true">
            <span className="text-muted-foreground">{t(KIND_LABEL[item.kind])} · </span>
            {itemWhy(item, t)}
          </span>
        </div>
        <Place item={item} />
        <TimeCue item={item} />
      </div>
      {choices.length > 0 ? (
        <div role="radiogroup" aria-label={t("factory.turn.choices")} className="flex flex-col gap-xxs pl-(--size-icon)">
          {choices.map((option, at) => (
            <ChoiceRow key={`${at}:${option.value}`} item={item} option={option} at={at} picked={at === picked} onPick={() => setPicked(at)} />
          ))}
          {needsText ? (
            <Input
              ref={field}
              value={text}
              onChange={(event) => setText(event.target.value)}
              placeholder={choice?.value === "request-changes" ? t("factory.turn.commentPlaceholder") : t("factory.turn.ownPlaceholder")}
              aria-label={choice?.value === "request-changes" ? t("factory.turn.commentPlaceholder") : t("factory.turn.ownPlaceholder")}
              data-factory-own-text="true"
            />
          ) : null}
        </div>
      ) : null}
      <div className="flex min-w-0 flex-wrap items-center gap-sm pl-(--size-icon)">
        {choices.length > 0 ? (
          <Button data-factory-send={request.state.phase} disabled={!command} aria-busy={sending} onClick={send}>
            {sending ? <LoaderCircleIcon className="animate-spin" /> : <CornerDownLeftIcon />}
            {sending ? t("factory.turn.sending") : label}
          </Button>
        ) : null}
        {/* What the engine's own pick does: its suggestion, or with none (a notice) its first choice. */}
        {choice && choice.value === (item.suggestion || choices[0]?.value) ? (
          <span className="min-w-0 text-caption text-subtle-foreground [overflow-wrap:anywhere]" data-factory-result={item.result_code}>
            {resultText(item, t)}
          </span>
        ) : null}
      </div>
      <Refusal state={request.state} />
      <div className="flex min-w-0 items-center gap-sm pl-(--size-icon) text-caption text-muted-foreground">
        <DefaultLine item={item} />
        <span className="flex-1" />
        <Button variant="ghost" size="sm" data-factory-details="true" onClick={() => useUiStore.getState().setFactoryPlace({ task: { factory: item.factory, task: item.task } })}>
          {t("factory.turn.details")}
          <ArrowRightIcon />
        </Button>
      </div>
    </div>
  );
}

function choiceLabel(item: InboxItem, choice: Choice, text: string, t: ReturnType<typeof useInterfaceTranslation>["t"]): string {
  if (VERB_ITEMS.has(item.kind)) return t(ACTION_LABEL[choice.value] ?? "factory.turn.send");
  if (choice.own) return text.trim() ? t("factory.turn.sendAs", { answer: text.trim() }) : t("factory.turn.send");
  return t("factory.turn.sendAs", { answer: choice.value });
}

function ChoiceRow({ item, option, at, picked, onPick }: { item: InboxItem; option: Choice; at: number; picked: boolean; onPick: () => void }) {
  const { t } = useInterfaceTranslation();
  const words = option.own ? t("factory.turn.own") : VERB_ITEMS.has(item.kind) ? t(ACTION_LABEL[option.value] ?? "factory.turn.send") : option.value;
  return (
    <button
      type="button"
      role="radio"
      aria-checked={picked}
      tabIndex={-1}
      className={cn("flex min-w-0 items-center gap-sm rounded-sm border px-md py-xs text-left text-body outline-none focus-visible:ring-1 focus-visible:ring-ring", picked ? "border-success bg-accent" : "border-border")}
      data-factory-choice={at + 1}
      onClick={onPick}
    >
      <Kbd>{at + 1}</Kbd>
      <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{words}</span>
      {option.value === item.suggestion && !option.own ? (
        <span className="flex shrink-0 items-center gap-xxs text-caption text-muted-foreground">
          {t("factory.turn.suggested")}
          {picked ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm) text-success" /> : null}
        </span>
      ) : null}
    </button>
  );
}

/** The default action and what happens with no answer; a blocking question has none and waits. */
function DefaultLine({ item }: { item: InboxItem }) {
  const { t } = useInterfaceTranslation();
  if (item.default_action) {
    return (
      <span className="min-w-0 truncate" data-factory-default="action">
        {t("factory.turn.defaultAction", { action: item.default_action })}
      </span>
    );
  }
  if (item.kind === "blocking") {
    return (
      <span className="flex min-w-0 items-center gap-xxs truncate" data-factory-default="none">
        <BanIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        {t("factory.turn.noDefault")}
      </span>
    );
  }
  return null;
}

/** A refused answer stays where it was asked, with the engine's next action (B10). */
export function Refusal({ state }: { state: RequestState }) {
  const { t } = useInterfaceTranslation();
  if (state.phase !== "refused") return null;
  const next = refusalText(state.answer?.reason, t);
  return (
    <p role="alert" className="flex items-center gap-xxs pl-(--size-icon) text-caption text-destructive" data-factory-refused={state.answer?.reason ?? "no_answer"}>
      <CircleAlertIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="[overflow-wrap:anywhere]">{next}</span>
    </p>
  );
}

/** The one line to the agents' own requests, when the Overview has some outside the Factory (B13). */
function OutsideRequests({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const count = useOverviewCount();
  if (count === 0) return null;
  return (
    <div className="sticky bottom-0 flex shrink-0 items-center gap-xs border-t border-border bg-background py-sm text-caption text-muted-foreground" data-factory-outside-requests={count}>
      {t("factory.turn.outsideRequests", { count })}
      <Button variant="ghost" size="sm" onClick={() => actions.openRequests()}>
        <ArrowRightIcon />
        {t("factory.turn.requests")}
      </Button>
    </div>
  );
}
