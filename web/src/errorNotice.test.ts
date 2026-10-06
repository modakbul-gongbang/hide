import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { errorNotice, watchErrorNotices } from "./errorNotice";
import type { SnapshotRest } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

const error = (kind: string, occurredAt: number) => ({ kind, message: `reason for ${kind}`, retryable: false, occurred_at: occurredAt });
const rest = (lastError: ReturnType<typeof error> | null, device = "local") =>
  ({ navigator: { focused_device_id: device }, status: { last_error: lastError } }) as unknown as SnapshotRest;
const publish = (next: SnapshotRest) => useShellStore.setState({ rest: next });

describe("the failures the operator is told about", () => {
  it("picks the ones the operator can act on and leaves the rest to the diagnostic log (design 13)", () => {
    expect(errorNotice(error("pane_reopen.not_restarted", 1))).toEqual({ text: "reason for pane_reopen.not_restarted", refreshable: false });
    expect(errorNotice(error("remote.control.close_status_unknown", 1))).toEqual({ text: "reason for remote.control.close_status_unknown", refreshable: true });
    expect(errorNotice(error("remote.control.refused", 1))?.refreshable).toBe(false);
    expect(errorNotice(error("view_layout.unsaved", 1))?.text).toBe("reason for view_layout.unsaved");
    expect(errorNotice(error("view_layout.stale_workspace", 1))).toBeNull();
    expect(errorNotice(error("file.save_failed", 1))).toBeNull();
    expect(errorNotice(null)).toBeNull();
  });
});

describe("the notice a failed Reopen leaves", () => {
  let stop = () => {};
  beforeEach(() => {
    useUiStore.getState().setNotice(null);
    useShellStore.setState({ rest: rest(null) });
    stop = watchErrorNotices();
  });
  afterEach(() => {
    stop();
    useUiStore.getState().setNotice(null);
    useShellStore.setState({ rest: null });
  });

  it("appears from the snapshot's last_error and stays when the core clears it at its next event", () => {
    publish(rest(error("pane_reopen.not_restarted", 7)));
    expect(useUiStore.getState().notice).toEqual({ text: "reason for pane_reopen.not_restarted", refreshable: false });

    // The core takes last_error at the next event of any kind; the operator has not read the line yet.
    publish(rest(null));
    expect(useUiStore.getState().notice?.text).toBe("reason for pane_reopen.not_restarted");
  });

  it("is raised once per occurrence: a dismissed notice does not come back with the next snapshot", () => {
    publish(rest(error("pane_reopen.not_restarted", 7)));
    useUiStore.getState().setNotice(null);
    publish(rest(error("pane_reopen.not_restarted", 7)));
    expect(useUiStore.getState().notice).toBeNull();
    publish(rest(error("pane_reopen.not_restarted", 8)));
    expect(useUiStore.getState().notice).not.toBeNull();
  });

  it("does not outlive the device it was about, and an unrelated failure raises none", () => {
    publish(rest(error("file.save_failed", 3)));
    expect(useUiStore.getState().notice).toBeNull();
    publish(rest(error("pane_reopen.not_restarted", 7)));
    publish(rest(null, "studio"));
    expect(useUiStore.getState().notice).toBeNull();
  });
});
