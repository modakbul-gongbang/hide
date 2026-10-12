// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { armKeyTarget, disarmKeyTarget, keyPayload, noteOperatorPointer, noteTerminalFocus, observeInputRequests, UNSEEN_REQUEST_LIMIT_MS } from "./keyTarget";
import { useShellStore } from "./store";

/** A terminal host holding DOM focus, the way xterm's textarea sits inside it. */
function focusTerminalOf(paneId: string) {
  const host = document.createElement("div");
  host.dataset.terminalHost = paneId;
  const textarea = document.createElement("textarea");
  host.append(textarea);
  document.body.append(host);
  textarea.focus();
}

describe("keys after a new tab or split", () => {
  afterEach(() => {
    disarmKeyTarget();
    document.body.replaceChildren();
    useShellStore.setState({ focusedPaneId: null, rest: null });
  });

  it("go to the request, not the pane that had the keyboard", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1");
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pending_request: "r1", bytes_base64: "YQ==" });
  });

  it("go to their own pane once the keyboard reaches another pane", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1");
    noteTerminalFocus("w1:p1");
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pending_request: "r1", bytes_base64: "YQ==" });
    noteTerminalFocus("w1:p2");
    expect(keyPayload("w1:p2", "YQ==")).toEqual({ pane_id: "w1:p2", bytes_base64: "YQ==" });
  });

  it("stop following the request when the operator points somewhere", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1");
    noteOperatorPointer();
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pane_id: "w1:p1", bytes_base64: "YQ==" });
  });

  it("stop following a request the core discarded, and only that one", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r2");
    observeInputRequests([{ request_id: "r1", state: "discarded" }, { request_id: "r2", state: "pending" }]);
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pending_request: "r2", bytes_base64: "YQ==" });
    observeInputRequests([{ request_id: "r2", state: "discarded" }]);
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pane_id: "w1:p1", bytes_base64: "YQ==" });
  });

  it("keep the request when focus comes back from a palette to the pane that had the keyboard", () => {
    useShellStore.setState({ focusedPaneId: "w1:p1" });
    armKeyTarget("r1");
    noteTerminalFocus("w1:p1");
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pending_request: "r1", bytes_base64: "YQ==" });
  });

  it("stop following a request the core listed and then dropped", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1");
    observeInputRequests([{ request_id: "r1", state: "pending" }]);
    observeInputRequests([]);
    expect(keyPayload("w1:p1", "YQ==")).toEqual({ pane_id: "w1:p1", bytes_base64: "YQ==" });
  });

  it("stop following a request the core never listed", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1", 1_000);
    observeInputRequests([]);
    expect(keyPayload("w1:p1", "YQ==", 1_000 + UNSEEN_REQUEST_LIMIT_MS)).toEqual({ pending_request: "r1", bytes_base64: "YQ==" });
    expect(keyPayload("w1:p1", "YQ==", 1_001 + UNSEEN_REQUEST_LIMIT_MS)).toEqual({ pane_id: "w1:p1", bytes_base64: "YQ==" });
  });

  // PRD fork-operator-root B4: a fork's pane has the keyboard before its agent
  // has started, and what is typed there waits for the agent.
  it("keep following a fork's request while its pane has the keyboard, until its agent starts", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1");
    const held = [{ request_id: "r1", state: "pending" as const, held_for: "w1:p9" }];
    useShellStore.setState({ rest: { terminal: { input_requests: held } } });
    observeInputRequests(held);
    noteTerminalFocus("w1:p9");
    expect(keyPayload("w1:p9", "YQ==")).toEqual({ pending_request: "r1", bytes_base64: "YQ==" });
    observeInputRequests([{ request_id: "r1", state: "ready" }]);
    expect(keyPayload("w1:p9", "YQ==")).toEqual({ pane_id: "w1:p9", bytes_base64: "YQ==" });
  });

  it("stop following a fork's request when the keyboard goes to a third pane", () => {
    focusTerminalOf("w1:p1");
    armKeyTarget("r1");
    const held = [{ request_id: "r1", state: "pending" as const, held_for: "w1:p9" }];
    useShellStore.setState({ rest: { terminal: { input_requests: held } } });
    noteTerminalFocus("w1:p3");
    expect(keyPayload("w1:p3", "YQ==")).toEqual({ pane_id: "w1:p3", bytes_base64: "YQ==" });
  });
});
