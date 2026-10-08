import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArrowRightIcon, BanIcon, BellIcon, CheckIcon, CircleAlertIcon, CirclePauseIcon, CornerDownLeftIcon, GitMergeIcon, LoaderCircleIcon, MessageSquareIcon, SparklesIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed, useRemaining } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Kbd } from "../components/ui/kbd";
import { useInterfaceTranslation } from "../i18n/client";
import { cn } from "../lib/utils";
import { useOverviewCount } from "../Overview";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { taskRef, type FactoryCommand } from "./commands";
import { actionKey, DECISION_KIND_LABEL, GROUP_LABEL, KIND_LABEL, MODE_LABEL, decisionWhy, itemWhy, noticeText, refusalText, resultText } from "./labels";
import type { FactorySummary, FactoryView, InboxItem } from "./model";
import { useFactoryRequest, type RequestState } from "./request";
import { inboxKey, shownFactories, shownInbox } from "./view";

/** A choice in an expanded item: a suggestion, a listed choice, a merge or stop verb, or the person's own words. */
type Choice = { value: string; own: boolean };

/** The verbs a merge, stop or paused item lists; a question's choices are words the engine answers with. */
const VERB_ITEMS = new Set(["merge", "stopped", "paused"]);

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
    if (choice.value === "resume") return { verb: "resume", task };
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
  const shown = shownFactories(summary, factory);
  const tasks = shown.reduce((sum, view) => sum + view.columns.reduce((count, column) => count + column.cards.length, 0) + view.cancelled.length, 0);
  const views = new Map(summary.factories.map((view) => [view.id, view]));
  const notices = items.filter((item) => item.group === "notice").length;
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
          const view = views.get(item.factory) ?? null;
          return (
            <div key={key} role="listitem" className="flex flex-col">
              {/* Notices are only read, so they sit under a rule and stay out of the count (D-43). */}
              {head && item.group === "notice" ? (
                <NoticesHead count={notices} factories={shown.filter((other) => items.some((notice) => notice.group === "notice" && notice.factory === other.id))} actions={actions} />
              ) : head ? (
                <h2 className="pt-md pb-xs text-caption text-subtle-foreground" data-factory-group={item.group}>
                  {t(GROUP_LABEL[item.group])} {count}
                </h2>
              ) : null}
              {key === openKey ? (
                <OpenItem item={item} view={view} actions={actions} />
              ) : (
                <ClosedItem item={item} view={view} onOpen={() => setOpen(key)} />
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
  const Icon =
    item.kind === "merge" ? GitMergeIcon
    : item.kind === "paused" ? CirclePauseIcon
    : item.group === "stopped" ? CircleAlertIcon
    : item.notice !== null && item.notice !== "daily_limit" ? SparklesIcon
    : item.group === "notice" ? BellIcon
    : MessageSquareIcon;
  return <Icon aria-hidden="true" className={cn("size-(--size-icon) shrink-0", className)} />;
}

/** The notices' head: how many, that they are only read, and one action that clears them all (D-43). */
function NoticesHead({ count, factories, actions }: { count: number; factories: FactoryView[]; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [asked, setAsked] = useState<string[]>([]);
  // One request per Factory; the first refusal stays in place with the engine's next action.
  const refused = useShellStore((s) => asked.map((id) => s.factory?.actions.find((row) => row.request_id === id)?.answer).find((answer) => answer !== undefined && !answer.ok) ?? null);
  return (
    <div className="mt-md flex flex-col border-t border-border pt-sm" data-factory-group="notice">
      <div className="flex min-w-0 items-center gap-sm px-md pb-xs text-caption text-subtle-foreground">
        <h2>
          {t(GROUP_LABEL.notice)} {count}
        </h2>
        <span className="min-w-0 truncate text-muted-foreground">{t("factory.turn.noticesHint")}</span>
        <span className="flex-1" />
        <Button variant="ghost" size="sm" data-factory-ack-all="true" onClick={() => setAsked(factories.map((view) => actions.factoryAction({ verb: "ack_notices", project: view.project })))}>
          {t("factory.turn.ackAll")}
        </Button>
      </div>
      {refused ? <Refusal state={{ phase: "refused", answer: refused }} /> : null}
    </div>
  );
}

/** What a notice is about beside its line: the decision's kind, or the Factory's mode that let the AI act. */
function noticeWhy(item: InboxItem, view: FactoryView | null, t: ReturnType<typeof useInterfaceTranslation>["t"]): string {
  if (item.notice === "daily_limit") return t("factory.notice.dailyLimitWhy");
  const kind = item.decision_kind ? t(DECISION_KIND_LABEL[item.decision_kind]) : null;
  const mode = view && item.notice !== "ai_answered" ? t(MODE_LABEL[view.observer_mode]) : null;
  return [kind, mode].filter((part) => part !== null).join(" · ");
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

function ClosedItem({ item, view, onOpen }: { item: InboxItem; view: FactoryView | null; onOpen: () => void }) {
  const { t } = useInterfaceTranslation();
  if (item.group === "notice") {
    return (
      <button
        type="button"
        className="flex min-w-0 items-center gap-sm rounded-sm px-md py-xs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
        aria-label={noticeText(item, t, view?.observer_limit ?? null)}
        data-factory-item={inboxKey(item)}
        data-factory-item-open="false"
        data-factory-notice={item.notice ?? "notice"}
        onClick={onOpen}
      >
        <KindIcon item={item} className="text-subtle-foreground" />
        <span className="min-w-0 shrink truncate text-body">{noticeText(item, t, view?.observer_limit ?? null)}</span>
        <span className="min-w-0 flex-1 truncate text-caption text-muted-foreground">{noticeWhy(item, view, t)}</span>
        {item.overridable ? <span className="shrink-0 text-caption text-foreground">{t("factory.turn.override")}</span> : item.notice && item.notice !== "daily_limit" ? <span className="shrink-0 text-caption text-foreground">{t("factory.turn.view")}</span> : null}
        <Place item={item} />
        <TimeCue item={item} />
      </button>
    );
  }
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

function OpenItem({ item, view, actions }: { item: InboxItem; view: FactoryView | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  if (item.group === "notice") return <OpenNotice item={item} view={view} actions={actions} />;
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
      className="flex flex-col gap-sm rounded-md border border-warning px-md py-sm"
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
          {item.decision_kind ? (
            <span className="text-body text-subtle-foreground [overflow-wrap:anywhere]" data-factory-decision-kind={item.decision_kind}>
              {decisionWhy(item.decision_kind, view?.observer_mode ?? null, t)}
              {item.observer_reason ? ` ${item.observer_reason}` : null}
            </span>
          ) : null}
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
          <Button data-factory-send={request.state.phase} disabled={!command || request.state.phase === "taken"} aria-busy={sending} onClick={send}>
            {sending ? <LoaderCircleIcon className="animate-spin" /> : <CornerDownLeftIcon />}
            {sending ? t("factory.turn.sending") : label}
          </Button>
        ) : null}
        {/* What the engine's own pick does: its suggestion, or with none (a notice) its first choice. */}
        {choice && choice.value === (item.suggestion || choices[0]?.value) ? (
          <span className="min-w-0 text-caption text-subtle-foreground [overflow-wrap:anywhere]" data-factory-result={view?.paused && item.group === "answer" ? "paused" : item.result_code}>
            {/* A paused Factory keeps the answer and hands it over on resume (D-48). */}
            {view?.paused && item.group === "answer" ? t("factory.turn.pausedSend") : resultText(item, t)}
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

/**
 * An open notice: ⏎ marks it read, 다른 답 replaces the answer Factory AI
 * gave while its Task is not finished (D-19), and 보기 opens the Task.
 */
function OpenNotice({ item, view, actions }: { item: InboxItem; view: FactoryView | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const request = useFactoryRequest(actions);
  const [text, setText] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (text !== null) field.current?.focus();
  }, [text !== null]);
  const task = taskRef(item.factory, item.task);
  const busy = request.state.phase === "sending" || request.state.phase === "taken";
  const command: FactoryCommand | null = text === null ? { verb: "answer", task, question: item.question, choice: "ok", text: null } : text.trim() && item.refers_to ? { verb: "answer", task, question: item.refers_to, choice: null, text: text.trim(), change: true } : null;
  const send = () => {
    if (command && !busy) request.send(command);
  };
  return (
    <div
      className="flex flex-col gap-sm rounded-md border border-border px-md py-sm"
      data-factory-item={inboxKey(item)}
      data-factory-item-open="true"
      data-factory-notice={item.notice ?? "notice"}
      onKeyDown={(event) => {
        if (event.key !== "Enter" || event.nativeEvent.isComposing || event.keyCode === 229) return;
        if (event.target instanceof HTMLButtonElement && !event.target.hasAttribute("data-factory-send")) return;
        event.preventDefault();
        send();
      }}
    >
      <div className="flex min-w-0 items-start gap-sm">
        <KindIcon item={item} className="mt-xxs text-subtle-foreground" />
        <div className="flex min-w-0 flex-1 flex-col gap-xxs">
          <span className="text-body font-semibold [overflow-wrap:anywhere]">{noticeText(item, t, view?.observer_limit ?? null)}</span>
          <span className="text-body text-subtle-foreground [overflow-wrap:anywhere]">
            {[item.title, noticeWhy(item, view, t)].filter((part) => part !== "").join(" · ")}
            {item.observer_reason ? ` · ${item.observer_reason}` : null}
          </span>
        </div>
        <Place item={item} />
        <TimeCue item={item} />
      </div>
      {text !== null ? (
        <Input ref={field} value={text} onChange={(event) => setText(event.target.value)} placeholder={t("factory.turn.overridePlaceholder")} aria-label={t("factory.turn.overridePlaceholder")} className="ml-(--size-icon) w-auto" data-factory-override-text="true" />
      ) : null}
      <div className="flex min-w-0 flex-wrap items-center gap-sm pl-(--size-icon)">
        <Button data-factory-send={request.state.phase} disabled={!command || busy} aria-busy={request.state.phase === "sending"} onClick={send}>
          {request.state.phase === "sending" ? <LoaderCircleIcon className="animate-spin" /> : <CornerDownLeftIcon />}
          {request.state.phase === "sending" ? t("factory.turn.sending") : text === null ? t("factory.action.acknowledge") : t("factory.turn.overrideSend")}
        </Button>
        <span className="min-w-0 text-caption text-subtle-foreground" data-factory-result={text === null ? item.result_code : "override"}>
          {text === null ? t("factory.result.acknowledge") : t("factory.turn.overrideResult")}
        </span>
        <span className="flex-1" />
        {item.overridable && text === null ? (
          <Button variant="outline" size="sm" data-factory-override="true" onClick={() => setText("")}>
            {t("factory.turn.override")}
          </Button>
        ) : null}
        <Button variant="ghost" size="sm" data-factory-details="true" onClick={() => useUiStore.getState().setFactoryPlace({ task: { factory: item.factory, task: item.task } })}>
          {t("factory.turn.view")}
          <ArrowRightIcon />
        </Button>
      </div>
      <Refusal state={request.state} />
    </div>
  );
}

function choiceLabel(item: InboxItem, choice: Choice, text: string, t: ReturnType<typeof useInterfaceTranslation>["t"]): string {
  if (VERB_ITEMS.has(item.kind)) return t(actionKey(choice.value, item.kind === "paused"));
  if (choice.own) return text.trim() ? t("factory.turn.sendAs", { answer: text.trim() }) : t("factory.turn.send");
  return t("factory.turn.sendAs", { answer: choice.value });
}

function ChoiceRow({ item, option, at, picked, onPick }: { item: InboxItem; option: Choice; at: number; picked: boolean; onPick: () => void }) {
  const { t } = useInterfaceTranslation();
  const words = option.own ? t("factory.turn.own") : VERB_ITEMS.has(item.kind) ? t(actionKey(option.value, item.kind === "paused")) : option.value;
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
