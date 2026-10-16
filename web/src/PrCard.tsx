import { ChevronDownIcon, ChevronRightIcon, EllipsisIcon, ExternalLinkIcon, GitBranchIcon, SquareTerminalIcon, TriangleAlertIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { lineTone, rowLine } from "./agentRow";
import { agentProductName } from "./agentAdapters";
import { Elapsed } from "./components/elapsed";
import { PrMark } from "./components/pr-mark";
import { StatusMark } from "./components/status-mark";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { markTone } from "./agentRow";
import { cn } from "./lib/utils";
import { LinkSessions } from "./LinkSessions";
import { renderMarkdown } from "./mobile/markdown";
import { boardProjects } from "./navigation";
import { bodyOutline, bodyPreview, confirmHolds, defaultMethod, fileOpen, mergeWhy, primaryAction, splitFiles, type Confirmed, type PrimaryAction } from "./prCardModel";
import { linkHandlers, PrLinks, type LinkHandlers } from "./PrLinks";
import { GROUP_TONE, ReasonLine } from "./prReason";
import { checksWord, reviewWord } from "./prMark";
import { findPrEntry, opensCard, type PrEntry } from "./prWork";
import { catalogWorkspaces, frontCheckout, type LinkPanel, type PrCard, type PrMergeMethod, type PullRequest, type Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { staleness } from "./prMark";
import { useUiStore } from "./ui";
import { useRequestViewDemand } from "./requestViewDemand";

// A pull request's decision card (PRD work-tab-pr-lens D-02, D-06, D-21, D-22,
// B20-B34, B37, B52): the body of a `PR #N` tab in File Views. The head, the
// reason and the buttons are drawn from the list's own values, so they stand
// while the card reads its body, files and review threads; each of those is a
// part with its own placeholder, failure and retry.

/** The project a View's Workspace belongs to, and its pull request `number`. */
function useCardTarget(workspace: { device_id: string; path: string }, number: number) {
  const rest = useShellStore((state) => state.rest);
  const agents = useShellStore((state) => state.agents);
  return useMemo(() => {
    const project = catalogWorkspaces(rest).find((row) => row.device_id === workspace.device_id && row.checkouts.some((checkout) => checkout.path === workspace.path)) ?? null;
    if (!project) return { project: null, entry: null, pr: null, agents: [] as typeof agents };
    const scope = boardProjects(rest, agents, project.device_id).find((item) => item.workspace.id === project.id);
    const pr = project.pull_requests?.find((row) => row.number === number) ?? null;
    return { project, entry: scope ? findPrEntry(scope, number) : null, pr, agents };
  }, [rest, agents, workspace.device_id, workspace.path, number]);
}

export function PrCardView({ workspace, number, actions }: { workspace: { device_id: string; path: string }; number: number; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const { project, entry, pr } = useCardTarget(workspace, number);
  const card = useShellStore((state) => (project ? state.rest?.pr_work?.cards?.find((row) => row.workspace_id === project.id && row.number === number) ?? null : null));
  const record = useShellStore((state) => state.linkPanel);
  const devices = useShellStore((state) => state.rest?.navigator?.devices);
  const live = useShellStore((state) => state.connection === "live");
  const projectId = project?.id ?? null;
  const updatedAt = pr?.updated_at_unix_ms ?? null;
  const checks = pr?.checks ?? null;
  // The card's own parts are read when it opens and when its pull request moved (B44);
  // a restored layout opens the tab with no `pr_open`, so the page asks here too.
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
    <article aria-label={t("work.card.aria", { number })} className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto bg-background text-foreground" data-pr-card={number} data-pr-card-project={project.id} data-pr-card-head={card?.head ?? undefined}>
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-lg p-lg">
        {pr ? <Head project={project} pr={pr} entry={entry} card={card} actions={actions} /> : <p className="text-caption text-muted-foreground" data-pr-card-unlisted={number}>{t("work.card.unlisted", { number })}</p>}
        {pr && entry ? <ReasonBox entry={entry} card={card} /> : null}
        {pr ? <Blocked pr={pr} entry={entry} card={card} actions={actions} projectId={project.id} /> : null}
        {pr && entry ? <AgentSaid entry={entry} /> : null}
        {pr ? <Body card={card} actions={actions} projectId={project.id} number={number} /> : null}
        {pr ? <Files pr={pr} project={project} card={card} actions={actions} /> : null}
        <Sessions project={project} pr={pr} entry={entry} panel={panel} handlers={handlers} actions={actions} number={number} />
        {pr ? <Buttons project={project} pr={pr} entry={entry} card={card} actions={actions} /> : null}
      </div>
    </article>
  );
}

// --- head ---------------------------------------------------------------------

function Head({ project, pr, entry, card, actions }: { project: Workspace; pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const checkoutId = entry?.row.checkout_ids[0] ?? entry?.row.checkout_id ?? null;
  const checkout = checkoutId ? project.checkouts.find((row) => row.id === checkoutId && row.exists) ?? null : null;
  const checks = checksWord(pr.checks);
  const review = reviewWord(pr.review);
  const author = pr.work && pr.work.mine !== true ? pr.work.author : null;
  const files = card?.files ?? null;
  const added = files?.reduce((sum, file) => sum + file.additions, 0) ?? 0;
  const removed = files?.reduce((sum, file) => sum + file.deletions, 0) ?? 0;
  const github = () => actions.openPullRequest(pr.url, project.device_id, true);
  const stale = staleness(project.checkouts.find((row) => row.github)?.github);
  return (
    <header className="flex flex-col gap-xs" data-pr-card-head="true">
      <div className="flex min-w-0 items-center gap-xs text-caption">
        <PrMark state={pr.state} number={pr.number} />
        {author ? <span className="min-w-0 truncate text-muted-foreground" data-pr-card-author={author}>{t("work.row.author", { name: author })}</span> : null}
        <span className="flex-1" />
        {checkout ? (
          <Hint label={t("work.card.openWorkspace")}>
            <Button variant="ghost" size="icon-sm" aria-label={t("work.card.openWorkspace")} onClick={() => actions.openWorkspace(project.device_id, checkout.workspace_id, checkout.id)} data-pr-card-workspace={pr.number}>
              <SquareTerminalIcon aria-hidden="true" />
            </Button>
          </Hint>
        ) : null}
        <Hint label={t("work.card.github")}>
          <Button variant="ghost" size="icon-sm" aria-label={t("work.card.github")} onClick={github} data-pr-card-github={pr.number}>
            <ExternalLinkIcon aria-hidden="true" />
          </Button>
        </Hint>
        <CardMenu project={project} pr={pr} entry={entry} />
      </div>
      <h2 className="break-words break-keep text-headline font-semibold" data-pr-card-title="true">{pr.title}</h2>
      <p className="flex min-w-0 flex-wrap items-center gap-x-xs gap-y-xxs text-caption text-muted-foreground" data-pr-card-facts="true">
        <span className="inline-flex min-w-0 max-w-full items-center gap-xxs">
          <GitBranchIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
          <span className="min-w-0 truncate font-mono" data-pr-card-branch={pr.head_branch}>{pr.head_branch}</span>
        </span>
        {checks ? <><span aria-hidden="true">·</span><button type="button" className={cn("rounded-xs outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring", checks.tone)} onClick={() => actions.openPullRequest(`${pr.url}/checks`, project.device_id, true)} data-pr-card-checks={pr.checks}>{t(checks.key)}</button></> : null}
        {review ? <button type="button" className={cn("rounded-xs outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring", review.tone)} onClick={() => actions.openPullRequest(`${pr.url}/files`, project.device_id, true)} data-pr-card-review={pr.review}>{t(review.key)}</button> : null}
        {files ? <><span aria-hidden="true">·</span><span data-pr-card-files={card?.files_total ?? files.length}>{t("work.card.files", { count: card?.files_total ?? files.length })} <span className="text-pr-mergeable">+{added}</span> <span className="text-pr-failed">−{removed}</span></span></> : null}
        {stale?.stale ? <Hint label={t("agentSessions.pr.stale", { time: "-" })}><span role="img" className="inline-flex text-warning" data-pr-card-stale="true"><TriangleAlertIcon aria-hidden="true" className="size-(--size-icon-sm)" /></span></Hint> : null}
      </p>
    </header>
  );
}

/** `⋯` of the card (D-28): link an issue, make one, copy the branch name. */
function CardMenu({ project, pr, entry }: { project: Workspace; pr: PullRequest; entry: PrEntry | null }) {
  const { t } = useInterfaceTranslation();
  const merged = pr.state === "merged" || pr.state === "closed";
  const linkable = !merged && !entry?.row.issue;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label={t("work.card.menu")} data-pr-card-menu={pr.number}>
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

// --- reason, blocked, agent -----------------------------------------------------

/** The reason box under the head (B24) with hide-ai's summary under it (B37). */
function ReasonBox({ entry, card }: { entry: PrEntry; card: PrCard | null }) {
  const { t } = useInterfaceTranslation();
  const group = entry.row.group;
  return (
    <section className={cn("flex flex-col gap-xs rounded-md border border-border bg-card px-md py-sm", group === "needs_you" && "border-warning")} data-pr-card-reason={entry.reason.kind} data-pr-card-group={group}>
      <ReasonLine reason={entry.reason} group={group} noAgent={group === "blocked"} className={cn("whitespace-normal text-caption", GROUP_TONE[group])} />
      {card?.summary ? (
        <p className="flex flex-col gap-xxs text-caption text-foreground" data-pr-card-summary="true">
          <Badge variant="secondary" className="w-fit">{t("work.card.aiSummary")}</Badge>
          <span className="whitespace-pre-line break-words">{card.summary}</span>
        </p>
      ) : null}
    </section>
  );
}

/** What blocks it (B25): a conflict, each failed check with its log, each unresolved thread. Nothing blocked, no section. */
function Blocked({ pr, entry, card, actions, projectId }: { pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions; projectId: string }) {
  const { t } = useInterfaceTranslation();
  const open = pr.state !== "merged" && pr.state !== "closed";
  const conflict = open && pr.work?.merge_state === "dirty";
  const failed = open ? pr.work?.failed_checks ?? [] : [];
  const threads = card?.threads ?? [];
  const device = entry?.project.device_id ?? "";
  // A thread read that failed says so with a retry, never "nothing blocked" (B39, B52).
  const threadsFailed = card?.failed.includes("threads") === true;
  if (!conflict && failed.length === 0 && threads.length === 0 && !threadsFailed) return null;
  return (
    <section className="flex flex-col gap-xs" aria-label={t("work.card.blocked")} data-pr-card-blocked="true">
      <h3 className="text-caption text-muted-foreground">{t("work.card.blocked")}</h3>
      {conflict ? <p className="text-caption text-pr-failed" data-pr-card-conflict="true">{t("work.card.conflict", { base: pr.base_branch ?? "" })}</p> : null}
      {failed.map((check) => (
        <p key={check.name} className="flex min-w-0 items-center gap-xs text-caption" data-pr-card-failed={check.name}>
          <span className="min-w-0 truncate text-pr-failed">✗ {check.name}</span>
          {check.url ? <button type="button" className="shrink-0 rounded-xs text-muted-foreground outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" onClick={() => actions.openPullRequest(check.url!, device, true)}>{t("work.card.checkLog")}</button> : null}
        </p>
      ))}
      {threadsFailed ? <PartState part="threads" card={card} actions={actions} projectId={projectId} number={pr.number} /> : null}
      {threads.length > 0 ? <h4 className="text-micro text-muted-foreground">{t("work.card.threads")}</h4> : null}
      {threads.map((thread, index) => {
        const place = thread.line == null ? thread.path : `${thread.path}:${thread.line}`;
        return <p key={`${thread.path}:${thread.line}:${index}`} className="min-w-0 truncate text-caption text-foreground" title={thread.text} data-pr-card-thread={place}>{t("work.card.thread", { reviewer: thread.reviewer ?? "?", place, text: thread.text })}</p>;
      })}
    </section>
  );
}

/** The agent's last words and time; when it asks, the question in the warning colour (B26). */
function AgentSaid({ entry }: { entry: PrEntry }) {
  const { t } = useInterfaceTranslation();
  const agent = entry.agents[0];
  if (!agent) return null;
  const said = rowLine(agent);
  const asking = entry.reason.kind === "reply";
  const text = (asking ? entry.reason.text : null) ?? said?.text ?? null;
  return (
    <section className="flex flex-col gap-xxs" data-pr-card-agent={agent.pane_id}>
      <h3 className="flex min-w-0 items-center gap-xs text-caption text-muted-foreground">
        <StatusMark mark={agent.mark} className={markTone(agent)} />
        <span className="min-w-0 truncate">{asking ? t("work.card.agentAsks", { name: agent.identity_label }) : agent.identity_label}</span>
        <span className="shrink-0 font-normal">{agentProductName(agent.agent_kind)}</span>
        <Elapsed since={agent.state.request_since} className="shrink-0" />
      </h3>
      {text ? <p className={cn("whitespace-pre-line break-words text-caption", asking ? "text-warning" : said ? lineTone(said, agent) : "text-foreground")} data-pr-card-agent-text={asking ? "ask" : "said"}>{text}</p> : null}
    </section>
  );
}

// --- body and files ------------------------------------------------------------------

/** A card part that has not been read, or could not be (B39, B52). */
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

/** The body folded to a preview and the `##` headings as chips; opened, it is Markdown (B27). */
function Body({ card, actions, projectId, number }: { card: PrCard | null; actions: Actions; projectId: string; number: number }) {
  const { t } = useInterfaceTranslation();
  const [open, setOpen] = useState(false);
  const body = card?.body ?? null;
  const outline = useMemo(() => (body === null ? [] : bodyOutline(body)), [body]);
  return (
    <section className="flex flex-col gap-xs" aria-label={t("work.card.body")} data-pr-card-body={open ? "open" : "folded"}>
      <h3 className="text-caption text-muted-foreground">{t("work.card.body")}</h3>
      {body === null ? <PartState part="body" card={card} actions={actions} projectId={projectId} number={number} /> : body.trim() === "" ? <p className="text-caption text-muted-foreground">{t("work.card.noBody")}</p> : (
        <>
          {outline.length > 0 ? <ul className="flex flex-wrap gap-xs" data-pr-card-outline="true">{outline.map((heading, index) => <li key={`${heading}:${index}`}><button type="button" onClick={() => setOpen(true)} className="rounded-xs bg-secondary px-xs py-xxs text-micro outline-none focus-visible:ring-1 focus-visible:ring-ring">{heading}</button></li>)}</ul> : null}
          {open ? <div className="flex flex-col gap-sm text-caption" data-pr-card-markdown="true">{renderMarkdown(body)}</div> : <p className="line-clamp-3 whitespace-pre-line break-words text-caption text-subtle-foreground" data-pr-card-preview="true">{bodyPreview(body)}</p>}
          <Button variant="ghost" size="sm" className="w-fit" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? <ChevronDownIcon aria-hidden="true" /> : <ChevronRightIcon aria-hidden="true" />}
            {open ? t("work.card.bodyHide") : t("work.card.bodyShow")}
          </Button>
        </>
      )}
    </section>
  );
}

/** The changed files with +/-, generated ones folded into one line (B28); a file opens as D-27 says. */
function Files({ pr, project, card, actions }: { pr: PullRequest; project: Workspace; card: PrCard | null; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [generatedOpen, setGeneratedOpen] = useState(false);
  const front = useShellStore((state) => frontCheckout(state.rest));
  const committed = useShellStore((state) => state.changes?.committed ?? null);
  const files = card?.files ?? null;
  const split = useMemo(() => splitFiles(files), [files]);
  const open = (file: string) => {
    const target = fileOpen({ pr, front: front ? { branch: front.branch ?? null, path: front.path } : null, committed, file });
    if (target.kind === "diff") actions.selectChange(target.path, true, false);
    else actions.openPullRequest(target.url, project.device_id, true);
  };
  return (
    <section className="flex flex-col gap-xs" aria-label={t("work.card.changedFiles")} data-pr-card-files-section="true">
      <h3 className="text-caption text-muted-foreground">{t("work.card.changedFiles")}</h3>
      {files === null ? <PartState part="files" card={card} actions={actions} projectId={project.id} number={pr.number} /> : (
        <ul className="flex flex-col" role="list">
          {split.shown.map((file) => <FileRow key={file.path} file={file} onOpen={() => open(file.path)} />)}
          {split.generated > 0 ? (
            <li>
              <button type="button" aria-expanded={generatedOpen} onClick={() => setGeneratedOpen(!generatedOpen)} className="flex w-full items-center gap-xs rounded-xs py-xs text-left text-caption text-muted-foreground outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" data-pr-card-generated={split.generated}>
                {generatedOpen ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
                {t("work.card.generated", { count: split.generated })}
              </button>
            </li>
          ) : null}
          {generatedOpen ? files.filter((file) => file.generated).map((file) => <FileRow key={file.path} file={file} onOpen={() => open(file.path)} />) : null}
          {card && card.files_total > files.length ? <li className="py-xs text-micro text-muted-foreground">{t("work.card.filesMore", { shown: files.length, total: card.files_total })}</li> : null}
        </ul>
      )}
    </section>
  );
}

function FileRow({ file, onOpen }: { file: NonNullable<PrCard["files"]>[number]; onOpen: () => void }) {
  return (
    <li>
      <button type="button" onClick={onOpen} className="flex w-full min-w-0 items-center gap-sm rounded-xs py-xs text-left text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring" data-pr-card-file={file.path} title={file.path}>
        <span className="min-w-0 flex-1 truncate font-mono">{file.path}</span>
        <span className="shrink-0 font-mono text-pr-mergeable">+{file.additions}</span>
        <span className="shrink-0 font-mono text-pr-failed">−{file.deletions}</span>
      </button>
    </li>
  );
}

// --- sessions and links ----------------------------------------------------------------

function Sessions({ project, pr, entry, panel, handlers, actions, number }: { project: Workspace; pr: PullRequest | null; entry: PrEntry | null; panel: LinkPanel | null; handlers: LinkHandlers; actions: Actions; number: number }) {
  const { t } = useInterfaceTranslation();
  const [linking, setLinking] = useState(false);
  const holder: LinkPanel = panel ?? { workspace_id: project.id, target: { kind: "pr", number }, loading: true, failure: null, pr: null, prs: [], sessions: [], total: 0 };
  const checkoutId = entry?.row.checkout_id ?? null;
  const checkout = checkoutId ? project.checkouts.find((row) => row.id === checkoutId && row.exists) ?? null : null;
  const local = project.tasks?.source?.kind === "local";
  const worktree = checkout?.is_worktree === true && checkout.is_primary !== true;
  const merged = pr ? pr.state === "merged" || pr.state === "closed" : true;
  const linkable = !merged && !entry?.row.issue && (!local || worktree);
  return (
    <section className="flex flex-col gap-md" aria-label={t("work.card.sessions")} data-pr-card-sessions="true">
      <PrLinks number={number} issue={entry?.row.issue ?? null} checkout={checkout} linkable={linkable} record={panel} project={project} handlers={handlers} linking={linking} onLinking={setLinking} />
      <LinkSessions panel={holder} project={project} branchOf={() => pr?.head_branch ?? null} onRetry={() => actions.openLinks(project.id, { kind: "pr", number })} empty={<p className="text-caption text-muted-foreground" data-link-none="true">{t("links.noSessions")}</p>} actions={actions} />
    </section>
  );
}

// --- buttons ----------------------------------------------------------------------------

type Confirm = Confirmed | null;

function confirmText(confirm: Confirmed, queue: boolean, t: (key: string, values?: Record<string, unknown>) => string, values: { number: number; base: string; method: string; head: string }): string {
  return t(confirm.action === "merge" ? (queue ? "work.card.confirm.queue" : "work.card.confirm.merge") : "work.card.confirm.update", values);
}

/** The button row (D-21, B30-B36): the primary button of the case, then the rest. */
function Buttons({ project, pr, entry, card, actions }: { project: Workspace; pr: PullRequest; entry: PrEntry | null; card: PrCard | null; actions: Actions }) {
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
  if (!open) return null;
  const base = pr.base_branch ?? "";
  const methodLabel = method ? t(`work.card.method.${method}`) : "";
  // A write GitHub refused for the head, the merge state or the permission would only be refused again, so both buttons stay off until Look again has read the card; a transient failure (unreachable, failed) is retried with the same button (B34).
  const repeats = write?.phase === "failed" && write.reason !== "unreachable" && write.reason !== "failed";
  const canAsk = head !== null && !writing && !repeats;
  const MergeButton = (
    <span className="flex min-w-0 items-center gap-xs" data-pr-card-merge-slot="true">
      <Button variant={primary === "merge" ? "default" : "secondary"} size="sm" disabled={why !== null || queued || !canAsk} onClick={() => head !== null && setConfirm({ action: "merge", head, method, queue })} data-pr-card-merge={queue ? "queue" : "merge"}>
        {queue ? t("work.card.mergeQueue") : t("work.card.merge")}
      </Button>
      {!queue && (card?.merge?.methods.length ?? 0) > 1 ? (
        <select aria-label={t("work.card.merge")} className="rounded-xs border border-border bg-background px-xs py-xxs text-micro outline-none focus-visible:ring-1 focus-visible:ring-ring" value={method ?? ""} onChange={(event) => setPicked(event.target.value as PrMergeMethod)} data-pr-card-method="true">
          {card!.merge!.methods.map((each) => <option key={each} value={each}>{t(`work.card.method.${each}`)}</option>)}
        </select>
      ) : null}
      {why ? <span className="min-w-0 truncate text-micro text-muted-foreground" data-pr-card-merge-why={why.key}>{t(`work.card.why.${why.key}`, { count: why.count, base: why.base })}</span> : null}
    </span>
  );
  const UpdateButton = (
    <Button variant={primary === "update" ? "default" : "secondary"} size="sm" disabled={!canAsk || write?.phase === "working"} onClick={() => head !== null && setConfirm({ action: "update", head, method: null, queue })} data-pr-card-update="true">{t("work.card.updateBranch")}</Button>
  );
  const askPane = agent ? () => actions.openAgent(agent.pane_id) : null;
  return (
    <section className="flex flex-col gap-sm border-t border-border pt-md" data-pr-card-buttons={primary}>
      {confirming !== null ? (
        <div role="alertdialog" aria-label={confirmText(confirming, queue, t, { number: pr.number, base, method: methodLabel, head: confirming.head.slice(0, 7) })} className="flex flex-col gap-sm rounded-md border border-border bg-card p-md" data-pr-card-confirm={confirming.action}>
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
      <div className="flex min-w-0 flex-wrap items-center gap-sm">
        {primary === "update" || pr.work?.merge_state === "behind" ? UpdateButton : null}
        {primary === "review" ? <><Button size="sm" onClick={() => github("/files")} data-pr-card-review-github="true">{t("work.card.reviewGithub")}</Button><Button size="sm" variant="secondary" onClick={() => review(project, pr)} data-pr-card-review-agent="true">{t("work.card.reviewAgent")}</Button></> : null}
        {primary === "assign" ? <Button size="sm" onClick={() => useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: pr.number })} data-pr-card-assign="true">{t("work.card.assign")}</Button> : null}
        {primary === "answer" && askPane ? <Button size="sm" onClick={askPane} data-pr-card-answer="true">{t("work.card.answer")}</Button> : null}
        {primary === "view_pane" && askPane ? <Button size="sm" onClick={askPane} data-pr-card-view-pane="true">{t("work.card.viewPane")}</Button> : null}
        {primary === "assign_reviewer" ? <Button size="sm" onClick={() => github()} data-pr-card-assign-reviewer="true">{t("work.card.assignReviewer")}</Button> : null}
        {mine && primary !== "review" ? MergeButton : null}
        {primary === "github" || primary === "merge" || primary === "update" ? <Button size="sm" variant="ghost" onClick={() => github()} data-pr-card-github-button="true">{t("work.card.github")}</Button> : null}
      </div>
      <WriteLine write={write} base={base} card={card} actions={actions} projectId={project.id} number={pr.number} />
    </section>
  );
}

/** What the last Merge or branch update did, in one line next to the buttons (B31, B34). */
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

/** `Have an agent review` (B35): the Start dialog's agent opens on the branch with the review instruction. */
function review(project: Workspace, pr: PullRequest) {
  useUiStore.getState().setWorkspaceDialog({ kind: "pr_delegate", workspaceId: project.id, prNumber: pr.number, intent: "review" });
}
