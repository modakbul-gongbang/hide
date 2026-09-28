// Add a project: a Host selector and, on this Mac, Browse folder, which opens
// macOS's own folder picker through the desktop app. A device's folders are
// not this Mac's to browse, so a device gets a `~/` path field its own helper
// judges. Either way the folder goes out as the one `create_workspace` event,
// and the answer is read, never assumed: the registrations growing closes the
// dialog, and a refusal (hided's `path_refused`, the core's error, or the one
// the shell can see itself) stays inside it, naming the folder, so another can
// be picked. A browser tab has no picker and never opens this dialog.

import { CornerDownLeftIcon, FolderOpenIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { addProjectHosts, alreadyRegistered, folderLabel, initialHost, refusalText, trimFolder } from "./addProject";
import { Status } from "./components/settings-rows";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./components/ui/select";
import type { WorkspaceRegistration } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { useErrorSince } from "./WorkspaceDialogs";

const NO_REGISTRATIONS: WorkspaceRegistration[] = [];

export function AddProjectDialog({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "add_project");
  // Mounted only while open, so every opening starts on the focused device with nothing sent.
  return open ? <AddProject actions={actions} /> : null;
}

/**
 * A folder sent as `create_workspace`, with the ids of its device's projects
 * then: a new id is the answer, whatever else was removed meanwhile, and
 * whichever spelling (canonical, or a device's own) the registration keeps.
 */
type Sent = { device: string; path: string; at: number; known: ReadonlySet<string> };

function AddProject({ actions }: { actions: Actions }) {
  const close = () => useUiStore.getState().closeOverlay("add_project");
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const focused = useShellStore((s) => s.rest?.navigator?.focused_device_id);
  const registrations = useShellStore((s) => s.rest?.ui_state?.workspace_registrations ?? NO_REGISTRATIONS);
  const pathRefusal = useShellStore((s) => s.pathRefusal);
  const hosts = addProjectHosts(devices);
  const [chosen, setChosen] = useState(() => initialHost(hosts, focused));
  const host = hosts.find((row) => row.id === chosen) ?? hosts[0]!;
  const [text, setText] = useState("~/");
  const [sent, setSent] = useState<Sent | null>(null);
  const [shellRefusal, setShellRefusal] = useState<{ path: string; reason: string } | null>(null);
  const coreError = useErrorSince(sent?.at ?? null, ["workspace.create", "workspace.remove_in_flight"]);
  const browseRef = useRef<HTMLButtonElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  const hostChanged = useRef(false);
  const added = sent !== null && registrations.some((row) => row.device_id === sent.device && !sent.known.has(row.id));

  const hidedReason = sent && pathRefusal?.kind === "create_workspace" && pathRefusal.path === sent.path ? pathRefusal.reason : null;
  const shown = shellRefusal
    ? { ...shellRefusal, text: refusalText(shellRefusal.reason) }
    : sent && hidedReason
      ? { path: sent.path, reason: hidedReason, text: refusalText(hidedReason) }
      : sent && coreError
        ? { path: sent.path, reason: "core", text: coreError }
        : null;
  const pending = sent !== null && shown === null;

  // The project appearing is the answer; the dialog closes on it.
  useEffect(() => {
    if (added) useUiStore.getState().closeOverlay("add_project");
  }, [added]);

  const add = (path: string) => {
    setShellRefusal(null);
    setSent(null);
    if (alreadyRegistered(path, host.id, registrations)) {
      setShellRefusal({ path, reason: "already_registered" });
      return;
    }
    useShellStore.getState().clearPathRefusal();
    setSent({ device: host.id, path, at: Date.now(), known: new Set(registrations.filter((row) => row.device_id === host.id).map((row) => row.id)) });
    actions.createWorkspace(path, folderLabel(path), host.id);
  };

  const browse = async () => {
    if (pending) return;
    const folder = await actions.pickFolder();
    // A cancelled pick leaves the dialog as it was.
    if (folder !== null) add(trimFolder(folder));
  };

  const submitField = () => {
    const path = trimFolder(text);
    if (pending || !path || path === "~") return;
    add(path);
  };

  return (
    <Dialog open onOpenChange={(next) => { if (!next) close(); }}>
      <DialogContent
        showCloseButton
        aria-describedby={undefined}
        data-add-project={host.id}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          (host.id === "local" ? browseRef.current : fieldRef.current)?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle>Add a project</DialogTitle>
        </DialogHeader>
        <DialogBody className="flex flex-col gap-md">
          <div className="flex items-center gap-sm">
            <span className="text-body text-subtle-foreground">Host</span>
            <Select
              value={host.id}
              onValueChange={(next) => {
                setChosen(next);
                setSent(null);
                setShellRefusal(null);
                hostChanged.current = true;
              }}
            >
              <SelectTrigger size="sm" className="w-auto" aria-label="Host" data-add-project-host="true">
                <SelectValue />
              </SelectTrigger>
              <SelectContent
                onCloseAutoFocus={(event) => {
                  // A new host hands the keyboard to its own primary control rather than back to the selector.
                  if (!hostChanged.current) return;
                  hostChanged.current = false;
                  event.preventDefault();
                  (browseRef.current ?? fieldRef.current)?.focus();
                }}
              >
                {hosts.map((row) => (
                  <SelectItem key={row.id} value={row.id} data-host-option={row.id}>
                    {row.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {host.id === "local" ? (
            <button
              ref={browseRef}
              type="button"
              disabled={pending}
              onClick={() => void browse()}
              data-add-project-browse="true"
              className="flex w-full items-center gap-md rounded-md border border-border bg-card px-md py-sm text-left outline-none transition-colors hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-(--opacity-disabled)"
            >
              <span aria-hidden="true" className="flex size-(--size-control) shrink-0 items-center justify-center rounded-sm bg-secondary text-foreground [&_svg]:size-(--size-icon)">
                <FolderOpenIcon />
              </span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="text-subhead font-medium text-foreground">Browse folder</span>
                <span className="text-body text-muted-foreground">A project, a Git repository, or a folder of repositories</span>
              </span>
              <Kbd aria-hidden="true">
                <CornerDownLeftIcon />
              </Kbd>
            </button>
          ) : (
            <div className="flex flex-col gap-xs">
              <div className="flex items-center gap-sm">
                <Input
                  ref={fieldRef}
                  mono
                  value={text}
                  placeholder="~/…"
                  aria-label={`Folder on ${host.label}`}
                  data-add-project-path="true"
                  onChange={(event) => setText(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.nativeEvent.isComposing || event.key !== "Enter") return;
                    event.preventDefault();
                    submitField();
                  }}
                />
                <Button variant="secondary" disabled={pending} onClick={submitField} data-add-project-submit="true">
                  Add
                </Button>
              </div>
              <p className="text-body text-muted-foreground">A folder inside {host.label}&apos;s home; that device checks it.</p>
            </div>
          )}
          {pending ? (
            <Status tone="pending" data-registration-pending="true">
              Adding {folderLabel(sent.path)}…
            </Status>
          ) : null}
          {shown ? (
            <div role="alert" className="flex min-w-0 flex-col gap-xxs" data-registration-reason={shown.reason} data-registration-path={shown.path}>
              <Status tone="error">{shown.text}</Status>
              <span className="break-all font-mono text-caption text-muted-foreground">{shown.path}</span>
            </div>
          ) : null}
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
