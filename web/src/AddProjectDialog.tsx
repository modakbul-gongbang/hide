// Add a project: a Host selector and, on this Mac, Browse folder, which opens
// macOS's own folder picker through the desktop app, and the other ways to
// add below it (Clone from URL, Create new project), each its own view of
// this dialog. A device's folders are not this Mac's to browse, so a device
// gets a `~/` path field its own helper judges. Every way sends one `create_workspace` and reads its answer through
// `useRegistration`. A browser tab has no picker and never opens this dialog.

import { CornerDownLeftIcon, FolderOpenIcon, FolderPlusIcon, LinkIcon, type LucideIcon } from "lucide-react";
import { useRef, useState } from "react";
import type { Actions } from "./actions";
import { addProjectHosts, alreadyRegistered, folderLabel, initialHost, trimFolder } from "./addProject";
import { CloneFromUrl, cloneRunning } from "./CloneFromUrl";
import { CreateProjectView } from "./CreateProjectView";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Kbd } from "./components/ui/kbd";
import { Select, SelectContent, SelectItem, SelectSeparator, SelectTrigger, SelectValue } from "./components/ui/select";
import { RegistrationStatus, useRegistration } from "./registration";
import type { WorkspaceRegistration } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

const NO_REGISTRATIONS: WorkspaceRegistration[] = [];

export function AddProjectDialog({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "add_project");
  // Mounted only while open, so every opening starts on the focused device with nothing sent.
  return open ? <AddProject actions={actions} /> : null;
}

/** The other ways to add on this Mac, each a view of this dialog. */
type OtherWay = { id: "clone" | "create"; label: string; detail: string; icon: LucideIcon };

const OTHER_WAYS: readonly OtherWay[] = [
  { id: "clone", label: "Clone from URL", detail: "A Git repository copied into a new folder", icon: LinkIcon },
  { id: "create", label: "Create new project", detail: "A new folder with its own Git repository", icon: FolderPlusIcon },
];

/** The Host list's last entry; it opens Add device instead of choosing a host. */
const ADD_DEVICE = "__add_device__";

function AddProject({ actions }: { actions: Actions }) {
  const close = () => useUiStore.getState().closeOverlay("add_project");
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  const focused = useShellStore((s) => s.rest?.navigator?.focused_device_id);
  const registrations = useShellStore((s) => s.rest?.ui_state?.workspace_registrations ?? NO_REGISTRATIONS);
  const hosts = addProjectHosts(devices);
  const [chosen, setChosen] = useState(() => initialHost(hosts, focused));
  const host = hosts.find((row) => row.id === chosen) ?? hosts[0]!;
  // A clone still running when the dialog opens is shown where it started.
  const [view, setView] = useState<"add" | OtherWay["id"]>(() => (cloneRunning(useShellStore.getState().rest?.repository_clone) ? "clone" : "add"));
  const [text, setText] = useState("~/");
  const registration = useRegistration(registrations);
  const { pending } = registration;
  const browseRef = useRef<HTMLButtonElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  const hostChanged = useRef(false);
  // The way Back returns from, whose row takes the keyboard again.
  const returnTo = useRef<OtherWay["id"] | null>(null);

  const add = (path: string) => {
    if (alreadyRegistered(path, host.id, registrations)) {
      registration.refuse(path, "already_registered");
      return;
    }
    registration.send(host.id, path, () => actions.createWorkspace(path, folderLabel(path), host.id));
  };

  const open = (next: typeof view) => {
    registration.reset();
    if (next === "add" && view !== "add") returnTo.current = view;
    setView(next);
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
        {view === "create" ? (
          <CreateProjectView actions={actions} registrations={registrations} onBack={() => open("add")} />
        ) : view === "clone" ? (
          <CloneFromUrl actions={actions} registrations={registrations} onBack={() => open("add")} />
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>Add a project</DialogTitle>
            </DialogHeader>
            <DialogBody className="flex flex-col gap-md">
              <div className="flex items-center gap-sm">
                <span className="text-body text-subtle-foreground">Host</span>
                <Select
                  value={host.id}
                  onValueChange={(next) => {
                    // The list's last entry is Add device, the one form every entry point opens (PRD home-device-rail B13).
                    if (next === ADD_DEVICE) {
                      close();
                      actions.openAddDevice();
                      return;
                    }
                    setChosen(next);
                    registration.reset();
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
                    <SelectSeparator />
                    <SelectItem value={ADD_DEVICE} data-host-option-add-device="true">
                      기기 추가…
                    </SelectItem>
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
              <RegistrationStatus registration={registration} />
              {host.id === "local" ? (
                <div className="flex flex-col gap-xs" data-add-project-other-ways="true">
                  <span className="text-caption text-muted-foreground">Other ways to add</span>
                  {OTHER_WAYS.map((way) => (
                    <button
                      key={way.id}
                      ref={(row) => {
                        if (row && returnTo.current === way.id) {
                          returnTo.current = null;
                          row.focus();
                        }
                      }}
                      type="button"
                      disabled={pending}
                      onClick={() => open(way.id)}
                      data-add-project-way={way.id}
                      className="flex w-full items-center gap-md rounded-md px-md py-sm text-left outline-none transition-colors hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-(--opacity-disabled)"
                    >
                      <span aria-hidden="true" className="flex size-(--size-control) shrink-0 items-center justify-center rounded-sm bg-secondary text-foreground [&_svg]:size-(--size-icon)">
                        <way.icon />
                      </span>
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="text-subhead font-medium text-foreground">{way.label}</span>
                        <span className="text-body text-muted-foreground">{way.detail}</span>
                      </span>
                    </button>
                  ))}
                </div>
              ) : null}
            </DialogBody>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
