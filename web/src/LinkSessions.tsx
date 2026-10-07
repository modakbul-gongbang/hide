import { ChevronDownIcon, ChevronRightIcon, CopyIcon, CornerLeftUpIcon, LoaderCircleIcon, MessageSquareIcon, PlayIcon, CircleHelpIcon, SquareTerminalIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { rowLine } from "./agentRow";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { cn } from "./lib/utils";
import { deviceLabel, failureKey, foldLines, resumeProvider, resumeBlock, resumeCheckout, sessionLines, spanText, viewBlock, type Blocked, type SessionLine } from "./linkPanel";
import { allAgents } from "./navigation";
import { catalogWorkspaces, localDeviceId, type LinkPanel, type Workspace } from "./snapshot";
import { startAnswer, startRequestId } from "./startAnswer";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// The sessions section of the PR and Issue panels (PRD link-graph B6-B25,
// B34): the record's session lines joined with what is live now, the live
// ones first, a fold past five, and the line's two ways back into the work,
// `View conversation` and `Resume`. The record's failure and its first read
// stand in the section's own place; the rest of the panel never moves.

/** How long a resume waits for the core's answer before it says so; a device start can take a Home sync and a tab. */
const RESUME_TIMEOUT_MS = 90_000;

export function LinkSessions({
  panel,
  project,
  branchOf,
  onRetry,
  onOpenPr,
  empty,
  actions,
}: {
  panel: LinkPanel;
  project: Workspace;
  /** The branch of a pull request the line belongs to, for where it resumes when its folder is gone. */
  branchOf: (pr: number) => string | null;
  onRetry: () => void;
  /** The Issue panel names each line's pull request; its chip opens that PR's panel. */
  onOpenPr?: (pr: number) => void;
  /** What stands under the heading when no session is recorded (B23). */
  empty: ReactNode;
  actions: Actions;
}) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((s) => s.rest);
  const filling = useShellStore((s) => s.linkSummaries?.filling === true);
  const [unfolded, setUnfolded] = useState(false);
  const agents = allAgents(rest?.status?.remote, rest?.navigator?.devices, rest?.navigator?.agents ?? []).map((entry) => entry.agent);
  const lines = sessionLines(panel.sessions, agents);
  const { shown, earlier } = foldLines(lines, unfolded);
  const finding = panel.loading || filling;
  const heading = panel.total > 0 ? t("links.sessionsCount", { count: panel.total }) : t("links.sessions");
  return (
    <section className="flex flex-col gap-sm" aria-label={heading} data-link-sessions={panel.total} data-link-loading={finding ? "true" : undefined}>
      <h3 className="flex items-center gap-xs text-caption text-muted-foreground">
        {heading}
        {finding ? (
          <span role="status" aria-label={t("links.finding")} data-link-finding="true">
            <LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />
          </span>
        ) : null}
      </h3>
      {panel.failure ? (
        <p role="alert" className="flex items-center gap-sm text-caption text-warning" data-link-failure={panel.failure}>
          {t(failureKey(panel.failure))}
          <Button variant="ghost" size="sm" onClick={onRetry} data-link-retry="true">
            {t("common.retry")}
          </Button>
        </p>
      ) : null}
      {lines.length === 0 && !panel.failure && !panel.loading ? empty : null}
      {shown.length > 0 ? (
        <ol className="flex flex-col" aria-label={heading}>
          {shown.map((entry, index) => (
            <SessionLineView
              key={entry.line.id}
              entry={entry}
              last={index === shown.length - 1}
              project={project}
              checkoutBranch={branchOf(entry.line.pr)}
              onOpenPr={onOpenPr}
              actions={actions}
            />
          ))}
        </ol>
      ) : null}
      {earlier > 0 ? (
        <button
          type="button"
          aria-expanded={unfolded}
          className="inline-flex items-center gap-xs self-start rounded-xs text-caption text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
          onClick={() => setUnfolded(!unfolded)}
          data-link-earlier={earlier}
        >
          {unfolded ? <ChevronDownIcon aria-hidden="true" className="size-(--size-icon-sm)" /> : <ChevronRightIcon aria-hidden="true" className="size-(--size-icon-sm)" />}
          {t("links.earlier", { count: earlier })}
        </button>
      ) : null}
    </section>
  );
}

/** A resume in flight: the request it sent, then the core's refusal if it came. */
type Resume = { request: string | null; failure: string | null };

/**
 * Follows one resume to its answer (B12, B14): a tab that opened moves the
 * screen to its pane, a refusal or a failure stays on the line with Retry.
 */
function useResume(): { resume: Resume; start: (send: (request: string) => void) => void } {
  const { t } = useInterfaceTranslation();
  const [resume, setResume] = useState<Resume>({ request: null, failure: null });
  const request = resume.request;
  const phase = useShellStore((s) => (request ? startAnswer(s.rest, request).phase : null));
  useEffect(() => {
    if (!request || !phase || phase === "pending") return;
    const answer = startAnswer(useShellStore.getState().rest, request);
    if (answer.phase === "ready") {
      const ui = useUiStore.getState();
      if (answer.agentPhase) ui.setWatchedTask(answer.taskId);
      if (answer.paneId) ui.setFocusWhenListed(answer.paneId);
      setResume({ request: null, failure: answer.agentPhase === "failed" ? (answer.agentMessage ?? "") : null });
    } else if (answer.phase === "refused" || answer.phase === "failed") {
      setResume({ request: null, failure: answer.message ?? "" });
    }
  }, [request, phase]);
  useEffect(() => {
    if (!request) return undefined;
    const timer = window.setTimeout(() => setResume({ request: null, failure: t("shell.startTimeout") }), RESUME_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [request, t]);
  return {
    resume,
    start: (send) => {
      const id = startRequestId();
      setResume({ request: id, failure: null });
      send(id);
    },
  };
}

function SessionLineView({ entry, last, project, checkoutBranch, onOpenPr, actions }: { entry: SessionLine; last: boolean; project: Workspace; checkoutBranch: string | null; onOpenPr?: (pr: number) => void; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((s) => s.rest);
  const devices = rest?.navigator?.devices;
  const node = localDeviceId(rest);
  const { line, live } = entry;
  const checkout = resumeCheckout(line, catalogWorkspaces(rest), checkoutBranch, node);
  const viewOff = viewBlock(line, devices, node);
  const resumeOff = resumeBlock(line, checkout, devices, node);
  const { resume, start } = useResume();
  const gone = line.file === "missing";
  const span = spanText(line.started_at_unix_ms, line.ended_at_unix_ms);
  const asked = live?.kind === "question" ? rowLine(live.agent)?.text : null;
  const view = () => actions.openOverview(project.device_id, project.id, { session: { id: line.id, request: line.request } });
  const resumeLine = () => {
    const provider = resumeProvider(line.agent);
    if (!checkout || !provider) return;
    start((requestId) => actions.startAgent({ target: { checkoutPath: checkout.path }, deviceId: line.device_id === node ? undefined : line.device_id, provider, resumeSessionId: line.id, requestId }));
  };
  const request = line.request ?? t("links.noRequest");
  return (
    <li
      tabIndex={0}
      aria-label={request}
      className="group/link-line relative flex gap-sm rounded-xs outline-none focus-visible:ring-1 focus-visible:ring-ring"
      data-link-session={line.id}
      data-link-live={live?.kind}
      data-link-role={line.role}
      data-link-file={line.file}
    >
      <span className="relative flex w-(--size-agent-badge-compact) shrink-0 flex-col items-center pt-xxs" aria-hidden="true">
        <span className={cn(gone && "opacity-(--opacity-secondary)")}>
          <AgentMark kind={line.agent} />
        </span>
        {last ? null : <span className="mt-xs w-(--size-hairline) flex-1 bg-border" />}
      </span>
      <div className={cn("flex min-w-0 flex-1 flex-col gap-xxs", last ? "pb-xxs" : "pb-md")}>
        <Hint label={request} reveals>
          <span className={cn("truncate text-body", line.request && !gone ? "text-foreground" : "text-muted-foreground")} data-link-request={line.id}>
            {request}
          </span>
        </Hint>
        {asked ? (
          <span className="truncate text-body text-warning" data-link-question={line.id}>
            {asked}
          </span>
        ) : null}
        <span className="flex min-w-0 flex-wrap items-center gap-x-xs gap-y-xxs text-caption text-muted-foreground">
          {live?.kind === "working" ? (
            <span className="inline-flex items-center gap-xs" data-link-state="working">
              <span aria-hidden="true" className="size-(--size-tab-status-dot) rounded-full bg-agent-working" />
              <span className="text-agent-working">{t("links.working")}</span>
              <span>{t("links.now")}</span>
            </span>
          ) : live?.kind === "question" ? (
            <span className="inline-flex items-center gap-xs" data-link-state="question">
              <span aria-hidden="true" className="size-(--size-tab-status-dot) rounded-full bg-warning" />
              <span className="text-warning">{t("links.waiting")}</span>
              <span>{t("links.now")}</span>
            </span>
          ) : (
            <>
              <RoleChip line={line} onOpenPr={onOpenPr} />
              {span ? <span className="font-mono">{span}</span> : null}
            </>
          )}
          {line.ids.length > 1 ? <span data-link-continued={line.ids.length}>· {t("links.continued", { count: line.ids.length })}</span> : null}
          {line.device_id !== node ? (
            <Badge variant="outline" className="font-normal text-muted-foreground" data-link-device={line.device_id}>
              {deviceLabel(devices, line.device_id)}
            </Badge>
          ) : null}
          {gone ? <MissingFile path={line.path} id={line.id} /> : null}
        </span>
        {line.parent ? <ParentLine parent={line.parent} project={project} actions={actions} /> : null}
        {live && live.kind !== "idle" ? (
          <span className="flex items-center gap-xs pt-xxs">
            {live.kind === "question" ? (
              <Button size="sm" onClick={() => actions.openAgent(live.agent.pane_id)} data-link-answer={line.id}>
                <CircleHelpIcon aria-hidden="true" />
                {t("links.answer")}
              </Button>
            ) : (
              <Button variant="secondary" size="sm" onClick={() => actions.openAgent(live.agent.pane_id)} data-link-pane={line.id}>
                <SquareTerminalIcon aria-hidden="true" />
                {t("links.goToPane")}
              </Button>
            )}
          </span>
        ) : (
          <span className={cn("items-center gap-xs pt-xxs", resume.request || resume.failure ? "flex" : "hidden group-focus-within/link-line:flex group-hover/link-line:flex")} data-link-actions={line.id}>
            <LineButton icon={<MessageSquareIcon aria-hidden="true" />} label={t("links.view")} blocked={viewOff} onClick={view} data="view" id={line.id} />
            {live?.kind === "idle" ? (
              <Button variant="secondary" size="sm" onClick={() => actions.openAgent(live.agent.pane_id)} data-link-pane={line.id}>
                <SquareTerminalIcon aria-hidden="true" />
                {t("links.goToPane")}
              </Button>
            ) : (
              <LineButton icon={resume.request ? <LoaderCircleIcon aria-hidden="true" className="animate-spin" /> : <PlayIcon aria-hidden="true" />} label={resume.request ? t("links.resuming") : t("links.resume")} blocked={resumeOff} busy={resume.request !== null} onClick={resumeLine} data="resume" id={line.id} />
            )}
          </span>
        )}
        {resume.failure !== null ? (
          <span role="alert" className="flex items-center gap-xs text-caption text-warning" data-link-resume-failed={line.id}>
            <span className="min-w-0 break-words">{resume.failure ? t("links.resumeFailed", { reason: resume.failure }) : t("links.resumeFailedPlain")}</span>
            <Button variant="ghost" size="sm" onClick={resumeLine} data-link-resume-retry={line.id}>
              {t("common.retry")}
            </Button>
          </span>
        ) : null}
      </div>
    </li>
  );
}

/** `Created PR`, or on the Issue panel `#N created` with its number, a chip that opens that PR; else the muted `Worked` (B6, B34). */
function RoleChip({ line, onOpenPr }: { line: SessionLine["line"]; onOpenPr?: (pr: number) => void }) {
  const { t } = useInterfaceTranslation();
  if (line.role !== "created") return <span data-link-chip="worked">{t("links.worked")}</span>;
  const label = onOpenPr ? t("links.createdNumber", { number: line.pr }) : t("links.created");
  const shape = "inline-flex h-(--size-control-sm) items-center rounded-xs border border-border px-xs text-caption text-pr-merged";
  if (!onOpenPr) return <span className={shape} data-link-chip="created">{label}</span>;
  return (
    <Hint label={t("links.chip", { number: line.pr })} reveals>
      <button type="button" className={cn(shape, "outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring")} onClick={() => onOpenPr(line.pr)} data-link-chip="created" data-link-chip-pr={line.pr}>
        {label}
      </button>
    </Hint>
  );
}

/** A button under an ended line; off, it stays reachable so its tooltip says why (B10). */
function LineButton({ icon, label, blocked, busy = false, onClick, data, id }: { icon: ReactNode; label: string; blocked: Blocked; busy?: boolean; onClick: () => void; data: string; id: string }) {
  const { t } = useInterfaceTranslation();
  const off = blocked !== null || busy;
  const button = (
    <Button
      variant="secondary"
      size="sm"
      aria-disabled={off ? "true" : undefined}
      aria-busy={busy ? "true" : undefined}
      className={cn(off && "opacity-(--opacity-disabled) hover:bg-secondary")}
      onClick={() => {
        if (!off) onClick();
      }}
      data-link-button={data}
      data-link-line={id}
      data-link-blocked={blocked ? blocked.key : undefined}
    >
      {icon}
      {label}
    </Button>
  );
  if (!blocked) return button;
  return (
    <Hint label={"device" in blocked ? t(blocked.key, { device: blocked.device }) : t(blocked.key)} reveals>
      {button}
    </Hint>
  );
}

/** `Conversation file gone` and the copy of where it was (B15). */
function MissingFile({ path, id }: { path: string | null; id: string }) {
  const { t } = useInterfaceTranslation();
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(() => () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
  }, []);
  return (
    <span className="inline-flex items-center gap-xs" data-link-missing={id}>
      · {t("links.fileMissing")}
      {path ? (
        <Hint label={copied ? t("links.copied") : t("links.copyPath")}>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => {
              void navigator.clipboard?.writeText(path).then(() => {
                setCopied(true);
                timer.current = window.setTimeout(() => setCopied(false), 1500);
              });
            }}
            data-link-copy={id}
          >
            <CopyIcon aria-hidden="true" />
          </Button>
        </Hint>
      ) : null}
    </span>
  );
}

/** `↰ <parent> delegated` (B19): the parent's session on the Sessions tab, dim and inert once its record is gone. */
function ParentLine({ parent, project, actions }: { parent: NonNullable<SessionLine["line"]["parent"]>; project: Workspace; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const text = (
    <>
      <CornerLeftUpIcon aria-hidden="true" className="size-(--size-icon-sm)" />
      {t("links.delegatedBy", { name: parent.name })}
    </>
  );
  if (!parent.available) {
    return (
      <span className="inline-flex items-center gap-xs text-caption text-muted-foreground" data-link-parent={parent.session_id} data-link-parent-available="false">
        {text}
      </span>
    );
  }
  return (
    <button
      type="button"
      className="inline-flex items-center gap-xs self-start rounded-xs text-caption text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
      onClick={() => actions.openOverview(project.device_id, project.id, { session: { id: parent.session_id, request: null } })}
      data-link-parent={parent.session_id}
    >
      {text}
    </button>
  );
}

const NO_CHIPS: readonly { number: number; created: boolean }[] = [];

/**
 * The pull requests a session made (`#N created`) or worked on (`#N`), on
 * its Sessions row and its open head (PRD link-graph B36); each opens that
 * PR's panel. A session the record has not linked has none, and a chip that
 * arrives while the record fills just appears.
 */
export function SessionPrChips({ workspaceId, sessionId, onPr, className }: { workspaceId: string; sessionId: string; onPr: (number: number) => void; className?: string }) {
  const { t } = useInterfaceTranslation();
  const chips = useShellStore((s) => s.linkSummaries?.projects[workspaceId]?.sessions?.[sessionId] ?? NO_CHIPS);
  if (chips.length === 0) return null;
  return (
    <span className={cn("flex flex-wrap items-center gap-xxs", className)} data-session-prs={sessionId}>
      {chips.map((chip) => (
        <Hint key={chip.number} label={t("links.chip", { number: chip.number })} reveals>
          <button
            type="button"
            className={cn("inline-flex h-(--size-control-sm) items-center rounded-xs border border-border px-xs text-caption outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring", chip.created ? "text-pr-merged" : "text-subtle-foreground")}
            onClick={() => onPr(chip.number)}
            data-session-pr={chip.number}
            data-session-pr-created={chip.created ? "true" : undefined}
          >
            {chip.created ? t("links.createdNumber", { number: chip.number }) : t("links.workedNumber", { number: chip.number })}
          </button>
        </Hint>
      ))}
    </span>
  );
}
