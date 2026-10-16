// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Actions } from "./actions";
import { gitProjectInFront, useGithubLook } from "./githubLook";
import type { SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";

const project = (id: string, over: Partial<Workspace> = {}) =>
  ({ id, device_id: "local", label: id, path: `/fixture/${id}`, is_git: true, checkouts: [{ id: `${id}-main` }], ...over }) as unknown as Workspace;

const rest = (front: string | null, workspaces: Workspace[], navigator: Record<string, unknown> = {}) =>
  ({ navigator: { devices: [{ id: "local", kind: "local" }], focused_device_id: "local", focused_checkout_id: front, workspaces, ...navigator } }) as unknown as SnapshotRest;

it("names the local Git project in front and nothing else", () => {
  const workspaces = [project("a"), project("folder", { is_git: false }), project("device", { remote_target_id: "mini", device_id: "mini" })];
  expect(gitProjectInFront(rest("a-main", workspaces))).toBe("a");
  expect(gitProjectInFront(rest("folder-main", workspaces))).toBeNull();
  expect(gitProjectInFront(rest("device-main", workspaces))).toBeNull();
  expect(gitProjectInFront(rest(null, workspaces))).toBeNull();
  expect(gitProjectInFront(rest("a-main", workspaces, { focused_device_id: "mini", devices: [{ id: "local", kind: "local" }, { id: "mini", kind: "remote" }] }))).toBeNull();
  expect(gitProjectInFront(null)).toBeNull();
});

function Looking({ actions }: { actions: Actions }) {
  useGithubLook(actions);
  return null;
}

let visibility: DocumentVisibilityState = "visible";
Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });

afterEach(() => {
  visibility = "visible";
});

it("looks when the project comes forward and each time the page gains focus or becomes visible, and stops with the page", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const looks: string[] = [];
  const actions = { lookAtProject: (id: string) => looks.push(id) } as unknown as Actions;
  const workspaces = [project("a"), project("b")];
  useShellStore.setState({ connection: "reconnecting", rest: rest("a-main", workspaces) });
  const root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Looking actions={actions} />));
  expect(looks, "nothing is said while the connection is down").toEqual([]);

  await act(async () => useShellStore.setState({ connection: "live" }));
  expect(looks).toEqual(["a"]);

  await act(async () => void window.dispatchEvent(new Event("focus")));
  expect(looks).toEqual(["a", "a"]);

  visibility = "hidden";
  await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
  expect(looks, "hiding the page is not a look").toEqual(["a", "a"]);
  visibility = "visible";
  await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
  expect(looks).toEqual(["a", "a", "a"]);

  await act(async () => useShellStore.setState({ rest: rest("b-main", workspaces) }));
  expect(looks.at(-1), "the project that came to the front is looked at").toBe("b");
  await act(async () => void window.dispatchEvent(new Event("focus")));
  expect(looks.at(-1)).toBe("b");

  await act(async () => useShellStore.setState({ rest: rest("b-main", workspaces) }));
  expect(looks.filter((id) => id === "b"), "the same project in front again says nothing new").toHaveLength(2);

  await act(async () => root.unmount());
  const settled = looks.length;
  await act(async () => void window.dispatchEvent(new Event("focus")));
  expect(looks).toHaveLength(settled);
});
