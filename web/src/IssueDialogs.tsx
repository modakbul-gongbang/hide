// Work that starts from an issue (the issue-first Overview, 2026-09-28): a
// new issue in a project's source, the Start dialog that turns an issue into
// a worktree with an agent and its first prompt, and the popover that links
// a worktree with no issue to one. Each sends the one core event that owns
// the change and reads the core's own answer for it (`issue_work`,
// `task_operation`); a dialog never decides that its request succeeded.

import { FileTextIcon, CircleDotIcon, PlusIcon, RotateCcwIcon, SparklesIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ComponentProps, type ReactNode } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Checkbox } from "./components/ui/checkbox";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandShortcut } from "./components/ui/command";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { RadioGroup, RadioGroupItem } from "./components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./components/ui/select";
import { Hint } from "./components/ui/tooltip";
import { Note, Status } from "./components/settings-rows";
import { defaultWorktreeName, firstPrompt, namePrefix } from "./issueStart";
import { cn } from "./lib/utils";
import type { Checkout, IssueSettings, Task, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { branchProblem, taskFor } from "./workspaceManage";
import { useErrorSince } from "./WorkspaceDialogs";

const AGENTS = [
  { id: "terminal", label: "터미널만" },
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
] as const;
type AgentChoice = (typeof AGENTS)[number]["id"];

const DEFAULT_SETTINGS: IssueSettings = { ai_worktree_name: true, default_agent: "claude", closes_instruction: true };

/** A multi-line field in the text field's look; the issue body and the first prompt. */
function TextArea({ className, ...props }: ComponentProps<"textarea">) {
  return (
    <textarea
      className={cn(
        "min-h-(--size-overview-group-header) w-full min-w-0 resize-y rounded-sm border border-input bg-background px-sm py-xs text-body text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-(--opacity-disabled)",
        className,
      )}
      {...props}
    />
  );
}

function Field({ label, aside, children }: { label: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <label className="block text-body text-subtle-foreground">
      <span className="flex items-center gap-xs">
        <span>{label}</span>
        {aside ? <span className="ml-auto text-caption">{aside}</span> : null}
      </span>
      <span className="mt-xxs block">{children}</span>
    </label>
  );
}

/** ⌘↵ sends the form from any field, the way the footer's keycap says. */
function submitOnCommandEnter(event: React.KeyboardEvent<HTMLFormElement>) {
  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    event.currentTarget.requestSubmit();
  }
}

/** The projects on this Mac an issue can be made in: each with its source. */
function issueProjects(workspaces: readonly Workspace[] | undefined): Workspace[] {
  return (workspaces ?? []).filter((workspace) => !workspace.remote_target_id && workspace.tasks?.source);
}

/** `herdr-ide · GitHub owner/repo`, `notes · Local`: where a new issue lands. */
export function sourcePlace(workspace: Workspace): string {
  const source = workspace.tasks?.source;
  if (!source) return workspace.label;
  return [workspace.label, source.name ? `${source.label} ${source.name}` : source.label].join(" · ");
}

/**
 * New issue: in the named project's source, or another project picked from
 * the list; `만들고 바로 시작` goes on to the Start dialog with the new issue
 * once the source has it.
 */
export function NewIssueDialog({ actions, workspace, onClose }: { actions: Actions; workspace: Workspace; onClose: () => void }) {
  const workspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const projects = useMemo(() => issueProjects(workspaces), [workspaces]);
  const [projectId, setProjectId] = useState(workspace.id);
  const project = projects.find((row) => row.id === projectId) ?? null;
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [thenStart, setThenStart] = useState(false);
  const [request, setRequest] = useState<{ after: number; at: number; start: boolean; projectId: string } | null>(null);
  const create = useShellStore((s) => s.rest?.issue_work?.create ?? null);
  const answer = request && create && create.id > request.after && create.workspace_id === request.projectId ? create : null;
  const refused = useErrorSince(request?.at ?? null, ["issue_create."]);
  const working = request !== null && refused === null && (answer === null || answer.phase === "working");
  const failure = answer?.phase === "failed" ? (answer.message ?? "이슈를 만들지 못했습니다.") : refused;

  useEffect(() => {
    if (answer?.phase !== "ready" || !request) return;
    if (request.start && answer.task_key) {
      useUiStore.getState().setWorkspaceDialog({ kind: "start_issue", workspaceId: request.projectId, taskKey: answer.task_key });
    } else {
      onClose();
    }
  }, [answer, request, onClose]);

  const submit = () => {
    if (working || !project || !title.trim()) return;
    setRequest({ after: create?.id ?? 0, at: Date.now(), start: thenStart, projectId: project.id });
    actions.createIssue(project.id, title.trim(), body);
  };
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <DialogContent data-new-issue={projectId}>
        <form className="flex min-h-0 flex-1 flex-col" onKeyDown={submitOnCommandEnter} onSubmit={(event) => { event.preventDefault(); submit(); }}>
          <DialogHeader>
            <DialogTitle>새 이슈</DialogTitle>
            <DialogDescription className="sr-only">프로젝트의 이슈 출처에 새 이슈를 만듭니다.</DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-sm">
            <Field label="어디에">
              <Select value={projectId} disabled={working} onValueChange={setProjectId}>
                <SelectTrigger className="w-full" aria-label="어디에" data-new-issue-project="true">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {projects.map((row) => (
                    <SelectItem key={row.id} value={row.id}>
                      {sourcePlace(row)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field label="제목">
              <Input value={title} autoFocus disabled={working} maxLength={256} onChange={(event) => setTitle(event.target.value.replace(/[\r\n]+/g, " "))} data-new-issue-title="true" />
            </Field>
            <Field label="내용">
              <TextArea value={body} rows={4} disabled={working} onChange={(event) => setBody(event.target.value)} data-new-issue-body="true" />
            </Field>
            <label className="inline-flex items-center gap-xs text-body text-foreground">
              <Checkbox checked={thenStart} disabled={working} onCheckedChange={(checked) => setThenStart(checked === true)} data-new-issue-start="true" />
              만들고 바로 시작
            </label>
            {project?.tasks?.source?.failure ? <Note tone="warn">{`${project.tasks.source.label}: ${project.tasks.source.failure}`}</Note> : null}
            {failure ? <Note tone="error" data-new-issue-error="true">{failure}</Note> : null}
            {working ? <Status tone="pending">이슈를 만드는 중…</Status> : null}
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={onClose}>{working ? "숨기기" : "취소"}</Button>
            <Button type="submit" disabled={working || !project || !title.trim()} data-new-issue-create="true">
              {thenStart ? "만들고 시작…" : "이슈 만들기"}
              <Kbd>⌘↵</Kbd>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The name field's note: who wrote the name, and the way back to the AI's. */
function NameNote({
  ai,
  answer,
  edited,
  onRestore,
}: {
  ai: boolean;
  answer: { phase: string; name: string | null; message: string | null } | null;
  edited: boolean;
  onRestore: () => void;
}) {
  if (!ai) return null;
  if (!answer || answer.phase === "working") {
    return (
      <span className="text-muted-foreground" data-name-state="working">
        AI가 이름 짓는 중…
      </span>
    );
  }
  if (answer.phase === "failed") {
    return (
      <Hint label={answer.message ?? "AI가 답하지 않았습니다."}>
        <span className="text-muted-foreground" tabIndex={0} data-name-state="failed">
          AI 이름을 못 지어 기본 이름
        </span>
      </Hint>
    );
  }
  if (edited && answer.name) {
    return (
      <button type="button" onClick={onRestore} className="inline-flex items-center gap-xxs rounded-xs text-primary outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" data-name-state="edited">
        직접 고침 · <RotateCcwIcon aria-hidden="true" className="size-(--size-icon-sm)" />
        AI 이름으로
      </button>
    );
  }
  return (
    <span className="inline-flex items-center gap-xxs text-muted-foreground" data-name-state="ai">
      <SparklesIcon aria-hidden="true" className="size-(--size-icon-sm) text-primary" />
      AI 지음 · 고칠 수 있음
    </span>
  );
}

/**
 * Start from an issue: one worktree named for the issue (the dialog's name
 * first, the background AI's once it answers unless the operator has typed),
 * its base, the agent, and the first prompt drawn from the issue's body. The
 * core creates the worktree, writes the link into it, starts the agent and
 * sends it the prompt once it is ready. A folder project has no worktree:
 * the agent starts in the folder with the prompt.
 */
export function StartIssueDialog({ actions, workspace, task, onClose }: { actions: Actions; workspace: Workspace; task: Task; onClose: () => void }) {
  const settings = useShellStore((s) => s.rest?.ui_state?.issue_settings) ?? DEFAULT_SETTINGS;
  const git = workspace.is_git === true;
  const branches = workspace.branches ?? [];
  const requestId = useMemo(() => `start-${task.key}-${Date.now()}`, [task.key]);
  const [name, setName] = useState(() => defaultWorktreeName(task));
  const [edited, setEdited] = useState(false);
  const [base, setBase] = useState(workspace.default_branch && branches.includes(workspace.default_branch) ? workspace.default_branch : (branches[0] ?? ""));
  const [agent, setAgent] = useState<AgentChoice>(settings.default_agent);
  const [prompt, setPrompt] = useState(() => firstPrompt(task, null, settings.closes_instruction));
  const promptEdited = useRef(false);
  const detail = useShellStore((s) => (s.rest?.issue_work?.detail?.task_key === task.key ? s.rest.issue_work.detail : null));
  const nameAnswer = useShellStore((s) => (s.rest?.issue_work?.name?.request_id === requestId ? s.rest.issue_work.name : null));
  const asked = useRef(false);
  const [request, setRequest] = useState<{ afterId: number; branch: string; at: number } | null>(null);
  const operation = useShellStore((s) => s.rest?.task_operation);
  const created = taskFor(operation, request ? { kind: "worktree_create", afterId: request.afterId, deviceId: workspace.device_id, repositoryRoot: workspace.path, branch: request.branch } : null);
  const refused = useErrorSince(request?.at ?? null, ["worktree.create", "task_operation."]);
  const working = request !== null && refused === null && (created === null || created.phase === "working");

  // The body is read once per dialog; a GitHub issue's comes from `gh`.
  useEffect(() => {
    actions.requestIssueDetail(workspace.id, task.key);
  }, [actions, workspace.id, task.key]);
  const bodyKnown = detail !== null && detail.phase !== "reading";
  useEffect(() => {
    if (!bodyKnown || promptEdited.current) return;
    setPrompt(firstPrompt(task, detail?.body ?? null, settings.closes_instruction));
  }, [bodyKnown, detail?.body, task, settings.closes_instruction]);
  // The AI names the worktree once the body is known, so the name can use it.
  useEffect(() => {
    if (!git || !settings.ai_worktree_name || !bodyKnown || asked.current) return;
    asked.current = true;
    actions.suggestWorktreeName(requestId, namePrefix(task), task.title, detail?.body ?? "");
  }, [actions, git, settings.ai_worktree_name, bodyKnown, detail?.body, requestId, task]);
  useEffect(() => {
    if (nameAnswer?.phase === "ready" && nameAnswer.name && !edited) setName(nameAnswer.name);
  }, [nameAnswer, edited]);
  useEffect(() => {
    if (created?.phase !== "ready") return;
    if (created.agent_phase) useUiStore.getState().setWatchedTask(created.id);
    if (created.pane_id) useUiStore.getState().setFocusWhenListed(created.pane_id);
    onClose();
  }, [created, onClose]);

  const branch = name.trim();
  const problem = git ? branchProblem(branch) : null;
  const existing = git && branches.includes(branch) ? (workspace.checkouts.find((checkout) => checkout.branch === branch) ?? null) : null;
  const taken = git && branches.includes(branch);
  const withAgent = agent !== "terminal";
  const failure = created?.phase === "failed" ? (created.message ?? "워크트리를 만들지 못했습니다.") : refused;

  const submit = () => {
    if (working) return;
    if (existing) {
      actions.openWorkspace(workspace.device_id, existing.workspace_id, existing.id);
      onClose();
      return;
    }
    const text = withAgent ? prompt.trim() : "";
    if (!git) {
      const folder = workspace.checkouts[0];
      if (!folder) return;
      actions.startAgent(folder.path, agent, text || null);
      onClose();
      return;
    }
    if (problem || taken) return;
    setRequest({ afterId: actions.taskIdNow(), branch, at: Date.now() });
    actions.createWorktree({
      deviceId: workspace.device_id,
      repositoryRoot: workspace.path,
      branch,
      baseBranch: base || null,
      agentKind: withAgent ? agent : null,
      purpose: null,
      taskKey: task.key,
      prompt: text || null,
    });
  };
  const Glyph = task.source === "github" ? CircleDotIcon : FileTextIcon;
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <DialogContent data-start-issue={task.key}>
        <form className="flex min-h-0 flex-1 flex-col" onKeyDown={submitOnCommandEnter} onSubmit={(event) => { event.preventDefault(); submit(); }}>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-xs">
              <Glyph aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />
              {task.id ?? "이슈"} 작업 시작
            </DialogTitle>
            <DialogDescription className="line-clamp-2 break-words">{task.title}</DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-sm">
            {git ? (
              <>
                <Field
                  label="워크트리 · 브랜치"
                  aside={
                    <NameNote
                      ai={settings.ai_worktree_name}
                      answer={nameAnswer}
                      edited={edited}
                      onRestore={() => {
                        if (nameAnswer?.name) setName(nameAnswer.name);
                        setEdited(false);
                      }}
                    />
                  }
                >
                  <Input
                    value={name}
                    mono
                    disabled={working}
                    autoComplete="off"
                    spellCheck={false}
                    aria-invalid={taken || problem !== null}
                    onChange={(event) => {
                      setName(event.target.value);
                      setEdited(true);
                    }}
                    data-start-name="true"
                  />
                </Field>
                {taken ? <Note tone="error" data-start-name-taken="true">이미 있는 브랜치입니다. 다른 이름을 쓰거나 {existing ? "기존 워크트리를 여세요." : "그 브랜치를 먼저 정리하세요."}</Note> : problem ? <Note tone="warn">{problem}</Note> : null}
                <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-md">
                  <Field label="기준">
                    <Select value={base || undefined} disabled={working || branches.length === 0} onValueChange={setBase}>
                      <SelectTrigger className="w-full" aria-label="기준 브랜치" data-start-base="true">
                        <SelectValue placeholder="아직 브랜치를 읽지 못함" />
                      </SelectTrigger>
                      <SelectContent>
                        {branches.map((row) => (
                          <SelectItem key={row} value={row}>
                            {row}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                  <AgentChoiceField agent={agent} disabled={working} onChange={setAgent} />
                </div>
              </>
            ) : (
              <AgentChoiceField agent={agent} disabled={working} onChange={setAgent} />
            )}
            {withAgent ? (
              <Field label="첫 지시" aside={<span className="text-muted-foreground">{detail?.phase === "reading" ? "이슈 본문 읽는 중…" : "이슈 본문에서 채움 · 고칠 수 있음"}</span>}>
                <TextArea
                  value={prompt}
                  rows={5}
                  disabled={working}
                  onChange={(event) => {
                    promptEdited.current = true;
                    setPrompt(event.target.value);
                  }}
                  data-start-prompt="true"
                />
              </Field>
            ) : null}
            {detail?.phase === "failed" ? <Note tone="warn">{`본문을 읽지 못해 제목만 넣었습니다: ${detail.message ?? ""}`}</Note> : null}
            {failure ? <Note tone="error" data-start-error="true">{failure}</Note> : null}
            {working ? <Status tone="pending">워크트리를 만드는 중…</Status> : null}
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={onClose}>{working ? "숨기기" : "취소"}</Button>
            <Button type="submit" disabled={working || (git && !existing && (taken || problem !== null))} data-start-submit={existing ? "open" : "start"}>
              {existing ? "기존 워크트리 열기" : working ? "시작하는 중…" : "시작"}
              <Kbd>⌘↵</Kbd>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function AgentChoiceField({ agent, disabled, onChange }: { agent: AgentChoice; disabled: boolean; onChange: (agent: AgentChoice) => void }) {
  return (
    <fieldset className="text-body text-subtle-foreground" disabled={disabled}>
      <legend>에이전트</legend>
      <RadioGroup className="mt-xxs grid-flow-col justify-start gap-md" value={agent} onValueChange={(value) => onChange(value as AgentChoice)}>
        {AGENTS.map((row) => (
          <label key={row.id} className="inline-flex h-(--size-control) items-center gap-xs text-foreground">
            <RadioGroupItem value={row.id} data-start-agent={row.id} />
            {row.label}
          </label>
        ))}
      </RadioGroup>
    </fieldset>
  );
}

/**
 * 이슈 연결 on a worktree with no issue: the project's open issues to pick
 * from, and, when none is the one, a new issue with the typed title made in
 * place and linked once the source has it.
 */
export function LinkIssuePopover({ workspaceId, checkout, actions, children }: { workspaceId: string; checkout: Checkout; actions: Actions; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [pending, setPending] = useState<{ after: number } | null>(null);
  const workspace = useShellStore((s) => s.rest?.navigator?.workspaces?.find((row) => row.id === workspaceId) ?? null);
  const create = useShellStore((s) => s.rest?.issue_work?.create ?? null);
  const answer = pending && create && create.id > pending.after && create.workspace_id === workspaceId ? create : null;
  const tasks = useMemo(() => (workspace?.tasks?.tasks ?? []).filter((task) => task.open), [workspace]);
  const worked = useMemo(() => new Set((workspace?.checkouts ?? []).map((row) => row.task_key).filter(Boolean)), [workspace]);

  useEffect(() => {
    if (answer?.phase !== "ready" || !answer.task_key) return;
    const task = tasks.find((row) => row.key === answer.task_key);
    if (!task?.id) return;
    actions.linkIssue(checkout.id, task.id);
    setPending(null);
    setOpen(false);
  }, [answer, tasks, actions, checkout.id]);

  const link = (task: Task) => {
    if (!task.id) return;
    actions.linkIssue(checkout.id, task.id);
    setOpen(false);
  };
  const title = query.trim();
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setQuery("");
      }}
    >
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent align="start" className="w-(--size-pr-popover) p-none" data-link-issue={checkout.id}>
        <Command loop>
          <CommandInput value={query} onValueChange={setQuery} placeholder="연결할 이슈 찾기" aria-label={`${checkout.branch ?? checkout.label}에 연결할 이슈`} data-link-issue-query="true" />
          <CommandList className="max-h-(--size-relationship-list-max)">
            <CommandEmpty>맞는 열린 이슈가 없습니다</CommandEmpty>
            <CommandGroup>
              {tasks.map((task) => (
                <CommandItem key={task.key} value={`${task.id ?? ""} ${task.title}`} onSelect={() => link(task)} data-link-issue-item={task.key}>
                  {task.source === "github" ? <CircleDotIcon aria-hidden="true" /> : <FileTextIcon aria-hidden="true" />}
                  <span className="shrink-0 font-mono text-caption text-muted-foreground">{task.id}</span>
                  <span className="min-w-0 flex-1 truncate">{task.title}</span>
                  {worked.has(task.key) ? <span className="text-caption text-muted-foreground">작업 중</span> : null}
                </CommandItem>
              ))}
            </CommandGroup>
            {title ? (
              <CommandGroup forceMount>
                <CommandItem
                  forceMount
                  value={`new-issue ${title}`}
                  disabled={pending !== null && answer?.phase !== "failed"}
                  onSelect={() => {
                    setPending({ after: create?.id ?? 0 });
                    actions.createIssue(workspaceId, title, "");
                  }}
                  data-link-issue-create="true"
                >
                  <PlusIcon aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate">"{title}"로 새 이슈 만들기</span>
                  <CommandShortcut>{pending && answer?.phase !== "failed" ? "만드는 중…" : null}</CommandShortcut>
                </CommandItem>
              </CommandGroup>
            ) : null}
          </CommandList>
          {answer?.phase === "failed" ? <p className="border-t border-border px-md py-xs text-caption text-destructive">{answer.message}</p> : null}
        </Command>
      </PopoverContent>
    </Popover>
  );
}

/** Whether a key press belongs to a field or a terminal rather than to the page. */
function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || target.closest("input, textarea, select, [contenteditable='true'], .xterm, [role='dialog'], [role='menu'], [role='listbox']") !== null;
}

/**
 * `C` on an Overview opens New issue, as Linear's does: only while the page
 * itself has the keyboard, never from a field, a terminal, a menu or a
 * dialog. `null` while the page has nowhere to put an issue.
 */
export function useNewIssueShortcut(open: (() => void) | null) {
  const latest = useRef(open);
  latest.current = open;
  const enabled = open !== null;
  useEffect(() => {
    if (!enabled) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
      if (event.key.toLowerCase() !== "c" || event.isComposing || typing(event.target)) return;
      const ui = useUiStore.getState();
      if (ui.overlay !== "none" || ui.workspaceDialog !== null) return;
      event.preventDefault();
      latest.current?.();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled]);
}
