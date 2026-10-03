import { ChevronDownIcon, ChevronRightIcon, GitPullRequestIcon, LinkIcon, SquareArrowOutUpRightIcon, SquareTerminalIcon } from "lucide-react";
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { markTone } from "./agentRow";
import { Elapsed } from "./components/elapsed";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { holdsCommandKey, hostBridge } from "./host";
import { fieldLabel } from "./shortcutLabels";
import { cn } from "./lib/utils";
import { IssueChip, gitHubClick, type LensHandlers } from "./OverviewLenses";
import { ageWords } from "./overviewLens";
import { prChip, type PrChip } from "./projectBoard";
import {
  OPEN_CHIPS,
  TAIL_SHARE,
  VERB_LABEL,
  childrenSummary,
  fullRequest,
  openCandidates,
  pullRequestChip,
  requestAccessibleName,
  requestGroups,
  requestLine,
  resultLine,
  rowIssues,
  rowIssueChips,
  verdictLine,
  rowSince,
  senderWords,
  splitTail,
  type OpenCandidate,
  type RequestRow,
} from "./requestList";
import type { AgentPullRequest, AgentRow, PullRequest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { ChecksMark, PR_TONE } from "./TaskBoards";
import { paneContext, pathLookups, probePaths, type FoundPath } from "./terminalLinkProvider";
import type { RequestLens } from "./ui";

// The request view (PRD overview-request-view): every agent of the scope as
// one row of what the operator asked it, what came of it and what is theirs
// to do, grouped by the core's verb. The rules are `requestList.ts`'s; this
// file draws them. Expanding a row, unfolding the resting group and showing
// a full request are the page's own state (`RequestLens`), so Recent Panels
// brings the view back as it was left; nothing here is stored.

export type RequestViewProps = {
  rows: readonly RequestRow[];
  scope: "project" | "all";
  lens: RequestLens;
  onLens: (patch: Partial<RequestLens>) => void;
  handlers: LensHandlers;
  actions: Actions;
  /** The scope's existing new-agent entry, for the empty view (B40). */
  onNewAgent?: () => void;
};

export const RequestView = memo(function RequestView({ rows, scope, lens, onLens, handlers, actions, onNewAgent }: RequestViewProps) {
  const groups = useMemo(() => requestGroups(rows), [rows]);
  // While a window shows the view, the core re-reads running checks (D-32).
  // A hidden page is not showing it, and a reconnect is a new connection
  // whose demand starts empty, so the demand is declared each time the page
  // is live, as the Settings agents tab does.
  const live = useShellStore((s) => s.connection === "live");
  useEffect(() => {
    if (!live) return;
    const report = () => actions.observeRequestView(document.visibilityState === "visible");
    report();
    document.addEventListener("visibilitychange", report);
    return () => {
      document.removeEventListener("visibilitychange", report);
      actions.observeRequestView(false);
    };
  }, [actions, live]);
  if (rows.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center gap-sm p-xl text-center text-caption text-muted-foreground" data-requests-empty="true">
        <p>실행 중인 에이전트가 없습니다</p>
        {onNewAgent ? (
          <Button variant="secondary" onClick={onNewAgent} data-requests-new-agent="true">
            <SquareTerminalIcon aria-hidden="true" />
            New agent
          </Button>
        ) : null}
      </div>
    );
  }
  const toggle = (paneId: string, row: RequestRow) => {
    const open = lens.open.includes(paneId);
    onLens({ open: open ? lens.open.filter((id) => id !== paneId) : [...lens.open, paneId], ...(!open && row.verb === "result" ? { resting: true } : {}) });
    // Opening a finished row is looking at it (D-29, B15).
    if (!open && row.verb === "result") actions.openResult(paneId);
  };
  const nothingToDo = groups.every((group) => group.verb === "idle");
  return (
    <div className="flex min-w-0 flex-col gap-md px-lg pb-xl" data-requests={scope} onKeyDown={moveFocus}>
      {nothingToDo ? (
        <p className="text-caption text-muted-foreground" data-requests-nothing="true">
          할 일 없음
        </p>
      ) : null}
      {groups.map((group) => {
        const folded = group.verb === "idle" && !lens.resting;
        return (
          <section key={group.verb} className="flex min-w-0 flex-col" aria-label={`${group.label} ${group.rows.length}`} data-request-group={group.verb}>
            <button
              type="button"
              aria-expanded={group.verb === "idle" ? !folded : undefined}
              data-request-focus="head"
              data-request-group-head={group.verb}
              className="flex min-w-0 items-center gap-xs rounded-xs py-xs text-left text-caption font-medium text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
              onClick={() => (group.verb === "idle" ? onLens({ resting: !lens.resting }) : undefined)}
            >
              <span>{`${group.label} ${group.rows.length}${folded ? " · 펼치기" : ""}`}</span>
              {group.verb === "idle" ? folded ? <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : null}
            </button>
            {folded ? null : (
              <ul className="flex min-w-0 flex-col" role="list">
                {group.rows.map((row) => (
                  <RequestRowView
                    key={row.lens.agent.pane_id}
                    row={row}
                    scope={scope}
                    open={lens.open.includes(row.lens.agent.pane_id)}
                    full={lens.full.includes(row.lens.agent.pane_id)}
                    onToggle={() => toggle(row.lens.agent.pane_id, row)}
                    onFull={() => onLens({ full: [...lens.full, row.lens.agent.pane_id] })}
                    handlers={handlers}
                    actions={actions}
                  />
                ))}
              </ul>
            )}
          </section>
        );
      })}
    </div>
  );
}, sameViewInputs);

/** The store already shares unchanged snapshot subtrees. Compare those
 * identities and the few project/checkout fields this view reads, rather
 * than rendering every row when another rest field changes. No text is
 * serialized and no timer, read or notification is added (B30, B43).
 */
function sameViewInputs(before: RequestViewProps, after: RequestViewProps): boolean {
  if (before.scope !== after.scope || before.lens !== after.lens || before.onLens !== after.onLens || before.handlers !== after.handlers || before.actions !== after.actions || before.onNewAgent !== after.onNewAgent || before.rows.length !== after.rows.length) return false;
  const projects = new Map<Workspace, Workspace>();
  return before.rows.every((row, index) => {
    const next = after.rows[index]!;
    const a = row.lens;
    const b = next.lens;
    if (row.verb !== next.verb || a.agent !== b.agent || a.task !== b.task || a.device !== b.device || a.checkout.branch !== b.checkout.branch || a.checkout.label !== b.checkout.label || row.children.length !== next.children.length || row.children.some((child, i) => child !== next.children[i])) return false;
    if (a.project === b.project || projects.get(a.project) === b.project) return true;
    const p = a.project;
    const q = b.project;
    if (p.id !== q.id || p.device_id !== q.device_id || p.label !== q.label || p.tasks !== q.tasks || p.pull_requests !== q.pull_requests || p.checkouts.find((checkout) => checkout.github)?.github !== q.checkouts.find((checkout) => checkout.github)?.github) return false;
    projects.set(p, q);
    return true;
  });
}

// --- keyboard ----------------------------------------------------------------

/**
 * The view's rows, group heads, the folded line and the open chips in the
 * order they are drawn (B8): the arrows step through them, Home and End go
 * to the ends. Enter is the focused button's click; Escape is the screen's.
 */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
  if (event.metaKey || event.ctrlKey || event.altKey) return;
  const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[event.key];
  const end = event.key === "Home" ? "first" : event.key === "End" ? "last" : null;
  if (step === undefined && end === null) return;
  const all = [...event.currentTarget.querySelectorAll<HTMLElement>("[data-request-focus]")];
  if (all.length === 0) return;
  const from = (event.target as HTMLElement).closest<HTMLElement>("[data-request-focus]");
  const index = from ? all.indexOf(from) : -1;
  const next = end === "first" ? all[0] : end === "last" ? all.at(-1) : all[Math.min(all.length - 1, Math.max(0, index + (step ?? 0)))];
  if (!next || next === from) return;
  event.preventDefault();
  next.focus();
  next.scrollIntoView({ block: "nearest" });
}

// --- a row -------------------------------------------------------------------

/** The result line of a row to answer is the warning colour (B5), of a row to fix the failure one. */
const VERB_TONE: Partial<Record<RequestRow["verb"], string>> = {
  answer: "text-warning",
  fix: "text-destructive",
};

/**
 * One agent (B4): its kind and title (with its project on All projects),
 * the request after who sent it, the result line with its open chips, and
 * on the right its descendants, pull request, issue, checkout and time. The
 * row is one button that expands it in place (B6); a double click or
 * ⌘Enter opens the agent's pane (B7).
 */
function RequestRowView({ row, scope, open, full, onToggle, onFull, handlers, actions }: { row: RequestRow; scope: "project" | "all"; open: boolean; full: boolean; onToggle: () => void; onFull: () => void; handlers: LensHandlers; actions: Actions }) {
  const { agent, project, checkout, device } = row.lens;
  const block = agent.request;
  const result = resultLine(row);
  const children = childrenSummary(row);
  const pulls = block?.pull_requests ?? [];
  const shown = pullRequestChip(pulls);
  const issues = rowIssueChips(row, project);
  const targets = useOpenTargets(agent, block?.reply?.text ?? "", pulls, device === null);
  const openPane = () => handlers.openAgent(agent.pane_id);
  const place = checkout.branch ?? checkout.label;
  return (
    <li className="group/row relative flex min-w-0 flex-col border-b border-border last:border-b-0" data-request-row={agent.pane_id} data-request-verb={row.verb} data-expanded={open ? "true" : undefined}>
      <div className="relative min-w-0">
        <button
          type="button"
          aria-label={requestAccessibleName(row, result)}
          aria-expanded={open}
          data-request-focus="row"
          data-request-toggle={agent.pane_id}
          className="absolute inset-0 rounded-xs outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          onClick={onToggle}
          onDoubleClick={openPane}
          onKeyDown={(event) => {
            if (event.key === "Enter" && holdsCommandKey(event)) {
              event.preventDefault();
              openPane();
            }
          }}
        />
        <div className="pointer-events-none relative flex min-w-0 flex-col gap-xxs px-sm py-xs">
          <span className="flex min-w-0 items-center gap-xs">
            <StatusMark symbol={agent.symbol} className={markTone(agent)} />
            <AgentMark kind={agent.agent_kind} />
            <span className="min-w-0 flex-1 truncate text-body text-foreground">
              <span data-request-title={agent.pane_id}>{agent.identity_label}</span>
              {scope === "all" ? <span className="text-caption text-muted-foreground"> · {project.label}</span> : null}
            </span>
            {children ? (
              <span className="shrink-0 font-mono text-caption text-muted-foreground" data-request-children={row.children.length}>
                {children.text}
                {children.asking > 0 ? <span className="text-warning"> · 질문 {children.asking}</span> : null}
              </span>
            ) : null}
            {shown ? <RowPullRequestChip project={project} pull={shown.chip} more={shown.more} handlers={handlers} /> : null}
            {issues[0] ? (
              <span className="inline-flex shrink-0 items-center gap-xxs" data-request-issues={issues.length}>
                <IssueChip project={project} task={issues[0]} handlers={handlers} now={Date.now()} />
                {issues.length > 1 ? <span className="font-mono text-caption text-muted-foreground">+{issues.length - 1}</span> : null}
              </span>
            ) : null}
            <span className="max-w-(--size-pane-child-chip-max) shrink truncate font-mono text-caption text-muted-foreground">{device ? `${device} · ${place}` : place}</span>
            <Elapsed since={rowSince(row)} className="shrink-0 font-mono text-caption text-muted-foreground" data-request-since={rowSince(row) ?? undefined} />
          </span>
          {block?.request ? (
            <span className="flex min-w-0 items-center gap-xs text-caption" data-request-line={agent.pane_id}>
              <span className="shrink-0 text-subtle-foreground" data-request-sender={block.request.sender.kind}>
                {senderWords(block.request.sender)} ›
              </span>
              <FittedLine text={requestLine(block.request.text, block.request.images)} />
              {block.later_by ? (
                <Badge variant="secondary" className="shrink-0" data-request-later={block.later_by.kind}>
                  이후 {senderWords(block.later_by)}
                </Badge>
              ) : null}
            </span>
          ) : null}
          {result || targets.length > 0 ? (
            <span className="flex min-w-0 items-center gap-xs text-caption">
              <span className={cn("min-w-0 flex-1 truncate", VERB_TONE[row.verb] ?? "text-muted-foreground")} data-request-result={agent.pane_id}>
                {result}
              </span>
              {targets.slice(0, OPEN_CHIPS).map((target) => (
                <OpenChip key={target.key} target={target} actions={actions} />
              ))}
            </span>
          ) : null}
        </div>
      </div>
      {open ? <RowDetail row={row} targets={targets} full={full} onFull={onFull} onOpen={openPane} handlers={handlers} actions={actions} /> : null}
    </li>
  );
}

/**
 * The request line in one line (D-42): when it does not fit, its end keeps up
 * to forty percent of the width from a word boundary and its front is cut
 * with an ellipsis in the rest. The width is measured on resize only.
 */
function FittedLine({ text }: { text: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const parts = useMemo(() => {
    const element = ref.current;
    if (!element || width === 0) return { head: text, tail: "" };
    const measure = measurer(getComputedStyle(element).font);
    return splitTail(
      text,
      (value) => measure(value) <= width,
      (value) => measure(value) <= width * TAIL_SHARE,
    );
  }, [text, width]);
  return (
    <span ref={ref} className="flex min-w-0 flex-1 text-foreground" data-request-text={text}>
      <span className="min-w-0 truncate">{parts.head}</span>
      {parts.tail ? <span className="shrink-0 whitespace-pre">{` ${parts.tail}`}</span> : null}
    </span>
  );
}

let canvas: CanvasRenderingContext2D | null = null;

/** The width of a text in a font, from one shared canvas. */
function measurer(font: string): (text: string) => number {
  canvas ??= document.createElement("canvas").getContext("2d");
  const context = canvas;
  if (!context) return (text) => text.length * 8;
  return (text) => {
    context.font = font;
    return context.measureText(text).width;
  };
}

// --- chips -------------------------------------------------------------------

const CHECKS_WORDS = { passing: "CI 통과", failed: "CI 실패", pending: "CI 진행 중" } as const;

/** The pull request's full facts where the project lists it, for the draft and review marks. */
function projectPull(project: Workspace, pull: AgentPullRequest): PullRequest | null {
  return (project.pull_requests ?? []).find((known) => known.url === pull.url) ?? null;
}

function chipOf(project: Workspace, pull: AgentPullRequest): PrChip {
  const known = projectPull(project, pull);
  if (known) return prChip(known);
  return prChip({ number: pull.number, title: pull.title, url: pull.url, badge: pull.badge, review: null, is_draft: false, checks: pull.checks });
}

/**
 * The row's pull request (D-46, B4): its lifecycle colour and CI mark, `+N`
 * for the other live ones. It opens its row on the PRs tab, ⌘-click GitHub.
 * While GitHub cannot be read the last value stays, dimmed, with its age in
 * the tooltip (B41).
 */
function RowPullRequestChip({ project, pull, more, handlers, historical = false }: { project: Workspace; pull: AgentPullRequest; more: number; handlers: LensHandlers; historical?: boolean }) {
  const chip = chipOf(project, pull);
  const status = project.checkouts.find((checkout) => checkout.github)?.github ?? null;
  const stale = status !== null && (status.stale || (!status.available && status.unavailable_reason !== null));
  const age = stale && status?.last_success_at_unix_ms != null ? `${ageWords(Date.now() - status.last_success_at_unix_ms)} 값` : null;
  const listed = projectPull(project, pull) !== null;
  const words = [`PR #${pull.number}`, pull.title, chip.checks ? CHECKS_WORDS[chip.checks] : null, age].filter(Boolean).join(" · ");
  const hint = [`PR #${pull.number}`, chip.checks ? CHECKS_WORDS[chip.checks] : null, age].filter(Boolean).join(" · ");
  return (
    <Hint label={hint}>
      <button
        type="button"
        data-request-pr-chip={pull.number}
        data-request-historical={historical ? "true" : undefined}
        data-request-focus="chip"
        data-pr-tone={chip.tone}
        data-stale={stale ? "true" : undefined}
        aria-label={words}
        className={cn("pointer-events-auto relative z-10 inline-flex shrink-0 items-center gap-xxs rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring", stale && "opacity-(--opacity-dimmed)")}
        onClick={(event) => {
          event.stopPropagation();
          if (gitHubClick(event, pull.url, project.device_id, handlers)) return;
          if (listed) handlers.openPullRequestRow(project, pull.number);
          else handlers.openGitHub(pull.url, project.device_id);
        }}
      >
        {historical ? <span className="text-muted-foreground">예전 PR #{pull.number} {BADGE_WORDS[pull.badge]}</span> : <Badge variant="outline" className={PR_TONE[chip.tone]}>
          <GitPullRequestIcon aria-hidden="true" />#{pull.number}
          {chip.checks ? <ChecksMark checks={chip.checks} /> : null}
        </Badge>}
        {more > 0 ? <span className="font-mono text-caption text-muted-foreground">+{more}</span> : null}
      </button>
    </Hint>
  );
}

/** Something to open from the agent's last words (D-39): a link by its short name, opened as a terminal link opens it (B49). */
function OpenChip({ target, actions }: { target: ResolvedTarget; actions: Actions }) {
  return (
    <Hint label={`${target.label} 열기`}>
      <button
        type="button"
        data-request-open={target.key}
        data-request-focus="chip"
        className="pointer-events-auto relative z-10 inline-flex max-w-(--size-pane-child-chip-max) shrink-0 items-center gap-xxs rounded-xs border border-border px-xs font-mono text-caption text-subtle-foreground outline-none hover:bg-secondary hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        onClick={(event) => {
          event.stopPropagation();
          openTarget(target, event, actions);
        }}
      >
        <LinkIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
        <span className="truncate">{target.label}</span>
      </button>
    </Hint>
  );
}

type ResolvedTarget = OpenCandidate & { found: FoundPath | null };

function openTarget(target: ResolvedTarget, event: MouseEvent, actions: Actions) {
  const external = holdsCommandKey(event);
  if (target.target.kind === "url") actions.openLink(target.target.url, external);
  else if (target.found) actions.openTerminalPath(target.found, target.target.line, target.target.column, external);
}

/**
 * The row's open targets: every URL, and each path the host says exists
 * under the pane's folder or its checkout (B49). Asked again only when the
 * agent's words change; the host's answers are cached for a while.
 */
function useOpenTargets(agent: AgentRow, reply: string, pulls: readonly AgentPullRequest[], local: boolean): ResolvedTarget[] {
  const exclude = useMemo(() => pulls.map((pull) => pull.url), [pulls]);
  const candidates = useMemo(() => openCandidates(reply, exclude, local && hostBridge() !== null), [reply, exclude, local]);
  const [found, setFound] = useState<ReadonlyMap<string, FoundPath>>(new Map());
  const paneId = agent.pane_id;
  useEffect(() => {
    const bridge = hostBridge();
    const paths = candidates.filter((candidate) => candidate.target.kind === "path");
    if (!bridge || paths.length === 0) return;
    const context = paneContext(useShellStore.getState().rest, paneId);
    if (!context) return;
    const lookups = new Map(paths.map((candidate) => [candidate.key, candidate.target.kind === "path" ? pathLookups(candidate.target.path, context.cwd, context.root) : []]));
    let current = true;
    void probePaths([...lookups.values()].flat(), (batch) => bridge.probePaths(batch)).then((answers) => {
      if (!current) return;
      const next = new Map<string, FoundPath>();
      for (const [key, spellings] of lookups) {
        for (const spelling of spellings) {
          const answer = answers.get(spelling);
          if (answer) {
            next.set(key, answer);
            break;
          }
        }
      }
      setFound(next);
    });
    return () => {
      current = false;
    };
  }, [candidates, paneId]);
  return useMemo(
    () =>
      candidates.flatMap((candidate): ResolvedTarget[] => {
        if (candidate.target.kind === "url") return [{ ...candidate, found: null }];
        const path = found.get(candidate.key);
        return path ? [{ ...candidate, found: path }] : [];
      }),
    [candidates, found],
  );
}

// --- the expanded row ----------------------------------------------------------

const BADGE_WORDS: Record<AgentPullRequest["badge"], string> = { merged: "머지됨", closed: "닫힘", review: "열림", open: "열림" };

/**
 * Everything the folded row cut (B6, B13, B56): the full request (twenty
 * lines, then `전부 보기`), the agent's last words, every open target, every
 * pull request with its state and those settled before the request as
 * `예전 PR #N 머지됨`, the descendants with their verb and line, and Open.
 */
function RowDetail({ row, targets, full, onFull, onOpen, handlers, actions }: { row: RequestRow; targets: readonly ResolvedTarget[]; full: boolean; onFull: () => void; onOpen: () => void; handlers: LensHandlers; actions: Actions }) {
  const { agent, project } = row.lens;
  const block = agent.request;
  const request = block?.request ? fullRequest(block.request.text, full) : null;
  const pulls = block?.pull_requests ?? [];
  const issues = rowIssues(row, project);
  return (
    <div className="flex min-w-0 flex-col gap-sm px-sm pb-sm pl-[calc(var(--spacing-sm)+var(--size-agent-mark)*2)] text-caption" data-request-detail={agent.pane_id}>
      <p className="min-w-0 break-words text-subtle-foreground" data-request-place={agent.pane_id}>
        {agent.identity_label} · {project.label} · {row.lens.device ? `${row.lens.device} · ` : ""}{row.lens.checkout.branch ?? row.lens.checkout.label}
      </p>
      {request ? (
        <div className="flex min-w-0 flex-col gap-xxs">
          <span className="text-subtle-foreground">{senderWords(block!.request!.sender)} ›</span>
          <p className="whitespace-pre-wrap break-words text-foreground" data-request-full={agent.pane_id}>
            {request.text}
            {block!.request!.images > 0 ? `\n이미지 ${block!.request!.images}` : ""}
          </p>
          {request.more ? (
            <button type="button" data-request-focus="chip" data-request-more={agent.pane_id} className="self-start rounded-xs text-subtle-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" onClick={onFull}>
              전부 보기
            </button>
          ) : null}
        </div>
      ) : null}
      {block?.reply?.text ? (
        <p className="whitespace-pre-wrap break-words text-muted-foreground" data-request-reply={agent.pane_id}>
          {block.reply.text}
        </p>
      ) : null}
      {verdictLine(block) ? (
        <span className="min-w-0 break-words text-subtle-foreground" data-request-verdict={block!.end}>
          {verdictLine(block)}
        </span>
      ) : null}
      {targets.length > 0 ? (
        <span className="flex min-w-0 flex-col gap-xs" data-request-targets={targets.length}>
          {targets.map((target) => (
            <span key={target.key} className="flex min-w-0 items-start gap-xs">
              <OpenChip target={target} actions={actions} />
              <span className="min-w-0 break-all text-muted-foreground">{target.key}</span>
            </span>
          ))}
        </span>
      ) : null}
      {pulls.length > 0 ? (
        <ul className="flex min-w-0 flex-col gap-xxs" role="list" data-request-pulls={pulls.length}>
          {pulls.map((pull) => (
            <li key={pull.url} className="flex min-w-0 items-start gap-xs" data-request-pull={pull.number} data-live={pull.live ? "true" : undefined}>
              <RowPullRequestChip project={project} pull={pull} more={0} handlers={handlers} historical={!pull.live} />
              <span className="min-w-0 flex-1 break-words text-foreground">{pull.title}</span>
              {pull.live ? <span className="shrink-0 text-muted-foreground">{BADGE_WORDS[pull.badge]}</span> : null}
            </li>
          ))}
        </ul>
      ) : null}
      {issues.length > 0 ? (
        <span className="flex min-w-0 flex-wrap items-center gap-sm" data-request-all-issues="true">
          {issues.map((task) => (
            <IssueChip key={task.key} project={project} task={task} handlers={handlers} now={Date.now()} />
          ))}
        </span>
      ) : null}
      {row.children.length > 0 ? (
        <ul className="flex min-w-0 flex-col gap-xxs" role="list" data-request-child-list={row.children.length}>
          {row.children.map((child) => (
            <li key={child.pane_id} className="flex min-w-0 items-center gap-xs" data-request-child={child.pane_id}>
              <StatusMark symbol={child.symbol} className={markTone(child)} />
              <AgentMark kind={child.agent_kind} />
              <span className="min-w-0 max-w-[40%] shrink-0 truncate text-foreground">{child.identity_label}</span>
              <span className="shrink-0 text-subtle-foreground">{VERB_LABEL[child.request?.verb ?? (child.group === "working" ? "working" : "idle")]}</span>
              <span className="min-w-0 flex-1 truncate text-muted-foreground">{child.request?.line ?? child.request?.reply?.text.split("\n").at(-1) ?? ""}</span>
              <button type="button" data-request-focus="chip" data-request-child-open={child.pane_id} className="shrink-0 rounded-xs text-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" onClick={() => handlers.openAgent(child.pane_id)}>
                열기
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <span className="flex items-center gap-sm">
        <Button variant="secondary" size="sm" onClick={onOpen} data-request-focus="chip" data-request-open-pane={agent.pane_id}>
          <SquareArrowOutUpRightIcon aria-hidden="true" />
          패널 열기
        </Button>
        <span className="text-muted-foreground">{fieldLabel("Enter")}</span>
      </span>
    </div>
  );
}
