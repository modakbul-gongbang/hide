import { EllipsisIcon, ExternalLinkIcon, GitBranchIcon, HouseIcon, PencilIcon, PlayIcon, SquareTerminalIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Hint } from "./components/ui/tooltip";
import { useCachedDetail, useDetailSlot } from "./issueDetails";
import { cn } from "./lib/utils";
import { MarkdownText } from "./MarkdownText";
import { PullRequestChip } from "./OverviewLenses";
import { STAGES, issueDate, type TaskCard } from "./projectBoard";
import type { IssueDetail } from "./snapshot";
import { useShellStore } from "./store";
import { CardAgentRow, EDIT_HINT, IssueLabelView, IssueMenu, ReviewMarks, START_HINT, TaskGlyph, neighbourCard, type BoardHandlers } from "./TaskBoards";
import { useEscapeLayer } from "./components/ui/layer";

// The issue panel beside the Issues board (PRD overview-lenses-issues D-08,
// B10-B19): one skeleton whatever the source, the head, the properties, what
// was done for the issue, then the body and comments; a source that has no
// value for a row has no row. The head and the work stand on the snapshot at
// once; the body, labels, author, assignees and comments are read once when
// the panel opens, off the core's lock, and the last answer shows while it
// reads again. A Local issue's title and body are edited in place; a GitHub
// issue is edited on GitHub.

/** 편집 on a card or a menu: the issue whose panel opens with its editor, and when it was asked, so a second ask is a new one. */
export type EditRequest = { key: string; at: number };

/** How the panel reads the issue now: its answer, the last one while it reads again, or why it could not. */
type Read = { detail: IssueDetail | null; reading: boolean; failure: string | null };

function useRead(taskKey: string): Read {
  const slot = useDetailSlot(taskKey);
  const cached = useCachedDetail(taskKey);
  if (slot?.phase === "ready") return { detail: slot, reading: false, failure: null };
  if (slot?.phase === "failed") return { detail: cached, reading: false, failure: slot.message ?? "읽지 못함" };
  return { detail: cached, reading: slot?.phase === "reading", failure: null };
}

export function IssuePanel({
  card,
  actions,
  handlers,
  focusedPaneId,
  editRequest,
  onClose,
}: {
  card: TaskCard;
  actions: Actions;
  handlers: BoardHandlers;
  focusedPaneId: string | null;
  /** 편집 asked for an issue's editor; the panel opens it when that issue is this one. */
  editRequest: EditRequest | null;
  onClose: () => void;
}) {
  const { task, owner } = card;
  const read = useRead(task.key);
  const [editing, setEditing] = useState(false);
  // One read each time the panel opens on an issue (B15), and on 재시도 (B16).
  const readIssue = () => actions.requestIssueDetail(card.place.projectId, task.key);
  useEffect(() => {
    actions.requestIssueDetail(card.place.projectId, task.key);
    setEditing(false);
  }, [actions, card.place.projectId, task.key]);
  useEffect(() => {
    if (editRequest?.key === task.key && card.editable) setEditing(true);
  }, [editRequest, task.key, card.editable]);
  // Escape closes the panel first, then leaves the Overview (B10); an editor takes it before the panel.
  useEscapeLayer(true, onClose);
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const target = event.target as HTMLElement;
    if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target.closest("[data-markdown-text]")) return;
    if (event.key.startsWith("Arrow")) {
      // The arrows move the panel through the board as they move the card focus (B10).
      const from = document.querySelector<HTMLElement>(`[data-issue-card="${CSS.escape(task.key)}"]`);
      const next = from ? neighbourCard(from, event.key) : null;
      if (!from) return;
      event.preventDefault();
      next?.focus();
    } else if (event.key.toLowerCase() === "s" && card.canStart) {
      event.preventDefault();
      handlers.startIssue(card);
    } else if (event.key.toLowerCase() === "o" && card.checkout) {
      event.preventDefault();
      handlers.openCheckout(card);
    }
  };
  const stage = STAGES.find((entry) => entry.stage === card.stage)?.label ?? card.stage;
  const github = task.source === "github";
  return (
    <aside
      aria-label={`${task.id ?? ""} ${card.title}`}
      className="flex min-h-0 w-2/5 min-w-(--issue-panel-min-width) shrink-0 flex-col gap-md overflow-y-auto rounded-md border border-border bg-card p-lg"
      data-issue-panel={task.key}
      data-issue-source={task.source}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <header className="flex min-w-0 items-center gap-xs text-caption text-muted-foreground">
        <TaskGlyph task={task} />
        <span className="font-mono">{task.id}</span>
        <span>{owner.tasks?.source?.label ?? (github ? "GitHub" : "Local")}</span>
        {card.project ? <span className="truncate">{card.project}</span> : null}
        <span className="flex-1" />
        <span className={cn("font-medium", task.open ? "text-success" : "text-muted-foreground")} data-issue-state={task.open ? "open" : "closed"}>
          {task.open ? "Open" : "Closed"}
        </span>
        <Hint label="닫기" shortcut={<Kbd>Esc</Kbd>}>
          <Button variant="ghost" size="icon-sm" onClick={onClose} data-issue-panel-close="true">
            <XIcon aria-hidden="true" />
          </Button>
        </Hint>
      </header>
      {editing ? (
        <LocalEditor
          card={card}
          body={read.detail?.body ?? ""}
          actions={actions}
          onCancel={() => setEditing(false)}
          onSaved={() => {
            // The body shown is the read from before the edit; the saved one is read again.
            setEditing(false);
            readIssue();
          }}
        />
      ) : (
        <>
          <h2
            className={cn("break-words text-headline font-semibold text-foreground", card.editable && "cursor-text rounded-xs hover:bg-accent")}
            onClick={card.editable ? () => setEditing(true) : undefined}
            data-issue-panel-title="true"
          >
            {card.title}
          </h2>
          <PanelActions card={card} actions={actions} handlers={handlers} onEdit={() => setEditing(true)} />
        </>
      )}
      <Properties card={card} read={read} stage={stage} />
      <WorkDone card={card} handlers={handlers} focusedPaneId={focusedPaneId} actions={actions} />
      {editing ? null : <Body card={card} read={read} onEdit={() => setEditing(true)} onRetry={readIssue} />}
      {github && !editing && read.detail ? <Comments detail={read.detail} /> : null}
    </aside>
  );
}

/** The action line (B11): the stage's first action and its key, GitHub or 편집 beside it, and `⋯` at the end. */
function PanelActions({ card, actions, handlers, onEdit }: { card: TaskCard; actions: Actions; handlers: BoardHandlers; onEdit: () => void }) {
  const { task, owner } = card;
  return (
    <div className="flex items-center gap-xs" data-issue-panel-actions="true">
      {card.canStart ? (
        <Hint label={START_HINT} shortcut={<Kbd>S</Kbd>}>
          <Button size="sm" onClick={() => handlers.startIssue(card)} data-issue-panel-start="true">
            <PlayIcon aria-hidden="true" />
            시작
          </Button>
        </Hint>
      ) : card.checkout ? (
        <Hint label="Workspace 열기" shortcut={<Kbd>O</Kbd>}>
          <Button size="sm" onClick={() => handlers.openCheckout(card)} data-issue-panel-workspace="true">
            <SquareTerminalIcon aria-hidden="true" />
            Workspace
          </Button>
        </Hint>
      ) : null}
      {card.canStart ? <Kbd>S</Kbd> : card.checkout ? <Kbd>O</Kbd> : null}
      {task.url ? (
        <Hint label="GitHub에서 열기">
          <Button variant="ghost" size="icon-sm" onClick={() => handlers.openGitHub(task.url as string, owner.device_id)} data-issue-panel-github="true">
            <ExternalLinkIcon aria-hidden="true" />
          </Button>
        </Hint>
      ) : null}
      {card.editable ? (
        <Hint label={EDIT_HINT}>
          <Button variant="ghost" size="icon-sm" onClick={onEdit} data-issue-panel-edit="true">
            <PencilIcon aria-hidden="true" />
          </Button>
        </Hint>
      ) : null}
      <span className="flex-1" />
      <IssueMenu card={card} actions={actions} handlers={{ ...handlers, editIssue: onEdit }} trigger={<EllipsisIcon aria-hidden="true" />} />
    </div>
  );
}

function Row({ label, children, data }: { label: string; children: ReactNode; data: string }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 text-foreground" data-issue-property={data}>
        {children}
      </dd>
    </>
  );
}

/** A value still being read: a quiet bar in its place (design 9). */
function Skeleton({ data }: { data: string }) {
  return <span className="block h-(--size-icon) w-2/5 rounded-xs bg-muted" data-issue-skeleton={data} />;
}

/**
 * The properties (B12): the stage; for a GitHub issue its labels, author and
 * date, and assignees when it has any; for a Local issue when it was made;
 * when it last changed; and what it waits on. A source without a value has no
 * row, and a value still being read is a skeleton.
 */
function Properties({ card, read, stage }: { card: TaskCard; read: Read; stage: string }) {
  const { task } = card;
  const detail = read.detail;
  const github = task.source === "github";
  const pending = detail === null && read.reading;
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-lg gap-y-xs text-body" data-issue-properties="true">
      <Row label="단계" data="stage">
        {stage}
      </Row>
      {github && pending ? (
        <>
          <Row label="라벨" data="labels">
            <Skeleton data="labels" />
          </Row>
          <Row label="작성" data="author">
            <Skeleton data="author" />
          </Row>
        </>
      ) : null}
      {github && detail && detail.labels.length > 0 ? (
        <Row label="라벨" data="labels">
          <span className="flex flex-wrap items-center gap-sm">
            {detail.labels.map((label) => (
              <IssueLabelView key={label.name} label={label} />
            ))}
          </span>
        </Row>
      ) : null}
      {github && detail?.author ? (
        <Row label="작성" data="author">
          {[detail.author, detail.created_at_unix_ms != null ? issueDate(detail.created_at_unix_ms) : null].filter(Boolean).join(" · ")}
        </Row>
      ) : null}
      {github && detail && detail.assignees.length > 0 ? (
        <Row label="담당" data="assignees">
          {detail.assignees.join(", ")}
        </Row>
      ) : null}
      {!github && detail?.created_at_unix_ms != null ? (
        <Row label="만듦" data="created">
          {issueDate(detail.created_at_unix_ms)}
        </Row>
      ) : null}
      {card.updatedAt !== null ? (
        <Row label="갱신" data="updated">
          {issueDate(card.updatedAt)}
        </Row>
      ) : null}
      {card.blockedBy.length > 0 ? (
        <Row label="막힘" data="blocked">
          <span className="text-warning">{card.blockedBy.map((blocker) => blocker.label).join(", ")}</span>
        </Row>
      ) : null}
    </dl>
  );
}

/**
 * `이 이슈로 한 일` (B13): the checkout line with its Workspace button, every
 * agent working there with a delegated one indented, and the pull request with
 * its title and the review GitHub asks for or its CI. With none of them the
 * section is not drawn.
 */
function WorkDone({ card, handlers, focusedPaneId, actions }: { card: TaskCard; handlers: BoardHandlers; focusedPaneId: string | null; actions: Actions }) {
  const { checkout, owner } = card;
  const pr = checkout?.pull_request ?? null;
  if (!checkout && card.rows.length === 0) return null;
  const branch = checkout?.branch ?? checkout?.label ?? "";
  const Glyph = checkout && (checkout.is_primary === true || !checkout.is_worktree) ? HouseIcon : GitBranchIcon;
  return (
    <section className="flex flex-col gap-xs" aria-label="이 이슈로 한 일" data-issue-work="true">
      <h3 className="text-caption font-medium text-muted-foreground">이 이슈로 한 일</h3>
      {checkout ? (
        <div className="flex min-w-0 items-center gap-xs font-mono text-caption text-muted-foreground" data-issue-work-checkout={checkout.id}>
          <Glyph aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />
          <span className="min-w-0 truncate">{branch}</span>
          {card.chip?.ahead != null ? <span>↑{card.chip.ahead}</span> : null}
          {card.chip?.files != null ? <span className="text-warning">{card.chip.files} files</span> : null}
          <span className="flex-1" />
          <Hint label="Workspace 열기" shortcut={<Kbd>O</Kbd>}>
            <Button variant="ghost" size="icon-sm" onClick={() => handlers.openCheckout(card)} data-issue-work-workspace="true">
              <SquareTerminalIcon aria-hidden="true" />
            </Button>
          </Hint>
        </div>
      ) : null}
      {card.rows.length > 0 ? (
        <ul className="-mx-xs flex flex-col" role="list">
          {card.rows.map((row) => (
            <CardAgentRow key={row.agent.pane_id} agent={row.agent} depth={row.depth} place={branch} selected={row.agent.pane_id === focusedPaneId} onOpen={actions.openAgent} />
          ))}
        </ul>
      ) : null}
      {checkout && pr && card.pr ? (
        <div className="flex min-w-0 items-center gap-xs text-caption text-muted-foreground" data-issue-work-pr={pr.number}>
          <PullRequestChip project={owner} checkout={checkout} onOpen={(url) => handlers.openGitHub(url, owner.device_id)} now={Date.now()} />
          <span className="min-w-0 flex-1 truncate text-foreground">{pr.title}</span>
          <ReviewMarks pr={card.pr} />
        </div>
      ) : null}
    </section>
  );
}

/**
 * The body in Markdown (B14), a skeleton while it is first read, and when the
 * read fails one line of why with `재시도`, which reads this issue again
 * (B16). A Local issue's body is edited by a click.
 */
function Body({ card, read, onEdit, onRetry }: { card: TaskCard; read: Read; onEdit: () => void; onRetry: () => void }) {
  const body = read.detail?.body ?? null;
  return (
    <section className="flex flex-col gap-xs" aria-label="본문" data-issue-body={read.failure ? "failed" : body === null ? "reading" : "ready"}>
      <h3 className="text-caption font-medium text-muted-foreground">{card.editable ? "본문 · 누르면 고침" : "본문"}</h3>
      {read.failure ? (
        <p className="flex items-center gap-sm text-caption text-muted-foreground" data-issue-body-failure="true">
          <span className="min-w-0 truncate">{read.failure}</span>
          <Button variant="secondary" size="sm" onClick={onRetry} data-issue-body-retry="true">
            재시도
          </Button>
        </p>
      ) : body === null ? (
        <span className="flex flex-col gap-xs" data-issue-skeleton="body">
          <span className="h-(--size-icon) w-full rounded-xs bg-muted" />
          <span className="h-(--size-icon) w-4/5 rounded-xs bg-muted" />
          <span className="h-(--size-icon) w-3/5 rounded-xs bg-muted" />
        </span>
      ) : body.trim() === "" ? (
        <p className="text-caption text-muted-foreground" onClick={card.editable ? onEdit : undefined} data-issue-body-empty="true">
          본문 없음
        </p>
      ) : (
        <div className={cn(card.editable && "cursor-text rounded-xs hover:bg-accent")} onClick={card.editable ? onEdit : undefined}>
          <MarkdownText text={body} />
        </div>
      )}
    </section>
  );
}

/** A GitHub issue's comments (B14): how many, the latest three, and that writing is on GitHub. */
function Comments({ detail }: { detail: IssueDetail }) {
  const count = detail.comment_count ?? 0;
  return (
    <section className="flex flex-col gap-sm" aria-label="댓글" data-issue-comments={count}>
      <h3 className="text-caption font-medium text-muted-foreground">댓글 {count}</h3>
      {detail.comments.map((comment, index) => (
        <article key={index} className="flex flex-col gap-xxs" data-issue-comment={index}>
          <span className="text-caption text-muted-foreground">{[comment.author, comment.created_at_unix_ms != null ? issueDate(comment.created_at_unix_ms) : null].filter(Boolean).join(" · ")}</span>
          <p className="whitespace-pre-line break-words text-body text-foreground">{comment.body}</p>
        </article>
      ))}
      <p className="text-caption text-muted-foreground">{count === 0 ? "댓글 없음 · 쓰기는 GitHub에서" : "쓰기는 GitHub에서"}</p>
    </section>
  );
}

/**
 * A Local issue's title and body edited in place (B18, D-41): ⌘↵ saves, Esc
 * cancels, and nothing on the board changes before the save. A refused save
 * keeps the text and says why in place.
 */
function LocalEditor({ card, body, actions, onCancel, onSaved }: { card: TaskCard; body: string; actions: Actions; onCancel: () => void; onSaved: () => void }) {
  const [title, setTitle] = useState(card.title);
  const [text, setText] = useState(body);
  const [request, setRequest] = useState<string | null>(null);
  const answer = useShellStore((s) => {
    const update = s.rest?.issue_work?.update ?? null;
    return request !== null && update?.request_id === request ? update : null;
  });
  useEscapeLayer(true, onCancel);
  const saved = useRef(onSaved);
  saved.current = onSaved;
  const ready = answer?.phase === "ready";
  useEffect(() => {
    if (ready) saved.current();
  }, [ready]);
  const save = () => {
    const id = `edit-${card.task.key}-${Date.now()}`;
    setRequest(id);
    actions.updateLocalIssue(id, card.task.key, title, text);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Enter" && event.metaKey) {
      event.preventDefault();
      save();
    }
  };
  const saving = request !== null && answer === null;
  const failure = answer?.phase === "failed" ? (answer.message ?? "저장하지 못함") : null;
  return (
    <div className="flex flex-col gap-sm" onKeyDown={onKeyDown} data-issue-editor="true">
      <Input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} aria-label="제목" className="text-title font-semibold" data-issue-editor-title="true" />
      <textarea
        value={text}
        onChange={(event) => setText(event.target.value)}
        aria-label="본문"
        rows={10}
        className="w-full min-w-0 resize-y rounded-sm border border-input bg-background p-sm text-body text-foreground outline-none focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring"
        data-issue-editor-body="true"
      />
      {failure ? (
        <p className="text-caption text-destructive" role="alert" data-issue-editor-failure="true">
          {failure}
        </p>
      ) : null}
      <div className="flex items-center gap-xs">
        <Button size="sm" onClick={save} disabled={saving} data-issue-editor-save="true">
          저장
          <Kbd>⌘↵</Kbd>
        </Button>
        <Button variant="ghost" size="sm" onClick={onCancel} data-issue-editor-cancel="true">
          취소
          <Kbd>Esc</Kbd>
        </Button>
      </div>
    </div>
  );
}
