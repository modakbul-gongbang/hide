import { useEffect, useRef, useState } from "react";
import type { Actions } from "./actions";
import { listingRootFor, localRefusal, normalizePath, readRecent, refusalText, rememberRecent, suggestions } from "./registration";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// The registration input (PRD S2 B8/B9): a path under home, completed from
// hided's directory listing, with recent registrations offered first. A
// refusal the shell can already see is shown under the input and no event
// goes out; a refusal hided answers lands in the same line. The text stays
// across a reconnect (the component is not remounted by the socket).

export function NewWorkspace({ actions }: { actions: Actions }) {
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

  useEffect(() => {
    if (!listing || !requested.current) return;
    if (requested.current === "~") setHome(listing.root_path);
    if (!text && requested.current === "~") setText(`${listing.root_path}/`);
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
    setRecent(rememberRecent(localStorage, path));
    actions.createWorkspace(path, path.slice(path.lastIndexOf("/") + 1));
  };

  return (
    <div data-new-workspace="true" className="border-t border-divider bg-panel p-sm text-caption">
      <div className="mb-xs flex items-center justify-between text-secondary">
        <span>새 워크스페이스</span>
        <button type="button" className="text-muted" aria-label="Close new workspace" onClick={() => closeOverlay("new_workspace")}>
          ×
        </button>
      </div>
      <input
        ref={inputRef}
        value={text}
        list="hide-workspace-paths"
        placeholder={home ? `${home}/…` : "~/…"}
        aria-label="Workspace path"
        className="w-full rounded-xs bg-elevated px-xs py-xxs font-mono text-body text-primary outline-none"
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
        <div role="alert" data-registration-reason={refusal?.kind === "create_workspace" ? refusal.reason : local ?? ""} className="mt-xs text-danger">
          {shown}
        </div>
      ) : null}
      {recent.length > 0 ? (
        <ul className="mt-xs">
          {recent.map((path) => (
            <li key={path}>
              <button
                type="button"
                className="w-full truncate text-left font-mono text-secondary hover:text-primary"
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
                className="w-full truncate text-left font-mono text-muted hover:text-primary"
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
