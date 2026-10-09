import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ChevronDownIcon, ChevronRightIcon, CheckIcon, CircleAlertIcon, CirclePauseIcon, CopyIcon, CornerDownLeftIcon, GitMergeIcon, HardDriveIcon, KeyRoundIcon, LoaderCircleIcon, MessageSquareIcon, RefreshCwIcon, SquareTerminalIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Elapsed, useRemaining } from "../components/elapsed";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { useInterfaceTranslation } from "../i18n/client";
import type { MessageKey } from "../i18n/catalogs";
import { cn } from "../lib/utils";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { choiceCommand, inboxKey, itemChoices, singleAction, singleCommand, takesOwnWords, type Choice } from "./choices";
import { actionKey, decisionWhy, engineChoice, ENV_HOLD_LABEL, FALLBACK_LABEL, FALLBACK_REASONS, GATE_LABEL, HOLDING_LABEL, MEANWHILE_LABEL, RECOVERY_OUTCOME_LABEL, RECOVERY_SHORT, refusalText, STOP_LABEL, type FallbackReason, type Translate } from "./labels";
import type { CardView, FactoryView, InboxItem } from "./model";
import { useFactoryRequest, type RequestState } from "./request";
import { factoryCards } from "./view";



const STAGE_NAME: readonly MessageKey[] = ["factory.card.stage.waiting", "factory.card.stage.work", "factory.card.stage.verify", "factory.card.stage.merge", "factory.card.stage.merge"];

/** What the merge, stop and pause verbs lead to; a question's results are its asker's. */
const VERB_RESULT: Record<string, MessageKey> = {
  merge: "factory.decide.verb.merge",
  "request-changes": "factory.decide.verb.requestChanges",
  cancel: "factory.decide.verb.cancel",
  resume: "factory.decide.verb.resume",
};

/**
 * 결정 필요 (PRD factory-human-loop D-09, D-33, B20-B23): only what a person
 * moves, in the engine's order, every item open in the same form. A question
 * offers its choices with what each leads to, the suggestion marked and
 * chosen, and a field for another answer; a to-do has one button. An item the
 * engine takes leaves the list; a refusal keeps it with the next action.
 * With nothing to decide the section is not drawn (B32).
 */
export function Decisions({ items, factories, actions }: { items: InboxItem[]; factories: FactoryView[]; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const focus = useUiStore((s) => (s.screen?.kind === "factory" ? s.screen.place.focus : null));
  const list = useRef<HTMLDivElement>(null);
  // A notification or a card's 다른 답 asks for one item: it is brought into view once.
  useEffect(() => {
    if (focus === null) return;
    const found = list.current?.querySelector<HTMLElement>(`[data-factory-item="${CSS.escape(focus)}"]`);
    found?.scrollIntoView({ block: "nearest" });
    found?.querySelector<HTMLElement>("[data-factory-send]")?.focus({ preventScroll: true });
    useUiStore.getState().setFactoryPlace({ focus: null });
  }, [focus]);
  if (items.length === 0) return null;
  const views = new Map(factories.map((view) => [view.id, view]));
  return (
    <section ref={list} className="flex flex-col gap-sm" aria-label={t("factory.decide.title")} data-factory-decisions={items.length}>
      <h2 className="flex items-center gap-xs text-caption font-semibold text-warning">
        {t("factory.decide.title")}
        <span data-factory-decide-count={items.length}>{items.length}</span>
      </h2>
      {items.map((item) => {
        const view = views.get(item.factory) ?? null;
        const card = view && item.task !== null ? (factoryCards(view).get(item.task) ?? null) : null;
        return <DecisionItem key={inboxKey(item)} item={item} view={view} card={card} actions={actions} />;
      })}
    </section>
  );
}

function KindIcon({ item }: { item: InboxItem }) {
  const Icon =
    item.kind === "merge" ? GitMergeIcon
    : item.kind === "paused" ? CirclePauseIcon
    : item.kind === "github" ? KeyRoundIcon
    : item.kind === "hold" ? HardDriveIcon
    : item.kind === "command" || item.kind === "start" ? SquareTerminalIcon
    : item.group === "stopped" ? CircleAlertIcon
    : MessageSquareIcon;
  return <Icon aria-hidden="true" className="mt-xxs size-(--size-icon) shrink-0 text-warning" />;
}

/** The item's one sentence: a question's words, or a merge, stop or to-do said from its codes. */
export function itemSentence(item: InboxItem, view: FactoryView | null, card: CardView | null, t: Translate): string {
  const id = item.display_id ?? "";
  switch (item.kind) {
    case "github":
      return view?.github_block?.forbidden ? t("factory.decide.githubPermission") : t("factory.decide.github");
    case "command":
      return item.text;
    case "start":
      return t("factory.decide.start", { id });
    case "hold":
      return item.env_hold ? t("factory.decide.hold", { reason: t(ENV_HOLD_LABEL[item.env_hold]) }) : item.text || t("factory.decide.holdReads");
    case "stopped":
      return t("factory.decide.stopped", { id, reason: item.stop ? t(STOP_LABEL[item.stop]) : t("factory.state.stopped") });
    case "paused":
      return t("factory.decide.paused", { id });
    case "merge": {
      const pr = card?.pr ? t("factory.decide.pr", { number: card.pr.number }) : id;
      return item.gates.length > 0 ? t("factory.decide.mergeGated", { pr, gates: item.gates.map((gate) => t(GATE_LABEL[gate])).join(", ") }) : t("factory.decide.merge", { pr });
    }
    default:
      return item.text;
  }
}

/** What the item holds up: the asker's words, else said from its code, with the Tasks waiting on it. */
export function stoppedText(item: InboxItem, card: CardView | null, t: Translate): string {
  const id = item.display_id ?? "";
  const what = item.stopped ?? t(HOLDING_LABEL[item.holding], { id, stage: t(STAGE_NAME[card?.stage ?? 1]!) });
  return item.unblocks.length > 0 ? t("factory.decide.waitingOn", { what, ids: item.unblocks.join(", ") }) : what;
}

/** The line beside the send button: the default and when it applies, or that there is none and what waits meanwhile. */
function DefaultLine({ item }: { item: InboxItem }) {
  const { t } = useInterfaceTranslation();
  const left = useRemaining(item.deadline);
  const time = left === "past" ? t("factory.turn.deadlinePassed") : left ? t("factory.turn.left", { time: left.text }) : null;
  const text = item.default_action ? [t("factory.turn.defaultAction", { action: item.default_action }), time].filter((part) => part !== null).join(" · ") : t("factory.decide.noDefault", { meanwhile: t(MEANWHILE_LABEL[item.holding]) });
  return (
    <span className="min-w-0 text-caption text-subtle-foreground [overflow-wrap:anywhere]" data-factory-default={item.default_action ? "action" : "none"}>
      {text}
    </span>
  );
}

/** What pressing a to-do's one button does. */
function singleResult(item: InboxItem, t: Translate): string {
  const id = item.display_id ?? "";
  switch (item.kind) {
    case "command":
      return item.impact ?? "";
    case "github":
      return t("factory.decide.githubResult");
    case "start":
      return t("factory.decide.startResult", { id });
    case "hold":
      return t("factory.decide.holdResult");
    default:
      return t("factory.decide.stoppedResult", { id });
  }
}

function singleLabel(item: InboxItem, t: Translate): { label: string; icon: typeof CheckIcon } {
  if (item.kind === "command") return { label: t("factory.decide.done"), icon: CheckIcon };
  if (item.kind === "stopped" || item.kind === "start") return { label: t("factory.decide.restart"), icon: RefreshCwIcon };
  return { label: t("factory.decide.recheck"), icon: RefreshCwIcon };
}

function DecisionItem({ item, view, card, actions }: { item: InboxItem; view: FactoryView | null; card: CardView | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const choices = itemChoices(item);
  const single = singleAction(item);
  const ownWords = takesOwnWords(item);
  const [picked, setPicked] = useState(0);
  const [text, setText] = useState("");
  const request = useFactoryRequest(actions);
  const field = useRef<HTMLInputElement>(null);
  // Typing another answer picks it; clearing the field returns to the listed pick.
  const own = ownWords && text.trim() !== "";
  const choice: Choice | null = own ? { value: "", own: true, result: null } : (choices[Math.min(picked, choices.length - 1)] ?? null);
  const needsText = choice?.value === "request-changes";
  useEffect(() => {
    if (needsText) field.current?.focus();
  }, [needsText]);
  const command = single ? singleCommand(item, view) : choice ? choiceCommand(item, choice, text) : null;
  const sending = request.state.phase === "sending";
  // Turning on Hide AI is the person's step in Settings; the engine retries the review once it can run.
  const aiOff = useShellStore((s) => s.rest?.status?.background_ai?.enabled === false);
  const opensAi = choice?.value === ENABLE_AI && aiOff;
  // A taken answer stays taken until the summary drops the item, so it is not sent twice.
  const send = () => {
    if (opensAi) useUiStore.getState().openSettings("hideAi");
    else if (command && !sending && request.state.phase !== "taken") request.send(command);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === "Enter") {
      if (event.nativeEvent.isComposing || event.keyCode === 229) return;
      // A focused button other than a choice keeps its own Enter.
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
  const verbs = item.kind === "merge" || item.kind === "paused";
  const SingleIcon = single ? singleLabel(item, t).icon : CornerDownLeftIcon;
  return (
    <div className="flex min-w-0 items-start gap-sm rounded-md border border-warning px-md py-sm" data-factory-item={inboxKey(item)} data-factory-item-kind={item.kind} onKeyDown={onKeyDown}>
      <KindIcon item={item} />
      <div className="flex min-w-0 flex-1 flex-col gap-sm">
        <div className="flex min-w-0 items-start gap-md">
          <div className="flex min-w-0 flex-1 flex-col gap-xxs">
            <span className="text-body font-semibold [overflow-wrap:anywhere]" data-factory-item-sentence="true">
              {itemSentence(item, view, card, t)}
            </span>
            <span className="text-caption text-subtle-foreground [overflow-wrap:anywhere]" data-factory-item-stopped={item.holding}>
              <span className="text-muted-foreground">{t("factory.decide.stoppedLabel")}</span> {stoppedText(item, card, t)}
            </span>
          </div>
          {item.display_id ? (
            <button type="button" className="shrink-0 font-mono text-caption text-muted-foreground hover:underline" data-factory-item-id={item.display_id} onClick={() => item.task && useUiStore.getState().setFactoryPlace({ task: { factory: item.factory, task: item.task } })}>
              {item.display_id}
            </button>
          ) : null}
          <TimeCue item={item} />
        </div>
        {choices.length > 0 ? (
          <div role="radiogroup" aria-label={t("factory.turn.choices")} className="flex max-w-3/4 flex-col gap-xs">
            {choices.map((option, at) => (
              <ChoiceRow key={`${at}:${option.value}`} item={item} option={option} at={at} picked={!own && at === Math.min(picked, choices.length - 1)} result={option.result ?? (verbs ? verbResult(option.value, t) : engineChoice(item, option.value, t)?.result ?? null)} onPick={() => { setPicked(at); if (option.value !== "request-changes") setText(""); }} />
            ))}
          </div>
        ) : null}
        {ownWords ? (
          <Input
            ref={field}
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder={item.kind === "merge" ? t("factory.turn.commentPlaceholder") : t("factory.decide.own")}
            aria-label={item.kind === "merge" ? t("factory.turn.commentPlaceholder") : t("factory.decide.own")}
            className="max-w-3/4"
            data-factory-own-text="true"
          />
        ) : null}
        {item.command ? <CommandLine command={item.command} actions={actions} /> : null}
        <div className="flex min-w-0 flex-wrap items-center gap-sm">
          <Button data-factory-send={request.state.phase} disabled={!command || request.state.phase === "taken"} aria-busy={sending} onClick={send}>
            {sending ? <LoaderCircleIcon className="animate-spin" /> : <SingleIcon />}
            {sending ? t("factory.turn.sending") : single ? singleLabel(item, t).label : opensAi ? t("factory.choice.openAi") : t("factory.decide.send")}
          </Button>
          {single ? <span className="min-w-0 text-caption text-subtle-foreground [overflow-wrap:anywhere]" data-factory-result={item.kind}>{singleResult(item, t)}</span> : view?.paused && item.group === "answer" ? <span className="min-w-0 text-caption text-subtle-foreground" data-factory-result="paused">{t("factory.turn.pausedSend")}</span> : <DefaultLine item={item} />}
        </div>
        <Refusal state={request.state} />
        <Evidence item={item} view={view} actions={actions} />
      </div>
    </div>
  );
}

const ENABLE_AI = "enable-ai";


function verbResult(value: string, t: Translate): string | null {
  const key = VERB_RESULT[value];
  return key ? t(key) : null;
}

/** The item's time cue: days a blocking question has waited, else how long it has waited. */
function TimeCue({ item }: { item: InboxItem }) {
  const { t } = useInterfaceTranslation();
  if (item.kind === "blocking" && item.waiting_days >= 1) {
    return <span className="shrink-0 text-caption text-muted-foreground" data-factory-time="waiting">{t("factory.decide.days", { count: item.waiting_days })}</span>;
  }
  return <Elapsed since={item.waiting_since} className="shrink-0 text-caption text-muted-foreground" data-factory-time="since" />;
}

function ChoiceRow({ item, option, at, picked, result, onPick }: { item: InboxItem; option: Choice; at: number; picked: boolean; result: string | null; onPick: () => void }) {
  const { t } = useInterfaceTranslation();
  const verbs = item.kind === "merge" || item.kind === "paused";
  const words = verbs ? t(actionKey(option.value, item.kind === "paused")) : (engineChoice(item, option.value, t)?.label ?? option.value);
  return (
    <button
      type="button"
      role="radio"
      aria-checked={picked}
      tabIndex={-1}
      className={cn("flex min-w-0 items-start gap-sm rounded-sm border px-md py-xs text-left outline-none focus-visible:ring-1 focus-visible:ring-ring", picked ? "border-foreground bg-accent" : "border-border")}
      data-factory-choice={at + 1}
      onClick={onPick}
    >
      <span aria-hidden="true" className={cn("mt-xxs size-(--size-checkbox) shrink-0 rounded-full border", picked ? "border-success bg-success" : "border-input")} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-body [overflow-wrap:anywhere]">{words}</span>
        {result ? <span className="text-caption text-subtle-foreground [overflow-wrap:anywhere]" data-factory-choice-result="true">{result}</span> : null}
      </span>
      {option.value === item.suggestion ? <span className="shrink-0 text-caption text-muted-foreground">{t("factory.turn.suggested")}</span> : null}
    </button>
  );
}

/** A to-do's command to copy (B16). */
function CommandLine({ command, actions }: { command: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  return (
    <div className="flex max-w-3/4 min-w-0 items-center gap-sm rounded-sm border border-border bg-muted px-md py-xs" data-factory-command="true">
      <code className="min-w-0 flex-1 font-mono text-body [overflow-wrap:anywhere]">{command}</code>
      <Button variant="ghost" size="sm" data-factory-copy="true" onClick={() => actions.copyText(command, "factory command")}>
        <CopyIcon />
        {t("factory.decide.copy")}
      </Button>
    </div>
  );
}

/** Why Factory AI left it to a person, in this Factory's mode. */
function whyMine(item: InboxItem, view: FactoryView | null, t: Translate): string | null {
  if (item.fallback !== null) return (FALLBACK_REASONS as readonly string[]).includes(item.fallback) ? t(FALLBACK_LABEL[item.fallback as FallbackReason]) : t("factory.fallback.failed");
  if (item.decision_kind) return decisionWhy(item.decision_kind, view?.observer_mode ?? null, t);
  return null;
}

/**
 * The folded 근거 (B22): why the item is the person's, Factory AI's reason,
 * what the recovery schedule already tried, and the evidence the engine
 * attached. Folded it reads as one line of its first parts.
 */
function Evidence({ item, view, actions }: { item: InboxItem; view: FactoryView | null; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const time = (at: number) => new Date(at).toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  const attempts = item.attempts.map((attempt) => [time(attempt.at), attempt.action ? t(RECOVERY_SHORT[attempt.action]) : t("factory.recovery.nothing"), attempt.outcome ? t(RECOVERY_OUTCOME_LABEL[attempt.outcome]) : t("factory.recovery.running")].join(" "));
  const rows: { key: MessageKey; text: string; links?: string[] }[] = [];
  const why = whyMine(item, view, t);
  if (why) rows.push({ key: "factory.decide.evidence.why", text: why });
  if (item.observer_reason) rows.push({ key: "factory.decide.evidence.reason", text: item.observer_reason });
  if (item.gates.length > 0) rows.push({ key: "factory.decide.evidence.gates", text: item.gates.map((gate) => t(GATE_LABEL[gate])).join(" · ") });
  if (attempts.length > 0) rows.push({ key: "factory.decide.evidence.recovery", text: attempts.join(" · ") });
  const links = item.evidence.filter((line) => /^https?:\/\//.test(line));
  const lines = item.evidence.filter((line) => !/^https?:\/\//.test(line));
  if (lines.length > 0 || links.length > 0) rows.push({ key: "factory.decide.evidence.facts", text: lines.join(" · "), links });
  if (rows.length === 0) return null;
  const folded = rows.map((row) => row.text).filter((part) => part !== "").slice(0, 2).join(" · ");
  return (
    <div className="flex min-w-0 flex-col gap-xs text-caption" data-factory-evidence={open ? "open" : "closed"}>
      <button type="button" className="flex min-w-0 items-center gap-xs self-start text-left text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />}
        <span className="shrink-0 font-semibold text-foreground">{t("factory.decide.evidence")}</span>
        {open ? null : <span className="min-w-0 truncate">{folded}</span>}
      </button>
      {open ? (
        <dl className="grid grid-cols-[max-content_1fr] gap-x-lg gap-y-xs pl-(--size-icon)">
          {rows.map((row) => (
            <div key={row.key} className="contents">
              <dt className="text-muted-foreground">{t(row.key)}</dt>
              <dd className="flex min-w-0 flex-wrap items-center gap-xs text-subtle-foreground [overflow-wrap:anywhere]">
                {row.text ? <span>{row.text}</span> : null}
                {(row.links ?? []).map((link) => (
                  <Button key={link} variant="link" size="sm" className="h-auto px-none" onClick={() => actions.openLink(link, true)}>
                    {link.replace(/^https?:\/\//, "")}
                  </Button>
                ))}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
    </div>
  );
}

/** A refused answer stays where it was asked, with the engine's next action. */
export function Refusal({ state }: { state: RequestState }) {
  const { t } = useInterfaceTranslation();
  if (state.phase !== "refused") return null;
  const next = refusalText(state.answer?.reason, t);
  return (
    <p role="alert" className="flex items-center gap-xxs text-caption text-destructive" data-factory-refused={state.answer?.reason ?? "no_answer"}>
      <CircleAlertIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
      <span className="[overflow-wrap:anywhere]">{next}</span>
    </p>
  );
}
