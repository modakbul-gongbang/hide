// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Actions } from "./actions";
import { useBacklogOrigin } from "./backlogOrigin";
import { useShellStore } from "./store";

function Panel({ actions }: { actions: Actions }) {
  useBacklogOrigin(actions, "w1", "github:acme/repo#7");
  return null;
}

it("reads an open item's origin again after a reconnect, whose new connection holds nothing yet", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const calls: string[] = [];
  const actions = {
    readBacklogOrigin: (_workspace: string, key: string) => calls.push(`read ${key}`),
    closeBacklogOrigin: (key: string) => calls.push(`close ${key}`),
  } as unknown as Actions;
  useShellStore.setState({ connection: "live" });
  const root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Panel actions={actions} />));
  expect(calls).toEqual(["read github:acme/repo#7"]);
  // The socket drops and comes back: the daemon let the old connection's hold go.
  await act(async () => useShellStore.setState({ connection: "reconnecting" }));
  await act(async () => useShellStore.setState({ connection: "live" }));
  expect(calls).toEqual(["read github:acme/repo#7", "read github:acme/repo#7"]);
  await act(async () => root.unmount());
  expect(calls.at(-1)).toBe("close github:acme/repo#7");
  expect(calls.filter((call) => call.startsWith("close"))).toHaveLength(1);
});
