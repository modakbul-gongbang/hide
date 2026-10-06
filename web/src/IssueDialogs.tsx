// Work that starts from an issue (the issue-first Overview, 2026-09-28): a
// new issue in a project's source and the Start dialog that turns an issue
// into a worktree with an agent and its first prompt. Each sends the one core event that owns
// the change and reads the core's own answer for it (`issue_work`,
// `task_operation`); a dialog never decides that its request succeeded.

import { FileTextIcon, CircleDotIcon, RotateCcwIcon, SparklesIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ComponentProps, type ReactNode } from "react";
import type { Actions } from "./actions";
import { rememberedSelection, modelToSend, type AgentSelection } from "./agentPicker";
import { AgentPicker } from "./components/agent-picker";
import { Button } from "./components/ui/button";
import { Checkbox } from "./components/ui/checkbox";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./components/ui/select";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { Note, Status } from "./components/settings-rows";
import { defaultWorktreeName, firstPrompt, namePrefix } from "./issueStart";
import { cn } from "./lib/utils";
import type { IssueSettings, Task, Workspace } from "./snapshot";
import { hideAiCanAnswer } from "./hideAi";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { branchProblem, taskFor } from "./workspaceManage";
import { useErrorSince } from "./WorkspaceDialogs";
import { fieldLabel } from "./shortcutLabels";

export const DEFAULT_SETTINGS: IssueSettings = { ai_worktree_name: true, closes_instruction: true };

/** A multi-line field in the text field's look; the issue body and the first prompt. */
export function TextArea({ className, ...props }: ComponentProps<"textarea">) {
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

export function Field({ label, aside, children }: { label: string; aside?: ReactNode; children: ReactNode }) {
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

/** ⌘↵ (Ctrl+Enter off macOS) sends the form from any field, the way the footer's keycap says. */
export function submitOnCommandEnter(event: React.KeyboardEvent<HTMLFormElement>) {
  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    event.currentTarget.requestSubmit();
  }
}

/** The projects on this Mac an issue can be made in: each with its source, never the Home, which is no project. */
function issueProjects(workspaces: readonly Workspace[] | undefined): Workspace[] {
  return (workspaces ?? []).filter((workspace) => !workspace.remote_target_id && !workspace.is_home && workspace.tasks?.source);
}

/** `herdr-ide · GitHub owner/repo`, `notes · Local`: where a new issue lands. */
export function sourcePlace(workspace: Workspace): string {
  const source = workspace.tasks?.source;
  if (!source) return workspace.label;
  return [workspace.label, source.name ? `${source.label} ${source.name}` : source.label].join(" · ");
}

/**
 * New issue: in the named project's source, or another project picked from
 * the list; `Create and start immediately` goes on to the Start dialog with the new issue
 * once the source has it.
 */
export function NewIssueDialog({ actions, workspace, onClose }: { actions: Actions; workspace: Workspace; onClose: () => void }) {
  const { t } = useInterfaceTranslation();
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
  const failure = answer?.phase === "failed" ? (answer.message ?? t("issue.createFailed")) : refused;

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
            <DialogTitle>{t("issue.newTitle")}</DialogTitle>
            <DialogDescription className="sr-only">{t("issue.newDescription")}</DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-sm">
            <Field label={t("issue.place")}>
              <Select value={projectId} disabled={working} onValueChange={setProjectId}>
                <SelectTrigger className="w-full" aria-label={t("issue.place")} data-new-issue-project="true">
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
            <Field label={t("issue.title")}>
              <Input value={title} autoFocus disabled={working} maxLength={256} onChange={(event) => setTitle(event.target.value.replace(/[\r\n]+/g, " "))} data-new-issue-title="true" />
            </Field>
            <Field label={t("issue.content")}>
              <TextArea value={body} rows={4} disabled={working} onChange={(event) => setBody(event.target.value)} data-new-issue-body="true" />
            </Field>
            <label className="inline-flex items-center gap-xs text-body text-foreground">
              <Checkbox checked={thenStart} disabled={working} onCheckedChange={(checked) => setThenStart(checked === true)} data-new-issue-start="true" />
              {t("issue.createAndStart")}
            </label>
            {project?.tasks?.source?.failure ? <Note tone="warn">{`${project.tasks.source.label}: ${project.tasks.source.failure}`}</Note> : null}
            {failure ? <Note tone="error" data-new-issue-error="true">{failure}</Note> : null}
            {working ? <Status tone="pending">{t("issue.creating")}</Status> : null}
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={onClose}>{t(working ? "issue.hide" : "common.cancel")}</Button>
            <Button type="submit" disabled={working || !project || !title.trim()} data-new-issue-create="true">
              {t(thenStart ? "issue.createStart" : "issue.create")}
              <Kbd>{fieldLabel("Enter")}</Kbd>
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
  const { t } = useInterfaceTranslation();
  if (!ai) return null;
  if (!answer || answer.phase === "working") {
    return (
      <span className="text-muted-foreground" data-name-state="working">
        {t("issue.name.working")}
      </span>
    );
  }
  if (answer.phase === "failed") {
    return (
      <Hint label={answer.message ?? t("issue.name.unanswered")}>
        <span className="text-muted-foreground" tabIndex={0} data-name-state="failed">
          {t("issue.name.defaultFallback")}
        </span>
      </Hint>
    );
  }
  if (edited && answer.name) {
    return (
      <button type="button" onClick={onRestore} className="inline-flex items-center gap-xxs rounded-xs text-primary outline-none hover:underline focus-visible:ring-1 focus-visible:ring-ring" data-name-state="edited">
        {t("issue.name.edited")} <RotateCcwIcon aria-hidden="true" className="size-(--size-icon-sm)" />
        {t("issue.name.restore")}
      </button>
    );
  }
  return (
    <span className="inline-flex items-center gap-xxs text-muted-foreground" data-name-state="ai">
      <SparklesIcon aria-hidden="true" className="size-(--size-icon-sm) text-primary" />
      {t("issue.name.aiEditable")}
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
  const { t } = useInterfaceTranslation();
  const settings = useShellStore((s) => s.rest?.ui_state?.issue_settings) ?? DEFAULT_SETTINGS;
  const hideAi = useShellStore((s) => s.rest?.status?.background_ai);
  // The AI names the worktree only when it is on and something can answer; otherwise the dialog keeps its default name and does not wait.
  const aiNames = settings.ai_worktree_name && hideAiCanAnswer(hideAi);
  const git = workspace.is_git === true;
  const branches = workspace.branches ?? [];
  const requestId = useMemo(() => `start-${task.key}-${Date.now()}`, [task.key]);
  const [name, setName] = useState(() => defaultWorktreeName(task));
  const [edited, setEdited] = useState(false);
  const [base, setBase] = useState(workspace.default_branch && branches.includes(workspace.default_branch) ? workspace.default_branch : (branches[0] ?? ""));
  const [agent, setAgent] = useState<AgentSelection>(() => rememberedSelection(useShellStore.getState().rest?.ui_state?.agent_start));
  const [prompt, setPrompt] = useState(() => firstPrompt(task, null, settings.closes_instruction, t));
  const promptEdited = useRef(false);
  const detail = useShellStore((s) => (s.rest?.issue_work?.detail?.task_key === task.key ? s.rest.issue_work.detail : null));
  const nameAnswer = useShellStore((s) => (s.rest?.issue_work?.name?.request_id === requestId ? s.rest.issue_work.name : null));
  const asked = useRef(false);
  const [request, setRequest] = useState<{ afterId: number; branch: string; at: number } | null>(null);
  const operation = useShellStore((s) => s.rest?.task_operation);
  const created = taskFor(operation, request ? { kind: "worktree_create", afterId: request.afterId, deviceId: workspace.device_id, repositoryRoot: workspace.path, branch: request.branch } : null);
  const refused = useErrorSince(request?.at ?? null, ["worktree.create", "task_operation.", "agent_start."]);
  const working = request !== null && refused === null && (created === null || created.phase === "working");

  // The body is read once per dialog; a GitHub issue's comes from `gh`.
  useEffect(() => {
    actions.requestIssueDetail(workspace.id, task.key);
  }, [actions, workspace.id, task.key]);
  const bodyKnown = detail !== null && detail.phase !== "reading";
  useEffect(() => {
    if (!bodyKnown || promptEdited.current) return;
    setPrompt(firstPrompt(task, detail?.body ?? null, settings.closes_instruction, t));
  }, [bodyKnown, detail?.body, task, settings.closes_instruction, t]);
  // The AI names the worktree once the body is known, so the name can use it.
  useEffect(() => {
    if (!git || !aiNames || !bodyKnown || asked.current) return;
    asked.current = true;
    actions.suggestWorktreeName(requestId, namePrefix(task), task.title, detail?.body ?? "");
  }, [actions, git, aiNames, bodyKnown, detail?.body, requestId, task]);
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
  const failure = created?.phase === "failed" ? (created.message ?? t("issue.worktreeFailed")) : refused;

  const submit = () => {
    if (working) return;
    if (existing) {
      actions.openWorkspace(workspace.device_id, existing.workspace_id, existing.id);
      onClose();
      return;
    }
    const text = prompt.trim();
    const model = modelToSend(agent);
    if (!git) {
      const folder = workspace.checkouts[0];
      if (!folder) return;
      if (agent.kind === "terminal") return;
      actions.startAgent({ target: { checkoutPath: folder.path }, deviceId: workspace.device_id, provider: agent.kind, model, prompt: text || null });
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
      agentKind: agent.kind === "terminal" ? null : agent.kind,
      model,
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
              {t("issue.startTitle", { issue: task.id ?? t("issue.label") })}
            </DialogTitle>
            <DialogDescription className="line-clamp-2 break-words">{task.title}</DialogDescription>
          </DialogHeader>
          <DialogBody className="space-y-sm">
            {git ? (
              <>
                <Field
                  label={t("issue.worktreeBranch")}
                  aside={
                    <NameNote
                      ai={aiNames}
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
                {taken ? <Note tone="error" data-start-name-taken="true">{t(existing ? "issue.branchTakenExisting" : "issue.branchTakenNoCheckout")}</Note> : problem ? <Note tone="warn">{problem}</Note> : null}
                <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-md">
                  <Field label={t("issue.base")}>
                    <Select value={base || undefined} disabled={working || branches.length === 0} onValueChange={setBase}>
                      <SelectTrigger className="w-full" aria-label={t("issue.baseBranch")} data-start-base="true">
                        <SelectValue placeholder={t("issue.branchesUnread")} />
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
                  <AgentField agent={agent} actions={actions} disabled={working} onChange={setAgent} />
                </div>
              </>
            ) : (
              <AgentField agent={agent} actions={actions} disabled={working} onChange={setAgent} />
            )}
            <Field label={t("issue.firstPrompt")} aside={<span className="text-muted-foreground">{t(detail?.phase === "reading" ? "issue.bodyReading" : "issue.bodyPrefilled")}</span>}>
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
            {detail?.phase === "failed" ? <Note tone="warn">{t("issue.bodyReadFailed", { message: detail.message ?? "" })}</Note> : null}
            {failure ? <Note tone="error" data-start-error="true">{failure}</Note> : null}
            {working ? <Status tone="pending">{t("issue.creatingWorktree")}</Status> : null}
          </DialogBody>
          <DialogFooter>
            <Button variant="secondary" onClick={onClose}>{t(working ? "issue.hide" : "common.cancel")}</Button>
            <Button type="submit" disabled={working || (git && !existing && (taken || problem !== null))} data-start-submit={existing ? "open" : "start"}>
              {t(existing ? "issue.openExisting" : working ? "issue.starting" : "common.start")}
              <Kbd>{fieldLabel("Enter")}</Kbd>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The kind and model to start with, the same control ⌘N's panel has (PRD home-device-rail B35). */
export function AgentField({ agent, actions, disabled, onChange }: { agent: AgentSelection; actions: Actions; disabled: boolean; onChange: (agent: AgentSelection) => void }) {
  const { t } = useInterfaceTranslation();
  return (
    <div className="text-body text-subtle-foreground">
      {t("common.agent")}
      <AgentPicker actions={actions} value={agent} onChange={onChange} disabled={disabled} className="mt-xxs" />
    </div>
  );
}

/** Whether a key press belongs to a field or a terminal rather than to the page. */
export function typing(target: EventTarget | null): boolean {
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
