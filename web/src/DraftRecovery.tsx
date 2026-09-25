// Stored drafts no open tab stands for (PRD S5.5 B10-B12, B26, B44): a
// daemon restart that lost its tabs, another device's or another host's
// documents, and drafts from before drafts named their device. Each can be
// opened where it belongs, exported, or discarded on purpose; none goes on
// its own. A draft whose origin is unverified is never opened on a device it
// cannot be proven to belong to.

import { useState } from "react";
import type { Actions } from "./actions";
import { allBuffers, deleteBufferId, flushBuffer, identity, recoveryBuffers, tabBufferKey, type StoredBuffer } from "./buffers";
import { Button } from "./components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogHeader, DialogTitle } from "./components/ui/dialog";
import { Hint } from "./components/ui/tooltip";
import { catalogWorkspaces, frontCheckout, type SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";

/** Re-reads the stored drafts and keeps the ones no open tab stands for. */
export async function refreshRecoveryDrafts(): Promise<void> {
  const state = useShellStore.getState();
  const host = state.daemon?.host_id ?? null;
  const keys = (state.editor?.tabs ?? [])
    .filter((tab) => tab.kind === "file")
    .map((tab) => tabBufferKey(host, state.rest, tab))
    .filter((key) => key !== null);
  await Promise.all(keys.map((key) => flushBuffer(key)));
  const open = new Set(keys.map((key) => identity(key)));
  useShellStore.getState().setRecoveryDrafts(recoveryBuffers(await allBuffers(), open));
}

export type DraftPlace =
  | { kind: "front" }
  | { kind: "device"; device: string; label: string }
  | { kind: "checkout"; workspaceId: string; checkoutId: string; label: string }
  | { kind: "none"; reason: string };

/**
 * Where a draft can be opened from here. It opens only in its own checkout
 * on its own device of this daemon host; a draft from before drafts named
 * their device may be claimed by this machine's own checkout at that root and
 * nowhere else (B11).
 */
export function draftPlace(draft: StoredBuffer, host: string | null, rest: SnapshotRest | null): DraftPlace {
  if (!draft.root) return { kind: "none", reason: "stored before drafts named their checkout" };
  const device = draft.device ?? "local";
  if (draft.host !== null && draft.host !== host) return { kind: "none", reason: "written for another Hide host" };
  const checkout = catalogWorkspaces(rest)
    .filter((workspace) => (workspace.device_id ?? "local") === device)
    .flatMap((workspace) => workspace.checkouts)
    .find((row) => row.path === draft.root);
  if (!checkout) return { kind: "none", reason: `its checkout ${draft.root} is not open on ${deviceLabel(rest, device)}` };
  const focusedDevice = rest?.navigator?.focused_device_id ?? "local";
  if (focusedDevice !== device) return { kind: "device", device, label: deviceLabel(rest, device) };
  if (frontCheckout(rest)?.id !== checkout.id) {
    return { kind: "checkout", workspaceId: checkout.workspace_id, checkoutId: checkout.id, label: checkout.label };
  }
  return { kind: "front" };
}

function deviceLabel(rest: SnapshotRest | null, device: string): string {
  if (device === "local") return "this machine";
  return rest?.navigator?.devices?.find((row) => row.id === device)?.label ?? device;
}

function origin(draft: StoredBuffer, host: string | null, rest: SnapshotRest | null): string {
  if (draft.host === null) return "origin unverified";
  if (draft.host !== host) return "another Hide host";
  return deviceLabel(rest, draft.device ?? "local");
}

function exportContents(draft: StoredBuffer) {
  const url = URL.createObjectURL(new Blob([draft.contents], { type: "text/plain;charset=utf-8" }));
  const link = window.document.createElement("a");
  link.href = url;
  link.download = `${draft.path.split("/").pop() || "draft"}.draft`;
  link.click();
  URL.revokeObjectURL(url);
}

export function DraftRecoveryLine({ actions }: { actions: Actions }) {
  const count = useShellStore((s) => s.recoveryDrafts.length);
  const [open, setOpen] = useState(false);
  if (count === 0) return null;
  return (
    <>
      <div role="status" data-draft-recovery={count} className="flex items-center gap-md border-b border-border bg-card px-md py-xs text-caption text-warning">
        <Hint label={`${count === 1 ? "1 unsaved draft is" : `${count} unsaved drafts are`} not open in any tab. Nothing is discarded until you choose.`}>
          <span className="min-w-0 flex-1 truncate">
            {count === 1 ? "1 unsaved draft is" : `${count} unsaved drafts are`} not open in any tab. Nothing is discarded until you choose.
          </span>
        </Hint>
        <button type="button" className="text-foreground underline" data-draft-recovery-review="true" onClick={() => setOpen(true)}>
          Review
        </button>
      </div>
      {open ? <DraftRecoverySheet actions={actions} onClose={() => setOpen(false)} /> : null}
    </>
  );
}

function DraftRecoverySheet({ actions, onClose }: { actions: Actions; onClose: () => void }) {
  const drafts = useShellStore((s) => s.recoveryDrafts);
  const host = useShellStore((s) => s.daemon?.host_id ?? null);
  const rest = useShellStore((s) => s.rest);
  const [confirming, setConfirming] = useState<string | null>(null);
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      <DialogContent data-draft-recovery-sheet="true" className="max-h-(--size-settings-sheet-h-max)">
        <DialogHeader>
          <DialogTitle>Unsaved drafts</DialogTitle>
        </DialogHeader>
        <DialogBody>
          {drafts.length === 0 ? <p className="text-caption text-muted-foreground">No draft is waiting.</p> : null}
          <ul className="flex flex-col gap-sm">
            {drafts.map((draft) => {
              const place = draftPlace(draft, host, rest);
              // The id joins its parts with NUL, which a DOM attribute selector
              // cannot match; the attributes carry it URI-encoded.
              const tag = encodeURIComponent(draft.id);
              return (
                <li key={draft.id} className="flex flex-col gap-xxs border-b border-border pb-sm" data-draft={tag}>
                  <Hint label={draft.path}>
                    <span className="truncate font-mono text-caption text-foreground">{draft.path}</span>
                  </Hint>
                  <span className="text-caption text-muted-foreground">
                    {origin(draft, host, rest)} · {new Date(draft.updated_at).toLocaleString()}
                    {place.kind === "none" ? ` · cannot open here: ${place.reason}` : ""}
                  </span>
                  <span className="flex flex-wrap gap-sm">
                    {place.kind === "front" ? (
                      <Button size="sm" data-draft-open={tag} onClick={() => { actions.openFile(draft.path, false); onClose(); }}>Open</Button>
                    ) : place.kind === "device" ? (
                      <Button size="sm" data-draft-show-device={tag} onClick={() => actions.focusDevice(place.device)}>Show {place.label}</Button>
                    ) : place.kind === "checkout" ? (
                      <Button size="sm" data-draft-show-checkout={tag} onClick={() => actions.focusCheckout(place.workspaceId, place.checkoutId)}>Show {place.label}</Button>
                    ) : null}
                    <Button variant="ghost" size="sm" data-draft-export={tag} onClick={() => exportContents(draft)}>Export</Button>
                    {confirming === draft.id ? (
                      <Button
                        variant="destructive"
                        size="sm"
                        data-draft-discard-confirm={tag}
                        onClick={() => {
                          setConfirming(null);
                          void deleteBufferId(draft.id).then(refreshRecoveryDrafts);
                        }}
                      >
                        Discard permanently
                      </Button>
                    ) : (
                      <Button variant="ghost" size="sm" data-draft-discard={tag} onClick={() => setConfirming(draft.id)}>Discard…</Button>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
