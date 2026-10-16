import { ChevronDownIcon, ChevronRightIcon, EllipsisIcon, ExternalLinkIcon, SparklesIcon, SquareTerminalIcon, TriangleAlertIcon } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { Actions } from "./actions";
import { lineTone, markTone, rowLine } from "./agentRow";
import { agentProductName } from "./agentAdapters";
import { Elapsed } from "./components/elapsed";
import { PrMark } from "./components/pr-mark";
import { StatusMark } from "./components/status-mark";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import { cn } from "./lib/utils";
import { LinkSessions } from "./LinkSessions";
import { renderMarkdown } from "./mobile/markdown";
import { boardProjects } from "./navigation";
import { bodyOutline, bodyPreview, confirmHolds, defaultMethod, fileOpen, mergeWhy, prFacts, primaryAction, prSteps, splitFiles, type Confirmed, type PrFact, type PrimaryAction, type PrStep, type StepNote } from "./prCardModel";
import { linkHandlers, PrLinks, type LinkHandlers } from "./PrLinks";
import { ReasonLine } from "./prReason";
import { staleLabel, staleness } from "./prMark";
import { findPrEntry, opensCard, type PrEntry } from "./prWork";
import { requireInterfaceLanguage } from "./i18n/locale";
import { catalogWorkspaces, frontCheckout, type LinkPanel, type PrCard, type PrMergeMethod, type PullRequest, type Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { useRequestViewDemand } from "./requestViewDemand";

// A pull request's decision card (PRD work-tab-pr-lens D-06, D-21, D-22;
// PRD sidebar-task-lens D-28, D-35, D-36, B32-B37): the body of the lane's
// 보기 slot, under the slot's own head (←, `PR #N · <project>`, ↗, ✕). A
// one-line head and its meta line, the decision box holding the case's word,
// facts, blockers and button, the four steps, the session that made it, and
// the body, the changed files and the links folded into one line each. The
// head, the box and the steps are drawn from the list's own values, so they
// stand while the card reads its body, files and review threads; each of
// those is a part with its own placeholder, failure and retry.

type Section = "body" | "files" | "links";

/**
 * The folded lines the operator opened, per card. The card leaves the screen
 * when a changed file turns the lane to Files, and the 보기 slot brings it
 * back as it was left (B49); the page keeps it while it runs, never stored.
 */
const OPENED = new Map<string, ReadonlySet<Section>>();
/** As many cards as the slot steps back through (`DETAIL_HISTORY_LIMIT`); past it the least recently changed card forgets its sections. */
const OPENED_LIMIT = 20;

function useSections(key: string): [ReadonlySet<Section>, (section: Section) => void] {
  const [open, setOpen] = useState<ReadonlySet<Section>>(() => OPENED.get(key) ?? new Set());
  useEffect(() => setOpen(OPENED.get(key) ?? new Set()), [key]);
  const toggle = (section: Section) => {
    const next = new Set(open);
    if (next.has(section)) next.delete(section);
    else next.add(section);
    OPENED.delete(key);
    OPENED.set(key, next);
    if (OPENED.size > OPENED_LIMIT) OPENED.delete(OPENED.keys().next().value!);
    setOpen(next);
  };
  return [open, toggle];
}

/** The catalog project `workspaceId` and its pull request `number`, with the list's row for it. */
function useCardTarget(workspaceId: string, number: number) {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  return useMemo(() => {
    const project = catalogWorkspaces(rest).find((row) => row.id === workspaceId) ?? null;
    if (!project) return { project: null, entry: null, pr: null };
    const scope = boardProjects(rest, agents, project.device_id).find((item) => item.workspace.id === project.id);
    const pr = project.pull_requests?.find((row) => row.number === number) ?? null;
    return { project, entry: scope ? findPrEntry(scope, number) : null, pr };
  }, [rest, agents, workspaceId, number]);
}

export function PrCardView({ workspaceId, number, actions }: { workspaceId: string; number: number; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const { project, entry, pr } = useCardTarget(workspaceId, number);
  const card = useShellStore((state) => (project ? state.rest?.pr_work?.cards?.find((row) => row.workspace_id === project.id && row.number === number) ?? null : null));
  const record = useShellStore((state) => state.linkPanel);
  const devices = useShellStore((state) => state.rest?.navigator?.devices);
  const live = useShellStore((state) => state.connection === "live");
  const [open, toggle] = useSections(`${workspaceId}#${number}`);
  const projectId = project?.id ?? null;
  const updatedAt = pr?.updated_at_unix_ms ?? null;
  const checks = pr?.checks ?? null;
  // The card's own parts are read again when its pull request moved (B44);
  // the core reads them on the event that opened the slot.
  const reads = project ? opensCard(project, devices) : false;
  useEffect(() => {
    if (live && projectId && reads) actions.readPrCard(projectId, number);
  }, [actions, live, projectId, reads, number, updatedAt, checks]);
  // Share the visible-board demand so pending checks are read again every minute (B44).
  useRequestViewDemand(actions, live);
  // The link record: the sessions that made or worked on it and what it is linked to.
  useEffect(() => {
    if (!projectId) return;
    actions.openLinks(projectId, { kind: "pr", number });
    return () => actions.closeLinks();
  }, [actions, projectId, number]);
  const handlers = useMemo(() => linkHandlers(actions), [actions]);
  if (!project) return <p className="p-lg text-caption text-muted-foreground" data-pr-card-missing={number}>{t("work.card.unlisted", { number })}</p>;
  const panel: LinkPanel | null = record && record.workspace_id === project.id && record.target.kind === "pr" && record.target.number === number ? record : null;
  return (
    <article aria-label={t("work.card.aria", { number })} className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto text-foreground" data-pr-card={number} data-pr-card-project={project.id} data-pr-card-head={card?.head ?? undefined}>
      <div className="flex w-full flex-col gap-md p-md">
        {pr ? <Head project={project} pr={pr} entry={entry} card={card} actions={actions} /> : <p className="text-caption text-muted-foreground" data-pr-card-unlisted={number}>{t("work.card.unlisted", { number })}</p>}
        {pr ? <DecisionBox project={project} pr={pr} entry={entry} card={card} actions={actions} /> : null}
        {pr ? <Steps pr={pr} entry={entry} /> : null}
        {pr && entry ? <MadeBy entry={entry} actions={actions} /> : null}
        <div className="flex flex-col">
          {pr ? <BodyRow card={card} actions={actions} projectId={project.id} number={number} open={open.has("body")} onToggle={() => toggle("body")} /> : null}
          {pr ? <FilesRow pr={pr} project={project} card={card} actions={actions} open={open.has("files")} onToggle={() => toggle("files")} /> : null}
          <LinksRow project={project} pr={pr} entry={entry} panel={panel} handlers={handlers} actions={actions} number={number} open={open.has("links")} onToggle={() => toggle("links")} />
        </div>
      </div>
    </article>
  );
}

// --- head ---------------------------------------------------------------------

/** One line of mark, number, title and author, the Workspace button, then `branch → base · files · +/-` (B32). */
function Head({ project, pr, entry, card, actions }: { project: Workspace; pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions }) {
  const { t, i18n } = useInterfaceTranslation();
  const checkoutId = entry?.row.checkout_ids[0] ?? entry?.row.checkout_id ?? null;
  const checkout = checkoutId ? project.checkouts.find((row) => row.id === checkoutId && row.exists) ?? null : null;
  const author = pr.work && pr.work.mine !== true ? pr.work.author : null;
  const files = card?.files ?? null;
  const added = files?.reduce((sum, file) => sum + file.additions, 0) ?? 0;
  const removed = files?.reduce((sum, file) => sum + file.deletions, 0) ?? 0;
  const stale = staleness(project.checkouts.find((row) => row.github)?.github);
  const staleText = staleLabel(stale, t, requireInterfaceLanguage(i18n.language));
  return (
    <header className="flex flex-col gap-xs" data-pr-card-header="true">
      <div className="flex min-w-0 items-start gap-sm">
        <h2 className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-xs text-body font-semibold">
          <PrMark state={pr.state} number={pr.number} />
          <span className="min-w-0 break-words break-keep" data-pr-card-title="true">{pr.title}</span>
          {author ? <span className="text-caption font-normal text-muted-foreground" data-pr-card-author={author}>{t("work.row.author", { name: author })}</span> : null}
        </h2>
        {checkout ? (
          <Hint label={t("work.card.openWorkspace")}>
            <Button variant="ghost" size="sm" className="shrink-0" onClick={() => actions.openWorkspace(project.device_id, checkout.workspace_id, checkout.id)} data-pr-card-workspace={pr.number}>
              <SquareTerminalIcon aria-hidden="true" />
              {t("work.card.workspace")}
            </Button>
          </Hint>
        ) : null}
        <CardMenu project={project} pr={pr} entry={entry} />
      </div>
      <p className="flex min-w-0 flex-wrap items-center gap-x-xs gap-y-xxs font-mono text-micro text-muted-foreground" data-pr-card-facts="true">
        <span className="min-w-0 truncate" data-pr-card-branch={pr.head_branch}>{pr.head_branch}</span>
        {pr.base_branch ? <><span aria-hidden="true">→</span><span className="min-w-0 truncate">{pr.base_branch}</span></> : null}
        {files ? <><span aria-hidden="true">·</span><span className="font-sans" data-pr-card-files={card?.files_total ?? files.length}>{t("work.card.files", { count: card?.files_total ?? files.length })}</span><span aria-hidden="true">·</span><span className="text-pr-mergeable">+{added}</span><span className="text-pr-failed">−{removed}</span></> : null}
        {staleText ? <Hint label={staleText}><span role="img" aria-label={staleText} className="inline-flex text-warning" data-pr-card-stale="true"><TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" /></span></Hint> : null}
      </p>
    </header>
  );
}

/** `⋯` of the card (D-28): hand the PR to an agent, make an issue for it, copy the branch name. */
function CardMenu({ project, pr, entry }: { project: Workspace; pr: PullRequest; entry: PrEntry | null }) {
  const { t } = useInterfaceTranslation();
  const merged = pr.state === "merged" || pr.state === "closed";
  const linkable = !merged && !entry?.row.issue;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon-sm" className="shrink-0" aria-label={t("work.card.menu")} data-pr-card-menu={pr.number}>
          <EllipsisIcon aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {!merged ? <DropdownMenuItem onSelect={() => useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: pr.number })}>{t("prList.delegate")}</DropdownMenuItem> : null}
        {linkable ? <DropdownMenuItem onSelect={() => useUiStore.getState().setWorkspaceDialog({ kind: "pr_new_issue", workspaceId: project.id, prNumber: pr.number })}>{t("prList.newIssue")}</DropdownMenuItem> : null}
        <DropdownMenuItem onSelect={() => void navigator.clipboard?.writeText(pr.head_branch ?? "")} data-pr-card-copy="true">{t("prList.copyBranch")}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// --- decision box ----------------------------------------------------------------

/** The box's edge says the case: green to merge, red when blocked, amber when it waits on the operator. */
function boxTone(entry: PrEntry | null): string {
  if (!entry) return "border-border";
  if (entry.reason.kind === "merge") return "border-pr-mergeable";
  if (entry.row.group === "blocked" || entry.reason.kind === "failed" || entry.reason.kind === "conflict") return "border-pr-failed";
  if (entry.row.group === "needs_you") return "border-warning";
  return "border-border";
}

function factText(fact: PrFact, t: ReturnType<typeof useInterfaceTranslation>["t"]): string {
  switch (fact.kind) {
    case "checks_passing":
      return t("work.card.fact.checksPassing");
    case "checks_running":
      return t("work.card.why.ci", { count: fact.count });
    case "checks_failed":
      return t("work.card.why.failed", { count: fact.count });
    case "approved":
      return t("board.review.approved");
    case "changes":
      return t("board.review.changes");
    case "review_required":
      return t("board.review.required");
    case "conflict":
      return t("work.card.conflict", { base: fact.base });
    case "no_conflict":
      return t("work.card.fact.noConflict", { base: fact.base });
    case "behind":
      return t("work.card.fact.behind", { base: fact.base });
  }
}

/**
 * The decision box (B33): the case's word and reason as the list row says
 * them, its button at the right, one line of facts, what blocks it, and
 * hide-ai's summary while it is on; the merge confirmation and the last
 * write's line stay inside it.
 */
function DecisionBox({ project, pr, entry, card, actions }: { project: Workspace; pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const facts = prFacts(pr);
  return (
    <section className={cn("flex flex-col gap-sm rounded-md border bg-card px-md py-sm", boxTone(entry))} data-pr-card-reason={entry?.reason.kind} data-pr-card-group={entry?.row.group}>
      <Buttons project={project} pr={pr} entry={entry} card={card} actions={actions}>
        {entry ? <ReasonLine reason={entry.reason} group={entry.row.group} noAgent={entry.row.group === "blocked"} className="min-w-0 flex-1 whitespace-normal text-caption font-medium" /> : <span className="flex-1" />}
      </Buttons>
      {facts.length > 0 ? <p className="text-micro text-muted-foreground" data-pr-card-fact-line="true">{facts.map((fact) => factText(fact, t)).join(" · ")}</p> : null}
      <Blocked pr={pr} entry={entry} card={card} actions={actions} projectId={project.id} />
      {card?.summary ? (
        <p className="flex flex-col gap-xxs text-caption text-foreground" data-pr-card-summary="true">
          <span className="inline-flex items-center gap-xxs text-micro text-subtle-foreground"><SparklesIcon aria-hidden="true" className="size-(--size-icon-sm)" />{t("work.card.aiSummary")}</span>
          <span className="whitespace-pre-line break-words">{card.summary}</span>
        </p>
      ) : null}
    </section>
  );
}

/** What blocks it (B33): a conflict, each failed check with its log, each unresolved thread. Nothing blocked, nothing drawn. */
function Blocked({ pr, entry, card, actions, projectId }: { pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions; projectId: string }) {
  const { t } = useInterfaceTranslation();
  const open = pr.state !== "merged" && pr.state !== "closed";
  const conflict = open && pr.work?.merge_state === "dirty";
  const failed = open ? pr.work?.failed_checks ?? [] : [];
  const threads = card?.threads ?? [];
  const device = entry?.project.device_id ?? "";
  // A thread read that failed says so with a retry, never "nothing blocked" (B37).
  const threadsFailed = card?.failed.includes("threads") === true;
  if (!conflict && failed.length === 0 && threads.length === 0 && !threadsFailed) return null;
  return (
    <ul className="flex flex-col gap-xxs" aria-label={t("work.card.blocked")} data-pr-card-blocked="true">
      {conflict ? <li className="text-caption text-pr-failed" data-pr-card-conflict="true">✗ {t("work.card.conflict", { base: pr.base_branch ?? "" })}</li> : null}
      {failed.map((check) => (
        <li key={check.name} className="flex min-w-0 items-center gap-xs text-caption" data-pr-card-failed={check.name}>
          <span className="min-w-0 flex-1 truncate"><span className="text-pr-failed">✗</span> <span className="font-mono">{check.name}</span></span>
          {check.url ? (
            <button type="button" className="inline-flex shrink-0 items-center gap-xxs rounded-xs text-micro text-muted-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" onClick={() => actions.openPullRequest(check.url!, device, true)}>
              {t("work.card.checkLog")}
              <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon-sm)" />
            </button>
          ) : null}
        </li>
      ))}
      {threadsFailed ? <li><PartState part="threads" card={card} actions={actions} projectId={projectId} number={pr.number} /></li> : null}
      {threads.map((thread, index) => {
        const place = thread.line == null ? thread.path : `${thread.path}:${thread.line}`;
        return <li key={`${thread.path}:${thread.line}:${index}`} className="min-w-0 truncate text-caption text-foreground" title={thread.text} data-pr-card-thread={place}>{t("work.card.thread", { reviewer: thread.reviewer ?? "?", place, text: thread.text })}</li>;
      })}
    </ul>
  );
}

type Confirm = Confirmed | null;

function confirmText(confirm: Confirmed, queue: boolean, t: (key: string, values?: Record<string, unknown>) => string, values: { number: number; base: string; method: string; head: string }): string {
  return t(confirm.action === "merge" ? (queue ? "work.card.confirm.queue" : "work.card.confirm.merge") : "work.card.confirm.update", values);
}

/**
 * The box's first line and its buttons (D-21, B33, B37): the case's word at
 * the left, the case's button at the right, and Merge, off with its real
 * reason beside it while it cannot be pressed. Merge and a branch update ask
 * once and go out on the head the card showed; what they did stands in one
 * line under them.
 */
function Buttons({ project, pr, entry, card, actions, children }: { project: Workspace; pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions; children: ReactNode }) {
  const { t } = useInterfaceTranslation();
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [picked, setPicked] = useState<PrMergeMethod | null>(null);
  const open = pr.state !== "merged" && pr.state !== "closed";
  const kind = entry?.reason.kind ?? "checking";
  const primary: PrimaryAction = primaryAction(kind);
  const mine = pr.work?.mine === true;
  const working = entry?.row.group === "working";
  const agent = entry?.agents[0] ?? null;
  const method = picked ?? defaultMethod(card);
  const queue = card?.merge?.queue === true;
  const queued = pr.work?.queue.state === "at";
  const why = mergeWhy(pr, card, working);
  const write = card?.write ?? null;
  const writing = write?.phase === "working";
  const head = card?.head ?? null;
  const github = (path = "") => actions.openPullRequest(`${pr.url}${path}`, project.device_id, true);
  const confirming = confirm !== null && confirmHolds(confirm, { head, queue, method, mergeBlocked: why !== null, behind: pr.work?.merge_state === "behind" }) ? confirm : null;
  useEffect(() => {
    if (confirm !== null && confirming === null) setConfirm(null);
  }, [confirm, confirming]);
  const line = <div className="flex min-w-0 flex-wrap items-center gap-sm">{children}</div>;
  if (!open) return line;
  const base = pr.base_branch ?? "";
  const methodLabel = method ? t(`work.card.method.${method}`) : "";
  // A write GitHub refused for the head, the merge state or the permission would only be refused again, so both buttons stay off until Look again has read the card; a transient failure (unreachable, failed) is retried with the same button (B37).
  const repeats = write?.phase === "failed" && write.reason !== "unreachable" && write.reason !== "failed";
  const canAsk = head !== null && !writing && !repeats;
  const MergeButton = (
    <span className="flex min-w-0 items-center gap-xs" data-pr-card-merge-slot="true">
      {why ? <span className="min-w-0 truncate text-micro text-muted-foreground" data-pr-card-merge-why={why.key}>{t(`work.card.why.${why.key}`, { count: why.count, base: why.base })}</span> : null}
      {!queue && (card?.merge?.methods.length ?? 0) > 1 ? (
        <select aria-label={t("work.card.merge")} className="rounded-xs border border-border bg-background px-xs py-xxs text-micro outline-none focus-visible:ring-1 focus-visible:ring-ring" value={method ?? ""} onChange={(event) => setPicked(event.target.value as PrMergeMethod)} data-pr-card-method="true">
          {card!.merge!.methods.map((each) => <option key={each} value={each}>{t(`work.card.method.${each}`)}</option>)}
        </select>
      ) : null}
      <Button variant={primary === "merge" ? "default" : "secondary"} size="sm" disabled={why !== null || queued || !canAsk} onClick={() => head !== null && setConfirm({ action: "merge", head, method, queue })} data-pr-card-merge={queue ? "queue" : "merge"}>
        {queue ? t("work.card.mergeQueue") : t("work.card.merge")}
      </Button>
    </span>
  );
  const UpdateButton = (
    <Button variant={primary === "update" ? "default" : "secondary"} size="sm" disabled={!canAsk || write?.phase === "working"} onClick={() => head !== null && setConfirm({ action: "update", head, method: null, queue })} data-pr-card-update="true">{t("work.card.updateBranch")}</Button>
  );
  const askPane = agent ? () => actions.openAgent(agent.pane_id) : null;
  return (
    <div className="flex flex-col gap-sm" data-pr-card-buttons={primary}>
      <div className="flex min-w-0 flex-wrap items-center gap-sm">
        {children}
        <span className="flex min-w-0 flex-wrap items-center justify-end gap-xs">
          {primary === "update" || pr.work?.merge_state === "behind" ? UpdateButton : null}
          {primary === "review" ? <><Button size="sm" variant="outline" onClick={() => review(project, pr)} data-pr-card-review-agent="true">{t("work.card.reviewAgent")}</Button><Button size="sm" onClick={() => github("/files")} data-pr-card-review-github="true"><ExternalLinkIcon aria-hidden="true" />{t("work.card.reviewGithub")}</Button></> : null}
          {primary === "assign" ? <Button size="sm" onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: pr.number })} data-pr-card-assign="true">{t("work.card.assign")}</Button> : null}
          {primary === "answer" && askPane ? <Button size="sm" onClick={askPane} data-pr-card-answer="true"><SquareTerminalIcon aria-hidden="true" />{t("work.card.answer")}</Button> : null}
          {primary === "view_pane" && askPane ? <Button size="sm" onClick={askPane} data-pr-card-view-pane="true"><SquareTerminalIcon aria-hidden="true" />{t("work.card.viewPane")}</Button> : null}
          {primary === "assign_reviewer" ? <Button size="sm" variant="outline" onClick={() => github()} data-pr-card-assign-reviewer="true">{t("work.card.assignReviewer")}</Button> : null}
          {mine && primary !== "review" ? MergeButton : null}
        </span>
      </div>
      {confirming !== null ? (
        <div role="alertdialog" aria-label={confirmText(confirming, queue, t, { number: pr.number, base, method: methodLabel, head: confirming.head.slice(0, 7) })} className="flex flex-col gap-sm rounded-md border border-border bg-background p-sm" data-pr-card-confirm={confirming.action}>
          <p className="text-caption">{confirmText(confirming, queue, t, { number: pr.number, base, method: methodLabel, head: confirming.head.slice(0, 7) })}</p>
          <span className="flex gap-xs">
            <Button size="sm" onClick={() => {
              if (confirming.action === "merge") actions.mergePullRequest(project.id, pr.number, confirming.head, confirming.queue ? null : confirming.method);
              else actions.updatePullRequestBranch(project.id, pr.number, confirming.head);
              setConfirm(null);
            }} data-pr-card-confirm-yes={confirming.action}>{t(confirming.action === "merge" ? (queue ? "work.card.confirm.doQueue" : "work.card.confirm.doMerge") : "work.card.confirm.doUpdate", { number: pr.number })}</Button>
            <Button size="sm" variant="ghost" autoFocus onClick={() => setConfirm(null)}>{t("common.cancel")}</Button>
          </span>
        </div>
      ) : null}
      <WriteLine write={write} base={base} card={card} actions={actions} projectId={project.id} number={pr.number} />
    </div>
  );
}

/** What the last Merge or branch update did, in one line under the buttons (B37). */
function WriteLine({ write, base, card, actions, projectId, number }: { write: PrCard["write"]; base: string; card: PrCard | null; actions: Actions; projectId: string; number: number }) {
  const { t } = useInterfaceTranslation();
  if (!write) return null;
  if (write.phase === "working") return <p role="status" className="text-caption text-muted-foreground" data-pr-card-write="working">{t("work.card.writing")}</p>;
  if (write.phase === "done") return <p role="status" className="text-caption text-pr-mergeable" data-pr-card-write="done">{t(write.action === "merge" ? "work.card.written.merge" : "work.card.written.update")}</p>;
  const reason = write.reason ?? "failed";
  return (
    <p role="alert" className="flex flex-wrap items-center gap-sm text-caption text-warning" data-pr-card-write="failed" data-pr-card-write-reason={reason}>
      <span>{t(`work.card.fail.${reason}`, { base })}</span>
      <Button size="sm" variant="secondary" disabled={card?.reading === true} onClick={() => actions.lookAgainPrCard(projectId, number)} data-pr-card-look-again="true">{t("work.card.lookAgain")}</Button>
    </p>
  );
}

/** `Have an agent review`: the Start dialog's agent opens on the branch with the review instruction. */
function review(project: Workspace, pr: PullRequest) {
  useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: pr.number, intent: "review" });
}

// --- steps ----------------------------------------------------------------------

const STEP_BAR: Record<PrStep["state"], string> = { done: "bg-success", active: "bg-pr-pending", failed: "bg-pr-failed", empty: "bg-border" };
const STEP_LABEL: Record<PrStep["id"], MessageKey> = { work: "work.card.step.work", pr: "work.card.step.pr", review: "work.card.step.review", merge: "work.card.step.merge" };

function noteText(note: StepNote, t: ReturnType<typeof useInterfaceTranslation>["t"]): string {
  switch (note.kind) {
    case "agent":
      return note.name;
    case "external":
      return t("work.card.note.external");
    case "draft":
      return t("work.card.note.draft");
    case "checks_passing":
      return t("work.card.fact.checksPassing");
    case "checks_running":
      return t("work.card.why.ci", { count: note.count });
    case "checks_failed":
      return t("work.card.note.failed", { count: note.count });
    case "approved":
      return t("board.review.approved");
    case "changes":
      return t("board.review.changes");
    case "review_waiting":
      return t("work.card.note.reviewWaiting");
    case "asked_of_me":
      return t("work.card.note.askedOfMe");
    case "no_reviewer":
      return t("work.card.note.noReviewer");
    case "merged":
      return t("work.card.note.merged");
    case "queued":
      return t("work.card.note.queued", { position: note.count });
    case "closed":
      return t("work.card.note.closed");
  }
}

/** `작업 · PR · 검토 · 머지` (B34): a display, not a button; each step says who worked, the checks or the review under its label. */
function Steps({ pr, entry }: { pr: PullRequest; entry: PrEntry | null }) {
  const { t } = useInterfaceTranslation();
  const agent = entry?.agents[0] ?? null;
  const steps = prSteps(pr, agent ? agentProductName(agent.agent_kind) : null, entry?.reason.kind ?? null);
  return (
    <ol className="grid grid-cols-4 gap-sm" aria-label={t("work.card.steps")} data-pr-card-steps={steps.filter((step) => step.state === "done").length}>
      {steps.map((step) => {
        const note = step.note ? noteText(step.note, t) : null;
        return (
          <li key={step.id} className="flex min-w-0 flex-col gap-xxs" aria-label={[t(STEP_LABEL[step.id]), t(`work.card.stepState.${step.state}`), note].filter(Boolean).join(", ")} data-pr-card-step={step.id} data-state={step.state}>
            <span aria-hidden="true" className={cn("h-(--size-hairline) rounded-full", STEP_BAR[step.state])} />
            <span aria-hidden="true" className="truncate text-micro font-medium text-foreground">{t(STEP_LABEL[step.id])}</span>
            {note ? <span aria-hidden="true" className={cn("truncate text-micro", step.state === "failed" ? "text-pr-failed" : "text-muted-foreground")}>{note}</span> : null}
          </li>
        );
      })}
    </ol>
  );
}

// --- the session that made it -----------------------------------------------------

/** The pull request's agent as one line (B35): its mark, `만든 세션 · <agent> · <when>`, its last words or its question, and `pane으로`. */
function MadeBy({ entry, actions }: { entry: PrEntry; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const agent = entry.agents[0];
  if (!agent) return null;
  const said = rowLine(agent);
  const asking = entry.reason.kind === "reply";
  const text = (asking ? entry.reason.text : null) ?? said?.text ?? null;
  return (
    <section className="flex min-w-0 items-start gap-sm border-t border-border pt-sm" data-pr-card-agent={agent.pane_id}>
      <StatusMark symbol={agent.symbol} className={cn("mt-xxs", markTone(agent))} />
      <div className="flex min-w-0 flex-1 flex-col gap-xxs">
        <p className="flex min-w-0 items-center gap-xs text-micro text-muted-foreground">
          <span className="min-w-0 truncate" title={agent.identity_label}>{t("work.card.made", { agent: agentProductName(agent.agent_kind) })}</span>
          <span aria-hidden="true">·</span>
          <Elapsed since={agent.state.request_since} className="shrink-0" />
        </p>
        {text ? <p className={cn("line-clamp-2 whitespace-pre-line break-words text-caption", asking ? "text-warning" : said ? lineTone(said, agent) : "text-foreground")} data-pr-card-agent-text={asking ? "ask" : "said"}>{text}</p> : null}
      </div>
      <Button variant="outline" size="sm" className="shrink-0" onClick={() => actions.openAgent(agent.pane_id)} data-pr-card-to-pane={agent.pane_id}>{t("work.card.toPane")}</Button>
    </section>
  );
}

// --- folded rows --------------------------------------------------------------------

/** One folded line that opens in place (B36): its name at the left, what it holds at the right. */
function FoldRow({ name, summary, open, onToggle, section, children }: { name: string; summary: ReactNode; open: boolean; onToggle: () => void; section: Section; children: ReactNode }) {
  return (
    <section className="flex flex-col border-t border-border" aria-label={name} data-pr-card-row={section} data-open={open ? "true" : undefined}>
      <button type="button" aria-expanded={open} onClick={onToggle} className="flex min-h-(--size-control) w-full min-w-0 items-center gap-xs rounded-xs py-xs text-left text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" data-pr-card-toggle={section}>
        {open ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />}
        <span className="shrink-0 text-foreground">{name}</span>
        <span className="flex min-w-0 flex-1 items-center justify-end gap-xs text-micro text-muted-foreground">{summary}</span>
      </button>
      {open ? <div className="flex flex-col gap-sm pb-sm pl-md">{children}</div> : null}
    </section>
  );
}

/** A card part that has not been read, or could not be (B37). */
function PartState({ part, card, actions, projectId, number }: { part: "body" | "files" | "threads"; card: PrCard | null; actions: Actions; projectId: string; number: number }) {
  const { t } = useInterfaceTranslation();
  // The last read's failure stays drawn while a new read runs, so a re-read moves nothing under the pointer.
  const failed = card?.failed.includes(part) === true;
  if (failed) {
    return (
      <p className="flex items-center gap-sm text-caption text-muted-foreground" data-pr-card-part={part} data-pr-card-part-state="failed">
        {t("work.card.part.failed")}
        <Button variant="secondary" size="sm" disabled={card?.reading === true} onClick={() => actions.readPrCard(projectId, number)}>{t("common.retry")}</Button>
      </p>
    );
  }
  return <span className="block h-(--size-control) rounded-sm bg-muted" role="status" aria-label={t("work.card.part.pending")} data-pr-card-part={part} data-pr-card-part-state="pending" />;
}

/** A folded line's summary while its part is not read: `읽지 못함` once a read failed, else nothing. */
function partSummary(part: "body" | "files", card: PrCard | null, t: ReturnType<typeof useInterfaceTranslation>["t"]): ReactNode {
  return card?.failed.includes(part) ? <span className="text-warning" data-pr-card-part-summary={part}>{t("work.card.part.failed")}</span> : null;
}

/** 본문 (B36): the `##` headings at the right; opened, the preview and the whole body as Markdown. */
function BodyRow({ card, actions, projectId, number, open, onToggle }: { card: PrCard | null; actions: Actions; projectId: string; number: number; open: boolean; onToggle: () => void }) {
  const { t } = useInterfaceTranslation();
  const [whole, setWhole] = useState(false);
  const body = card?.body ?? null;
  const outline = useMemo(() => (body === null ? [] : bodyOutline(body)), [body]);
  const summary = body === null ? partSummary("body", card, t) : outline.length > 0 ? <span className="min-w-0 truncate" data-pr-card-outline="true">{outline.join(" · ")}</span> : null;
  return (
    <FoldRow name={t("work.card.body")} summary={summary} open={open} onToggle={onToggle} section="body">
      <div className="flex flex-col gap-xs" data-pr-card-body={whole ? "open" : "folded"}>
        {body === null ? <PartState part="body" card={card} actions={actions} projectId={projectId} number={number} /> : body.trim() === "" ? <p className="text-caption text-muted-foreground">{t("work.card.noBody")}</p> : (
          <>
            {whole ? <div className="flex flex-col gap-sm text-caption" data-pr-card-markdown="true">{renderMarkdown(body)}</div> : <p className="line-clamp-3 whitespace-pre-line break-words text-caption text-subtle-foreground" data-pr-card-preview="true">{bodyPreview(body)}</p>}
            <Button variant="ghost" size="sm" className="w-fit" aria-expanded={whole} onClick={() => setWhole(!whole)}>
              {whole ? <ChevronDownIcon aria-hidden="true" /> : <ChevronRightIcon aria-hidden="true" />}
              {whole ? t("work.card.bodyHide") : t("work.card.bodyShow")}
            </Button>
          </>
        )}
      </div>
    </FoldRow>
  );
}

/**
 * 바뀐 파일 N (B36, B49): +/- at the right; opened, the files with generated
 * ones folded into one line. A file opens as D-27 says: the diff in this
 * checkout's Files, where the lane turns while the 보기 slot stays on the
 * rail, or GitHub's files page.
 */
function FilesRow({ pr, project, card, actions, open, onToggle }: { pr: PullRequest; project: Workspace; card: PrCard | null; actions: Actions; open: boolean; onToggle: () => void }) {
  const { t } = useInterfaceTranslation();
  const [generatedOpen, setGeneratedOpen] = useState(false);
  const front = useShellStore((state) => frontCheckout(state.rest));
  const committed = useShellStore((state) => state.changes?.committed ?? null);
  const files = card?.files ?? null;
  const split = useMemo(() => splitFiles(files), [files]);
  const added = files?.reduce((sum, file) => sum + file.additions, 0) ?? 0;
  const removed = files?.reduce((sum, file) => sum + file.deletions, 0) ?? 0;
  const openFile = (file: string) => {
    const target = fileOpen({ pr, front: front ? { branch: front.branch ?? null, path: front.path } : null, committed, file });
    if (target.kind === "diff") actions.selectChange(target.path, true, false);
    else actions.openPullRequest(target.url, project.device_id, true);
  };
  const name = files === null ? t("work.card.changedFiles") : t("work.card.filesCount", { count: card?.files_total ?? files.length });
  const summary = files === null ? partSummary("files", card, t) : <><span className="font-mono text-pr-mergeable">+{added}</span><span className="font-mono text-pr-failed">−{removed}</span></>;
  return (
    <FoldRow name={name} summary={summary} open={open} onToggle={onToggle} section="files">
      {files === null ? <PartState part="files" card={card} actions={actions} projectId={project.id} number={pr.number} /> : (
        <ul className="flex flex-col" role="list" data-pr-card-files-section="true">
          {split.shown.map((file) => <FileRow key={file.path} file={file} onOpen={() => openFile(file.path)} />)}
          {split.generated > 0 ? (
            <li>
              <button type="button" aria-expanded={generatedOpen} onClick={() => setGeneratedOpen(!generatedOpen)} className="flex w-full items-center gap-xs rounded-xs py-xs text-left text-caption text-muted-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" data-pr-card-generated={split.generated}>
                {generatedOpen ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
                {t("work.card.generated", { count: split.generated })}
              </button>
            </li>
          ) : null}
          {generatedOpen ? files.filter((file) => file.generated).map((file) => <FileRow key={file.path} file={file} onOpen={() => openFile(file.path)} />) : null}
          {card && card.files_total > files.length ? <li className="py-xs text-micro text-muted-foreground">{t("work.card.filesMore", { shown: files.length, total: card.files_total })}</li> : null}
        </ul>
      )}
    </FoldRow>
  );
}

function FileRow({ file, onOpen }: { file: NonNullable<PrCard["files"]>[number]; onOpen: () => void }) {
  return (
    <li>
      <button type="button" onClick={onOpen} className="flex w-full min-w-0 items-center gap-sm rounded-xs px-xs py-xs text-left text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" data-pr-card-file={file.path} title={file.path}>
        <span className="min-w-0 flex-1 truncate font-mono">{file.path}</span>
        <span className="shrink-0 font-mono text-micro text-pr-mergeable">+{file.additions}</span>
        <span className="shrink-0 font-mono text-micro text-pr-failed">−{file.deletions}</span>
      </button>
    </li>
  );
}

/**
 * 연결 (B36): `이슈 · 워크트리 · 세션` at the right; opened, the issue (or
 * `이슈 잇기`), the worktrees and the sessions with `대화 보기` and `이어서 하기`.
 */
function LinksRow({ project, pr, entry, panel, handlers, actions, number, open, onToggle }: { project: Workspace; pr: PullRequest | null; entry: PrEntry | null; panel: LinkPanel | null; handlers: LinkHandlers; actions: Actions; number: number; open: boolean; onToggle: () => void }) {
  const { t } = useInterfaceTranslation();
  const [linking, setLinking] = useState(false);
  const holder: LinkPanel = panel ?? { workspace_id: project.id, target: { kind: "pr", number }, loading: true, failure: null, pr: null, prs: [], sessions: [], total: 0 };
  const checkoutId = entry?.row.checkout_id ?? null;
  const checkout = checkoutId ? project.checkouts.find((row) => row.id === checkoutId && row.exists) ?? null : null;
  const local = project.tasks?.source?.kind === "local";
  const worktree = checkout?.is_worktree === true && checkout.is_primary !== true;
  const merged = pr ? pr.state === "merged" || pr.state === "closed" : true;
  const linkable = !merged && !entry?.row.issue && (!local || worktree);
  // The summary counts what the open line lists: the record's worktrees with the board's checkout, and its sessions.
  const issue = entry?.row.issue?.label ?? null;
  const trees = new Set([...(panel?.pr?.worktrees ?? []), ...(checkout ? [checkout.path] : [])]).size;
  const parts = [issue ? t("work.card.links.issue", { label: issue }) : t("work.card.links.noIssue"), ...(trees > 0 ? [t("work.card.links.worktrees", { n: trees })] : []), ...(panel && !panel.loading ? [t("work.card.links.sessions", { n: panel.total })] : [])];
  return (
    <FoldRow name={t("links.connections")} summary={<span className="min-w-0 truncate" data-pr-card-links-summary="true">{parts.join(" · ")}</span>} open={open} onToggle={onToggle} section="links">
      <div className="flex flex-col gap-md" data-pr-card-sessions="true">
        <PrLinks number={number} issue={entry?.row.issue ?? null} checkout={checkout} linkable={linkable} record={panel} project={project} handlers={handlers} linking={linking} onLinking={setLinking} />
        <LinkSessions panel={holder} project={project} branchOf={() => pr?.head_branch ?? null} onRetry={() => actions.openLinks(project.id, { kind: "pr", number })} empty={<p className="text-caption text-muted-foreground" data-link-none="true">{t("links.noSessions")}</p>} actions={actions} />
      </div>
    </FoldRow>
  );
}
