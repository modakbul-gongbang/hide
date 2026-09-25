import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { listingRootFor, localRefusal, normalizePath, readRecent, refusalText, rememberRecent, suggestions } from "./registration";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { useErrorSince } from "./WorkspaceDialogs";

// The registration input (PRD S2 B8/B9): a path under home, completed from
// hided's directory listing, with recent registrations offered first. A
// refusal the shell can already see is shown under the input and no event
// goes out; a refusal hided answers lands in the same line. The text stays
// across a reconnect (the component is not remounted by the socket).

export function NewWorkspace({ actions }: { actions: Actions }) {
  const device = useShellStore((s) => s.rest?.navigator?.focused_device_id ?? "local");
  return device === "local" ? <LocalNewWorkspace actions={actions} /> : <DeviceNewWorkspace actions={actions} device={device} />;
}

/**
 * A folder on the selected SSH device (PRD S5.5 B23, B24). This machine's
 * home and listing say nothing about that device, so the path goes to the
 * core as typed (`~/` is that device's home) and the device's own helper
 * judges it; its refusal is shown here, and the field closes once the
 * device's registrations grow.
 */
function DeviceNewWorkspace({ actions, device }: { actions: Actions; device: string }) {
  const open = useUiStore((s) => s.overlay === "new_workspace");
  const closeOverlay = useUiStore((s) => s.closeOverlay);
  const label = useShellStore((s) => s.rest?.navigator?.devices?.find((row) => row.id === device)?.label ?? device);
  const count = useShellStore((s) => s.rest?.ui_state?.workspace_registrations?.filter((row) => row.device_id === device).length ?? 0);
  const [text, setText] = useState("~/");
  const [sent, setSent] = useState<{ at: number; count: number } | null>(null);
  const refused = useErrorSince(sent?.at ?? null, ["workspace.create"]);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);
  useEffect(() => {
    if (!sent || count <= sent.count) return;
    setSent(null);
    setText("~/");
    closeOverlay("new_workspace");
  }, [sent, count, closeOverlay]);
  if (!open) return null;
  const submit = () => {
    const path = text.trim().replace(/\/+$/, "");
    if (!path) return;
    setSent({ at: Date.now(), count });
    actions.createWorkspace(path, path.slice(path.lastIndexOf("/") + 1), device);
  };
  return (
    <div data-new-workspace={device} className="border-t border-border bg-card p-sm text-caption">
      <div className="mb-xs flex items-center justify-between text-subtle-foreground">
        <span className="min-w-0 break-words">새 워크스페이스 · {label}</span>
        <button type="button" className="text-muted-foreground" aria-label="Close new workspace" onClick={() => closeOverlay("new_workspace")}>
          ×
        </button>
      </div>
      <input
        ref={inputRef}
        value={text}
        placeholder="~/…"
        aria-label={`Workspace path on ${label}`}
        className="w-full rounded-xs bg-secondary px-xs py-xxs font-mono text-body text-foreground outline-none"
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
      />
      <p className="mt-xs text-muted-foreground">A folder inside {label}&apos;s home; that device checks it.</p>
      {refused ? (
        <div role="alert" data-registration-reason="device" className="mt-xs text-destructive">
          {refused}
        </div>
      ) : sent ? (
        <div className="mt-xs text-muted-foreground" data-registration-pending="true">
          Asking {label}…
        </div>
      ) : null}
    </div>
  );
}

function LocalNewWorkspace({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "new_workspace");
  const closeOverlay = useUiStore((s) => s.closeOverlay);
  const listing = useShellStore((s) => s.directoryList);
  const refusal = useShellStore((s) => s.pathRefusal);
  const clearRefusal = useShellStore((s) => s.clearPathRefusal);
  const registrations = useShellStore((s) => s.rest?.ui_state?.workspace_registrations);
  const [text, setText] = useState("");
  const [home, setHome] = useState<string | null>(null);
  const [local, setLocal] = useState<string | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [submitted, setSubmitted] = useState<string | null>(null);
  const requested = useRef<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Opening lists home; the answer's root_path is the home path the checks use.
  useEffect(() => {
    if (!open) return;
    setRecent(readRecent(localStorage));
    inputRef.current?.focus();
    if (!home) {
      requested.current = "~";
      actions.listDirectory("~");
    }
  }, [open, home, actions]);

  // Only the answer to the pending request settles it: a listing that arrived
  // for an earlier root would otherwise clear the mark and the same directory
  // would be asked for again. `~` is answered with the real home path, so it
  // is matched by being the only request the field has made.
  useEffect(() => {
    if (!listing || !requested.current) return;
    if (requested.current === "~") {
      setHome(listing.root_path);
      if (!text) setText(`${listing.root_path}/`);
    } else if (listing.root_path !== requested.current) {
      return;
    }
    requested.current = null;
  }, [listing, text]);

  // Typing into a new directory asks for that directory's children once.
  useEffect(() => {
    if (!open || !home) return;
    const root = listingRootFor(text, home);
    if (listing?.root_path === root || requested.current === root) return;
    requested.current = root;
    actions.listDirectory(root);
  }, [open, home, text, listing, actions]);

  // A path joins the recent list once the core registered it, not when it
  // was submitted: a refused path is not one to offer again.
  useEffect(() => {
    if (!submitted) return;
    if (registrations?.some((row) => normalizePath(row.path) === submitted)) {
      setRecent(rememberRecent(localStorage, submitted));
      setSubmitted(null);
    } else if (refusal?.kind === "create_workspace") {
      setSubmitted(null);
    }
  }, [submitted, registrations, refusal]);

  if (!open) return null;

  const options = suggestions(text, listing);
  const shown = refusal?.kind === "create_workspace" ? refusalText(refusal.reason) : local ? refusalText(local) : null;

  const submit = (candidate: string = text) => {
    clearRefusal();
    if (!home) return;
    const reason = localRefusal(candidate, home, registrations ?? [], listing);
    if (reason) {
      setLocal(reason);
      return;
    }
    setLocal(null);
    const path = normalizePath(candidate);
    setSubmitted(path);
    actions.createWorkspace(path, path.slice(path.lastIndexOf("/") + 1));
  };

  return (
    <div data-new-workspace="true" className="border-t border-border bg-card p-sm text-caption">
      <div className="mb-xs flex items-center justify-between text-subtle-foreground">
        <span>새 워크스페이스</span>
        <button type="button" className="text-muted-foreground" aria-label="Close new workspace" onClick={() => closeOverlay("new_workspace")}>
          ×
        </button>
      </div>
      <input
        ref={inputRef}
        value={text}
        list="hide-workspace-paths"
        placeholder={home ? `${home}/…` : "~/…"}
        aria-label="Workspace path"
        className="w-full rounded-xs bg-secondary px-xs py-xxs font-mono text-body text-foreground outline-none"
        onChange={(event) => {
          setText(event.target.value);
          setLocal(null);
          clearRefusal();
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
      />
      <datalist id="hide-workspace-paths">
        {options.map((path) => (
          <option key={path} value={path} />
        ))}
      </datalist>
      {shown ? (
        <div role="alert" data-registration-reason={refusal?.kind === "create_workspace" ? refusal.reason : local ?? ""} className="mt-xs text-destructive">
          {shown}
        </div>
      ) : null}
      {recent.length > 0 ? (
        <ul className="mt-xs">
          {recent.map((path) => (
            <li key={path}>
              <button
                type="button"
                className="w-full truncate text-left font-mono text-subtle-foreground hover:text-foreground"
                data-recent-path={path}
                onClick={() => {
                  setText(path);
                  submit(path);
                }}
              >
                {path}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      {options.length > 0 ? (
        <ul className="mt-xs max-h-[var(--size-relationship-list-max)] overflow-auto" data-suggestions={options.length}>
          {options.slice(0, 12).map((path) => (
            <li key={path}>
              <button
                type="button"
                className="w-full truncate text-left font-mono text-muted-foreground hover:text-foreground"
                data-suggestion={path}
                onClick={() => setText(`${path}/`)}
              >
                {path.slice(path.lastIndexOf("/") + 1)}/
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
