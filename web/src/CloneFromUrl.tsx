// Add a project's Clone from URL: a Git URL, the parent folder the clone goes
// into, and Clone. The folder the URL names is shown as it is typed, and
// hided is asked whether it is free under that parent (`clone_target`), so
// Clone is offered only for a clone that can start. The clone itself is the
// core's (`repository_clone`): its progress, its cancel and its end are read
// from the snapshot. A finished clone is registered with the same
// `create_workspace` a picked folder sends, and its answer is read the same
// way (`useRegistration`): the dialog closes when that project appears.
// Closing the dialog leaves a running clone running; opening it again shows
// it here.

import { ArrowLeftIcon, FolderOpenIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { defaultProjectParent, folderLabel, parseCloneUrl, refusalText, trimFolder } from "./addProject";
import { Note, Status } from "./components/settings-rows";
import { Button } from "./components/ui/button";
import { DialogBody, DialogDescription, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { RegistrationStatus, useRegistration } from "./registration";
import type { RepositoryClone, WorkspaceRegistration } from "./snapshot";
import { useShellStore } from "./store";
import { useErrorSince } from "./WorkspaceDialogs";

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

export function CloneFromUrl({ actions, registrations, onBack }: { actions: Actions; registrations: readonly WorkspaceRegistration[]; onBack: () => void }) {
  const slot = useShellStore((s) => s.rest?.repository_clone ?? null);
  const target = useShellStore((s) => s.cloneTarget);
  const pathRefusal = useShellStore((s) => s.pathRefusal);
  const [url, setUrl] = useState("");
  const [parent, setParent] = useState(() => defaultProjectParent(registrations));
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
  // The registration a finished clone makes; its refusal and its closing the dialog are `useRegistration`'s.
  const registration = useRegistration(registrations);
  const cloneError = useErrorSince(sentAt, ["repository.clone", "workspace.control_unavailable"]);
  const hidedRefusal =
    following && "path" in following && pathRefusal?.kind === "clone_repository" && pathRefusal.path === following.path ? pathRefusal.reason : null;
  // A refusal of the clone itself (hided's, or the core's), or the core
  // unable to register the finished folder; a running clone has neither.
  const refusal = hidedRefusal ? refusalText(hidedRefusal) : cloneError && !running ? cloneError : null;
  const ended = clone?.phase === "failed" || clone?.phase === "cancelled" || refusal !== null;
  const answer = target && name && target.parent === parentPath && target.name === name ? target : null;
  const waiting = following !== null && !ended && registration.shown === null;
  const canClone = parsed.ok && answer !== null && answer.reason === null && !waiting;

  // A clone that ended without a folder leaves no registration to wait for.
  const { reset } = registration;
  useEffect(() => {
    if (ended) reset();
  }, [ended, reset]);

  useEffect(() => {
    if (!name || !parentPath) return;
    const timer = window.setTimeout(() => actions.checkCloneTarget(parentPath, name), TARGET_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
    // A clone that ended may have taken the folder, so its end asks again.
  }, [actions, name, parentPath, clone?.phase]);

  const start = () => {
    if (!canClone || !name) return;
    const path = `${parentPath.replace(/\/+$/, "")}/${name}`;
    setFollowing({ after: slot?.id ?? 0, at: Date.now(), path });
    registration.send("local", path, () => actions.cloneRepository(url.trim(), parentPath, name));
  };

  const browse = async () => {
    const folder = await actions.pickFolder();
    if (folder !== null) setParent(trimFolder(folder));
  };

  return (
    <>
      <DialogHeader>
        <Button type="button" variant="ghost" size="sm" className="-ml-sm mb-xs self-start" disabled={running} onClick={onBack} data-add-project-back="true">
          <ArrowLeftIcon />
          Back
        </Button>
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
        <CloneProgress clone={clone} following={following !== null} />
        {refusal ? (
          <div role="alert" className="flex min-w-0 flex-col gap-xxs" data-clone-refused={hidedRefusal ?? "core"}>
            <Status tone="error">{refusal}</Status>
          </div>
        ) : clone?.phase === "finished" ? (
          <RegistrationStatus registration={registration} />
        ) : null}
        {clone?.phase === "finished" && (refusal || registration.shown) ? (
          <Note data-clone-kept={clone.path}>The repository was cloned there; add it with Browse folder.</Note>
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
function CloneProgress({ clone, following }: { clone: RepositoryClone | null; following: boolean }) {
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
  // Finished: the registration it hands over says `Adding <folder>…` itself.
  return null;
}
