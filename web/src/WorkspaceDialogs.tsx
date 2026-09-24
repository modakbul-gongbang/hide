// The project and checkout management dialogs (PRD S5 B11-B17): a worktree
// creation, a purpose edit and a worktree deletion. Each sends the one core
// event that owns the change and then reads the core's own receipt for it
// (`task_operation`, `worktree_removal`); a dialog never decides that its
// request succeeded, and a receipt for another target is never its answer.

import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { Button, Dialog, Field, Note, Select, Status } from "./components/ui/controls";
import type { Checkout, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import {
  PURPOSE_HARD_LIMIT,
  branchProblem,
  deletionConsequences,
  normalizePurpose,
  purposeCountLabel,
  purposeIsLong,
  removalFor,
  taskFor,
} from "./workspaceManage";

function useErrorSince(since: number | null, prefixes: readonly string[]): string | null {
  const error = useShellStore((s) => s.rest?.status?.last_error ?? null);
  if (since === null || !error || error.occurred_at < since) return null;
  return prefixes.some((prefix) => error.kind.startsWith(prefix)) ? error.message : null;
}

/** The row a dialog names, among this machine's projects or a connected device's workspaces (a remote purpose). */
function findTarget(workspaceId: string, checkoutId?: string): { workspace: Workspace; checkout: Checkout | null } | null {
  const rest = useShellStore.getState().rest;
  const workspace =
    rest?.navigator?.workspaces?.find((row) => row.id === workspaceId) ??
    rest?.status?.remote?.flatMap((row) => row.session?.workspaces ?? []).find((row) => row.id === workspaceId);
  if (!workspace) return null;
  return { workspace, checkout: checkoutId ? (workspace.checkouts.find((row) => row.id === checkoutId) ?? null) : null };
}

export function WorkspaceDialogs({ actions }: { actions: Actions }) {
  const dialog = useUiStore((s) => s.workspaceDialog);
  // Re-read on every snapshot so a dialog follows its row (a purpose saved
  // elsewhere, a gate that changed) rather than a copy from when it opened.
  useShellStore((s) => s.rest?.navigator?.workspaces);
  useShellStore((s) => s.rest?.status?.remote);
  // A deleted worktree leaves the navigator while its dialog still reports
  // the result, so the dialog keeps the last row it saw for its checkout.
  const lastSeen = useRef<{ key: string; target: { workspace: Workspace; checkout: Checkout | null } } | null>(null);
  const close = () => useUiStore.getState().setWorkspaceDialog(null);
  if (!dialog) return null;
  const key = JSON.stringify(dialog);
  const found = findTarget(dialog.workspaceId, "checkoutId" in dialog ? dialog.checkoutId : undefined);
  if (found && (!("checkoutId" in dialog) || found.checkout)) lastSeen.current = { key, target: found };
  const target = found?.checkout || !("checkoutId" in dialog) ? found : dialog.kind === "delete_worktree" && lastSeen.current?.key === key ? lastSeen.current.target : found;
  if (!target || ("checkoutId" in dialog && !target.checkout)) {
    return (
      <Dialog label="Unavailable" onClose={close}>
        <div className="p-lg">
          <Note tone="warn">That project or checkout is no longer listed. Nothing was changed.</Note>
          <div className="mt-md flex justify-end">
            <Button onClick={close}>Close</Button>
          </div>
        </div>
      </Dialog>
    );
  }
  if (dialog.kind === "new_worktree") return <NewWorktreeDialog actions={actions} workspace={target.workspace} onClose={close} />;
  if (dialog.kind === "purpose" && target.checkout) return <PurposeDialog actions={actions} checkout={target.checkout} onClose={close} />;
  if (dialog.kind === "delete_worktree" && target.checkout) return <DeleteWorktreeDialog actions={actions} deviceId={target.workspace.device_id} checkout={target.checkout} onClose={close} />;
  return null;
}

function DialogHeader({ title, detail }: { title: string; detail?: string }) {
  return (
    <div className="mb-md">
      <h2 className="break-words text-title font-semibold text-primary">{title}</h2>
      {detail ? <p className="break-all font-mono text-caption text-muted">{detail}</p> : null}
    </div>
  );
}

const AGENTS = [
  { id: "terminal", label: "Terminal only" },
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
] as const;

function NewWorktreeDialog({ actions, workspace, onClose }: { actions: Actions; workspace: Workspace; onClose: () => void }) {
  const branches = workspace.branches ?? [];
  const [branch, setBranch] = useState("");
  const [base, setBase] = useState(workspace.default_branch && branches.includes(workspace.default_branch) ? workspace.default_branch : (branches[0] ?? ""));
  const [agent, setAgent] = useState<(typeof AGENTS)[number]["id"]>("terminal");
  const [purpose, setPurpose] = useState("");
  const [request, setRequest] = useState<{ afterId: number; branch: string; at: number } | null>(null);
  const operation = useShellStore((s) => s.rest?.task_operation);
  const task = taskFor(operation, request ? { kind: "worktree_create", afterId: request.afterId, deviceId: workspace.device_id, repositoryRoot: workspace.path, branch: request.branch } : null);
  const refused = useErrorSince(request?.at ?? null, ["worktree.create", "task_operation."]);
  const working = request !== null && refused === null && (task === null || task.phase === "working");
  const problem = branch ? branchProblem(branch) : null;

  useEffect(() => {
    if (task?.phase !== "ready") return;
    // The core focused the created pane; the agent's answer, if one was
    // chosen, is reported by the task notice after this dialog is gone.
    if (task.agent_phase) useUiStore.getState().setWatchedTask(task.id);
    if (task.pane_id) useUiStore.getState().setFocusWhenListed(task.pane_id);
    onClose();
  }, [task, onClose]);

  const submit = () => {
    const name = branch.trim();
    if (branchProblem(name) || working) return;
    setRequest({ afterId: actions.taskIdNow(), branch: name, at: Date.now() });
    actions.createWorktree({
      deviceId: workspace.device_id,
      repositoryRoot: workspace.path,
      branch: name,
      baseBranch: base || null,
      agentKind: agent === "terminal" ? null : agent,
      purpose: purpose.trim() ? normalizePurpose(purpose.trim()) : null,
    });
  };
  const failure = task?.phase === "failed" ? (task.message ?? "The worktree was not created.") : refused;
  return (
    <Dialog label={`New worktree in ${workspace.label}`} onClose={onClose} data-new-worktree={workspace.id}>
      <form
        className="space-y-sm p-lg"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <DialogHeader title={`New worktree in ${workspace.label}`} detail={workspace.path} />
        <label className="block text-body text-secondary">
          Branch
          <Field value={branch} disabled={working} autoComplete="off" spellCheck={false} placeholder="feature/name" className="mt-xxs w-full" onChange={(event) => setBranch(event.target.value)} data-worktree-branch="true" />
        </label>
        {problem ? <Note tone="warn">{problem}</Note> : null}
        <label className="block text-body text-secondary">
          Base
          <Select value={base} disabled={working || branches.length === 0} className="mt-xxs w-full" onChange={(event) => setBase(event.target.value)} data-worktree-base="true">
            {branches.length === 0 ? <option value="">No branches read yet</option> : null}
            {branches.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </Select>
        </label>
        <fieldset className="text-body text-secondary" disabled={working}>
          <legend>Start in the new pane</legend>
          <div className="mt-xxs flex flex-wrap gap-md">
            {AGENTS.map((row) => (
              <label key={row.id} className="inline-flex items-center gap-xs text-primary">
                <input type="radio" name="worktree-agent" value={row.id} checked={agent === row.id} onChange={() => setAgent(row.id)} data-worktree-agent={row.id} />
                {row.label}
              </label>
            ))}
          </div>
        </fieldset>
        <label className="block text-body text-secondary">
          Purpose (optional)
          <Field value={purpose} mono={false} disabled={working} maxLength={PURPOSE_HARD_LIMIT * 2} className="mt-xxs w-full" onChange={(event) => setPurpose(normalizePurpose(event.target.value))} data-worktree-purpose="true" />
        </label>
        {purpose ? (
          <p className={`text-caption ${purposeIsLong(purpose) ? "text-warning" : "text-muted"}`}>{purposeCountLabel(purpose)}{purposeIsLong(purpose) ? " · longer than a sidebar row shows" : ""}</p>
        ) : null}
        {failure ? <Note tone="error" data-worktree-error="true">{failure}</Note> : null}
        {working ? <Status tone="pending">Creating the worktree…</Status> : null}
        <div className="flex justify-end gap-sm pt-sm">
          <Button onClick={onClose}>{working ? "Hide" : "Cancel"}</Button>
          <Button appearance="prominent" type="submit" disabled={working || !branch.trim() || problem !== null} data-worktree-create="true">
            {working ? "Creating…" : "Create worktree"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function PurposeDialog({ actions, checkout, onClose }: { actions: Actions; checkout: Checkout; onClose: () => void }) {
  // Only a purpose someone wrote is the field's value; a title the row falls
  // back to is shown as the placeholder, so saving never stores a guess.
  const written = checkout.purpose && (checkout.purpose.origin === "token" || checkout.purpose.origin === "branch_description") ? checkout.purpose.text : "";
  const [text, setText] = useState(written);
  const [request, setRequest] = useState<{ afterId: number; at: number; text: string } | null>(null);
  const operation = useShellStore((s) => s.rest?.task_operation);
  const task = taskFor(operation, request ? { kind: "checkout_purpose", afterId: request.afterId, path: checkout.path } : null);
  const refused = useErrorSince(request?.at ?? null, ["checkout_purpose.", "task_operation."]);
  const working = request !== null && refused === null && (task === null || task.phase === "working");
  const [saved, setSaved] = useState<string | null>(null);
  useEffect(() => {
    if (task?.phase === "ready" && request) setSaved(request.text);
  }, [task, request]);
  const send = (value: string) => {
    setSaved(null);
    setRequest({ afterId: actions.taskIdNow(), at: Date.now(), text: value });
    actions.setPurpose(checkout.id, value);
  };
  const failure = task?.phase === "failed" ? (task.message ?? "The purpose was not saved.") : refused;
  const label = checkout.branch ?? checkout.label;
  return (
    <Dialog label={`Purpose of ${label}`} onClose={onClose} data-purpose-dialog={checkout.id}>
      <form
        className="space-y-sm p-lg"
        onSubmit={(event) => {
          event.preventDefault();
          if (!working) send(normalizePurpose(text).trim());
        }}
      >
        <DialogHeader title={`Purpose of ${label}`} detail={checkout.path} />
        <Field
          value={text}
          mono={false}
          disabled={working}
          aria-label="Purpose"
          placeholder={checkout.purpose && !written ? checkout.purpose.text : "What this workspace is for"}
          className="w-full"
          onChange={(event) => {
            setSaved(null);
            setText(normalizePurpose(event.target.value));
          }}
          data-purpose-field="true"
        />
        <p className={`text-caption ${purposeIsLong(text) ? "text-warning" : "text-muted"}`} data-purpose-count="true">
          {purposeCountLabel(text)}
          {purposeIsLong(text) ? " · longer than a sidebar row shows" : ""}
        </p>
        {failure ? (
          <Note tone="error" data-purpose-error="true">
            {failure} Your text is kept; Save tries again.
          </Note>
        ) : null}
        {saved !== null ? (
          <Status tone="ok" data-purpose-saved="true">
            {saved ? "Saved" : "Cleared"}
          </Status>
        ) : null}
        {working ? <Status tone="pending">Saving…</Status> : null}
        <div className="flex justify-end gap-sm pt-sm">
          <Button onClick={onClose}>{saved !== null ? "Done" : "Cancel"}</Button>
          <Button disabled={working || (!written && !text)} onClick={() => send("")} data-purpose-clear="true">
            Clear
          </Button>
          <Button appearance="prominent" type="submit" disabled={working} data-purpose-save="true">
            {working ? "Saving…" : "Save"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function DeleteWorktreeDialog({ actions, deviceId, checkout, onClose }: { actions: Actions; deviceId: string; checkout: Checkout; onClose: () => void }) {
  const row = checkout.worktree;
  const gate = row?.deletion_gate;
  const paneCount = checkout.tabs.reduce((count, tab) => count + tab.panes.length, 0);
  const [deleteBranch, setDeleteBranch] = useState(false);
  const [request, setRequest] = useState<{ afterId: number; at: number } | null>(null);
  const current = useShellStore((s) => s.rest?.worktree_removal);
  const removal = request ? removalFor(current, deviceId, checkout.path, request.afterId) : null;
  const refused = useErrorSince(request?.at ?? null, ["worktree.remove"]);
  const inFlight = request !== null && refused === null && (removal === null || removal.phase === "closing" || removal.phase === "removing");
  const settled = removal && (removal.phase === "finished" || removal.phase === "failed") ? removal : null;
  const branch = row?.branch ?? checkout.branch;
  const confirm = () => {
    const afterId = useShellStore.getState().rest?.worktree_removal?.id ?? 0;
    setRequest({ afterId, at: Date.now() });
    useUiStore.getState().setWatchedRemoval({ deviceId, path: checkout.path, afterId });
    actions.removeWorktree(deviceId, checkout.path, deleteBranch && !!gate?.can_delete_branch);
  };
  const hide = () => {
    // A removal the operator stopped watching still reports its end through
    // the notice; one that already ended needs nothing more.
    if (!inFlight) useUiStore.getState().setWatchedRemoval(null);
    onClose();
  };
  return (
    <Dialog label={`Delete worktree ${branch ?? checkout.label}`} role="alertdialog" initialFocus="container" onClose={hide} data-delete-worktree={checkout.id}>
      <div className="space-y-sm p-lg">
        <DialogHeader title={`Delete worktree ${branch ?? checkout.label}?`} detail={checkout.path} />
        {!row || !gate ? <Note tone="warn">The worktree row has not been read yet, so nothing can be deleted.</Note> : null}
        {gate?.blocked_reason ? (
          <Note tone="warn" data-delete-blocked="true">
            {gate.blocked_reason}
          </Note>
        ) : null}
        {row && gate && !gate.blocked_reason && !request ? (
          <>
            <ul className="list-disc space-y-xxs pl-lg text-body text-secondary" data-delete-consequences="true">
              {deletionConsequences(checkout, paneCount).map((line) => (
                <li key={line} className="break-words">
                  {line}
                </li>
              ))}
            </ul>
            {branch ? (
              <label className={`flex items-start gap-xs text-body ${gate.can_delete_branch ? "text-primary" : "text-muted"}`}>
                <input type="checkbox" checked={deleteBranch} disabled={!gate.can_delete_branch} onChange={(event) => setDeleteBranch(event.target.checked)} data-delete-branch="true" />
                <span>
                  Also delete the local branch {branch}
                  {gate.can_delete_branch ? " (only if Git agrees it is merged)" : " (not offered: the branch is not safely merged or not named)"}
                </span>
              </label>
            ) : null}
          </>
        ) : null}
        {inFlight ? (
          <Status tone="pending" data-delete-phase={removal?.phase ?? "requested"}>
            {removal?.phase === "removing" ? "Rechecking and removing the folder…" : `Closing ${paneCount} pane${paneCount === 1 ? "" : "s"}…`}
          </Status>
        ) : null}
        {refused && refused !== gate?.blocked_reason ? <Note tone="error">{refused}</Note> : null}
        {settled ? (
          <Note tone={settled.phase === "finished" ? "ok" : "error"} data-delete-result={settled.phase}>
            {settled.message ?? (settled.phase === "finished" ? "Worktree removed." : "Worktree removal failed.")}
          </Note>
        ) : null}
        <div className="flex justify-end gap-sm pt-sm">
          <Button onClick={hide} data-delete-cancel="true">
            {settled || refused ? "Close" : inFlight ? "Hide" : "Keep worktree"}
          </Button>
          {row && gate && !gate.blocked_reason && !request ? (
            <Button appearance="danger" onClick={confirm} data-delete-confirm="true">
              {gate.button_label || "Delete worktree"}
            </Button>
          ) : null}
        </div>
      </div>
    </Dialog>
  );
}

/**
 * The answers that arrive after their dialog is gone: the chosen agent's start
 * in a created pane, and a removal the operator hid while it ran. Only the
 * request this page made is reported, by its id or its checkout.
 */
export function WorkspaceNotices({ actions }: { actions: Actions }) {
  const watchedTask = useUiStore((s) => s.watchedTask);
  const watchedRemoval = useUiStore((s) => s.watchedRemoval);
  const operation = useShellStore((s) => s.rest?.task_operation);
  const removal = useShellStore((s) => s.rest?.worktree_removal);
  const dialogOpen = useUiStore((s) => s.workspaceDialog?.kind === "delete_worktree");
  const task = operation && operation.id === watchedTask ? operation : null;

  useEffect(() => {
    if (!watchedRemoval || dialogOpen) return;
    const answer = removalFor(removal, watchedRemoval.deviceId, watchedRemoval.path, watchedRemoval.afterId);
    if (!answer || (answer.phase !== "finished" && answer.phase !== "failed")) return;
    useUiStore.getState().setNotice({ text: answer.message ?? (answer.phase === "finished" ? "Worktree removed." : "Worktree removal failed."), refreshable: false });
    useUiStore.getState().setWatchedRemoval(null);
  }, [watchedRemoval, removal, dialogOpen]);

  useEffect(() => {
    if (watchedTask !== null && operation && operation.id !== watchedTask) useUiStore.getState().setWatchedTask(null);
  }, [watchedTask, operation]);

  // The created worktree's pane is where the operator goes next (B13). The
  // core selects it when the creation lands, but only a focus request is
  // tracked until Herdr confirms it, so the move is made explicit once the
  // pane is listed: a late focus event for the pane left behind cannot undo it.
  const focusWhenListed = useUiStore((s) => s.focusWhenListed);
  const listed = useShellStore((s) =>
    focusWhenListed !== null &&
    [...(s.rest?.navigator?.workspaces ?? []), ...(s.rest?.status?.remote ?? []).flatMap((row) => row.session?.workspaces ?? [])].some((workspace) =>
      workspace.checkouts.some((checkout) => checkout.tabs.some((tab) => tab.panes.some((pane) => pane.id === focusWhenListed))),
    ),
  );
  useEffect(() => {
    if (!focusWhenListed || !listed) return;
    useUiStore.getState().setFocusWhenListed(null);
    actions.focusPane(focusWhenListed);
  }, [focusWhenListed, listed, actions]);

  if (!task || !task.agent_phase || task.agent_phase === "started") return null;
  const dismiss = () => useUiStore.getState().setWatchedTask(null);
  return (
    <div role="status" data-task-agent={task.agent_phase} className="flex flex-wrap items-center gap-md border-b border-divider bg-panel px-md py-xs text-caption text-secondary">
      <span className="min-w-0 flex-1 break-words">
        {task.agent_phase === "starting"
          ? `Starting ${task.agent_kind} in the new pane…`
          : `${task.agent_message ?? "The agent did not start."} The worktree and its pane are kept.`}
      </span>
      {task.agent_phase === "failed" ? (
        <button type="button" className="text-primary underline" onClick={() => actions.retryTaskAgent(task.id)} data-task-agent-retry="true">
          Retry agent start
        </button>
      ) : null}
      {task.pane_id ? (
        <button type="button" className="text-primary underline" onClick={() => actions.focusPane(task.pane_id as string)}>
          Show pane
        </button>
      ) : null}
      <button type="button" className="text-muted" aria-label="Dismiss" onClick={dismiss}>
        ×
      </button>
    </div>
  );
}
