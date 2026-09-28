// Add a project's Clone from URL: a Git URL, the parent folder the clone goes
// into, and Clone. The folder the URL names is shown as it is typed, and
// hided is asked whether it is free under that parent (`clone_target`), so
// Clone is offered only for a clone that can start. The clone itself is the
// core's (`repository_clone`): its progress, its cancel and its end are read
// from the snapshot, and a finished clone is registered like a picked folder,
// so the dialog closes when that project appears. Closing the dialog leaves a
// running clone running; opening it again shows it here.

import { ArrowLeftIcon, FolderOpenIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { defaultCloneParent, folderLabel, parseCloneUrl, refusalText, trimFolder } from "./addProject";
import { Note, Status } from "./components/settings-rows";
import { Button } from "./components/ui/button";
import { DialogBody, DialogDescription, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import type { RepositoryClone, WorkspaceRegistration } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { useErrorSince } from "./WorkspaceDialogs";

const NO_REGISTRATIONS: WorkspaceRegistration[] = [];
/** How long typing settles before hided is asked about the folder. */
const TARGET_CHECK_DELAY_MS = 150;

/** Whether a clone is still running: nothing else may start until it ends. */
export function cloneRunning(clone: RepositoryClone | null | undefined): boolean {
  return clone?.phase === "cloning" || clone?.phase === "cancelling";
}

/**
 * Which clone this view follows: one running when it opened, or the first one
 * newer than `after`, the slot's id when Clone was pressed.
 */
type Following = { id: number } | { after: number; at: number; path: string };

export function CloneFromUrl({ actions, onBack }: { actions: Actions; onBack: () => void }) {
  const registrations = useShellStore((s) => s.rest?.ui_state?.workspace_registrations ?? NO_REGISTRATIONS);
  const slot = useShellStore((s) => s.rest?.repository_clone ?? null);
  const target = useShellStore((s) => s.cloneTarget);
  const pathRefusal = useShellStore((s) => s.pathRefusal);
  const [url, setUrl] = useState("");
  const [parent, setParent] = useState(() => defaultCloneParent(useShellStore.getState().rest?.ui_state?.workspace_registrations ?? []));
  const [following, setFollowing] = useState<Following | null>(() => {
    const running = useShellStore.getState().rest?.repository_clone;
    return running && cloneRunning(running) ? { id: running.id } : null;
  });
  const urlRef = useRef<HTMLInputElement>(null);

  const parsed = parseCloneUrl(url);
  const parentPath = trimFolder(parent);
  const name = parsed.ok ? parsed.name : null;
  const clone = slot && following && ("id" in following ? slot.id === following.id : slot.id > following.after) ? slot : null;
  const running = cloneRunning(clone);
  const sentAt = following && "at" in following ? following.at : null;
  const coreError = useErrorSince(sentAt, ["repository.clone", "workspace.create", "workspace.remove_in_flight", "workspace.control_unavailable"]);
  const hidedRefusal =
    following && "path" in following && pathRefusal?.kind === "clone_repository" && pathRefusal.path === following.path ? pathRefusal.reason : null;
  // A refusal ends what Clone started, before the clone (hided's, or the core
  // refusing the clone) or after it (the folder could not be registered).
  const refused = (clone === null && hidedRefusal !== null) || (coreError !== null && !cloneRunning(clone));
  const answer = target && name && target.parent === parentPath && target.name === name ? target : null;
  const added = clone?.phase === "finished" && registrations.some((row) => row.device_id === "local" && trimFolder(row.path) === clone.path);
  const waiting = following !== null && !refused && (clone === null || running || (clone.phase === "finished" && !added));
  const canClone = parsed.ok && answer !== null && answer.reason === null && !waiting;

  useEffect(() => {
    if (added) useUiStore.getState().closeOverlay("add_project");
  }, [added]);

  useEffect(() => {
    if (!name || !parentPath) return;
    const timer = window.setTimeout(() => actions.checkCloneTarget(parentPath, name), TARGET_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
    // A clone that ended may have taken the folder, so its end asks again.
  }, [actions, name, parentPath, clone?.phase]);

  const start = () => {
    if (!canClone || !name) return;
    useShellStore.getState().clearPathRefusal();
    setFollowing({ after: slot?.id ?? 0, at: Date.now(), path: `${parentPath.replace(/\/+$/, "")}/${name}` });
    actions.cloneRepository(url.trim(), parentPath, name);
  };

  const browse = async () => {
    const folder = await actions.pickFolder();
    if (folder !== null) setParent(trimFolder(folder));
  };

  return (
    <>
      <DialogHeader>
        <button
          type="button"
          disabled={running}
          onClick={onBack}
          data-add-project-back="true"
          className="-ml-xxs flex w-fit items-center gap-xs rounded-sm px-xxs text-body text-subtle-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-(--opacity-disabled) [&_svg]:size-(--size-icon)"
        >
          <ArrowLeftIcon aria-hidden="true" />
          Back
        </button>
        <DialogTitle>Clone from URL</DialogTitle>
        <DialogDescription>Enter the Git URL and choose where to clone it.</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-md">
        <label className="flex flex-col gap-xs">
          <span className="text-body text-subtle-foreground">Git URL</span>
          <Input
            ref={urlRef}
            autoFocus
            mono
            value={url}
            disabled={running}
            placeholder="https://github.com/user/repo.git"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            aria-invalid={url.trim() !== "" && !parsed.ok}
            data-clone-url="true"
            onChange={(event) => setUrl(event.target.value)}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || event.key !== "Enter") return;
              event.preventDefault();
              start();
            }}
          />
          {url.trim() !== "" && !parsed.ok ? (
            <Note tone="error" data-clone-url-reason="true">
              {parsed.reason}
            </Note>
          ) : null}
        </label>
        <div className="flex flex-col gap-xs">
          <label htmlFor="clone-parent" className="text-body text-subtle-foreground">
            Parent folder
          </label>
          <div className="flex items-center gap-sm">
            <Input
              id="clone-parent"
              mono
              value={parent}
              disabled={running}
              spellCheck={false}
              data-clone-parent="true"
              onChange={(event) => setParent(event.target.value)}
            />
            <Button variant="secondary" size="icon" disabled={running} aria-label="Choose parent folder" onClick={() => void browse()} data-clone-browse="true">
              <FolderOpenIcon />
            </Button>
          </div>
          {name && answer?.reason ? (
            <Note tone="error" data-clone-target-reason={answer.reason}>
              {refusalText(answer.reason)}
            </Note>
          ) : name ? (
            <p className="break-all text-caption text-muted-foreground" data-clone-target={answer?.path ?? ""}>
              Clones into a new folder <span className="font-mono text-foreground">{name}</span>
            </p>
          ) : null}
        </div>
        <CloneProgress clone={clone} following={following !== null} added={added || refused} />
        {refused ? (
          <div role="alert" className="flex min-w-0 flex-col gap-xxs" data-clone-refused={hidedRefusal ?? "core"}>
            <Status tone="error">{hidedRefusal ? refusalText(hidedRefusal) : coreError}</Status>
            {clone?.phase === "finished" ? (
              <span className="break-all text-caption text-muted-foreground" data-clone-kept={clone.path}>
                The repository is at <span className="font-mono">{clone.path}</span>; add it with Browse folder.
              </span>
            ) : null}
          </div>
        ) : null}
        {running && clone ? (
          <Button variant="secondary" size="lg" disabled={clone.phase === "cancelling"} onClick={() => actions.cancelRepositoryClone(clone.id)} data-clone-cancel="true">
            {clone.phase === "cancelling" ? "Cancelling…" : "Cancel"}
          </Button>
        ) : (
          <Button size="lg" disabled={!canClone} onClick={start} data-clone-submit="true">
            Clone
          </Button>
        )}
      </DialogBody>
    </>
  );
}

/** What the followed clone is doing, in the smallest form each state needs. */
function CloneProgress({ clone, following, added }: { clone: RepositoryClone | null; following: boolean; added: boolean }) {
  if (!following) return null;
  if (clone === null) {
    return (
      <Status tone="pending" data-clone-phase="sent">
        Starting the clone…
      </Status>
    );
  }
  const name = folderLabel(clone.path);
  if (clone.phase === "cloning" || clone.phase === "cancelling") {
    return (
      <div className="flex flex-col gap-xs" data-clone-phase={clone.phase}>
        <Status tone="pending">
          {clone.phase === "cancelling" ? `Cancelling the clone of ${name}…` : `Cloning ${name} from ${clone.host}…`}
        </Status>
        <div
          role="progressbar"
          aria-label={clone.stage ?? "Cloning"}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={clone.percent ?? undefined}
          className="h-xxs overflow-hidden rounded-full bg-secondary"
        >
          <div className="h-full bg-primary transition-[width]" style={{ width: `${clone.percent ?? 0}%` }} />
        </div>
        <span className="text-caption text-muted-foreground" data-clone-stage={clone.stage ?? ""}>
          {clone.stage ? `${clone.stage}${clone.percent === null ? "" : ` ${clone.percent}%`}` : "Connecting…"}
        </span>
      </div>
    );
  }
  if (clone.phase === "failed") {
    return (
      <div role="alert" data-clone-phase="failed">
        <Status tone="error">{clone.message ?? "The clone failed."}</Status>
      </div>
    );
  }
  if (clone.phase === "cancelled") {
    return (
      <Note tone="muted" data-clone-phase="cancelled">
        Clone cancelled; nothing was kept.
      </Note>
    );
  }
  return added ? null : (
    <Status tone="pending" data-clone-phase="finished">
      Adding {name}…
    </Status>
  );
}
