// Work that starts from a pull request on the PRs tab (PRD
// overview-lenses-prs): linking it to an issue, which for a GitHub issue
// writes `Closes #N` into its body, and handing it to an agent on its branch.
// Each sends the one core event that owns the change after the operator's one
// confirmation, and reads the core's own answer for it (`pr_work`,
// `task_operation`); a dialog never decides that its request succeeded.

import { useEffect, useMemo, useRef, useState } from "react";
import type { Actions } from "./actions";
import { rememberedSelection, sendableModel, type AgentSelection } from "./agentPicker";
import { AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./components/ui/alert-dialog";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Note, Status } from "./components/settings-rows";
import { AgentField, Field, submitOnCommandEnter, TextArea } from "./IssueDialogs";
import { delegatePrompt } from "./prDelegate";
import type { PrFeedback, PrLink, PullRequest, Task, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { taskFor } from "./workspaceManage";
import { useErrorSince } from "./WorkspaceDialogs";

/** A request this dialog sent, by the id the core answers under. */
type Sent = { id: string; at: number };

function requestId(kind: string, number: number): string {
  return `${kind}-${number}-${Date.now()}`;
}

/**
 * The core's answer to this dialog's own link request and a refusal since it
 * was sent; a link that reached `ready` closes the dialog.
 */
function useLinkAnswer(sent: Sent | null, onClose: () => void): { answer: PrLink | null; refused: string | null; working: boolean; failed: boolean } {
  const link = useShellStore((s) => s.rest?.pr_work?.link ?? null);
  const refused = useErrorSince(sent?.at ?? null, ["pr_link."]);
  const answer = sent && link?.request_id === sent.id ? link : null;
  useEffect(() => {
    if (answer?.phase === "ready") onClose();
  }, [answer?.phase, onClose]);
  return {
    answer,
    refused,
    working: sent !== null && refused === null && (answer === null || answer.phase === "working"),
    failed: refused !== null || answer?.phase === "failed",
  };
}

/** The core's read of a pull request's body, checks and reviews under `readId`, asked for once per id. */
function useFeedbackRead(actions: Actions, readId: string, workspaceId: string, number: number): PrFeedback | null {
  const read = useShellStore((s) => (s.rest?.pr_work?.feedback?.request_id === readId ? s.rest.pr_work.feedback : null));
  useEffect(() => {
    actions.readPullRequestFeedback(readId, workspaceId, number);
  }, [actions, readId, workspaceId, number]);
  return read;
}

/** The number `Closes #N` names for a GitHub issue of this repository (`github:owner/repo#12`). */
function issueNumber(task: Task): string | null {
  return task.key.match(/#(\d+)$/)?.[1] ?? null;
}

/** What failed, in the operator's words; why is the core's one line, the rest is in the log (design 13). */
function failureHeadline(answer: PrLink): string {
  if (answer.step === "body") return answer.created ? `이슈 ${answer.issue_id ?? ""}은 만들었고 PR 본문 쓰기는 실패했습니다` : "이슈는 이었고 PR 본문 쓰기는 실패했습니다";
  if (answer.step === "create") return "이슈를 만들지 못했습니다";
  return "이슈를 잇지 못했습니다";
}

/** The retry names what it does again: only the body once the issue is made and linked (B10, B13). */
function retryLabel(answer: PrLink | null, created: string): string {
  if (answer?.step === "body") return "본문 다시 쓰기";
  if (answer?.step === "create") return created;
  return "다시 잇기";
}

function LinkFailure({ answer, refused }: { answer: PrLink | null; refused: string | null }) {
  const headline = answer ? failureHeadline(answer) : "이슈를 잇지 못했습니다";
  const reason = answer?.message ?? refused;
  return (
    <Note tone="error" data-pr-link-failure={answer?.step ?? "refused"}>
      <span className="block">{headline}</span>
      {reason ? <span className="line-clamp-2 block break-words text-caption text-muted-foreground">{reason}</span> : null}
    </Note>
  );
}

/**
 * An issue of the project's source chosen for a pull request (B10, B11,
 * B14). A GitHub issue asks once, `PR #N 본문에 "Closes #M"을 씁니다`, with
 * 그만두기 the default; confirming leaves Hide's link and writes the body. A
 * Local issue is Hide's link alone and asks nothing, so the dialog appears only
 * if that link fails. A failed body write keeps the link and offers to write
 * the body again, which the core finds already written if it was.
 */
export function PrLinkDialog({ actions, workspace, pr, task, onClose }: { actions: Actions; workspace: Workspace; pr: PullRequest; task: Task; onClose: () => void }) {
  const local = task.source === "local";
  const [sent, setSent] = useState<Sent | null>(null);
  const { answer, refused, working, failed } = useLinkAnswer(sent, onClose);
  const send = (key: string) => {
    const id = requestId("pr-link", pr.number);
    setSent({ id, at: Date.now() });
    actions.linkPullRequestIssue(id, workspace.id, pr.number, { key });
  };
  // A Local issue asks nothing: the link goes out as the dialog opens.
  const autoSent = useRef(false);
  useEffect(() => {
    if (!local || autoSent.current) return;
    autoSent.current = true;
    send(task.key);
  });
  if (local && !failed) return null;
  const number = issueNumber(task);
  return (
    <AlertDialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <AlertDialogContent initialFocus="cancel" data-pr-link={pr.number} data-pr-link-issue={task.key}>
        <AlertDialogHeader>
          <AlertDialogTitle className="break-words">PR #{pr.number}을 {task.id ?? task.title}에 잇기</AlertDialogTitle>
          <AlertDialogDescription className="break-words" data-pr-link-confirm="true">
            {local ? `${task.id ?? task.title}을 이 PR의 이슈로 기록합니다.` : `PR #${pr.number} 본문에 "Closes #${number}"을 씁니다. 머지되면 GitHub가 이슈를 닫습니다.`}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {failed ? <LinkFailure answer={answer} refused={refused} /> : null}
        {working ? <Status tone="pending">PR 본문에 쓰는 중…</Status> : null}
        <AlertDialogFooter>
          <AlertDialogCancel data-pr-link-cancel="true">{failed ? "닫기" : working ? "숨기기" : "그만두기"}</AlertDialogCancel>
          {!working ? (
            <Button onClick={() => send(answer?.issue_key ?? task.key)} data-pr-link-write={failed ? "retry" : "write"}>
              {failed ? retryLabel(answer, "다시 쓰기") : "본문에 쓰기"}
            </Button>
          ) : null}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * `새 이슈 만들기` for a pull request (B12, B13): the title and body start as
 * the pull request's own, and one confirmation makes the issue, then writes
 * `Closes #(새 번호)` into the body (a Local source's issue is only linked).
 * If the body write fails the issue stays made and linked, and the retry
 * writes only the body.
 */
export function PrNewIssueDialog({ actions, workspace, pr, onClose }: { actions: Actions; workspace: Workspace; pr: PullRequest; onClose: () => void }) {
  const github = workspace.tasks?.source?.kind === "github";
  const [title, setTitle] = useState(pr.title);
  const [body, setBody] = useState("");
  const bodyEdited = useRef(false);
  const readId = useMemo(() => requestId("pr-body", pr.number), [pr.number]);
  const read = useFeedbackRead(actions, readId, workspace.id, pr.number);
  useEffect(() => {
    if (read?.phase === "ready" && !bodyEdited.current) setBody(read.body ?? "");
  }, [read?.phase, read?.body]);
  const [sent, setSent] = useState<Sent | null>(null);
  const { answer, refused, working, failed } = useLinkAnswer(sent, onClose);
  const submit = () => {
    if (working) return;
    const id = requestId("pr-link", pr.number);
    setSent({ id, at: Date.now() });
    // Once the issue exists, only its link and the body are asked for again.
    if (answer?.issue_key) actions.linkPullRequestIssue(id, workspace.id, pr.number, { key: answer.issue_key });
    else actions.linkPullRequestIssue(id, workspace.id, pr.number, { title: title.trim(), body });
  };
  const made = answer?.issue_key != null;
  const confirm = github ? `이슈를 만들고 PR #${pr.number} 본문에 "Closes #(새 번호)"를 씁니다` : "Local 이슈를 만들어 이 PR에 잇습니다. GitHub에는 쓰지 않습니다.";
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <DialogContent data-pr-new-issue={pr.number}>
        <form className="flex min-h-0 flex-1 flex-col" onKeyDown={submitOnCommandEnter} onSubmit={(event) => { event.preventDefault(); submit(); }}>
          <DialogHeader>
            <DialogTitle>PR #{pr.number}의 이슈 만들기</DialogTitle>
            <DialogDescription className="break-words" data-pr-new-issue-confirm="true">{confirm}</DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-sm">
            <Field label="제목">
              <Input value={title} autoFocus disabled={working || made} maxLength={256} onChange={(event) => setTitle(event.target.value.replace(/[\r\n]+/g, " "))} data-pr-new-issue-title="true" />
            </Field>
            <Field label="내용" aside={read?.phase === "reading" ? <span className="text-muted-foreground">PR 본문 읽는 중…</span> : null}>
              <TextArea
                value={body}
                rows={5}
                disabled={working || made}
                onChange={(event) => {
                  bodyEdited.current = true;
                  setBody(event.target.value);
                }}
                data-pr-new-issue-body="true"
              />
            </Field>
            {read?.phase === "failed" ? <Note tone="warn" data-pr-new-issue-body-failed="true">PR 본문을 읽지 못해 제목만 채웠습니다.</Note> : null}
            {failed ? <LinkFailure answer={answer} refused={refused} /> : null}
            {working ? <Status tone="pending">{answer?.step === "body" ? "PR 본문에 쓰는 중…" : "이슈를 만드는 중…"}</Status> : null}
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={onClose} data-pr-new-issue-cancel="true">{failed ? "닫기" : working ? "숨기기" : "그만두기"}</Button>
            {!working ? (
              <Button type="submit" disabled={!title.trim()} data-pr-new-issue-submit={failed ? "retry" : "create"}>
                {failed ? retryLabel(answer, github ? "다시 만들고 쓰기" : "다시 만들기") : github ? "만들고 쓰기" : "만들고 잇기"}
                <Kbd>⌘↵</Kbd>
              </Button>
            ) : null}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * `▷ 맡기기` (B15, B16, B18): the Start dialog on the pull request's branch.
 * The worktree is the branch, shown and fixed; with no checkout of it here
 * the start makes the worktree of that branch first and says so. The first
 * prompt is what the core read of the failed checks and change requests,
 * editable; a failed read leaves it empty with why and `다시 읽기`, and never
 * blocks the start. Started, the screen goes to the agent's pane.
 */
export function PrDelegateDialog({ actions, workspace, pr, onClose }: { actions: Actions; workspace: Workspace; pr: PullRequest; onClose: () => void }) {
  const branch = pr.head_branch ?? "";
  const checkout = workspace.checkouts.find((row) => row.exists && row.branch === branch) ?? null;
  // A pull request is handed to an agent, never a bare terminal: the remembered kind, else Claude.
  const [agent, setAgent] = useState<AgentSelection>(() => rememberedSelection(useShellStore.getState().rest?.ui_state?.agent_start));
  const [prompt, setPrompt] = useState("");
  const promptEdited = useRef(false);
  const [readId, setReadId] = useState(() => requestId("pr-feedback", pr.number));
  const read = useFeedbackRead(actions, readId, workspace.id, pr.number);
  useEffect(() => {
    if (read?.phase !== "ready" || promptEdited.current) return;
    setPrompt(delegatePrompt({ number: pr.number, title: pr.title, branch }, read));
  }, [read, pr.number, pr.title, branch]);
  // Whether the start makes the branch's worktree is fixed when it is asked
  // for: the worktree it makes joins the checkouts while its agent is still
  // starting, and reading `checkout` then would wait for the wrong task.
  const [request, setRequest] = useState<{ afterId: number; at: number; newWorktree: boolean } | null>(null);
  const newWorktree = request ? request.newWorktree : checkout === null;
  const operation = useShellStore((s) => s.rest?.task_operation);
  const started = taskFor(
    operation,
    request ? (request.newWorktree ? { kind: "worktree_create", afterId: request.afterId, repositoryRoot: workspace.path, branch } : { kind: "agent_start", afterId: request.afterId, repositoryRoot: workspace.path }) : null,
  );
  const refused = useErrorSince(request?.at ?? null, ["pr_delegate.", "worktree.create", "task_operation.", "agent_start.", "overview.unknown_checkout"]);
  const working = request !== null && refused === null && (started === null || started.phase === "working");
  const failure = started?.phase === "failed" ? (started.message ?? "에이전트를 시작하지 못했습니다.") : refused;
  useEffect(() => {
    if (started?.phase !== "ready") return;
    if (started.agent_phase) useUiStore.getState().setWatchedTask(started.id);
    if (started.pane_id) useUiStore.getState().setFocusWhenListed(started.pane_id);
    onClose();
  }, [started, onClose]);
  const submit = () => {
    if (working || !branch || agent.kind === "terminal") return;
    setRequest({ afterId: actions.taskIdNow(), at: Date.now(), newWorktree: checkout === null });
    actions.delegatePullRequest(workspace.id, pr.number, agent.kind, sendableModel(agent, useShellStore.getState().rest?.status?.background_ai), prompt);
  };
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <DialogContent data-pr-delegate={pr.number}>
        <form className="flex min-h-0 flex-1 flex-col" onKeyDown={submitOnCommandEnter} onSubmit={(event) => { event.preventDefault(); submit(); }}>
          <DialogHeader>
            <DialogTitle>PR #{pr.number} 맡기기</DialogTitle>
            <DialogDescription className="line-clamp-2 break-words">{pr.title}</DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-sm">
            <Field label="워크트리 · 브랜치">
              <Input value={branch} mono disabled readOnly data-pr-delegate-branch="true" />
            </Field>
            {newWorktree ? (
              <Note tone="muted" data-pr-delegate-new-worktree="true">
                이 브랜치의 워크트리를 만들고 시작합니다
              </Note>
            ) : null}
            <AgentField agent={agent} actions={actions} disabled={working} onChange={setAgent} />
            <Field label="첫 지시" aside={<span className="text-muted-foreground">{read?.phase === "reading" || !read ? "실패한 검사 · 리뷰 코멘트 읽는 중…" : "실패한 검사 · 리뷰 코멘트에서 채움 · 고칠 수 있음"}</span>}>
              <TextArea
                value={prompt}
                rows={7}
                disabled={working}
                onChange={(event) => {
                  promptEdited.current = true;
                  setPrompt(event.target.value);
                }}
                data-pr-delegate-prompt="true"
              />
            </Field>
            {read?.phase === "failed" ? (
              <Note tone="warn" data-pr-delegate-read-failed="true">
                <span className="flex items-center gap-sm">
                  <span className="min-w-0 flex-1 break-words">{`검사 · 리뷰 코멘트를 읽지 못했습니다: ${read.message ?? ""}`}</span>
                  <Button type="button" variant="secondary" size="sm" onClick={() => setReadId(requestId("pr-feedback", pr.number))} data-pr-delegate-reread="true">
                    다시 읽기
                  </Button>
                </span>
              </Note>
            ) : null}
            {failure ? <Note tone="error" data-pr-delegate-error="true">{failure}</Note> : null}
            {working ? <Status tone="pending">{newWorktree ? "워크트리를 만드는 중…" : "에이전트를 시작하는 중…"}</Status> : null}
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={onClose}>{working ? "숨기기" : "취소"}</Button>
            <Button type="submit" disabled={working || !branch} data-pr-delegate-submit="true">
              {working ? "시작하는 중…" : "시작"}
              <Kbd>⌘↵</Kbd>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
